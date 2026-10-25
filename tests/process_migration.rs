//! Process-level migration and verification checks.
//!
//! These tests exercise the real `spike` binary, real files, real hard links,
//! and explicit crash cut points. The crash switch is compiled out of release
//! builds.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use narrative_universe::{
    EntityId, Mention, MentionEdit, MentionId, PageEdit, PageId, PageRevision, Store,
};
use rusqlite::Connection;
use tempfile::TempDir;

const FIXTURE: &str = include_str!("fixtures/v1_universe.sql");
const UNIVERSE: &str = "11111111-1111-4111-8111-111111111111";
const PAGE: &str = "22222222-2222-4222-8222-222222222222";
const ENTITY: &str = "55555555-5555-4555-8555-555555555555";
const MENTION: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

struct Layout {
    _dir: TempDir,
    source: PathBuf,
    destination: PathBuf,
}

fn layout() -> Layout {
    let dir = tempfile::tempdir().expect("temp dir");
    let source = dir.path().join("source.sqlite");
    let destination = dir.path().join("destination.sqlite");
    write_v1(&source);
    Layout {
        _dir: dir,
        source,
        destination,
    }
}

fn write_v1(path: &Path) {
    let conn = Connection::open(path).expect("open v1");
    conn.execute_batch("PRAGMA page_size = 4096")
        .expect("page size");
    conn.execute_batch(FIXTURE).expect("v1 schema");
    conn.execute(
        "INSERT INTO metadata (singleton, universe_id, format_version) VALUES (1, ?1, 1)",
        [UNIVERSE],
    )
    .expect("metadata");
    conn.execute(
        "INSERT INTO pages (id, title, body) VALUES (?1, 't', 'b')",
        [PAGE],
    )
    .expect("page");
    conn.execute_batch("PRAGMA user_version = 1")
        .expect("user version");
}

fn run(args: &[&OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spike"))
        .args(args)
        .output()
        .expect("run spike")
}

fn run_at(args: &[&OsStr], crash: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spike"))
        .args(args)
        .env("NARRATIVE_UNIVERSE_CRASH_AT", crash)
        .output()
        .expect("run spike with a crash point")
}

fn success(output: &Output) -> String {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).expect("utf-8 stdout")
}

fn failure(output: &Output) -> String {
    assert!(!output.status.success(), "command unexpectedly succeeded");
    String::from_utf8(output.stderr.clone()).expect("utf-8 stderr")
}

fn candidate_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .expect("read directory")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().starts_with(".spike-migrate-"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    files
}

fn verify_cli(path: &Path) -> String {
    success(&run(&[OsStr::new("verify"), path.as_os_str()]))
}

#[test]
fn cli_migrates_once_and_verifies_the_destination() {
    let layout = layout();
    let source_before = fs::read(&layout.source).expect("source bytes");

    let stdout = success(&run(&[
        OsStr::new("migrate"),
        layout.source.as_os_str(),
        layout.destination.as_os_str(),
    ]));
    assert!(
        stdout.contains(&format!("universe_id={UNIVERSE}")),
        "{stdout}"
    );
    assert!(stdout.contains("pages=1"), "{stdout}");
    assert!(
        stdout.contains(&format!("destination={}", layout.destination.display())),
        "{stdout}"
    );
    assert_eq!(
        fs::read(&layout.source).expect("source bytes"),
        source_before
    );
    assert!(layout.destination.is_file());
    assert!(candidate_files(layout._dir.path()).is_empty());

    let verified = verify_cli(&layout.destination);
    assert!(verified.contains("verified=true"), "{verified}");
    assert!(verified.contains("mentions=0"), "{verified}");

    let destination_before = fs::read(&layout.destination).expect("destination bytes");
    let stderr = failure(&run(&[
        OsStr::new("migrate"),
        layout.source.as_os_str(),
        layout.destination.as_os_str(),
    ]));
    assert!(stderr.contains("destination preflight"), "{stderr}");
    assert!(stderr.contains("PathExists"), "{stderr}");
    assert_eq!(
        fs::read(&layout.destination).expect("destination bytes"),
        destination_before
    );
    assert!(candidate_files(layout._dir.path()).is_empty());
}

#[test]
fn cli_verifies_an_edited_v2_database_with_a_mention() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("edited.sqlite");
    success(&run(&[OsStr::new("seed"), path.as_os_str()]));

    let mut store = Store::open(&path).expect("open seeded database");
    let document = store
        .save_page_edits(
            &PageId::parse(PAGE).expect("page id"),
            PageRevision::ZERO,
            &[PageEdit::Mention(MentionEdit::Link {
                mention: Mention {
                    id: MentionId::parse(MENTION).expect("mention id"),
                    entity_id: EntityId::parse(ENTITY).expect("entity id"),
                    start_utf16: 0,
                    end_utf16: 5,
                },
            })],
        )
        .expect("link mention");
    assert_eq!(
        document.revision,
        PageRevision::new(1).expect("revision one")
    );
    drop(store);

    let stdout = verify_cli(&path);
    assert!(stdout.contains("verified=true"), "{stdout}");
    assert!(stdout.contains("mentions=1"), "{stdout}");
    assert!(stdout.contains("pages=1"), "{stdout}");
}

