use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags};

use crate::error::{Error, Result, ValidationReason};
use crate::model::{
    value_fits, Assertion, AssertionId, Cardinality, Definition, DefinitionId, Entity, EntityId,
    EntityType, Owner, Page, PageId, PageRevision, TypeId, UniverseId, Value, ValueClass,
};
use crate::store::{
    self, assertion_from_parts, ensure_unique_snapshot, read_flag, AssertionParts, Counts,
    PageSnapshot, Snapshot, Store, ASSERTION_COLUMNS, SCHEMA,
};

const HEADER_LEN: usize = 100;
const MAGIC: &[u8] = b"SQLite format 3\0";
const SOURCE_BUSY_TIMEOUT: Duration = Duration::from_millis(1000);
const LEGACY_USER_VERSION: i32 = 1;
const CURRENT_USER_VERSION: i32 = 2;
const LEGACY_FORMAT_VERSION: i64 = 1;
const UTF8_ENCODING: i32 = 1;
const SIDECARS: [&str; 3] = ["-journal", "-wal", "-shm"];
const CANDIDATE_ATTEMPTS: u32 = 32;
const CANDIDATE_PREFIX: &str = ".spike-migrate-";
static CANDIDATE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

const LEGACY_V1_EARLY: &str = include_str!("migration/legacy_v1_early.sql");
const LEGACY_V1_LATE: &str = include_str!("migration/legacy_v1_late.sql");

/// One observable boundary of a migration run.
///
/// The stage is part of the error so a caller can tell a malformed source from
/// a destination collision, a failed candidate check, or a publication that
/// was already visible when it failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationStage {
    PreflightSource,
    ReadSource,
    SnapshotSource,
    PreflightDestination,
    BuildCandidate,
    VerifyCandidate,
    PublishDestination,
    VerifyDestination,
    ReleaseSource,
    CleanupCandidate,
}

impl fmt::Display for MigrationStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::PreflightSource => "source preflight",
            Self::ReadSource => "source open",
            Self::SnapshotSource => "source snapshot",
            Self::PreflightDestination => "destination preflight",
            Self::BuildCandidate => "candidate build",
            Self::VerifyCandidate => "candidate verification",
            Self::PublishDestination => "destination publication",
            Self::VerifyDestination => "destination verification",
            Self::ReleaseSource => "source release",
            Self::CleanupCandidate => "candidate cleanup",
        };
        formatter.write_str(name)
    }
}

/// A failed migration with its stage, both paths, and every residual file.
///
/// A pre-publication candidate this run owns is removed on failure only while
/// it still verifies against the pinned snapshot. After a successful link the
/// destination and the candidate alias are retained and reported, because
/// either may be the only complete copy. A concurrent writer that replaces a
/// path is outside the alpha's attribution guarantee, so an unexplained path is
/// reported for inspection instead of being deleted.
#[derive(Debug)]
pub struct MigrationError {
    pub stage: MigrationStage,
    pub source: PathBuf,
    pub destination: PathBuf,
    pub residuals: Vec<PathBuf>,
    pub cause: Box<Error>,
}

impl fmt::Display for MigrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "migration failed during {} for {} -> {}: {}",
            self.stage,
            self.source.display(),
            self.destination.display(),
            self.cause
        )?;
        for residual in &self.residuals {
            write!(formatter, "; residual file {}", residual.display())?;
        }
        Ok(())
    }
}

impl std::error::Error for MigrationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.cause.as_ref())
    }
}

/// The result of a completed migration.
///
/// `residuals` is normally empty. It lists paths this run did not remove, each
/// for manual inspection. In particular, a candidate alias is removed only when
/// it still reproduces the migrated database; a path that changed or was
/// replaced is left in place and reported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationReport {
    pub universe_id: UniverseId,
    pub counts: Counts,
    pub destination: PathBuf,
    pub residuals: Vec<PathBuf>,
}

/// The result of a general version 2 verification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerificationReport {
    pub universe_id: UniverseId,
    pub counts: Counts,
    pub mentions: u64,
}

/// A failed general version 2 verification.
#[derive(Debug)]
pub struct VerificationError {
    pub path: PathBuf,
    pub cause: Error,
}

impl fmt::Display for VerificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "verification failed for {}: {}",
            self.path.display(),
            self.cause
        )
    }
}

