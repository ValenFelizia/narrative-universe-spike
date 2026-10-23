use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags};
use tempfile::TempDir;

use crate::error::{Error, ValidationReason};
use crate::model::{
    Assertion, AssertionId, Canon, Cardinality, Definition, DefinitionId, Entity, EntityId,
    EntityType, Owner, Page, PageId, PageRevision, Provenance, TypeId, UniverseId, Value,
    ValueClass,
};
use crate::store::{Counts, PageSnapshot, Snapshot, Store};

use super::{
    build_verified_candidate, preflight_source, sidecar_path, verify_candidate, MigrationStage,
    PinnedSource, LEGACY_V1_EARLY, LEGACY_V1_LATE,
};

const FIXTURE: &str = include_str!("../../tests/fixtures/v1_universe.sql");

const UNIVERSE: &str = "11111111-1111-4111-8111-111111111111";
const PAGE_ONE: &str = "22222222-2222-4222-8222-222222222222";
const PAGE_TWO: &str = "22222222-2222-4222-8222-222222222223";
const PAGE_THREE: &str = "22222222-2222-4222-8222-222222222224";
const TYPE_ONE: &str = "33333333-3333-4333-8333-333333333333";
const TYPE_TWO: &str = "44444444-4444-4444-8444-444444444444";
const ENTITY_ONE: &str = "55555555-5555-4555-8555-555555555555";
const ENTITY_TWO: &str = "66666666-6666-4666-8666-666666666666";
const DEFINITION_REFERENCE: &str = "77777777-7777-4777-8777-777777777777";
const DEFINITION_TEXT: &str = "99999999-9999-4999-8999-999999999999";
const DEFINITION_BOOLEAN: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const ASSERTION_REFERENCE: &str = "88888888-8888-4888-8888-888888888888";
const ASSERTION_TEXT: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const ASSERTION_BOOLEAN: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const ASSERTION_UNKNOWN: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const ASSERTION_COMPETING: &str = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";

fn universe_id() -> UniverseId {
    UniverseId::parse(UNIVERSE).expect("universe id")
}

fn page_id(raw: &str) -> PageId {
    PageId::parse(raw).expect("page id")
}

fn type_id(raw: &str) -> TypeId {
    TypeId::parse(raw).expect("type id")
}

fn entity_id(raw: &str) -> EntityId {
    EntityId::parse(raw).expect("entity id")
}

fn definition_id(raw: &str) -> DefinitionId {
    DefinitionId::parse(raw).expect("definition id")
}

fn assertion_id(raw: &str) -> AssertionId {
    AssertionId::parse(raw).expect("assertion id")
}

fn base_snapshot() -> Snapshot {
    Snapshot {
        universe_id: universe_id(),
        pages: vec![
            PageSnapshot {
                page: Page {
                    id: page_id(PAGE_ONE),
                    title: "Página sintética".to_string(),
                    body: "Texto con acento áéíóú ñ y emoji 📚".to_string(),
                },
                mentions: Vec::new(),
            },
            PageSnapshot {
                page: Page {
                    id: page_id(PAGE_TWO),
                    title: "Segunda página".to_string(),
                    body: "Cuerpo de la segunda página".to_string(),
                },
                mentions: Vec::new(),
            },
        ],
        types: vec![
            EntityType {
                id: type_id(TYPE_ONE),
                label: "Personaje".to_string(),
            },
            EntityType {
                id: type_id(TYPE_TWO),
                label: "Casa".to_string(),
            },
        ],
        entities: vec![
            Entity {
                id: entity_id(ENTITY_ONE),
                label: "Narrador sintético".to_string(),
                type_id: Some(type_id(TYPE_ONE)),
                page_id: Some(page_id(PAGE_ONE)),
            },
            Entity {
                id: entity_id(ENTITY_TWO),
                label: "Casa sintética".to_string(),
                type_id: Some(type_id(TYPE_TWO)),
                page_id: None,
            },
        ],
        definitions: vec![
            Definition {
                id: definition_id(DEFINITION_REFERENCE),
                owner: Owner::Type(type_id(TYPE_ONE)),
                value_class: ValueClass::Reference,
                cardinality: Cardinality::Singular,
                archived: false,
            },
            Definition {
                id: definition_id(DEFINITION_TEXT),
                owner: Owner::Entity(entity_id(ENTITY_ONE)),
                value_class: ValueClass::Text,
                cardinality: Cardinality::Multiple,
                archived: true,
            },
            Definition {
                id: definition_id(DEFINITION_BOOLEAN),
                owner: Owner::Type(type_id(TYPE_TWO)),
                value_class: ValueClass::Boolean,
                cardinality: Cardinality::Singular,
                archived: false,
            },
        ],
        assertions: vec![
            Assertion {
                id: assertion_id(ASSERTION_REFERENCE),
                subject_id: entity_id(ENTITY_ONE),
                definition_id: definition_id(DEFINITION_REFERENCE),
                value: Value::Reference(entity_id(ENTITY_TWO)),
                canon: Canon::Provisional,
                source_status_raw: None,
                provenance: Provenance::absent(),
                scope_note: Some("al inicio".to_string()),
                archived: false,
            },
            Assertion {
                id: assertion_id(ASSERTION_TEXT),
                subject_id: entity_id(ENTITY_ONE),
                definition_id: definition_id(DEFINITION_TEXT),
                value: Value::Text(String::new()),
                canon: Canon::Acordado,
                source_status_raw: Some(String::new()),
                provenance: Provenance {
                    external_ref: Some(String::new()),
                    locator: Some(String::new()),
                    fragment: Some(String::new()),
                    source_revision: Some(String::new()),
                },
                scope_note: Some(String::new()),
                archived: true,
            },
            Assertion {
                id: assertion_id(ASSERTION_BOOLEAN),
                subject_id: entity_id(ENTITY_TWO),
                definition_id: definition_id(DEFINITION_BOOLEAN),
                value: Value::Boolean(false),
                canon: Canon::MisterioIntencional,
                source_status_raw: Some("raw".to_string()),
                provenance: Provenance {
                    external_ref: Some("external".to_string()),
                    locator: Some("locator".to_string()),
                    fragment: Some("fragment".to_string()),
                    source_revision: Some("revision".to_string()),
                },
                scope_note: Some("nota".to_string()),
                archived: false,
            },
            Assertion {
                id: assertion_id(ASSERTION_UNKNOWN),
                subject_id: entity_id(ENTITY_TWO),
                definition_id: definition_id(DEFINITION_BOOLEAN),
                value: Value::Unknown,
                canon: Canon::Abierto,
                source_status_raw: None,
                provenance: Provenance::absent(),
                scope_note: None,
                archived: false,
            },
            Assertion {
                id: assertion_id(ASSERTION_COMPETING),
                subject_id: entity_id(ENTITY_TWO),
                definition_id: definition_id(DEFINITION_BOOLEAN),
                value: Value::Boolean(true),
                canon: Canon::Acordado,
                source_status_raw: Some("raw-two".to_string()),
                provenance: Provenance {
                    external_ref: Some("external-two".to_string()),
                    locator: None,
                    fragment: Some("fragment-two".to_string()),
                    source_revision: Some("revision-two".to_string()),
                },
                scope_note: Some("nota dos".to_string()),
                archived: true,
            },
        ],
    }
}

