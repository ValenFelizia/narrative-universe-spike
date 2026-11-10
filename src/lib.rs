mod error;
mod exchange;
mod mentions;
mod migration;
mod model;
mod store;

pub use error::{Error, Result, ValidationReason};
pub use exchange::{
    export_json, import_json, import_new_json, read_limited, write_exclusive, MAX_DOCUMENT_BYTES,
};
pub use mentions::{apply_mention_edit, validate_mentions, AnnotatedBody, Mention, MentionEdit};
pub use migration::{
    migrate_v1_to_current, verify_current, MigrationError, MigrationReport, MigrationStage,
    VerificationError, VerificationReport,
};
pub use model::{
    Assertion, AssertionId, Canon, Cardinality, Definition, DefinitionId, DefinitionLabel, Entity,
    EntityId, EntityType, MentionId, Owner, Page, PageId, PageRevision, Provenance, TypeId,
    UniverseId, Value, ValueClass,
};
pub use store::{
    Counts, EntitySummary, ImportOutcome, MentionOccurrence, PageDocument, PageEdit, PageSnapshot,
    PageSummary, PossibleIncompatibility, Snapshot, Store,
};

/// The storage schema this build creates, reads and verifies.
///
/// `store.rs` and `migration.rs` both read this constant. A bumped copy in
/// either of them would let the migration pipeline build a candidate that its
/// own header or schema check rejects.
pub const SCHEMA_VERSION: i32 = 3;

/// The JSON exchange format this build writes.
///
/// The written number is bound to the version 3 envelope shape; see
/// `EnvelopeV3::FORMAT_VERSION` in `exchange.rs`.
pub const FORMAT_VERSION: i32 = 3;

/// The only historical storage version this build still migrates.
pub(crate) const LEGACY_V1_SCHEMA_VERSION: i32 = 1;

/// A preserved storage version with no supported in-place conversion.
///
/// Version 2 databases are kept intact. They are recreated from JSON fixtures
/// at a new path, or left alone. See DEC-002.
pub(crate) const LEGACY_V2_SCHEMA_VERSION: i32 = 2;