impl std::error::Error for VerificationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LegacyVariant {
    Early,
    Late,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceObservation {
    header: [u8; HEADER_LEN],
    len: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Publication {
    CandidateOnly,
    HardLink,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Checkpoint {
    CandidateBuilt,
    CandidateVerified,
    Published,
    DestinationVerified,
    DestinationRechecked,
    AliasVerified,
    CleanedUp,
}

// Test observers read these fields; the production observer is a no-op.
#[allow(dead_code)]
struct CheckpointView<'a> {
    candidate: &'a Path,
    destination: &'a Path,
}

#[derive(Default)]
struct RunState {
    candidate: Option<PathBuf>,
    snapshot: Option<Snapshot>,
    published: bool,
}

impl RunState {
    fn residuals(&self, destination: &Path) -> Vec<PathBuf> {
        let mut residuals = match &self.candidate {
            Some(candidate) => residual_files(candidate),
            None => Vec::new(),
        };
        // Before publication the destination is never this run's file, so a
        // pre-existing file is not claimed. After publication the destination
        // path belongs to this run; whatever is there now (the linked file, a
        // replacement, or a leftover sidecar) is reported for inspection. It is
        // never deleted, so misattributing a same-user replacement is safe.
        if self.published {
            for path in residual_files(destination) {
                if !residuals.contains(&path) {
                    residuals.push(path);
                }
            }
        }
        residuals
    }
}

/// Migrates a version 1 database into a new version 2 destination.
///
/// The source is opened read-only with one pinned read transaction and a
/// captured canonical snapshot. The transaction stays alive through candidate
/// construction, candidate verification, publication, and post-publication
/// verification of the destination. Publication uses only an atomic
/// no-overwrite hard link in the destination directory; there is no rename,
/// copy, or replace fallback. A destination that appears by any other means is
/// never overwritten. The published destination is fully re-verified while the
/// source is still pinned. After the source is released, the candidate alias is
/// fully verified once more, then the destination's type, sidecars, header, and
/// length are observed as the last check before the unlink. Only then is the
/// temporary alias removed, and only when it matched the migrated database
/// immediately before that observation.
///
/// Source rechecks compare the regular-file type, the absence of sidecars, the
/// SQLite header, and the file length. A same-size out-of-band edit that
/// preserves those fields is part of the documented path-race limit, not a
/// stronger byte-for-byte guarantee.
///
/// On error the destination is absent, or a file is present at the destination
/// path this run published to (it may have been changed or replaced by an
/// out-of-band writer); every surviving destination or candidate path is
/// reported in `residuals`. Cleanup never deletes the destination after
/// publication, and removes a pre-publication candidate only when it still
/// verifies against the pinned snapshot.
///
/// Process crashes are tested cut points; Windows directory-entry durability
/// after power loss is not claimed.
pub fn migrate_v1_to_v2(
    source: &Path,
    destination: &Path,
) -> std::result::Result<MigrationReport, MigrationError> {
    run_migration(source, destination, Publication::HardLink, &crash_observer)
}

/// Debug-build fault injection for the process-level cut-point tests.
///
/// Setting `NARRATIVE_UNIVERSE_CRASH_AT` to one of `candidate-built`,
/// `candidate-verified`, `published`, `destination-verified`,
/// `destination-rechecked`, `alias-verified`, or `cleaned-up` aborts the
/// process at that boundary. Release builds compile this out.
fn crash_observer(point: Checkpoint, _view: CheckpointView<'_>) -> Result<()> {
    #[cfg(debug_assertions)]
    {
        let name = match point {
            Checkpoint::CandidateBuilt => "candidate-built",
            Checkpoint::CandidateVerified => "candidate-verified",
            Checkpoint::Published => "published",
            Checkpoint::DestinationVerified => "destination-verified",
            Checkpoint::DestinationRechecked => "destination-rechecked",
            Checkpoint::AliasVerified => "alias-verified",
            Checkpoint::CleanedUp => "cleaned-up",
        };
        if std::env::var("NARRATIVE_UNIVERSE_CRASH_AT").ok().as_deref() == Some(name) {
            std::process::abort();
        }
    }
    Ok(())
}

/// Verifies a general version 2 database without migrating or upgrading it.
///
/// The check is read-only. It accepts edited databases with nonzero page
/// revisions and nonempty, geometrically valid, correctly referenced
/// mentions. It rejects an unsupported version, a foreign schema, a hot
/// sidecar, broken integrity, broken foreign keys, malformed identifiers, and
/// malformed mention geometry. It never calls `Store::open`, so verification
/// cannot silently create or upgrade a database.
pub fn verify_v2(path: &Path) -> std::result::Result<VerificationReport, VerificationError> {
    let facts = inspect_v2_database(path, "database").map_err(|cause| VerificationError {
        path: path.to_path_buf(),
        cause,
    })?;
    Ok(VerificationReport {
        universe_id: facts.snapshot.universe_id,
        counts: facts.counts,
        mentions: facts.mentions,
    })
}

/// C1's candidate-only orchestration, retained for the migration tests.
///
/// Production publication goes through [`migrate_v1_to_v2`], which uses the
/// same lifecycle and keeps the source pinned through the final check.
#[cfg(test)]
pub(crate) fn build_verified_candidate(
    source: &Path,
    candidate: &Path,
) -> std::result::Result<MigrationReport, MigrationError> {
    run_migration(
        source,
        candidate,
        Publication::CandidateOnly,
        &|_, _| Ok(()),
    )
}

fn run_migration(
    source: &Path,
    destination: &Path,
    publication: Publication,
    observer: &dyn Fn(Checkpoint, CheckpointView<'_>) -> Result<()>,
) -> std::result::Result<MigrationReport, MigrationError> {
    let mut state = RunState::default();
    match run_migration_inner(source, destination, publication, observer, &mut state) {
        Ok(report) => Ok(report),
        Err(mut error) => {
            cleanup_failed_run(&mut state, &mut error);
            Err(error)
        }
    }
}

fn run_migration_inner(
    source: &Path,
    destination: &Path,
    publication: Publication,
    observer: &dyn Fn(Checkpoint, CheckpointView<'_>) -> Result<()>,
    state: &mut RunState,
) -> std::result::Result<MigrationReport, MigrationError> {
    let observation = preflight_source(source).map_err(|cause| {
        migration_failure(
            MigrationStage::PreflightSource,
            source,
            destination,
            state,
            cause,
        )
    })?;

    preflight_destination(source, destination).map_err(|cause| {
        migration_failure(
            MigrationStage::PreflightDestination,
            source,
            destination,
            state,
            cause,
        )
    })?;

    let pinned = PinnedSource::open(source, &observation).map_err(|cause| {
        migration_failure(
            MigrationStage::ReadSource,
            source,
            destination,
            state,
            cause,
        )
    })?;

    let snapshot = pinned.snapshot().map_err(|cause| {
        migration_failure(
            MigrationStage::SnapshotSource,
            source,
            destination,
            state,
            cause,
        )
    })?;
    // Cleanup uses the captured snapshot to prove that a pre-publication
    // candidate path still names the database this run built before deleting it.
    state.snapshot = Some(snapshot.clone());
    require_source_observation(source, &observation).map_err(|cause| {
        migration_failure(
            MigrationStage::SnapshotSource,
            source,
            destination,
            state,
            cause,
        )
    })?;

    let candidate = match publication {
        Publication::CandidateOnly => destination.to_path_buf(),
        Publication::HardLink => build_private_candidate(source, destination, &snapshot, state)?,
    };
    state.candidate = Some(candidate.clone());

    if publication == Publication::CandidateOnly {
        let store = Store::import_new(&candidate, snapshot.clone()).map_err(|cause| {
            migration_failure(
                MigrationStage::BuildCandidate,
                source,
                destination,
                state,
                cause,
            )
        })?;
        drop(store);
    }

    let candidate_observation = flush_candidate(&candidate).map_err(|cause| {
        migration_failure(
            MigrationStage::BuildCandidate,
            source,
            destination,
            state,
            cause,
        )
    })?;
    observer(
        Checkpoint::CandidateBuilt,
        CheckpointView {
            candidate: &candidate,
            destination,
        },
    )
    .map_err(|cause| {
        migration_failure(
            MigrationStage::BuildCandidate,
            source,
            destination,
            state,
            cause,
        )
    })?;

    verify_migration_database(&candidate, &snapshot, "candidate").map_err(|cause| {
        migration_failure(
            MigrationStage::VerifyCandidate,
            source,
            destination,
            state,
            cause,
        )
    })?;
    require_candidate_observation(&candidate, &candidate_observation).map_err(|cause| {
        migration_failure(
            MigrationStage::VerifyCandidate,
            source,
            destination,
            state,
            cause,
        )
    })?;
    require_source_observation(source, &observation).map_err(|cause| {
        migration_failure(
            MigrationStage::VerifyCandidate,
            source,
            destination,
            state,
            cause,
        )
    })?;
    observer(
        Checkpoint::CandidateVerified,
        CheckpointView {
            candidate: &candidate,
            destination,
        },
    )
    .map_err(|cause| {
        migration_failure(
            MigrationStage::VerifyCandidate,
            source,
            destination,
            state,
            cause,
        )
    })?;

    let mut destination_observation = None;
    let mut residuals = Vec::new();
    if publication == Publication::HardLink {
        preflight_destination(source, destination).map_err(|cause| {
            migration_failure(
                MigrationStage::PublishDestination,
                source,
                destination,
                state,
                cause,
            )
        })?;
        // Recheck the candidate immediately before the link. The bytes that
        // become visible are then the bytes of a freshly verified database,
        // not merely a database verified earlier in the run.
        verify_migration_database(&candidate, &snapshot, "candidate").map_err(|cause| {
            migration_failure(
                MigrationStage::VerifyCandidate,
                source,
                destination,
                state,
                cause,
            )
        })?;
        require_source_observation(source, &observation).map_err(|cause| {
            migration_failure(
                MigrationStage::PublishDestination,
                source,
                destination,
                state,
                cause,
            )
        })?;
        publish(&candidate, destination).map_err(|cause| {
            migration_failure(
                MigrationStage::PublishDestination,
                source,
                destination,
                state,
                cause,
            )
        })?;
        state.published = true;
        observer(
            Checkpoint::Published,
            CheckpointView {
                candidate: &candidate,
                destination,
            },
        )
        .map_err(|cause| {
            migration_failure(
                MigrationStage::VerifyDestination,
                source,
                destination,
                state,
                cause,
            )
        })?;

        let observed = read_observation(destination)
            .map(|(header, len)| SourceObservation { header, len })
            .map_err(|cause| {
                migration_failure(
                    MigrationStage::VerifyDestination,
                    source,
                    destination,
                    state,
                    cause,
                )
            })?;
        destination_observation = Some(observed.clone());
        verify_migration_database(destination, &snapshot, "destination").map_err(|cause| {
            migration_failure(
                MigrationStage::VerifyDestination,
                source,
                destination,
                state,
                cause,
            )
        })?;
        require_destination_observation(destination, &observed).map_err(|cause| {
            migration_failure(
                MigrationStage::VerifyDestination,
                source,
                destination,
                state,
                cause,
            )
        })?;
        require_source_observation(source, &observation).map_err(|cause| {
            migration_failure(
                MigrationStage::VerifyDestination,
                source,
                destination,
                state,
                cause,
            )
        })?;
        observer(
            Checkpoint::DestinationVerified,
            CheckpointView {
                candidate: &candidate,
                destination,
            },
        )
        .map_err(|cause| {
            migration_failure(
                MigrationStage::VerifyDestination,
                source,
                destination,
                state,
                cause,
            )
        })?;

        // Final success gate while the source is still pinned: verify the
        // published destination again. The alias is not removed yet. It is
        // removed only after the last fallible destination observation, which
        // happens after the source is released, so a destination that
        // disappears in between cannot leave the migrated data with no name.
        verify_migration_database(destination, &snapshot, "destination").map_err(|cause| {
            migration_failure(
                MigrationStage::VerifyDestination,
                source,
                destination,
                state,
                cause,
            )
        })?;
        require_destination_observation(destination, &observed).map_err(|cause| {
            migration_failure(
                MigrationStage::VerifyDestination,
                source,
                destination,
                state,
                cause,
            )
        })?;

        observer(
            Checkpoint::DestinationRechecked,
            CheckpointView {
                candidate: &candidate,
                destination,
            },
        )
        .map_err(|cause| {
            migration_failure(
                MigrationStage::VerifyDestination,
                source,
                destination,
                state,
                cause,
            )
        })?;
    }

    pinned.finish(source, &observation).map_err(|cause| {
        migration_failure(
            MigrationStage::ReleaseSource,
            source,
            destination,
            state,
            cause,
        )
    })?;
    require_source_observation(source, &observation).map_err(|cause| {
        migration_failure(
            MigrationStage::ReleaseSource,
            source,
            destination,
            state,
            cause,
        )
    })?;

    if publication == Publication::HardLink {
        let observed = destination_observation
            .as_ref()
            .expect("a published destination has an observation");
        // Verify the alias before the last destination observation. Ordering
        // matters: the observation must be the last fallible check before the
        // unlink, so a destination removed during the full alias scan is
        // detected while the alias still names the complete database.
        let alias_verified = verify_migration_database(&candidate, &snapshot, "candidate").is_ok();
        observer(
            Checkpoint::AliasVerified,
            CheckpointView {
                candidate: &candidate,
                destination,
            },
        )
        .map_err(|cause| {
            migration_failure(
                MigrationStage::CleanupCandidate,
                source,
                destination,
                state,
                cause,
            )
        })?;
        require_destination_observation(destination, observed).map_err(|cause| {
            migration_failure(
                MigrationStage::VerifyDestination,
                source,
                destination,
                state,
                cause,
            )
        })?;
        // The last destination observation passed. Remove the alias only when
        // it still reproduced the migrated database immediately before that
        // observation. A changed, replaced, or unremovable alias is kept and
        // reported; a stale sidecar is reported and never scanned for.
        if alias_verified && std::fs::remove_file(&candidate).is_ok() {
            state.candidate = None;
        }
        residuals = residual_files(&candidate);
        observer(
            Checkpoint::CleanedUp,
            CheckpointView {
                candidate: &candidate,
                destination,
            },
        )
        .map_err(|cause| {
            migration_failure(
                MigrationStage::CleanupCandidate,
                source,
                destination,
                state,
                cause,
            )
        })?;
    }

    Ok(MigrationReport {
        universe_id: snapshot.universe_id.clone(),
        counts: counts_of(&snapshot),
        destination: destination.to_path_buf(),
        residuals,
    })
}

fn migration_failure(
    stage: MigrationStage,
    source: &Path,
    destination: &Path,
    state: &RunState,
    cause: Error,
) -> MigrationError {
    MigrationError {
        stage,
        source: source.to_path_buf(),
        destination: destination.to_path_buf(),
        residuals: state.residuals(destination),
        cause: Box::new(cause),
    }
}

fn cleanup_failed_run(state: &mut RunState, error: &mut MigrationError) {
    // Before publication, this run owns the candidate only while the path still
    // holds the database built from the pinned snapshot. Re-verifying before
    // deleting means a path that was replaced is reported, never removed.
    //
    // After a successful link the candidate is a second name for the published
    // destination. It is never removed here: the destination may have been
    // removed or replaced, and the alias may be the only complete copy left.
    if !state.published {
        let owned = match (&state.candidate, &state.snapshot) {
            (Some(candidate), Some(snapshot)) => {
                verify_migration_database(candidate, snapshot, "candidate").is_ok()
            }
            _ => false,
        };
        if owned {
            if let Some(candidate) = &state.candidate {
                let _ = std::fs::remove_file(candidate);
            }
        }
    }
    // The destination is never removed here: after a successful link it is the
    // only complete copy, and before a link this run did not create it.
    error.residuals = state.residuals(&error.destination);
}

fn counts_of(snapshot: &Snapshot) -> Counts {
    Counts {
        pages: snapshot.pages.len() as u64,
        types: snapshot.types.len() as u64,
        entities: snapshot.entities.len() as u64,
        definitions: snapshot.definitions.len() as u64,
        assertions: snapshot.assertions.len() as u64,
    }
}

fn build_private_candidate(
    source: &Path,
    destination: &Path,
    snapshot: &Snapshot,
    state: &mut RunState,
) -> std::result::Result<PathBuf, MigrationError> {
    for _ in 0..CANDIDATE_ATTEMPTS {
        let candidate = candidate_path(source, destination).map_err(|cause| {
            migration_failure(
                MigrationStage::BuildCandidate,
                source,
                destination,
                state,
                cause,
            )
        })?;
        // Record the path before reserving it, so a failure after `create_new`
        // still knows which file this run owns and may remove.
        state.candidate = Some(candidate.clone());
        match Store::import_new_private(&candidate, snapshot.clone()) {
            Ok(_) => return Ok(candidate),
            Err(Error::Validation {
                reason: ValidationReason::PathExists,
                ..
            }) => {
                // This name was taken before the reservation; it is not ours.
                state.candidate = None;
            }
            Err(cause) => {
                return Err(migration_failure(
                    MigrationStage::BuildCandidate,
                    source,
                    destination,
                    state,
                    cause,
                ))
            }
        }
    }
    Err(migration_failure(
        MigrationStage::BuildCandidate,
        source,
        destination,
        state,
        Error::Storage {
            message: "could not reserve a unique candidate file".to_string(),
        },
    ))
}

/// Flushes the candidate and returns its exact header and length.
///
/// The store is already closed when this runs. The extra handle is opened for
/// writing only to call `sync_all`; no SQLite page is written through it.
fn flush_candidate(path: &Path) -> Result<SourceObservation> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|err| io_error("open", path, err))?;
    file.sync_all()
        .map_err(|err| io_error("flush", path, err))?;
    drop(file);
    let (header, len) = read_observation(path)?;
    Ok(SourceObservation { header, len })
}

/// Publishes a verified candidate with one atomic no-overwrite hard link.
///
/// The candidate lives in the destination directory, so both names are on the
/// same filesystem. A filesystem without hard-link support fails closed; there
/// is no copy or rename fallback, and an existing destination is never
/// replaced.
fn publish(candidate: &Path, destination: &Path) -> Result<()> {
    match std::fs::hard_link(candidate, destination) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Err(Error::validation(
            ValidationReason::PathExists,
            "destination",
            None,
        )),
        Err(err) => Err(Error::Io {
            message: format!(
                "publish {} as {} with a no-overwrite hard link: {err}; \
                 the filesystem must support hard links",
                candidate.display(),
                destination.display()
            ),
        }),
    }
}

fn preflight_source(path: &Path) -> Result<SourceObservation> {
    require_regular_file(path, "source")?;
    require_no_sidecars(path)?;
    let (header, len) = read_observation(path)?;
    validate_legacy_header(&header, len)?;
    require_source_observation(path, &SourceObservation { header, len })?;
    Ok(SourceObservation { header, len })
}

fn preflight_destination(source: &Path, destination: &Path) -> Result<()> {
    if destination.file_name().is_none() {
        return Err(Error::validation(
            ValidationReason::PathMissing,
            "destination",
            None,
        ));
    }
    let parent = destination_parent(destination);
    let parent_metadata = match std::fs::metadata(parent) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::validation(
                ValidationReason::PathMissing,
                "destination_parent",
                None,
            ))
        }
        Err(err) => return Err(io_error("inspect", parent, err)),
    };
    if !parent_metadata.is_dir() {
        return Err(Error::validation(
            ValidationReason::PathMissing,
            "destination_parent",
            None,
        ));
    }
    match std::fs::symlink_metadata(destination) {
        Ok(_) => {
            return Err(Error::validation(
                ValidationReason::PathExists,
                "destination",
                None,
            ))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(io_error("inspect", destination, err)),
    }
    require_no_sidecars(destination)?;
    let resolved_destination = resolve_new_path(destination)?;
    let canonical_source = std::fs::canonicalize(source).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            Error::validation(ValidationReason::PathMissing, "source", None)
        } else {
            io_error("resolve", source, err)
        }
    })?;
    let collides_with_source = same_path(&resolved_destination, &canonical_source);
    let collides_with_sidecar = SIDECARS.iter().any(|suffix| {
        same_path(
            &resolved_destination,
            &sidecar_path(&canonical_source, suffix),
        )
    });
    if collides_with_source || collides_with_sidecar {
        return Err(Error::validation(
            ValidationReason::PathExists,
            "destination",
            None,
        ));
    }
    Ok(())
}