fn expected_counts() -> Counts {
    Counts {
        pages: 2,
        types: 2,
        entities: 2,
        definitions: 3,
        assertions: 5,
    }
}

fn insert_snapshot(conn: &Connection, snapshot: &Snapshot) {
    conn.execute(
        "INSERT INTO metadata (singleton, universe_id, format_version) VALUES (1, ?1, 1)",
        [snapshot.universe_id.as_str()],
    )
    .expect("metadata");
    for page in &snapshot.pages {
        conn.execute(
            "INSERT INTO pages (id, title, body) VALUES (?1, ?2, ?3)",
            params![page.page.id.as_str(), page.page.title, page.page.body],
        )
        .expect("page");
    }
    for entity_type in &snapshot.types {
        conn.execute(
            "INSERT INTO types (id, label) VALUES (?1, ?2)",
            params![entity_type.id.as_str(), entity_type.label],
        )
        .expect("type");
    }
    for entity in &snapshot.entities {
        conn.execute(
            "INSERT INTO entities (id, label, type_id, page_id) VALUES (?1, ?2, ?3, ?4)",
            params![
                entity.id.as_str(),
                entity.label,
                entity.type_id.as_ref().map(TypeId::as_str),
                entity.page_id.as_ref().map(PageId::as_str)
            ],
        )
        .expect("entity");
    }
    for definition in &snapshot.definitions {
        let (owner_type, owner_entity) = match &definition.owner {
            Owner::Type(id) => (Some(id.as_str()), None),
            Owner::Entity(id) => (None, Some(id.as_str())),
        };
        conn.execute(
            "INSERT INTO definitions (
                id, owner_type_id, owner_entity_id, value_class, cardinality, archived
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                definition.id.as_str(),
                owner_type,
                owner_entity,
                definition.value_class.as_str(),
                definition.cardinality.as_str(),
                if definition.archived { 1 } else { 0 }
            ],
        )
        .expect("definition");
    }
    for assertion in &snapshot.assertions {
        let (value_kind, text_value, bool_value, target) = match &assertion.value {
            Value::Text(text) => ("text", Some(text.as_str()), None, None),
            Value::Boolean(value) => ("boolean", None, Some(if *value { 1 } else { 0 }), None),
            Value::Reference(target) => ("reference", None, None, Some(target.as_str())),
            Value::Unknown => ("unknown", None, None, None),
        };
        conn.execute(
            "INSERT INTO assertions (
                id, subject_id, definition_id, value_kind, text_value, bool_value,
                target_entity_id, canon, source_status_raw, provenance_external_ref,
                provenance_locator, provenance_fragment, provenance_source_revision,
                scope_note, archived
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                assertion.id.as_str(),
                assertion.subject_id.as_str(),
                assertion.definition_id.as_str(),
                value_kind,
                text_value,
                bool_value,
                target,
                assertion.canon.as_str(),
                assertion.source_status_raw,
                assertion.provenance.external_ref,
                assertion.provenance.locator,
                assertion.provenance.fragment,
                assertion.provenance.source_revision,
                assertion.scope_note,
                if assertion.archived { 1 } else { 0 }
            ],
        )
        .expect("assertion");
    }
}

fn write_v1(path: &Path, schema: &str, snapshot: &Snapshot) {
    write_v1_with_page_size(path, schema, snapshot, 4096);
}

fn write_v1_with_page_size(path: &Path, schema: &str, snapshot: &Snapshot, page_size: u32) {
    let conn = Connection::open(path).expect("open v1");
    conn.execute_batch(&format!("PRAGMA page_size = {page_size}"))
        .expect("page size");
    conn.execute_batch("PRAGMA foreign_keys = ON")
        .expect("foreign keys");
    conn.execute_batch(schema).expect("v1 schema");
    insert_snapshot(&conn, snapshot);
    conn.execute_batch("PRAGMA user_version = 1")
        .expect("user version");
}

fn layout() -> (TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let source_dir = dir.path().join("source");
    fs::create_dir(&source_dir).expect("source dir");
    let source = source_dir.join("universe.sqlite");
    let candidate = dir.path().join("universe.v2.sqlite");
    (dir, source, candidate)
}

fn dir_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

fn assert_source_unchanged(source: &Path, bytes: &[u8], entries: &[String]) {
    assert_eq!(fs::read(source).expect("read source"), bytes);
    assert_eq!(
        dir_names(source.parent().expect("source parent")),
        entries,
        "source directory gained or lost files"
    );
}

fn assert_no_sidecars(path: &Path) {
    for suffix in ["-journal", "-wal", "-shm"] {
        assert!(
            !sidecar_path(path, suffix).exists(),
            "unexpected sidecar {}",
            sidecar_path(path, suffix).display()
        );
    }
}

fn migration_error(source: &Path, candidate: &Path) -> (MigrationStage, Error) {
    let error = build_verified_candidate(source, candidate).expect_err("migration must fail");
    (error.stage, error.cause)
}

fn migrate_failure(source: &Path, candidate: &Path) -> (MigrationStage, Error) {
    let failure = migration_error(source, candidate);
    assert!(
        !candidate.exists(),
        "failed pre-build migration left {}",
        candidate.display()
    );
    failure
}

fn modify(path: &Path, sql: &str) {
    let conn = Connection::open(path).expect("open for modification");
    conn.execute_batch(sql).expect("modify");
}