/// A stored UniverseId is untrusted input. The CLI must reject it without
/// printing the stored value and without rewriting the database.
#[test]
fn cli_does_not_echo_an_invalid_stored_universe_id() {
    const MARKER: &str = "SECRET_LORE_123456789012345678901234";
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("tampered.sqlite");
    success(&run(&[OsStr::new("seed"), path.as_os_str()]));
    let conn = Connection::open(&path).expect("open seeded database");
    conn.execute("UPDATE metadata SET universe_id = ?1", [MARKER])
        .expect("tamper");
    drop(conn);
    let before = fs::read(&path).expect("tampered bytes");

    let output = run(&[OsStr::new("verify"), path.as_os_str()]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "verification failure must exit 1"
    );
    let stderr = String::from_utf8(output.stderr).expect("utf-8 stderr");
    assert!(!stderr.contains("SECRET_LORE"), "{stderr}");
    assert!(stderr.contains("universe_id"), "{stderr}");
    assert_eq!(
        fs::read(&path).expect("bytes after verify"),
        before,
        "verification must not rewrite the database"
    );
}

/// Definition ids read from storage are untrusted too: the CLI must reject an
/// invalid one without printing it.
#[test]
fn cli_does_not_echo_an_invalid_stored_definition_id() {
    const MARKER: &str = "SECRET_LORE_123456789012345678901234";
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("tampered.sqlite");
    success(&run(&[OsStr::new("seed"), path.as_os_str()]));
    let conn = Connection::open(&path).expect("open seeded database");
    conn.execute_batch("PRAGMA ignore_check_constraints = ON")
        .expect("ignore check constraints");
    conn.execute(
        "INSERT INTO definitions
             (id, owner_type_id, owner_entity_id, value_class, cardinality, archived)
         VALUES (?1, NULL, NULL, 'text', 'singular', 0)",
        [MARKER],
    )
    .expect("tamper");
    drop(conn);
    let before = fs::read(&path).expect("tampered bytes");

    let output = run(&[OsStr::new("verify"), path.as_os_str()]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "verification failure must exit 1"
    );
    let stderr = String::from_utf8(output.stderr).expect("utf-8 stderr");
    assert!(!stderr.contains("SECRET_LORE"), "{stderr}");
    assert!(stderr.contains("invalid stored definition id"), "{stderr}");
    assert_eq!(
        fs::read(&path).expect("bytes after verify"),
        before,
        "verification must not rewrite the database"
    );
}

#[test]
fn crash_before_publication_leaves_the_destination_absent_and_never_adopts_it() {
    for point in ["candidate-built", "candidate-verified"] {
        let layout = layout();
        let source_before = fs::read(&layout.source).expect("source bytes");

        let output = run_at(
            &[
                OsStr::new("migrate"),
                layout.source.as_os_str(),
                layout.destination.as_os_str(),
            ],
            point,
        );
        assert!(
            !output.status.success(),
            "{point}: crash must abort the process"
        );
        assert!(
            !layout.destination.exists(),
            "{point}: destination appeared early"
        );
        let stale = candidate_files(layout._dir.path());
        assert_eq!(stale.len(), 1, "{point}: {stale:?}");
        let stale_bytes = fs::read(&stale[0]).expect("stale candidate bytes");
        assert_eq!(
            fs::read(&layout.source).expect("source bytes"),
            source_before
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&stale[0])
                .expect("candidate metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "{point}: candidate is not owner-only");
        }

        success(&run(&[
            OsStr::new("migrate"),
            layout.source.as_os_str(),
            layout.destination.as_os_str(),
        ]));
        assert!(layout.destination.is_file());
        assert_eq!(
            fs::read(&stale[0]).expect("stale candidate bytes"),
            stale_bytes,
            "{point}: a later run must not adopt or rewrite the stale candidate"
        );
        assert_eq!(
            candidate_files(layout._dir.path()),
            stale,
            "{point}: the later run must only remove its own candidate"
        );
        assert!(verify_cli(&layout.destination).contains("verified=true"));
    }
}