fn destination_parent(destination: &Path) -> &Path {
    destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

/// Chooses one unique, dot-prefixed candidate name in the destination directory.
///
/// The name carries the process id, a clock-derived nonce, and an atomic
/// sequence. It is only a proposal: `Store::import_new_private` reserves it
/// with `create_new`, and the caller retries on a real collision.
fn candidate_path(source: &Path, destination: &Path) -> Result<PathBuf> {
    let parent = destination_parent(destination);
    let canonical_parent = std::fs::canonicalize(parent).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            Error::validation(ValidationReason::PathMissing, "destination_parent", None)
        } else {
            io_error("resolve", parent, err)
        }
    })?;
    let canonical_source = std::fs::canonicalize(source).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            Error::validation(ValidationReason::PathMissing, "source", None)
        } else {
            io_error("resolve", source, err)
        }
    })?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let sequence = CANDIDATE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = format!(
        "{CANDIDATE_PREFIX}{}-{nanos:x}-{sequence:x}",
        std::process::id()
    );
    let path = canonical_parent.join(name);
    let collides_with_source = same_path(&path, &canonical_source)
        || SIDECARS
            .iter()
            .any(|suffix| same_path(&path, &sidecar_path(&canonical_source, suffix)));
    if collides_with_source {
        return Err(Error::Storage {
            message: "could not choose a candidate name outside the source".to_string(),
        });
    }
    Ok(path)
}