#[test]
fn migrates_both_historical_v1_shapes_and_preserves_every_record() {
    for (label, schema) in [
        ("early", LEGACY_V1_EARLY),
        ("late", LEGACY_V1_LATE),
        ("checked-in fixture", FIXTURE),
    ] {
        let (_dir, source, candidate) = layout();
        let expected = base_snapshot();
        write_v1(&source, schema, &expected);
        let before = fs::read(&source).expect("source bytes");
        let entries = dir_names(source.parent().expect("source parent"));

        let report = build_verified_candidate(&source, &candidate)
            .unwrap_or_else(|error| panic!("{label}: {error}"));
        assert_eq!(report.universe_id, expected.universe_id, "{label}");
        assert_eq!(report.counts, expected_counts(), "{label}");
        assert_source_unchanged(&source, &before, &entries);
        assert_no_sidecars(&candidate);

        let mut store = Store::open(&candidate).unwrap_or_else(|error| panic!("{label}: {error}"));
        let actual = store.export_snapshot().expect("export candidate");
        assert_eq!(actual, expected, "{label}");
        for page in &expected.pages {
            let document = store
                .page_document(&page.page.id)
                .expect("read candidate page");
            assert_eq!(document.revision, PageRevision::ZERO, "{label}");
            assert!(document.body.mentions.is_empty(), "{label}");
        }
        assert!(
            matches!(
                Store::open(&source),
                Err(Error::MigrationRequired { found: 1 })
            ),
            "{label}: the source must still require a migration"
        );
    }
}

#[test]
fn migrates_an_empty_universe() {
    let (_dir, source, candidate) = layout();
    let expected = Snapshot {
        universe_id: universe_id(),
        pages: Vec::new(),
        types: Vec::new(),
        entities: Vec::new(),
        definitions: Vec::new(),
        assertions: Vec::new(),
    };
    write_v1(&source, LEGACY_V1_LATE, &expected);
    let before = fs::read(&source).expect("source bytes");
    let entries = dir_names(source.parent().expect("source parent"));

    build_verified_candidate(&source, &candidate).expect("empty migration");

    assert_source_unchanged(&source, &before, &entries);
    let mut store = Store::open(&candidate).expect("open candidate");
    assert_eq!(store.export_snapshot().expect("export"), expected);
    assert_eq!(
        store.counts().expect("counts"),
        Counts {
            pages: 0,
            types: 0,
            entities: 0,
            definitions: 0,
            assertions: 0,
        }
    );
}

#[test]
fn accepts_historical_page_sizes() {
    for page_size in [512u32, 65_536u32] {
        let (_dir, source, candidate) = layout();
        let expected = base_snapshot();
        write_v1_with_page_size(&source, LEGACY_V1_LATE, &expected, page_size);
        let bytes = fs::read(&source).expect("source bytes");
        let encoded = if page_size == 65_536 {
            1u16
        } else {
            page_size as u16
        };
        assert_eq!(u16::from_be_bytes([bytes[16], bytes[17]]), encoded);
        let entries = dir_names(source.parent().expect("source parent"));

        build_verified_candidate(&source, &candidate).expect("page size migration");

        assert_source_unchanged(&source, &bytes, &entries);
        let mut store = Store::open(&candidate).expect("open candidate");
        assert!(
            store.export_snapshot().expect("export") == expected,
            "page size {page_size}"
        );
    }
}

#[test]
fn rejects_a_schema_with_a_missing_table() {
    let (_dir, source, candidate) = layout();
    let missing_pages = LEGACY_V1_LATE.replace(
        "CREATE TABLE pages (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36),
    title TEXT NOT NULL,
    body TEXT NOT NULL
);",
        "",
    );
    assert_ne!(missing_pages, LEGACY_V1_LATE);
    let empty = Snapshot {
        universe_id: universe_id(),
        pages: Vec::new(),
        types: Vec::new(),
        entities: Vec::new(),
        definitions: Vec::new(),
        assertions: Vec::new(),
    };
    write_v1(&source, &missing_pages, &empty);
    let before = fs::read(&source).expect("source bytes");
    let entries = dir_names(source.parent().expect("source parent"));

    let (stage, cause) = migrate_failure(&source, &candidate);
    assert_eq!(stage, MigrationStage::SnapshotSource);
    assert!(
        matches!(
            cause,
            Error::Validation {
                reason: ValidationReason::ForeignDatabase,
                field: "schema",
                ..
            }
        ),
        "{cause}"
    );
    assert_source_unchanged(&source, &before, &entries);
}