#[test]
fn crash_after_publication_leaves_a_complete_destination_and_is_not_adopted() {
    for point in [
        "published",
        "destination-verified",
        "destination-rechecked",
        "alias-verified",
    ] {
        let layout = layout();
        let output = run_at(
            &[
                OsStr::new("migrate"),
                layout.source.as_os_str(),
                layout.destination.as_os_str(),
            ],
            point,
        );
        assert!(!output.status.success(), "{point}: crash must abort");
        assert!(layout.destination.is_file(), "{point}: destination missing");
        let destination_before = fs::read(&layout.destination).expect("destination bytes");
        assert!(verify_cli(&layout.destination).contains("verified=true"));
        let stale = candidate_files(layout._dir.path());
        assert_eq!(stale.len(), 1, "{point}: {stale:?}");

        let stderr = failure(&run(&[
            OsStr::new("migrate"),
            layout.source.as_os_str(),
            layout.destination.as_os_str(),
        ]));
        assert!(
            stderr.contains("destination preflight"),
            "{point}: {stderr}"
        );
        assert_eq!(
            fs::read(&layout.destination).expect("destination bytes"),
            destination_before,
            "{point}: the published destination changed"
        );
        assert_eq!(
            candidate_files(layout._dir.path()),
            stale,
            "{point}: an unknown residual was deleted or adopted"
        );
    }
}

#[test]
fn crash_after_cleanup_leaves_a_complete_destination_and_no_alias() {
    let layout = layout();
    let output = run_at(
        &[
            OsStr::new("migrate"),
            layout.source.as_os_str(),
            layout.destination.as_os_str(),
        ],
        "cleaned-up",
    );
    assert!(!output.status.success(), "crash must abort");
    assert!(layout.destination.is_file());
    assert!(
        candidate_files(layout._dir.path()).is_empty(),
        "candidate alias survived cleanup"
    );
    assert!(verify_cli(&layout.destination).contains("verified=true"));
}

#[test]
fn concurrent_cli_migrations_publish_exactly_once() {
    let layout = layout();
    let mut children: Vec<_> = (0..4)
        .map(|_| {
            Command::new(env!("CARGO_BIN_EXE_spike"))
                .args([
                    OsStr::new("migrate"),
                    layout.source.as_os_str(),
                    layout.destination.as_os_str(),
                ])
                .spawn()
                .expect("spawn migration")
        })
        .collect();
    let statuses: Vec<_> = children
        .iter_mut()
        .map(|child| child.wait().expect("wait for migration"))
        .collect();
    assert_eq!(
        statuses.iter().filter(|status| status.success()).count(),
        1,
        "exactly one concurrent migration must publish"
    );
    assert!(layout.destination.is_file());
    assert!(candidate_files(layout._dir.path()).is_empty());
    assert!(verify_cli(&layout.destination).contains("verified=true"));
}

#[test]
fn cli_uses_distinct_usage_and_failure_exit_codes() {
    let layout = layout();

    let usage = run(&[OsStr::new("migrate")]);
    assert_eq!(usage.status.code(), Some(2), "usage must exit 2");
    assert!(
        String::from_utf8_lossy(&usage.stderr).contains("usage: spike migrate"),
        "{}",
        String::from_utf8_lossy(&usage.stderr)
    );

    let unknown = run(&[OsStr::new("not-a-command")]);
    assert_eq!(
        unknown.status.code(),
        Some(2),
        "unknown command must exit 2"
    );

    fs::write(&layout.destination, b"sentinel").expect("destination");
    let failed = run(&[
        OsStr::new("migrate"),
        layout.source.as_os_str(),
        layout.destination.as_os_str(),
    ]);
    assert_eq!(
        failed.status.code(),
        Some(1),
        "migration failure must exit 1"
    );
    assert_eq!(
        fs::read(&layout.destination).expect("destination bytes"),
        b"sentinel"
    );

    let missing = layout._dir.path().join("missing.sqlite");
    let verification = run(&[OsStr::new("verify"), missing.as_os_str()]);
    assert_eq!(
        verification.status.code(),
        Some(1),
        "verification failure must exit 1"
    );
}

#[test]
fn cli_errors_do_not_echo_page_prose() {
    let layout = layout();
    let conn = Connection::open(&layout.source).expect("open source");
    conn.execute_batch(
        "PRAGMA ignore_check_constraints = ON;
         UPDATE pages SET body = 'SECRET_PROSE_MARKER';
         UPDATE metadata SET universe_id = 'SECRET_ID_MARKER';",
    )
    .expect("tamper source");
    drop(conn);

    let stderr = failure(&run(&[
        OsStr::new("migrate"),
        layout.source.as_os_str(),
        layout.destination.as_os_str(),
    ]));
    assert!(!stderr.contains("SECRET_PROSE_MARKER"), "{stderr}");
    assert!(!stderr.contains("SECRET_ID_MARKER"), "{stderr}");
    assert!(!layout.destination.exists());
}