/// Resolves the directory and the final component of a path that does not
/// exist yet. `canonicalize` cannot be called on the path itself, but it
/// resolves `..` and symlinked parents, and the final name is normalized for
/// the alias rules of the host filesystem.
fn resolve_new_path(path: &Path) -> Result<PathBuf> {
    let parent = destination_parent(path);
    let canonical_parent = std::fs::canonicalize(parent).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            Error::validation(ValidationReason::PathMissing, "destination_parent", None)
        } else {
            io_error("resolve", parent, err)
        }
    })?;
    let name = path
        .file_name()
        .ok_or_else(|| Error::validation(ValidationReason::PathMissing, "destination", None))?;
    Ok(canonical_parent.join(normalized_new_name(name)?))
}

#[cfg(windows)]
fn normalized_new_name(name: &std::ffi::OsStr) -> Result<std::ffi::OsString> {
    let text = name.to_string_lossy();
    let trimmed = text.trim_end_matches(['.', ' ']);
    if trimmed.is_empty() || trimmed != text.as_ref() {
        return Err(Error::validation(
            ValidationReason::PathMissing,
            "destination",
            None,
        ));
    }
    if trimmed.contains(':') || is_windows_reserved_name(trimmed) {
        return Err(Error::validation(
            ValidationReason::PathMissing,
            "destination",
            None,
        ));
    }
    Ok(name.to_os_string())
}

#[cfg(not(windows))]
fn normalized_new_name(name: &std::ffi::OsStr) -> Result<std::ffi::OsString> {
    Ok(name.to_os_string())
}

#[cfg(windows)]
fn is_windows_reserved_name(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end_matches(['.', ' '])
        .to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    let bytes = stem.as_bytes();
    bytes.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && bytes[3].is_ascii_digit()
        && bytes[3] != b'0'
}

#[cfg(any(windows, target_os = "macos"))]
fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.as_os_str().to_string_lossy();
    let right = right.as_os_str().to_string_lossy();
    left.eq_ignore_ascii_case(right.as_ref())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn same_path(left: &Path, right: &Path) -> bool {
    left == right
}

/// Keeps the source read transaction alive across candidate construction,
/// verification, publication, and post-publication destination verification.
/// Dropping the guard rolls the transaction back.
struct PinnedSource {
    conn: Connection,
    transaction: bool,
}

impl PinnedSource {
    fn open(path: &Path, observation: &SourceObservation) -> Result<Self> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|err| map_sql("open legacy source", err))?;
        conn.busy_timeout(SOURCE_BUSY_TIMEOUT)
            .map_err(|err| map_sql("busy_timeout", err))?;
        conn.execute_batch("PRAGMA query_only = ON")
            .map_err(|err| map_sql("query_only", err))?;
        require_query_only(&conn)?;
        conn.execute_batch("PRAGMA foreign_keys = ON")
            .map_err(|err| map_sql("foreign_keys", err))?;
        require_foreign_keys_enabled(&conn)?;
        require_source_observation(path, observation)?;

        conn.execute_batch("BEGIN DEFERRED")
            .map_err(|err| map_sql("begin", err))?;
        let pinned = Self {
            conn,
            transaction: true,
        };
        // BEGIN DEFERRED alone does not take a snapshot. The first real read
        // establishes it, so it happens before any validation query.
        pinned
            .conn
            .query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|err| map_sql("pin snapshot", err))?;
        require_source_observation(path, observation)?;
        Ok(pinned)
    }

    fn snapshot(&self) -> Result<Snapshot> {
        read_legacy_snapshot(&self.conn)
    }

    fn finish(mut self, path: &Path, observation: &SourceObservation) -> Result<()> {
        require_source_observation(path, observation)?;
        self.conn
            .execute_batch("ROLLBACK")
            .map_err(|err| map_sql("rollback", err))?;
        self.transaction = false;
        require_source_observation(path, observation)?;
        Ok(())
    }
}

impl Drop for PinnedSource {
    fn drop(&mut self) {
        if self.transaction {
            let _ = self.conn.execute_batch("ROLLBACK");
        }
    }
}

fn read_legacy_snapshot(conn: &Connection) -> Result<Snapshot> {
    let user_version: i32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|err| map_sql("user_version", err))?;
    if user_version != LEGACY_USER_VERSION {
        return Err(Error::UnsupportedVersion {
            found: user_version,
        });
    }
    let encoding: String = conn
        .query_row("PRAGMA encoding", [], |row| row.get(0))
        .map_err(|err| map_sql("encoding", err))?;
    if encoding != "UTF-8" {
        return Err(Error::validation(
            ValidationReason::ForeignDatabase,
            "encoding",
            None,
        ));
    }

    let _variant = detect_legacy_variant(conn)?;
    let universe_id = read_legacy_metadata(conn)?;
    require_integrity(conn)?;
    require_foreign_keys(conn)?;

    let mut snapshot = Snapshot {
        universe_id,
        pages: read_legacy_pages(conn)?,
        types: read_legacy_types(conn)?,
        entities: read_legacy_entities(conn)?,
        definitions: read_legacy_definitions(conn)?,
        assertions: read_legacy_assertions(conn)?,
    };
    validate_logical_snapshot(&snapshot)?;
    canonicalize_snapshot(&mut snapshot);
    Ok(snapshot)
}

fn read_legacy_metadata(conn: &Connection) -> Result<UniverseId> {
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM metadata", [], |row| row.get(0))
        .map_err(|err| map_sql("metadata", err))?;
    if rows == 0 {
        return Err(Error::validation(
            ValidationReason::MissingMetadata,
            "metadata",
            None,
        ));
    }
    if rows != 1 {
        return Err(Error::validation(
            ValidationReason::ForeignDatabase,
            "metadata",
            None,
        ));
    }
    let (singleton, universe_id, format_version) = conn
        .query_row(
            "SELECT singleton, universe_id, format_version FROM metadata",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .map_err(|err| map_sql("metadata", err))?;
    if singleton != 1 {
        return Err(Error::validation(
            ValidationReason::ForeignDatabase,
            "metadata",
            None,
        ));
    }
    if format_version != LEGACY_FORMAT_VERSION {
        return Err(Error::validation(
            ValidationReason::ForeignDatabase,
            "format_version",
            None,
        ));
    }
    UniverseId::parse(&universe_id)
        .map_err(|_| Error::validation(ValidationReason::InvalidId, "universe_id", None))
}

fn read_legacy_pages(conn: &Connection) -> Result<Vec<PageSnapshot>> {
    let mut statement = conn
        .prepare("SELECT id, title, body FROM pages ORDER BY id")
        .map_err(|err| map_sql("prepare pages", err))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|err| map_sql("read pages", err))?;
    let mut pages = Vec::new();
    for row in rows {
        let (id, title, body) = row.map_err(|err| map_sql("page row", err))?;
        let id = required_id("page", id, PageId::parse)?;
        pages.push(PageSnapshot {
            page: Page { id, title, body },
            mentions: Vec::new(),
        });
    }
    Ok(pages)
}