#[test]
fn preserves_special_text_and_the_empty_versus_null_distinction() {
    let (_dir, source, candidate) = layout();
    let mut expected = base_snapshot();
    expected.pages = vec![
        PageSnapshot {
            page: Page {
                id: page_id(PAGE_ONE),
                title: String::new(),
                body: String::new(),
            },
            mentions: Vec::new(),
        },
        PageSnapshot {
            page: Page {
                id: page_id(PAGE_TWO),
                title: " \t\r\n".to_string(),
                body: "línea 1\r\nlínea 2\0fin  ".to_string(),
            },
            mentions: Vec::new(),
        },
        PageSnapshot {
            page: Page {
                id: page_id(PAGE_THREE),
                title: "emoji 📚 y combinación e\u{301}".to_string(),
                body: "  inicio\n\tfinal  ".to_string(),
            },
            mentions: Vec::new(),
        },
    ];
    expected.entities = vec![Entity {
        id: entity_id(ENTITY_ONE),
        label: "Entidad sintética".to_string(),
        type_id: Some(type_id(TYPE_ONE)),
        page_id: Some(page_id(PAGE_TWO)),
    }];
    expected.definitions = vec![
        Definition {
            id: definition_id(DEFINITION_TEXT),
            owner: Owner::Entity(entity_id(ENTITY_ONE)),
            value_class: ValueClass::Text,
            cardinality: Cardinality::Multiple,
            archived: false,
        },
        Definition {
            id: definition_id(DEFINITION_BOOLEAN),
            owner: Owner::Type(type_id(TYPE_ONE)),
            value_class: ValueClass::Boolean,
            cardinality: Cardinality::Singular,
            archived: true,
        },
    ];
    expected.assertions = vec![
        Assertion {
            id: assertion_id(ASSERTION_TEXT),
            subject_id: entity_id(ENTITY_ONE),
            definition_id: definition_id(DEFINITION_TEXT),
            value: Value::Text(String::new()),
            canon: Canon::Provisional,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        },
        Assertion {
            id: assertion_id(ASSERTION_BOOLEAN),
            subject_id: entity_id(ENTITY_ONE),
            definition_id: definition_id(DEFINITION_TEXT),
            value: Value::Text("texto".to_string()),
            canon: Canon::Acordado,
            source_status_raw: Some(String::new()),
            provenance: Provenance {
                external_ref: Some(String::new()),
                locator: Some(String::new()),
                fragment: Some(String::new()),
                source_revision: Some(String::new()),
            },
            scope_note: Some(String::new()),
            archived: true,
        },
        Assertion {
            id: assertion_id(ASSERTION_UNKNOWN),
            subject_id: entity_id(ENTITY_ONE),
            definition_id: definition_id(DEFINITION_BOOLEAN),
            value: Value::Unknown,
            canon: Canon::Abierto,
            source_status_raw: Some("raw".to_string()),
            provenance: Provenance {
                external_ref: Some("external".to_string()),
                locator: None,
                fragment: Some("fragment".to_string()),
                source_revision: None,
            },
            scope_note: Some("nota".to_string()),
            archived: false,
        },
    ];

    write_v1(&source, LEGACY_V1_LATE, &expected);
    build_verified_candidate(&source, &candidate).expect("special text migration");

    let mut store = Store::open(&candidate).expect("open candidate");
    assert!(store.export_snapshot().expect("export") == expected);

    let conn = Connection::open_with_flags(&candidate, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("read-only candidate");
    let body: String = conn
        .query_row("SELECT body FROM pages WHERE id = ?1", [PAGE_TWO], |row| {
            row.get(0)
        })
        .expect("body");
    assert_eq!(body, "línea 1\r\nlínea 2\0fin  ");
    let nulls: (Option<String>, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT source_status_raw, provenance_external_ref, scope_note
             FROM assertions WHERE id = ?1",
            [ASSERTION_TEXT],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("null optionals");
    assert_eq!(nulls, (None, None, None));
    let empty: (Option<String>, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT source_status_raw, provenance_external_ref, scope_note
             FROM assertions WHERE id = ?1",
            [ASSERTION_BOOLEAN],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("empty optionals");
    assert_eq!(
        empty,
        (
            Some(String::new()),
            Some(String::new()),
            Some(String::new())
        )
    );
}

#[test]
fn rejects_a_null_identifier_in_the_early_schema() {
    let (_dir, source, candidate) = layout();
    write_v1(&source, LEGACY_V1_EARLY, &base_snapshot());
    modify(
        &source,
        "INSERT INTO pages (id, title, body) VALUES (NULL, 'SECRETO', 'SECRETO')",
    );
    let before = fs::read(&source).expect("source bytes");
    let entries = dir_names(source.parent().expect("source parent"));

    let (stage, cause) = migrate_failure(&source, &candidate);
    assert_eq!(stage, MigrationStage::SnapshotSource);
    assert!(
        matches!(
            &cause,
            Error::Validation {
                reason: ValidationReason::InvalidId,
                field: "page",
                ..
            }
        ),
        "{cause}"
    );
    assert!(!format!("{cause}").contains("SECRETO"));
    assert_source_unchanged(&source, &before, &entries);
}

#[test]
fn rejects_mixed_and_unknown_schema_shapes() {
    let mutations = [
        (
            "mixed identifier nullability",
            LEGACY_V1_LATE.replacen("id TEXT PRIMARY KEY NOT NULL", "id TEXT PRIMARY KEY", 1),
        ),
        (
            "missing identifier length constraint",
            LEGACY_V1_LATE.replace("CHECK (length(id) = 36)", ""),
        ),
        (
            "missing index",
            LEGACY_V1_LATE.replace(
                "CREATE INDEX idx_assertions_target ON assertions (target_entity_id);",
                "",
            ),
        ),
        (
            "extra table",
            format!("{LEGACY_V1_LATE}\nCREATE TABLE unexpected (id TEXT);"),
        ),
        (
            "extra index",
            format!("{LEGACY_V1_LATE}\nCREATE INDEX idx_unexpected ON pages (id);"),
        ),
        (
            "extra column",
            format!("{LEGACY_V1_LATE}\nALTER TABLE pages ADD COLUMN unexpected TEXT;"),
        ),
        (
            "view",
            format!("{LEGACY_V1_LATE}\nCREATE VIEW unexpected_view AS SELECT 1;"),
        ),
        (
            "trigger",
            format!(
                "{LEGACY_V1_LATE}\nCREATE TRIGGER unexpected_trigger AFTER INSERT ON pages BEGIN SELECT 1; END;"
            ),
        ),
    ];

    for (label, schema) in mutations {
        let (_dir, source, candidate) = layout();
        write_v1(&source, &schema, &base_snapshot());
        let before = fs::read(&source).expect("source bytes");
        let entries = dir_names(source.parent().expect("source parent"));

        let (stage, cause) = migrate_failure(&source, &candidate);
        assert_eq!(stage, MigrationStage::SnapshotSource, "{label}");
        assert!(
            matches!(
                cause,
                Error::Validation {
                    reason: ValidationReason::ForeignDatabase,
                    field: "schema",
                    ..
                }
            ),
            "{label}: {cause}"
        );
        assert_source_unchanged(&source, &before, &entries);
    }
}

#[test]
fn rejects_bad_metadata() {
    let cases = [
        (
            "wrong format version",
            "PRAGMA ignore_check_constraints = ON;
             UPDATE metadata SET format_version = 2;",
            ValidationReason::ForeignDatabase,
        ),
        (
            "missing metadata",
            "DELETE FROM metadata;",
            ValidationReason::MissingMetadata,
        ),
        (
            "noncanonical universe",
            "PRAGMA ignore_check_constraints = ON;
             UPDATE metadata SET universe_id = 'not-a-uuid';",
            ValidationReason::InvalidId,
        ),
    ];

    for (label, sql, reason) in cases {
        let (_dir, source, candidate) = layout();
        write_v1(&source, LEGACY_V1_LATE, &base_snapshot());
        modify(&source, sql);
        let before = fs::read(&source).expect("source bytes");
        let entries = dir_names(source.parent().expect("source parent"));

        let (stage, cause) = migrate_failure(&source, &candidate);
        assert_eq!(stage, MigrationStage::SnapshotSource, "{label}");
        assert!(
            matches!(cause, Error::Validation { reason: found, .. } if found == reason),
            "{label}: {cause}"
        );
        assert_source_unchanged(&source, &before, &entries);
    }
}

#[test]
fn rejects_a_source_that_is_not_schema_version_one() {
    let (_dir, source, candidate) = layout();
    write_v1(&source, LEGACY_V1_LATE, &base_snapshot());
    modify(&source, "PRAGMA user_version = 2");
    let before = fs::read(&source).expect("source bytes");
    let entries = dir_names(source.parent().expect("source parent"));

    let (stage, cause) = migrate_failure(&source, &candidate);
    assert_eq!(stage, MigrationStage::PreflightSource);
    assert!(
        matches!(cause, Error::UnsupportedVersion { found: 2 }),
        "{cause}"
    );
    assert_source_unchanged(&source, &before, &entries);
}

#[test]
fn rejects_a_version_two_database_used_as_a_source() {
    let (_dir, source, candidate) = layout();
    let store = Store::create(&source, universe_id()).expect("v2 database");
    drop(store);

    let (stage, cause) = migrate_failure(&source, &candidate);
    assert_eq!(stage, MigrationStage::PreflightSource);
    assert!(
        matches!(cause, Error::UnsupportedVersion { found: 2 }),
        "{cause}"
    );
}

#[test]
fn rejects_corruption() {
    let (_dir, source, candidate) = layout();
    write_v1(&source, LEGACY_V1_LATE, &base_snapshot());
    {
        use std::io::{Seek, SeekFrom, Write};
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(&source)
            .expect("open source");
        file.seek(SeekFrom::Start(0)).expect("seek");
        file.write_all(&[0u8; 16]).expect("corrupt magic");
    }
    let (stage, cause) = migrate_failure(&source, &candidate);
    assert_eq!(stage, MigrationStage::PreflightSource);
    assert!(
        matches!(
            cause,
            Error::Validation {
                reason: ValidationReason::NotADatabase,
                ..
            }
        ),
        "{cause}"
    );

    let (_dir, source, candidate) = layout();
    write_v1(&source, LEGACY_V1_LATE, &base_snapshot());
    let len = fs::metadata(&source).expect("metadata").len();
    fs::OpenOptions::new()
        .write(true)
        .open(&source)
        .expect("open source")
        .set_len(len - 4096)
        .expect("truncate");
    let (stage, cause) = migrate_failure(&source, &candidate);
    assert_eq!(stage, MigrationStage::PreflightSource);
    assert!(
        matches!(
            cause,
            Error::Validation {
                reason: ValidationReason::NotADatabase,
                field: "database_size",
                ..
            }
        ),
        "{cause}"
    );
}

#[test]
fn rejects_a_file_with_a_partial_trailing_page() {
    let (_dir, source, candidate) = layout();
    write_v1(&source, LEGACY_V1_LATE, &base_snapshot());
    {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&source)
            .expect("open source");
        file.write_all(&[0]).expect("trailing byte");
    }

    let (stage, cause) = migrate_failure(&source, &candidate);
    assert_eq!(stage, MigrationStage::PreflightSource);
    assert!(
        matches!(
            cause,
            Error::Validation {
                reason: ValidationReason::NotADatabase,
                field: "file_size",
                ..
            }
        ),
        "{cause}"
    );
}

#[test]
fn rejects_foreign_key_violations() {
    let (_dir, source, candidate) = layout();
    write_v1(&source, LEGACY_V1_LATE, &base_snapshot());
    modify(
        &source,
        "PRAGMA foreign_keys = OFF;
         INSERT INTO entities (id, label, type_id, page_id)
         VALUES ('12121212-1212-4212-8212-121212121212', 'huérfana',
                 '34343434-3434-4434-8434-343434343434', NULL);",
    );
    let (stage, cause) = migrate_failure(&source, &candidate);
    assert_eq!(stage, MigrationStage::SnapshotSource);
    assert!(
        matches!(&cause, Error::Storage { message } if message.contains("foreign key")),
        "{cause}"
    );
}

#[test]
fn rejects_wal_mode_from_the_header_even_without_sidecars() {
    let (_dir, source, candidate) = layout();
    write_v1(&source, LEGACY_V1_LATE, &base_snapshot());
    {
        let conn = Connection::open(&source).expect("open source");
        let mode: String = conn
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
            .expect("wal mode");
        assert_eq!(mode, "wal");
    }
    for suffix in ["-wal", "-shm"] {
        let _ = fs::remove_file(sidecar_path(&source, suffix));
    }
    let bytes = fs::read(&source).expect("source bytes");
    assert_eq!(bytes[18], 2);
    assert_eq!(bytes[19], 2);
    let entries = dir_names(source.parent().expect("source parent"));

    let (stage, cause) = migrate_failure(&source, &candidate);
    assert_eq!(stage, MigrationStage::PreflightSource);
    assert!(
        matches!(
            cause,
            Error::Validation {
                reason: ValidationReason::ForeignDatabase,
                field: "journal_mode",
                ..
            }
        ),
        "{cause}"
    );
    assert_source_unchanged(&source, &bytes, &entries);
    assert_no_sidecars(&source);
}

#[test]
fn rejects_each_sidecar_without_removing_it() {
    for suffix in ["-journal", "-wal", "-shm"] {
        let (_dir, source, candidate) = layout();
        write_v1(&source, LEGACY_V1_LATE, &base_snapshot());
        fs::write(sidecar_path(&source, suffix), b"sidecar").expect("sidecar");
        let before = fs::read(&source).expect("source bytes");
        let entries = dir_names(source.parent().expect("source parent"));

        let sidecar = sidecar_path(&source, suffix);
        let sidecar_before = fs::read(&sidecar).expect("sidecar bytes");
        let (stage, cause) = migrate_failure(&source, &candidate);
        assert_eq!(stage, MigrationStage::PreflightSource, "{suffix}");
        assert!(
            matches!(
                cause,
                Error::Validation {
                    reason: ValidationReason::ForeignDatabase,
                    field: "sidecar",
                    ..
                }
            ),
            "{suffix}: {cause}"
        );
        assert_source_unchanged(&source, &before, &entries);
        assert_eq!(fs::read(sidecar).expect("sidecar bytes"), sidecar_before);
    }
}

#[test]
fn rejects_a_candidate_sidecar_before_creation() {
    let (_dir, source, candidate) = layout();
    write_v1(&source, LEGACY_V1_LATE, &base_snapshot());
    fs::write(sidecar_path(&candidate, "-wal"), b"sidecar").expect("candidate sidecar");

    let (stage, cause) = migrate_failure(&source, &candidate);
    assert_eq!(stage, MigrationStage::PreflightCandidate);
    assert!(
        matches!(
            cause,
            Error::Validation {
                reason: ValidationReason::ForeignDatabase,
                field: "sidecar",
                ..
            }
        ),
        "{cause}"
    );
    assert!(!candidate.exists());
}

#[test]
fn refuses_an_existing_candidate_without_overwriting_it() {
    let (_dir, source, candidate) = layout();
    write_v1(&source, LEGACY_V1_LATE, &base_snapshot());
    fs::write(&candidate, b"sentinel").expect("candidate");
    let before_candidate = fs::read(&candidate).expect("candidate bytes");
    let before_source = fs::read(&source).expect("source bytes");
    let entries = dir_names(source.parent().expect("source parent"));

    let (stage, cause) = migration_error(&source, &candidate);
    assert_eq!(stage, MigrationStage::PreflightCandidate);
    assert!(
        matches!(
            cause,
            Error::Validation {
                reason: ValidationReason::PathExists,
                field: "candidate",
                ..
            }
        ),
        "{cause}"
    );
    assert_eq!(
        fs::read(&candidate).expect("candidate bytes"),
        before_candidate
    );
    assert_source_unchanged(&source, &before_source, &entries);
}

#[test]
fn refuses_to_use_the_source_or_its_sidecar_as_the_candidate() {
    let (_dir, source, _candidate) = layout();
    write_v1(&source, LEGACY_V1_LATE, &base_snapshot());
    let before = fs::read(&source).expect("source bytes");
    let entries = dir_names(source.parent().expect("source parent"));

    let (stage, cause) = migration_error(&source, &source);
    assert_eq!(stage, MigrationStage::PreflightCandidate);
    assert!(
        matches!(
            cause,
            Error::Validation {
                reason: ValidationReason::PathExists,
                field: "candidate",
                ..
            }
        ),
        "{cause}"
    );
    assert_source_unchanged(&source, &before, &entries);

    for suffix in ["-journal", "-wal", "-shm"] {
        let sidecar = sidecar_path(&source, suffix);
        let (stage, cause) = migration_error(&source, &sidecar);
        assert_eq!(stage, MigrationStage::PreflightCandidate, "{suffix}");
        assert!(
            matches!(
                cause,
                Error::Validation {
                    reason: ValidationReason::PathExists,
                    field: "candidate",
                    ..
                }
            ),
            "{suffix}: {cause}"
        );
        assert!(!sidecar.exists(), "{suffix} was created");
        assert_source_unchanged(&source, &before, &entries);
    }
}

#[test]
fn refuses_candidate_aliases_that_resolve_to_the_source_or_a_sidecar() {
    let (dir, source, _candidate) = layout();
    #[cfg(not(unix))]
    let _ = &dir;
    write_v1(&source, LEGACY_V1_LATE, &base_snapshot());
    let before = fs::read(&source).expect("source bytes");
    let entries = dir_names(source.parent().expect("source parent"));
    let source_parent = source.parent().expect("source parent");
    let source_name = source.file_name().expect("source name").to_string_lossy();
    let alias_parent = source_parent.join("..").join("source");

    let source_alias = alias_parent.join(source_name.as_ref());
    let (stage, _cause) = migration_error(&source, &source_alias);
    assert_eq!(stage, MigrationStage::PreflightCandidate);
    assert_source_unchanged(&source, &before, &entries);

    for suffix in ["-journal", "-wal", "-shm"] {
        let alias = alias_parent.join(format!("{source_name}{suffix}"));
        let sidecar = sidecar_path(&source, suffix);
        let (stage, _cause) = migration_error(&source, &alias);
        assert_eq!(stage, MigrationStage::PreflightCandidate, "{suffix}");
        assert!(!sidecar.exists(), "{suffix} was created through an alias");
        assert_source_unchanged(&source, &before, &entries);
    }

    #[cfg(windows)]
    {
        let case_alias = source_parent.join(format!("{}-journal", source_name.to_uppercase()));
        let (stage, _cause) = migration_error(&source, &case_alias);
        assert_eq!(stage, MigrationStage::PreflightCandidate, "case alias");
        assert!(!sidecar_path(&source, "-journal").exists());
        assert_source_unchanged(&source, &before, &entries);

        for alias_name in [
            format!("{source_name}-journal."),
            format!("{source_name}-journal "),
        ] {
            let alias = source_parent.join(alias_name);
            let (stage, _cause) = migration_error(&source, &alias);
            assert_eq!(stage, MigrationStage::PreflightCandidate, "{alias:?}");
            assert!(!sidecar_path(&source, "-journal").exists());
            assert_source_unchanged(&source, &before, &entries);
        }
    }

    #[cfg(unix)]
    {
        let symlinked_parent = dir.path().join("symlinked-source");
        std::os::unix::fs::symlink(source_parent, &symlinked_parent).expect("parent symlink");
        let alias = symlinked_parent.join(format!("{source_name}-journal"));
        let (stage, _cause) = migration_error(&source, &alias);
        assert_eq!(
            stage,
            MigrationStage::PreflightCandidate,
            "symlinked parent"
        );
        assert!(!sidecar_path(&source, "-journal").exists());
        assert_source_unchanged(&source, &before, &entries);
    }
}

#[test]
fn refuses_an_unusable_source_or_candidate_path() {
    let (_dir, source, candidate) = layout();
    write_v1(&source, LEGACY_V1_LATE, &base_snapshot());
    let missing_parent = candidate
        .parent()
        .expect("parent")
        .join("missing")
        .join("candidate.sqlite");
    let (stage, cause) = migrate_failure(&source, &missing_parent);
    assert_eq!(stage, MigrationStage::PreflightCandidate);
    assert!(
        matches!(
            cause,
            Error::Validation {
                reason: ValidationReason::PathMissing,
                ..
            }
        ),
        "{cause}"
    );

    let (_dir, source, candidate) = layout();
    let source_directory = source.parent().expect("parent").join("a-directory");
    fs::create_dir(&source_directory).expect("directory source");
    let (stage, cause) = migrate_failure(&source_directory, &candidate);
    assert_eq!(stage, MigrationStage::PreflightSource);
    assert!(
        matches!(
            cause,
            Error::Validation {
                reason: ValidationReason::NotADatabase,
                ..
            }
        ),
        "{cause}"
    );
}

#[cfg(unix)]
#[test]
fn rejects_a_symlink_source() {
    let (_dir, source, candidate) = layout();
    write_v1(&source, LEGACY_V1_LATE, &base_snapshot());
    let link = source.parent().expect("parent").join("link.sqlite");
    std::os::unix::fs::symlink(&source, &link).expect("symlink");
    let (stage, cause) = migrate_failure(&link, &candidate);
    assert_eq!(stage, MigrationStage::PreflightSource);
    assert!(
        matches!(
            cause,
            Error::Validation {
                reason: ValidationReason::NotADatabase,
                ..
            }
        ),
        "{cause}"
    );
}

#[test]
fn rejects_invalid_identifiers_and_typed_values() {
    let cases = [
        (
            "invalid page identifier",
            "PRAGMA ignore_check_constraints = ON;
             UPDATE pages SET id = 'short' WHERE id = '22222222-2222-4222-8222-222222222223';",
            "InvalidId",
        ),
        (
            "uppercase page identifier",
            "PRAGMA ignore_check_constraints = ON;
             UPDATE pages SET id = '22222222-2222-4222-8222-22222222222A'
             WHERE id = '22222222-2222-4222-8222-222222222223';",
            "InvalidId",
        ),
        (
            "invalid value class",
            "PRAGMA ignore_check_constraints = ON;
             UPDATE definitions SET value_class = 'integer'
             WHERE id = '77777777-7777-4777-8777-777777777777';",
            "invalid value class",
        ),
        (
            "invalid cardinality",
            "PRAGMA ignore_check_constraints = ON;
             UPDATE definitions SET cardinality = 'many'
             WHERE id = '77777777-7777-4777-8777-777777777777';",
            "invalid cardinality",
        ),
        (
            "invalid definition archive flag",
            "PRAGMA ignore_check_constraints = ON;
             UPDATE definitions SET archived = 2
             WHERE id = '77777777-7777-4777-8777-777777777777';",
            "invalid archived",
        ),
        (
            "invalid canon",
            "PRAGMA ignore_check_constraints = ON;
             UPDATE assertions SET canon = 'OTRO'
             WHERE id = '88888888-8888-4888-8888-888888888888';",
            "invalid canon",
        ),
        (
            "invalid assertion archive flag",
            "PRAGMA ignore_check_constraints = ON;
             UPDATE assertions SET archived = 2
             WHERE id = '88888888-8888-4888-8888-888888888888';",
            "invalid archived",
        ),
        (
            "invalid boolean value",
            "PRAGMA ignore_check_constraints = ON;
             UPDATE assertions SET bool_value = 2
             WHERE id = 'cccccccc-cccc-4ccc-8ccc-cccccccccccc';",
            "invalid boolean assertion",
        ),
        (
            "value class mismatch",
            "PRAGMA ignore_check_constraints = ON;
             UPDATE assertions SET value_kind = 'text', text_value = 'texto',
                 bool_value = NULL, target_entity_id = NULL
             WHERE id = '88888888-8888-4888-8888-888888888888';",
            "ValueMismatch",
        ),
        (
            "definition does not apply to subject",
            "UPDATE assertions SET subject_id = '66666666-6666-4666-8666-666666666666'
             WHERE id = '88888888-8888-4888-8888-888888888888';",
            "NotApplicable",
        ),
    ];

    for (label, sql, expected) in cases {
        let (_dir, source, candidate) = layout();
        write_v1(&source, LEGACY_V1_LATE, &base_snapshot());
        modify(&source, sql);
        let before = fs::read(&source).expect("source bytes");
        let entries = dir_names(source.parent().expect("source parent"));

        let (stage, cause) = migrate_failure(&source, &candidate);
        assert_eq!(stage, MigrationStage::SnapshotSource, "{label}");
        let rendered = format!("{cause}");
        assert!(rendered.contains(expected), "{label}: {rendered}");
        assert_source_unchanged(&source, &before, &entries);
    }
}

#[test]
fn source_busy_is_reported_without_retrying() {
    let (_dir, source, candidate) = layout();
    write_v1(&source, LEGACY_V1_LATE, &base_snapshot());
    let writer = Connection::open(&source).expect("writer");
    writer
        .busy_timeout(Duration::from_millis(0))
        .expect("writer timeout");
    writer
        .execute_batch("BEGIN EXCLUSIVE")
        .expect("exclusive lock");
    let before = fs::read(&source).expect("source bytes");
    let entries = dir_names(source.parent().expect("source parent"));

    let (stage, cause) = migrate_failure(&source, &candidate);
    assert!(
        matches!(
            stage,
            MigrationStage::ReadSource | MigrationStage::SnapshotSource
        ),
        "{stage}"
    );
    assert!(matches!(cause, Error::Busy), "{cause}");
    assert_source_unchanged(&source, &before, &entries);
    writer.execute_batch("ROLLBACK").expect("writer rollback");
}

#[test]
fn the_source_read_transaction_blocks_a_delete_mode_commit() {
    let (_dir, source, _candidate) = layout();
    write_v1(&source, LEGACY_V1_LATE, &base_snapshot());
    let observation = preflight_source(&source).expect("source preflight");
    let pinned = PinnedSource::open(&source, &observation).expect("pin source");
    let writer = Connection::open(&source).expect("writer");
    writer
        .busy_timeout(Duration::from_millis(0))
        .expect("writer timeout");
    writer
        .execute_batch("BEGIN IMMEDIATE")
        .expect("writer begin");
    writer
        .execute(
            "INSERT INTO pages (id, title, body) VALUES (?1, 't', 'b')",
            [PAGE_THREE],
        )
        .expect("writer insert");
    let error = writer
        .execute_batch("COMMIT")
        .expect_err("commit must block");
    assert!(
        matches!(
            error,
            rusqlite::Error::SqliteFailure(ref ffi, _)
                if ffi.code == rusqlite::ErrorCode::DatabaseBusy
                    || ffi.code == rusqlite::ErrorCode::DatabaseLocked
        ),
        "{error}"
    );
    drop(pinned);
    writer
        .execute_batch("COMMIT")
        .expect("writer commits after release");
}

#[test]
fn candidate_verification_is_read_only_and_rejects_tampering() {
    let pair = migrated_pair();
    let before = fs::read(&pair.candidate).expect("candidate bytes");
    verify_candidate(&pair.candidate, &pair.snapshot).expect("verify valid candidate");
    assert_eq!(
        fs::read(&pair.candidate).expect("candidate bytes"),
        before,
        "verification modified the candidate"
    );
    assert_no_sidecars(&pair.candidate);
    drop(pair);

    let cases = [
        (
            "records",
            "UPDATE pages SET body = body || 'x'
             WHERE id = '22222222-2222-4222-8222-222222222222';",
            "records differ",
        ),
        (
            "revision",
            "UPDATE pages SET revision = 1
             WHERE id = '22222222-2222-4222-8222-222222222222';",
            "revised pages",
        ),
        (
            "mentions",
            "INSERT INTO mentions (id, page_id, entity_id, start_utf16, end_utf16)
             VALUES ('abababab-abab-4bab-8bab-abababababab',
                     '22222222-2222-4222-8222-222222222222',
                     '55555555-5555-4555-8555-555555555555', 0, 1);",
            "occurrences",
        ),
        (
            "user version",
            "PRAGMA user_version = 3;",
            "unsupported schema version 3",
        ),
        (
            "extra table",
            "CREATE TABLE unexpected (id TEXT);",
            "ForeignDatabase",
        ),
        (
            "missing index",
            "DROP INDEX idx_assertions_target;",
            "ForeignDatabase",
        ),
        (
            "missing record",
            "DELETE FROM assertions
             WHERE id = '88888888-8888-4888-8888-888888888888';",
            "record counts differ",
        ),
        (
            "universe",
            "UPDATE metadata SET universe_id = '12121212-1212-4212-8212-121212121212';",
            "records differ",
        ),
        (
            "metadata rows",
            "PRAGMA ignore_check_constraints = ON;
             INSERT INTO metadata (singleton, universe_id)
             VALUES (2, '33333333-3333-4333-8333-333333333333');",
            "metadata",
        ),
        (
            "foreign key",
            "PRAGMA foreign_keys = OFF;
             INSERT INTO entities (id, label, type_id, page_id)
             VALUES ('12121212-1212-4212-8212-121212121212', 'huérfana',
                     '34343434-3434-4434-8434-343434343434', NULL);",
            "foreign key",
        ),
    ];

    for (label, sql, expected) in cases {
        let pair = migrated_pair();
        modify(&pair.candidate, sql);
        let before = fs::read(&pair.candidate).expect("candidate bytes");
        let error =
            verify_candidate(&pair.candidate, &pair.snapshot).expect_err("tampering must fail");
        let rendered = format!("{error}");
        assert!(rendered.contains(expected), "{label}: {rendered}");
        assert_eq!(
            fs::read(&pair.candidate).expect("candidate bytes"),
            before,
            "{label}: verification modified the candidate"
        );
        assert_no_sidecars(&pair.candidate);
    }

    let pair = migrated_pair();
    fs::write(sidecar_path(&pair.candidate, "-wal"), b"sidecar").expect("candidate sidecar");
    let error = verify_candidate(&pair.candidate, &pair.snapshot).expect_err("sidecar must fail");
    assert!(
        matches!(
            error,
            Error::Validation {
                reason: ValidationReason::ForeignDatabase,
                field: "sidecar",
                ..
            }
        ),
        "{error}"
    );
}

struct MigratedPair {
    _dir: TempDir,
    candidate: PathBuf,
    snapshot: Snapshot,
}

fn migrated_pair() -> MigratedPair {
    let (dir, source, candidate) = layout();
    let snapshot = base_snapshot();
    write_v1(&source, LEGACY_V1_LATE, &snapshot);
    build_verified_candidate(&source, &candidate).expect("migration");
    MigratedPair {
        _dir: dir,
        candidate,
        snapshot,
    }
}

#[test]
fn reports_do_not_contain_page_prose_or_raw_domain_values() {
    let cases = [
        (
            "canon",
            "UPDATE assertions SET canon = '{marker}'
             WHERE id = (SELECT id FROM assertions LIMIT 1);",
            "invalid canon",
        ),
        (
            "value_class",
            "UPDATE definitions SET value_class = '{marker}'
             WHERE id = (SELECT id FROM definitions LIMIT 1);",
            "invalid value class",
        ),
        (
            "cardinality",
            "UPDATE definitions SET cardinality = '{marker}'
             WHERE id = (SELECT id FROM definitions LIMIT 1);",
            "invalid cardinality",
        ),
        (
            "value_kind",
            "UPDATE assertions SET value_kind = '{marker}'
             WHERE id = (SELECT id FROM assertions LIMIT 1);",
            "invalid value kind",
        ),
        (
            "page id",
            "UPDATE pages SET id = '{marker}'
             WHERE id = (SELECT id FROM pages ORDER BY id DESC LIMIT 1);",
            "InvalidId on page",
        ),
        (
            "target id",
            "UPDATE assertions SET target_entity_id = '{marker}'
             WHERE value_kind = 'reference';",
            "foreign key",
        ),
        (
            "universe id",
            "UPDATE metadata SET universe_id = '{marker}';",
            "InvalidId on universe_id",
        ),
    ];

    for (label, sql, expected) in cases {
        let (_dir, source, candidate) = layout();
        let mut snapshot = base_snapshot();
        snapshot.pages[0].page.title = "SECRETO_TITULO".to_string();
        snapshot.pages[0].page.body = "SECRETO_CUERPO".to_string();
        write_v1(&source, LEGACY_V1_LATE, &snapshot);
        let marker = format!("SECRETO_{}", label.to_uppercase().replace(' ', "_"));
        let sql = format!(
            "PRAGMA foreign_keys = OFF;\nPRAGMA ignore_check_constraints = ON;\n{}",
            sql.replace("{marker}", &marker)
        );
        modify(&source, &sql);

        let failure =
            build_verified_candidate(&source, &candidate).expect_err("malformed source must fail");
        assert_eq!(failure.stage, MigrationStage::SnapshotSource, "{label}");
        let rendered = format!("{failure}");
        assert!(rendered.contains(expected), "{label}: {rendered}");
        assert!(!rendered.contains(&marker), "{label}: {rendered}");
        assert!(!rendered.contains("SECRETO_TITULO"), "{label}: {rendered}");
        assert!(!rendered.contains("SECRETO_CUERPO"), "{label}: {rendered}");
        assert!(!candidate.exists(), "{label}");
    }
}

#[test]
fn migrates_a_page_larger_than_ten_mebibytes() {
    let (_dir, source, candidate) = layout();
    let mut snapshot = base_snapshot();
    snapshot.pages[0].page.body = "x".repeat(10 * 1024 * 1024 + 1);
    write_v1(&source, LEGACY_V1_LATE, &snapshot);
    let before = fs::read(&source).expect("source bytes");
    let entries = dir_names(source.parent().expect("source parent"));

    build_verified_candidate(&source, &candidate).expect("large migration");

    assert_source_unchanged(&source, &before, &entries);
    assert_no_sidecars(&candidate);
    assert!(
        fs::metadata(&candidate).expect("candidate metadata").len() > 10 * 1024 * 1024,
        "candidate did not carry the large page"
    );
    let mut store = Store::open(&candidate).expect("open candidate");
    let actual = store.export_snapshot().expect("export");
    assert!(actual == snapshot, "large snapshot differs");
}