fn read_legacy_types(conn: &Connection) -> Result<Vec<EntityType>> {
    let mut statement = conn
        .prepare("SELECT id, label FROM types ORDER BY id")
        .map_err(|err| map_sql("prepare types", err))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|err| map_sql("read types", err))?;
    let mut types = Vec::new();
    for row in rows {
        let (id, label) = row.map_err(|err| map_sql("type row", err))?;
        types.push(EntityType {
            id: required_id("type", id, TypeId::parse)?,
            label,
        });
    }
    Ok(types)
}

fn read_legacy_entities(conn: &Connection) -> Result<Vec<Entity>> {
    let mut statement = conn
        .prepare("SELECT id, label, type_id, page_id FROM entities ORDER BY id")
        .map_err(|err| map_sql("prepare entities", err))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|err| map_sql("read entities", err))?;
    let mut entities = Vec::new();
    for row in rows {
        let (id, label, type_id, page_id) = row.map_err(|err| map_sql("entity row", err))?;
        entities.push(Entity {
            id: required_id("entity", id, EntityId::parse)?,
            label,
            type_id: optional_id("entity type", type_id, TypeId::parse)?,
            page_id: optional_id("entity page", page_id, PageId::parse)?,
        });
    }
    Ok(entities)
}

fn read_legacy_definitions(conn: &Connection) -> Result<Vec<Definition>> {
    let mut statement = conn
        .prepare(
            "SELECT id, owner_type_id, owner_entity_id, value_class, cardinality, archived
             FROM definitions ORDER BY id",
        )
        .map_err(|err| map_sql("prepare definitions", err))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(|err| map_sql("read definitions", err))?;
    let mut definitions = Vec::new();
    for row in rows {
        let (id, owner_type, owner_entity, value_class, cardinality, archived) =
            row.map_err(|err| map_sql("definition row", err))?;
        let id = required_id("definition", id, DefinitionId::parse)?;
        let owner = match (owner_type, owner_entity) {
            (Some(raw), None) => Owner::Type(TypeId::parse(&raw).map_err(|_| {
                Error::validation(ValidationReason::InvalidId, "owner_type_id", None)
            })?),
            (None, Some(raw)) => Owner::Entity(EntityId::parse(&raw).map_err(|_| {
                Error::validation(ValidationReason::InvalidId, "owner_entity_id", None)
            })?),
            _ => {
                return Err(Error::Storage {
                    message: format!("invalid definition owner {}", id.as_str()),
                })
            }
        };
        definitions.push(Definition {
            id,
            owner,
            value_class: ValueClass::parse(&value_class)?,
            cardinality: Cardinality::parse(&cardinality)?,
            archived: read_flag(archived, "archived")?,
        });
    }
    Ok(definitions)
}

fn read_legacy_assertions(conn: &Connection) -> Result<Vec<Assertion>> {
    let sql = format!("SELECT {ASSERTION_COLUMNS} FROM assertions ORDER BY id");
    let mut statement = conn
        .prepare(&sql)
        .map_err(|err| map_sql("prepare assertions", err))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, Option<String>>(12)?,
                row.get::<_, Option<String>>(13)?,
                row.get::<_, i64>(14)?,
            ))
        })
        .map_err(|err| map_sql("read assertions", err))?;
    let mut assertions = Vec::new();
    for row in rows {
        let parts = row.map_err(|err| map_sql("assertion row", err))?;
        let id = required_id("assertion", parts.0, AssertionId::parse)?;
        let subject_id = required_id("subject", parts.1, EntityId::parse)?;
        let definition_id = required_id("definition", parts.2, DefinitionId::parse)?;
        assertions.push(assertion_from_parts(AssertionParts {
            id: id.as_str().to_string(),
            subject_id: subject_id.as_str().to_string(),
            definition_id: definition_id.as_str().to_string(),
            value_kind: parts.3,
            text_value: parts.4,
            bool_value: parts.5,
            target_entity_id: parts.6,
            canon: parts.7,
            source_status_raw: parts.8,
            provenance_external_ref: parts.9,
            provenance_locator: parts.10,
            provenance_fragment: parts.11,
            provenance_source_revision: parts.12,
            scope_note: parts.13,
            archived: parts.14,
        })?);
    }
    Ok(assertions)
}

fn validate_logical_snapshot(snapshot: &Snapshot) -> Result<()> {
    ensure_unique_snapshot(snapshot)?;

    let mut pages = BTreeMap::new();
    for page in &snapshot.pages {
        pages.insert(page.page.id.as_str(), &page.page);
    }
    let mut types = BTreeMap::new();
    for entity_type in &snapshot.types {
        types.insert(entity_type.id.as_str(), entity_type);
    }
    let mut entities = BTreeMap::new();
    for entity in &snapshot.entities {
        entities.insert(entity.id.as_str(), entity);
    }
    let mut definitions = BTreeMap::new();
    for definition in &snapshot.definitions {
        definitions.insert(definition.id.as_str(), definition);
    }

    for entity in &snapshot.entities {
        if let Some(type_id) = &entity.type_id {
            if !types.contains_key(type_id.as_str()) {
                return Err(Error::validation(
                    ValidationReason::UnknownType,
                    "type_id",
                    Some(type_id.as_str()),
                ));
            }
        }
        if let Some(page_id) = &entity.page_id {
            if !pages.contains_key(page_id.as_str()) {
                return Err(Error::validation(
                    ValidationReason::UnknownPage,
                    "page_id",
                    Some(page_id.as_str()),
                ));
            }
        }
    }

    for definition in &snapshot.definitions {
        match &definition.owner {
            Owner::Type(type_id) => {
                if !types.contains_key(type_id.as_str()) {
                    return Err(Error::validation(
                        ValidationReason::UnknownType,
                        "owner_type_id",
                        Some(type_id.as_str()),
                    ));
                }
            }
            Owner::Entity(entity_id) => {
                if !entities.contains_key(entity_id.as_str()) {
                    return Err(Error::validation(
                        ValidationReason::UnknownEntity,
                        "owner_entity_id",
                        Some(entity_id.as_str()),
                    ));
                }
            }
        }
    }

    for assertion in &snapshot.assertions {
        let subject = entities.get(assertion.subject_id.as_str()).ok_or_else(|| {
            Error::validation(
                ValidationReason::UnknownSubject,
                "subject_id",
                Some(assertion.subject_id.as_str()),
            )
        })?;
        let definition = definitions
            .get(assertion.definition_id.as_str())
            .ok_or_else(|| {
                Error::validation(
                    ValidationReason::UnknownDefinition,
                    "definition_id",
                    Some(assertion.definition_id.as_str()),
                )
            })?;
        if !value_fits(definition.value_class, &assertion.value) {
            return Err(Error::validation(
                ValidationReason::ValueMismatch,
                "value",
                Some(assertion.id.as_str()),
            ));
        }
        let applicable = match &definition.owner {
            Owner::Entity(owner) => owner == &subject.id,
            Owner::Type(owner) => subject.type_id.as_ref() == Some(owner),
        };
        if !applicable {
            return Err(Error::validation(
                ValidationReason::NotApplicable,
                "definition_id",
                Some(definition.id.as_str()),
            ));
        }
        if let Value::Reference(target) = &assertion.value {
            if !entities.contains_key(target.as_str()) {
                return Err(Error::validation(
                    ValidationReason::UnknownTarget,
                    "target_entity_id",
                    Some(target.as_str()),
                ));
            }
        }
    }
    Ok(())
}

fn canonicalize_snapshot(snapshot: &mut Snapshot) {
    snapshot
        .pages
        .sort_by(|left, right| left.page.id.as_str().cmp(right.page.id.as_str()));
    snapshot
        .types
        .sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    snapshot
        .entities
        .sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    snapshot
        .definitions
        .sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    snapshot
        .assertions
        .sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
}

struct DatabaseFacts {
    snapshot: Snapshot,
    counts: Counts,
    mentions: u64,
    revised_pages: u64,
}

/// Runs the checks shared by general verification and migration verification.
///
/// It never opens a write handle and never calls `Store::open`, so it cannot
/// change a database or trigger an upgrade. The returned snapshot is the typed,
/// canonical page, type, entity, definition, and assertion reading, including
/// mention geometry; page revisions are validated separately and counted in
/// `DatabaseFacts::revised_pages`.
fn inspect_v2_database(path: &Path, field: &'static str) -> Result<DatabaseFacts> {
    require_regular_file(path, field)?;
    require_no_sidecars(path)?;
    let (header, len) = read_observation(path)?;
    validate_candidate_header(&header, len)?;
    let observation = SourceObservation { header, len };

    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|err| map_sql("open database", err))?;
    conn.busy_timeout(SOURCE_BUSY_TIMEOUT)
        .map_err(|err| map_sql("busy_timeout", err))?;
    conn.execute_batch("PRAGMA query_only = ON")
        .map_err(|err| map_sql("query_only", err))?;
    require_query_only(&conn)?;
    conn.execute_batch("PRAGMA foreign_keys = ON")
        .map_err(|err| map_sql("foreign_keys", err))?;
    require_foreign_keys_enabled(&conn)?;
    require_observed(path, field, &observation)?;

    conn.execute_batch("BEGIN DEFERRED")
        .map_err(|err| map_sql("begin database", err))?;
    conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| {
        row.get::<_, i64>(0)
    })
    .map_err(|err| map_sql("pin database snapshot", err))?;
    require_observed(path, field, &observation)?;

    let user_version: i32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|err| map_sql("user_version", err))?;
    if user_version != CURRENT_USER_VERSION {
        return Err(Error::UnsupportedVersion {
            found: user_version,
        });
    }
    let encoding: String = conn
        .query_row("PRAGMA encoding", [], |row| row.get(0))
        .map_err(|err| map_sql("encoding", err))?;
    if encoding != "UTF-8" {
        return Err(Error::validation(
            ValidationReason::ForeignDatabase,
            "encoding",
            None,
        ));
    }

    verify_candidate_schema(&conn)?;
    require_candidate_metadata(&conn)?;
    require_integrity(&conn)?;
    require_foreign_keys(&conn)?;

    let snapshot = store::read_snapshot(&conn)?;
    let counts = counts_from_connection(&conn)?;
    let mentions = count_table(&conn, "mentions")?;
    let revised_pages = validate_page_revisions(&conn)?;

    require_observed(path, field, &observation)?;
    conn.execute_batch("ROLLBACK")
        .map_err(|err| map_sql("rollback database", err))?;
    require_observed(path, field, &observation)?;
    Ok(DatabaseFacts {
        snapshot,
        counts,
        mentions,
        revised_pages,
    })
}

#[cfg(test)]
fn verify_candidate(candidate: &Path, expected: &Snapshot) -> Result<()> {
    verify_migration_database(candidate, expected, "candidate")
}

/// Applies the strict migration contract on top of general verification.
///
/// A migrated database must still equal the pinned legacy snapshot exactly,
/// with zero page revisions and zero occurrences. The published destination is
/// checked with the same function as the candidate.
fn verify_migration_database(path: &Path, expected: &Snapshot, field: &'static str) -> Result<()> {
    let facts = inspect_v2_database(path, field)?;
    if facts.mentions != 0 {
        return Err(Error::Storage {
            message: format!("{field} has {} occurrences", facts.mentions),
        });
    }
    if facts.revised_pages != 0 {
        return Err(Error::Storage {
            message: format!("{field} has {} revised pages", facts.revised_pages),
        });
    }
    if facts.counts != counts_of(expected) {
        return Err(Error::Storage {
            message: format!("{field} record counts differ from the legacy snapshot"),
        });
    }
    if &facts.snapshot != expected {
        return Err(Error::Storage {
            message: format!("{field} records differ from the legacy snapshot"),
        });
    }
    Ok(())
}

/// Reads every revision as a `PageRevision` and counts nonzero ones.
///
/// The SQL schema already constrains the column, but a database can be edited
/// with checks disabled. General verification accepts any valid nonnegative
/// revision; migration verification additionally requires that the count be
/// zero.
fn validate_page_revisions(conn: &Connection) -> Result<u64> {
    let mut statement = conn
        .prepare("SELECT revision FROM pages ORDER BY id")
        .map_err(|err| map_sql("prepare revisions", err))?;
    let rows = statement
        .query_map([], |row| row.get::<_, i64>(0))
        .map_err(|err| map_sql("query revisions", err))?;
    let mut revised = 0u64;
    for row in rows {
        let raw = row.map_err(|err| map_sql("revision row", err))?;
        let revision = PageRevision::new(raw)?;
        if revision != PageRevision::ZERO {
            revised += 1;
        }
    }
    Ok(revised)
}

fn require_observed(path: &Path, field: &'static str, expected: &SourceObservation) -> Result<()> {
    match field {
        "candidate" => require_candidate_observation(path, expected),
        "destination" => require_destination_observation(path, expected),
        _ => require_file_observation(path, field, expected, database_changed),
    }
}

fn require_candidate_metadata(conn: &Connection) -> Result<()> {
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM metadata", [], |row| row.get(0))
        .map_err(|err| map_sql("metadata", err))?;
    if rows != 1 {
        return Err(Error::Storage {
            message: format!("metadata row count is {rows}"),
        });
    }
    let singleton: i64 = conn
        .query_row("SELECT singleton FROM metadata", [], |row| row.get(0))
        .map_err(|err| map_sql("metadata", err))?;
    if singleton != 1 {
        return Err(Error::Storage {
            message: "metadata singleton is not 1".to_string(),
        });
    }
    Ok(())
}

fn counts_from_connection(conn: &Connection) -> Result<Counts> {
    Ok(Counts {
        pages: count_table(conn, "pages")?,
        types: count_table(conn, "types")?,
        entities: count_table(conn, "entities")?,
        definitions: count_table(conn, "definitions")?,
        assertions: count_table(conn, "assertions")?,
    })
}

fn count_table(conn: &Connection, table: &'static str) -> Result<u64> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    let count: i64 = conn
        .query_row(&sql, [], |row| row.get(0))
        .map_err(|err| map_sql("count", err))?;
    u64::try_from(count).map_err(|_| Error::Storage {
        message: format!("negative count in {table}"),
    })
}

fn require_integrity(conn: &Connection) -> Result<()> {
    let mut statement = conn
        .prepare("PRAGMA integrity_check")
        .map_err(|err| map_sql("integrity_check", err))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|err| map_sql("integrity_check", err))?;
    let mut messages = Vec::new();
    for row in rows {
        messages.push(row.map_err(|err| map_sql("integrity_check row", err))?);
    }
    if messages.len() != 1 || messages[0] != "ok" {
        return Err(Error::Storage {
            message: "database failed SQLite integrity_check".to_string(),
        });
    }
    Ok(())
}

fn require_foreign_keys(conn: &Connection) -> Result<()> {
    let mut statement = conn
        .prepare("PRAGMA foreign_key_check")
        .map_err(|err| map_sql("foreign_key_check", err))?;
    let rows = statement
        .query_map([], |_| Ok(()))
        .map_err(|err| map_sql("foreign_key_check", err))?;
    let mut violations = 0u64;
    for row in rows {
        row.map_err(|err| map_sql("foreign_key_check row", err))?;
        violations += 1;
    }
    if violations != 0 {
        return Err(Error::Storage {
            message: format!("database has {violations} foreign key violations"),
        });
    }
    Ok(())
}

fn require_query_only(conn: &Connection) -> Result<()> {
    let enabled: i64 = conn
        .query_row("PRAGMA query_only", [], |row| row.get(0))
        .map_err(|err| map_sql("query_only", err))?;
    if enabled != 1 {
        return Err(Error::Storage {
            message: "query_only pragma stayed off".to_string(),
        });
    }
    Ok(())
}

fn require_foreign_keys_enabled(conn: &Connection) -> Result<()> {
    let enabled: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .map_err(|err| map_sql("foreign_keys", err))?;
    if enabled != 1 {
        return Err(Error::Storage {
            message: "foreign_keys pragma stayed off".to_string(),
        });
    }
    Ok(())
}

fn require_regular_file(path: &Path, field: &'static str) -> Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::validation(
                ValidationReason::PathMissing,
                field,
                None,
            ))
        }
        Err(err) => return Err(io_error("inspect", path, err)),
    };
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(Error::validation(
            ValidationReason::NotADatabase,
            field,
            None,
        ));
    }
    Ok(())
}

fn require_no_sidecars(path: &Path) -> Result<()> {
    for suffix in SIDECARS {
        let sidecar = sidecar_path(path, suffix);
        match std::fs::symlink_metadata(&sidecar) {
            Ok(_) => {
                return Err(Error::validation(
                    ValidationReason::ForeignDatabase,
                    "sidecar",
                    None,
                ))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(io_error("inspect", &sidecar, err)),
        }
    }
    Ok(())
}

fn require_file_observation(
    path: &Path,
    field: &'static str,
    expected: &SourceObservation,
    changed: fn() -> Error,
) -> Result<()> {
    require_regular_file(path, field)?;
    require_no_sidecars(path)?;
    let (header, len) = read_observation(path)?;
    if header != expected.header || len != expected.len {
        return Err(changed());
    }
    Ok(())
}

fn require_source_observation(path: &Path, expected: &SourceObservation) -> Result<()> {
    require_file_observation(path, "source", expected, source_changed)
}

fn require_candidate_observation(path: &Path, expected: &SourceObservation) -> Result<()> {
    require_file_observation(path, "candidate", expected, candidate_changed)
}

fn require_destination_observation(path: &Path, expected: &SourceObservation) -> Result<()> {
    require_file_observation(path, "destination", expected, destination_changed)
}

fn read_observation(path: &Path) -> Result<([u8; HEADER_LEN], u64)> {
    let mut file = std::fs::File::open(path).map_err(|err| io_error("open", path, err))?;
    let mut header = [0u8; HEADER_LEN];
    match file.read_exact(&mut header) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(Error::validation(
                ValidationReason::NotADatabase,
                "path",
                None,
            ))
        }
        Err(err) => return Err(io_error("read header", path, err)),
    }
    let len = file
        .metadata()
        .map_err(|err| io_error("inspect", path, err))?
        .len();
    Ok((header, len))
}

fn validate_legacy_header(header: &[u8; HEADER_LEN], len: u64) -> Result<()> {
    validate_header(header, len, LEGACY_USER_VERSION)
}

fn validate_candidate_header(header: &[u8; HEADER_LEN], len: u64) -> Result<()> {
    validate_header(header, len, CURRENT_USER_VERSION)
}

fn validate_header(header: &[u8; HEADER_LEN], len: u64, expected_user: i32) -> Result<()> {
    if &header[..16] != MAGIC {
        return Err(Error::validation(
            ValidationReason::NotADatabase,
            "path",
            None,
        ));
    }
    let page_size = u16::from_be_bytes([header[16], header[17]]);
    let page_size = page_size_value(page_size)
        .ok_or_else(|| Error::validation(ValidationReason::NotADatabase, "page_size", None))?;
    if len < page_size as u64 {
        return Err(Error::validation(
            ValidationReason::NotADatabase,
            "page_size",
            None,
        ));
    }
    if !len.is_multiple_of(page_size as u64) {
        return Err(Error::validation(
            ValidationReason::NotADatabase,
            "file_size",
            None,
        ));
    }
    if header[18] != 1 || header[19] != 1 {
        return Err(Error::validation(
            ValidationReason::ForeignDatabase,
            "journal_mode",
            None,
        ));
    }
    let schema_format = u32::from_be_bytes([header[44], header[45], header[46], header[47]]);
    if !(1..=4).contains(&schema_format) {
        return Err(Error::validation(
            ValidationReason::NotADatabase,
            "schema_format",
            None,
        ));
    }
    let encoding = u32::from_be_bytes([header[56], header[57], header[58], header[59]]);
    if encoding != UTF8_ENCODING as u32 {
        return Err(Error::validation(
            ValidationReason::ForeignDatabase,
            "encoding",
            None,
        ));
    }
    let database_pages = u32::from_be_bytes([header[28], header[29], header[30], header[31]]);
    if database_pages == 0 {
        return Err(Error::validation(
            ValidationReason::NotADatabase,
            "database_size",
            None,
        ));
    }
    if u64::from(database_pages) * page_size as u64 > len {
        return Err(Error::validation(
            ValidationReason::NotADatabase,
            "database_size",
            None,
        ));
    }
    let user_version = i32::from_be_bytes([header[60], header[61], header[62], header[63]]);
    if user_version != expected_user {
        return Err(Error::UnsupportedVersion {
            found: user_version,
        });
    }
    Ok(())
}

fn page_size_value(raw: u16) -> Option<usize> {
    let size = if raw == 1 { 65_536 } else { usize::from(raw) };
    if !(512..=65_536).contains(&size) || !size.is_power_of_two() {
        return None;
    }
    Some(size)
}

fn required_id<T>(
    field: &'static str,
    raw: Option<String>,
    parse: impl FnOnce(&str) -> Result<T>,
) -> Result<T> {
    let raw = raw.ok_or_else(|| Error::validation(ValidationReason::InvalidId, field, None))?;
    parse(&raw).map_err(|_| Error::validation(ValidationReason::InvalidId, field, None))
}

fn optional_id<T>(
    field: &'static str,
    raw: Option<String>,
    parse: impl FnOnce(&str) -> Result<T>,
) -> Result<Option<T>> {
    raw.map(|raw| parse(&raw))
        .transpose()
        .map_err(|_| Error::validation(ValidationReason::InvalidId, field, None))
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut raw = path.as_os_str().to_os_string();
    raw.push(suffix);
    PathBuf::from(raw)
}

fn residual_files(candidate: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if std::fs::symlink_metadata(candidate).is_ok() {
        paths.push(candidate.to_path_buf());
    }
    for suffix in SIDECARS {
        let path = sidecar_path(candidate, suffix);
        if std::fs::symlink_metadata(&path).is_ok() {
            paths.push(path);
        }
    }
    paths
}

fn source_changed() -> Error {
    Error::Storage {
        message: "source changed during migration".to_string(),
    }
}

fn candidate_changed() -> Error {
    Error::Storage {
        message: "candidate changed during verification".to_string(),
    }
}

fn destination_changed() -> Error {
    Error::Storage {
        message: "destination changed during verification".to_string(),
    }
}

fn database_changed() -> Error {
    Error::Storage {
        message: "database changed during verification".to_string(),
    }
}

fn io_error(context: &str, path: &Path, err: std::io::Error) -> Error {
    Error::Io {
        message: format!("{context} {}: {err}", path.display()),
    }
}

fn map_sql(context: &str, err: rusqlite::Error) -> Error {
    if let rusqlite::Error::SqliteFailure(ffi, _) = &err {
        if ffi.code == rusqlite::ErrorCode::DatabaseBusy
            || ffi.code == rusqlite::ErrorCode::DatabaseLocked
        {
            return Error::Busy;
        }
    }
    Error::Storage {
        message: format!("{context}: {err}"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SchemaKind {
    Table,
    Index,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SchemaObject {
    kind: SchemaKind,
    sql: String,
}

fn detect_legacy_variant(conn: &Connection) -> Result<LegacyVariant> {
    let actual = actual_schema_objects(conn)?;
    if actual == expected_schema_objects(LEGACY_V1_LATE)? {
        return Ok(LegacyVariant::Late);
    }
    if actual == expected_schema_objects(LEGACY_V1_EARLY)? {
        return Ok(LegacyVariant::Early);
    }
    Err(schema_mismatch())
}

fn verify_candidate_schema(conn: &Connection) -> Result<()> {
    let actual = actual_schema_objects(conn)?;
    let expected = expected_schema_objects(SCHEMA)?;
    if actual != expected {
        return Err(schema_mismatch());
    }
    Ok(())
}

fn actual_schema_objects(conn: &Connection) -> Result<BTreeMap<String, SchemaObject>> {
    let mut statement = conn
        .prepare("SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY type, name")
        .map_err(|err| map_sql("prepare schema", err))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|err| map_sql("read schema", err))?;
    let mut objects = BTreeMap::new();
    for row in rows {
        let (kind, name, _table, sql) = row.map_err(|err| map_sql("schema row", err))?;
        match kind.as_str() {
            "table" => {
                let sql = sql.ok_or_else(schema_mismatch)?;
                insert_schema_object(
                    &mut objects,
                    name.to_ascii_uppercase(),
                    SchemaObject {
                        kind: SchemaKind::Table,
                        sql: normalize_sql(&sql)?,
                    },
                )?;
            }
            "index" => {
                if name.starts_with("sqlite_autoindex_") {
                    if sql.is_some() {
                        return Err(schema_mismatch());
                    }
                    continue;
                }
                let sql = sql.ok_or_else(schema_mismatch)?;
                insert_schema_object(
                    &mut objects,
                    name.to_ascii_uppercase(),
                    SchemaObject {
                        kind: SchemaKind::Index,
                        sql: normalize_sql(&sql)?,
                    },
                )?;
            }
            "view" | "trigger" => return Err(schema_mismatch()),
            _ => return Err(schema_mismatch()),
        }
    }
    Ok(objects)
}

fn expected_schema_objects(sql: &str) -> Result<BTreeMap<String, SchemaObject>> {
    let mut objects = BTreeMap::new();
    for statement in split_statements(sql) {
        let normalized = normalize_sql(statement)?;
        if normalized.is_empty() {
            continue;
        }
        let (name, kind) = parse_schema_object(&normalized)?;
        insert_schema_object(
            &mut objects,
            name,
            SchemaObject {
                kind,
                sql: normalized,
            },
        )?;
    }
    if objects.is_empty() {
        return Err(Error::Storage {
            message: "expected schema has no objects".to_string(),
        });
    }
    Ok(objects)
}

fn parse_schema_object(sql: &str) -> Result<(String, SchemaKind)> {
    let (prefix, kind) = if let Some(rest) = sql.strip_prefix("CREATE TABLE ") {
        (rest, SchemaKind::Table)
    } else if let Some(rest) = sql.strip_prefix("CREATE INDEX ") {
        (rest, SchemaKind::Index)
    } else {
        return Err(Error::Storage {
            message: "unsupported expected schema statement".to_string(),
        });
    };
    let name: String = prefix
        .chars()
        .take_while(|character| !character.is_whitespace() && *character != '(')
        .collect();
    if name.is_empty() {
        return Err(Error::Storage {
            message: "expected schema object has no name".to_string(),
        });
    }
    Ok((name.to_ascii_uppercase(), kind))
}

fn insert_schema_object(
    objects: &mut BTreeMap<String, SchemaObject>,
    name: String,
    object: SchemaObject,
) -> Result<()> {
    if objects.insert(name.clone(), object).is_some() {
        return Err(Error::Storage {
            message: format!("duplicate schema object {name}"),
        });
    }
    Ok(())
}

fn schema_mismatch() -> Error {
    Error::validation(ValidationReason::ForeignDatabase, "schema", None)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SqlToken {
    Word(String),
    Quoted(String),
    Punct(u8),
}

fn normalize_sql(sql: &str) -> Result<String> {
    let tokens = tokenize_sql(sql)?;
    let mut normalized = String::new();
    let mut previous: Option<&SqlToken> = None;
    for token in &tokens {
        if let Some(previous) = previous {
            if needs_space(previous, token) {
                normalized.push(' ');
            }
        }
        match token {
            SqlToken::Word(word) | SqlToken::Quoted(word) => normalized.push_str(word),
            SqlToken::Punct(byte) => normalized.push(char::from(*byte)),
        }
        previous = Some(token);
    }
    Ok(normalized)
}

fn needs_space(previous: &SqlToken, next: &SqlToken) -> bool {
    if matches!(
        next,
        SqlToken::Punct(
            b'(' | b')' | b',' | b';' | b'=' | b'+' | b'-' | b'*' | b'/' | b'<' | b'>' | b'.'
        )
    ) {
        return false;
    }
    if matches!(
        previous,
        SqlToken::Punct(b'(' | b',' | b'=' | b'+' | b'-' | b'*' | b'/' | b'<' | b'>' | b'.')
    ) {
        return false;
    }
    matches!(
        (previous, next),
        (
            SqlToken::Word(_) | SqlToken::Quoted(_),
            SqlToken::Word(_) | SqlToken::Quoted(_)
        ) | (
            SqlToken::Punct(b')'),
            SqlToken::Word(_) | SqlToken::Quoted(_)
        )
    )
}

fn tokenize_sql(sql: &str) -> Result<Vec<SqlToken>> {
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if byte == b'-' && index + 1 < bytes.len() && bytes[index + 1] == b'-' {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if byte == b'/' && index + 1 < bytes.len() && bytes[index + 1] == b'*' {
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            continue;
        }
        if byte == b'\'' {
            let (quoted, next) = read_quoted(bytes, index, b'\'')?;
            tokens.push(SqlToken::Quoted(quoted));
            index = next;
            continue;
        }
        if byte == b'"' || byte == b'`' {
            let (quoted, next) = read_quoted(bytes, index, byte)?;
            tokens.push(SqlToken::Quoted(quoted));
            index = next;
            continue;
        }
        if byte == b'[' {
            let mut end = index + 1;
            while end < bytes.len() && bytes[end] != b']' {
                end += 1;
            }
            if end == bytes.len() {
                return Err(Error::Storage {
                    message: "unterminated schema identifier".to_string(),
                });
            }
            let quoted = std::str::from_utf8(&bytes[index..=end])
                .map_err(|_| Error::Storage {
                    message: "schema is not UTF-8".to_string(),
                })?
                .to_string();
            tokens.push(SqlToken::Quoted(quoted));
            index = end + 1;
            continue;
        }
        if is_word_byte(byte) {
            let start = index;
            while index < bytes.len() && is_word_byte(bytes[index]) {
                index += 1;
            }
            let word = std::str::from_utf8(&bytes[start..index])
                .map_err(|_| Error::Storage {
                    message: "schema is not UTF-8".to_string(),
                })?
                .to_ascii_uppercase();
            tokens.push(SqlToken::Word(word));
            continue;
        }
        tokens.push(SqlToken::Punct(byte));
        index += 1;
    }
    Ok(tokens)
}

fn read_quoted(bytes: &[u8], start: usize, quote: u8) -> Result<(String, usize)> {
    let mut index = start + 1;
    while index < bytes.len() {
        if bytes[index] == quote {
            if index + 1 < bytes.len() && bytes[index + 1] == quote {
                index += 2;
                continue;
            }
            index += 1;
            let quoted = std::str::from_utf8(&bytes[start..index])
                .map_err(|_| Error::Storage {
                    message: "schema is not UTF-8".to_string(),
                })?
                .to_string();
            return Ok((quoted, index));
        }
        index += 1;
    }
    Err(Error::Storage {
        message: "unterminated schema string".to_string(),
    })
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || !byte.is_ascii()
}

fn split_statements(sql: &str) -> Vec<&str> {
    let bytes = sql.as_bytes();
    let mut statements = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'\'' || byte == b'"' || byte == b'`' {
            index = skip_quoted(bytes, index, byte);
        } else if byte == b'[' {
            index += 1;
            while index < bytes.len() && bytes[index] != b']' {
                index += 1;
            }
            index = (index + 1).min(bytes.len());
        } else if byte == b'-' && index + 1 < bytes.len() && bytes[index + 1] == b'-' {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
        } else if byte == b'/' && index + 1 < bytes.len() && bytes[index + 1] == b'*' {
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
        } else if byte == b';' {
            statements.push(&sql[start..index]);
            index += 1;
            start = index;
        } else {
            index += 1;
        }
    }
    if start < sql.len() {
        statements.push(&sql[start..]);
    }
    statements
        .into_iter()
        .filter(|statement| !statement.trim().is_empty())
        .collect()
}

fn skip_quoted(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut index = start + 1;
    while index < bytes.len() {
        if bytes[index] == quote {
            if index + 1 < bytes.len() && bytes[index + 1] == quote {
                index += 2;
                continue;
            }
            return index + 1;
        }
        index += 1;
    }
    bytes.len()
}

#[cfg(test)]
mod tests;
