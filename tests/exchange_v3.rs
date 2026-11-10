//! Acceptance tests for PA-I5/R1-A: the validated definition label, the
//! version 3 JSON format, and the preserved-version-2 database refusal.
//!
//! The label rule has exactly one owner (`DefinitionLabel::parse`). These tests
//! drive every entry point that must consult it: version 3 JSON decoding,
//! record creation, stored-row reads, logical verification, and the synthetic
//! version 3 fixture.

#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use narrative_universe::{
    export_json, import_json, import_new_json, verify_current, Canon, Cardinality, Definition,
    DefinitionId, DefinitionLabel, Entity, EntityId, EntityType, Error, Owner, PageId, Store,
    TypeId, UniverseId, ValidationReason, ValueClass,
};
use rusqlite::Connection;
use serde_json::{json, Value as Json};
use support::{bytes, scratch};

const UNIVERSE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const ENTITY_ONE: &str = "11111111-1111-4111-8111-111111111111";
const TYPE_ONE: &str = "44444444-4444-4444-8444-444444444444";
const DEFINITION_ONE: &str = "77777777-7777-4777-8777-777777777777";
const DEFINITION_TWO: &str = "99999999-9999-4999-8999-999999999999";
const ASSERTION_ONE: &str = "88888888-8888-4888-8888-888888888888";

const REVEAL_FIXTURE: &str = include_str!("fixtures/reveal-facts.json");
const V2_SCHEMA: &str = include_str!("../src/migration/legacy_v2.sql");

fn universe() -> UniverseId {
    UniverseId::parse(UNIVERSE).expect("universe")
}

fn definition_id(raw: &str) -> DefinitionId {
    DefinitionId::parse(raw).expect("definition")
}

fn type_id() -> TypeId {
    TypeId::parse(TYPE_ONE).expect("type")
}

fn entity_id() -> EntityId {
    EntityId::parse(ENTITY_ONE).expect("entity")
}

fn created(name: &str) -> (support::Scratch, Store) {
    let scratch = scratch(name);
    let store = Store::create(&scratch.path, universe()).expect("create");
    (scratch, store)
}

/// A store with one type and one entity, so a definition can have a real owner.
fn with_owner(name: &str) -> (support::Scratch, Store) {
    let (scratch, mut store) = created(name);
    store
        .create_entity_type(EntityType {
            id: type_id(),
            label: "Person".to_string(),
        })
        .expect("type");
    store
        .create_entity(Entity {
            id: entity_id(),
            label: "Mara Vey".to_string(),
            type_id: Some(type_id()),
            page_id: None,
        })
        .expect("entity");
    (scratch, store)
}

fn definition(label: Option<&str>) -> Definition {
    Definition {
        id: definition_id(DEFINITION_ONE),
        owner: Owner::Type(type_id()),
        value_class: ValueClass::Text,
        cardinality: Cardinality::Singular,
        label: label.map(|raw| DefinitionLabel::parse(raw).expect("label")),
        archived: false,
    }
}

/// A version 3 definition record carrying the given JSON `label` member.
fn definition_record(label: Json) -> Json {
    json!({
        "id": DEFINITION_ONE,
        "owner": {"type": "type", "id": TYPE_ONE},
        "value_class": "text",
        "cardinality": "singular",
        "archived": false,
        "label": label,
    })
}

fn document(version: i64, definitions: Json) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "format_version": version,
        "universe_id": UNIVERSE,
        "pages": [],
        "types": [],
        "entities": [],
        "definitions": definitions,
        "assertions": [],
    }))
    .expect("document")
}

fn label_error(err: &Error) -> bool {
    matches!(
        err,
        Error::Validation {
            reason: ValidationReason::InvalidLabel,
            field: "label",
            id: None,
        }
    )
}

#[test]
fn a_legacy_definition_with_a_label_is_not_an_unnamed_definition() {
    let (_scratch, mut store) = with_owner("legacy-label.sqlite");
    let before = store.counts().expect("counts");

    for version in [1, 2] {
        let err = import_json(
            &mut store,
            &document(version, json!([definition_record(json!("member of"))])),
        )
        .expect_err("a legacy definition cannot carry a label");
        assert!(matches!(err, Error::InvalidDocument { .. }), "{err}");
        assert_eq!(store.counts().expect("counts"), before, "version {version}");
    }

    for version in [1, 2] {
        let missing = scratch(&format!("legacy-label-new-v{version}.sqlite"));
        let err = import_new_json(
            &missing.path,
            &document(version, json!([definition_record(json!("member of"))])),
        )
        .expect_err("import-new refuses before reserving the path");
        assert!(
            matches!(err, Error::InvalidDocument { .. }),
            "version {version}: {err}"
        );
        assert!(
            !missing.path.exists(),
            "version {version}: a rejected document left a file"
        );
    }
}

#[test]
fn legacy_documents_still_import_as_unnamed_definitions() {
    for version in [1, 2] {
        let (_scratch, mut store) = with_owner(&format!("unnamed-v{version}.sqlite"));
        let record = json!({
            "id": DEFINITION_ONE,
            "owner": {"type": "type", "id": TYPE_ONE},
            "value_class": "text",
            "cardinality": "singular",
            "archived": false,
        });
        let outcome = import_json(&mut store, &document(version, json!([record])))
            .unwrap_or_else(|err| panic!("version {version}: {err}"));
        assert_eq!(outcome.inserted, 1);
        assert_eq!(
            store
                .definition(&definition_id(DEFINITION_ONE))
                .expect("definition")
                .label,
            None,
            "version {version}"
        );

        let exported: Json =
            serde_json::from_slice(&export_json(&mut store).expect("export")).expect("document");
        assert_eq!(exported["format_version"], 3);
        assert_eq!(exported["definitions"][0]["label"], Json::Null);
    }
}

#[test]
fn a_version_three_definition_without_a_label_member_is_unnamed() {
    let (_scratch, mut store) = with_owner("absent-label.sqlite");
    let record = json!({
        "id": DEFINITION_ONE,
        "owner": {"type": "type", "id": TYPE_ONE},
        "value_class": "text",
        "cardinality": "singular",
        "archived": false,
    });
    import_json(&mut store, &document(3, json!([record]))).expect("absent label");
    assert_eq!(
        store
            .definition(&definition_id(DEFINITION_ONE))
            .expect("definition")
            .label,
        None
    );

    let exported: Json =
        serde_json::from_slice(&export_json(&mut store).expect("export")).expect("document");
    // The writer always emits the key, including null: "unnamed" never depends
    // on an omission.
    assert_eq!(
        exported["definitions"][0]
            .as_object()
            .expect("definition")
            .get("label"),
        Some(&Json::Null)
    );
}

#[test]
fn an_invalid_label_string_is_refused_without_writing_anything() {
    let (_scratch, mut store) = with_owner("invalid-label.sqlite");
    let before = store.counts().expect("counts");
    let over_limit = "a".repeat(DefinitionLabel::MAX_SCALARS + 1);
    let cases: [(&str, &str); 11] = [
        ("empty", ""),
        ("whitespace only", "   "),
        ("leading space", " member of"),
        ("trailing space", "member of "),
        ("leading tab", "\tmember of"),
        ("trailing newline", "member of\n"),
        ("inner tab", "member\tof"),
        ("leading no-break space", "\u{00a0}member of"),
        ("trailing ideographic space", "member of\u{3000}"),
        ("nul character", "member\u{0000}of"),
        ("over the limit", over_limit.as_str()),
    ];

    for (name, label) in cases {
        let err = import_json(
            &mut store,
            &document(3, json!([definition_record(json!(label))])),
        )
        .expect_err("an invalid label must be refused");
        assert!(label_error(&err), "{name}: {err}");
        assert_eq!(store.counts().expect("counts"), before, "{name}");
    }

    assert!(
        store.definition(&definition_id(DEFINITION_ONE)).is_err(),
        "no definition may be created by a rejected document"
    );
    drop(store);

    let other = scratch("invalid-label-new.sqlite");
    let err = import_new_json(
        &other.path,
        &document(3, json!([definition_record(json!(" member of"))])),
    )
    .expect_err("import-new refuses before reserving the path");
    assert!(label_error(&err), "{err}");
    assert!(!other.path.exists(), "a rejected document left a file");
}

#[test]
fn a_whitespace_only_label_is_refused_and_leaves_no_file() {
    let rejected = scratch("whitespace-label-new.sqlite");
    let err = import_new_json(
        &rejected.path,
        &document(3, json!([definition_record(json!("   "))])),
    )
    .expect_err("import-new refuses a whitespace-only label");
    assert!(label_error(&err), "{err}");
    assert!(!rejected.path.exists(), "a rejected document left a file");
}

#[test]
fn a_label_member_that_is_not_a_single_string_is_refused() {
    let (_scratch, mut store) = with_owner("label-shape.sqlite");
    let before = store.counts().expect("counts");

    for (name, label) in [
        ("number", json!(5)),
        ("object", json!({})),
        ("array", json!(["member of"])),
        ("boolean", json!(true)),
    ] {
        let err = import_json(&mut store, &document(3, json!([definition_record(label)])))
            .expect_err("a label must be a string or null");
        assert!(
            matches!(err, Error::InvalidDocument { .. }),
            "{name}: {err}"
        );
    }

    let duplicate = format!(
        r#"{{"format_version":3,"universe_id":"{UNIVERSE}","pages":[],"types":[],"entities":[],"definitions":[{{"id":"{DEFINITION_ONE}","owner":{{"type":"type","id":"{TYPE_ONE}"}},"value_class":"text","cardinality":"singular","archived":false,"label":"member of","label":"born in"}}],"assertions":[]}}"#
    );
    let err = import_json(&mut store, duplicate.as_bytes()).expect_err("duplicate label member");
    assert!(matches!(err, Error::InvalidDocument { .. }), "{err}");

    let unknown = format!(
        r#"{{"format_version":3,"universe_id":"{UNIVERSE}","pages":[],"types":[],"entities":[],"definitions":[{{"id":"{DEFINITION_ONE}","owner":{{"type":"type","id":"{TYPE_ONE}"}},"value_class":"text","cardinality":"singular","archived":false,"labels":"member of"}}],"assertions":[]}}"#
    );
    let err = import_json(&mut store, unknown.as_bytes()).expect_err("unknown member");
    assert!(matches!(err, Error::InvalidDocument { .. }), "{err}");

    assert_eq!(store.counts().expect("counts"), before);
}

#[test]
fn a_null_label_is_the_only_spelling_of_unnamed() {
    let (_scratch, mut store) = with_owner("null-label.sqlite");
    import_json(
        &mut store,
        &document(3, json!([definition_record(Json::Null)])),
    )
    .expect("null label");
    assert_eq!(
        store
            .definition(&definition_id(DEFINITION_ONE))
            .expect("definition")
            .label,
        None
    );
    // An empty string is not another spelling of null.
    let err = import_json(
        &mut store,
        &document(3, json!([definition_record(json!(""))])),
    )
    .expect_err("empty string");
    assert!(label_error(&err), "{err}");
}

/// The label rule is shared, so a valid label of exactly the maximum length is
/// accepted, kept byte for byte, and survives a reopen and a JSON round trip.
#[test]
fn a_label_is_kept_exactly_and_its_boundary_is_inclusive() {
    let decomposed = "e\u{0301}lara \u{1f4da} member of";

    let (scratch, mut store) = with_owner("label-boundary.sqlite");
    store
        .create_definition(definition(Some(decomposed)))
        .expect("definition");
    drop(store);

    let mut store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(
        store
            .definition(&definition_id(DEFINITION_ONE))
            .expect("definition")
            .label
            .as_ref()
            .map(DefinitionLabel::as_str),
        Some(decomposed)
    );

    let exported = export_json(&mut store).expect("export");
    let document: Json = serde_json::from_slice(&exported).expect("document");
    assert_eq!(document["definitions"][0]["label"], json!(decomposed));

    let imported_path = scratch.path.with_file_name("label-imported.sqlite");
    let (imported, _outcome) = import_new_json(&imported_path, &exported).expect("import-new");
    assert_eq!(
        imported
            .definition(&definition_id(DEFINITION_ONE))
            .expect("definition")
            .label
            .as_ref()
            .map(DefinitionLabel::as_str),
        Some(decomposed)
    );

    let longest = "l".repeat(DefinitionLabel::MAX_SCALARS);
    assert!(DefinitionLabel::parse(&longest).is_ok());
    assert!(DefinitionLabel::parse(&format!("{longest}l")).is_err());
    assert!(DefinitionLabel::parse("").is_err());
    assert!(DefinitionLabel::parse(" ").is_err());
}

#[test]
fn conflicting_labels_collide_in_both_directions_and_change_nothing() {
    let cases: [(Option<&str>, Option<&str>); 4] = [
        (Some("member of"), Some("born in")),
        (Some("born in"), Some("member of")),
        (Some("member of"), None),
        (None, Some("member of")),
    ];
    for (first, second) in cases {
        let (_scratch, mut store) = with_owner("label-conflict.sqlite");
        import_json(
            &mut store,
            &document(3, json!([definition_record(json!(first))])),
        )
        .expect("first import");
        let before = store.counts().expect("counts");

        // The brand-new record comes first, so it is inserted before the
        // conflicting one is reached and the rollback is observable.
        let fresh = json!({
            "id": DEFINITION_TWO,
            "owner": {"type": "type", "id": TYPE_ONE},
            "value_class": "text",
            "cardinality": "singular",
            "archived": false,
            "label": "fresh",
        });
        let err = import_json(
            &mut store,
            &document(3, json!([fresh, definition_record(json!(second))])),
        )
        .expect_err("a different label cannot reuse the definition id");
        match err {
            Error::ImportConflict { class, id } => {
                assert_eq!(class, "definition", "{first:?} then {second:?}");
                assert_eq!(id, DEFINITION_ONE, "{first:?} then {second:?}");
            }
            other => panic!("expected import conflict, got {other}"),
        }
        assert_eq!(
            store.counts().expect("counts"),
            before,
            "{first:?} then {second:?}"
        );
        assert!(
            store.definition(&definition_id(DEFINITION_TWO)).is_err(),
            "{first:?} then {second:?}: the batch's new record survived the rollback"
        );
        assert_eq!(
            store
                .definition(&definition_id(DEFINITION_ONE))
                .expect("definition")
                .label
                .as_ref()
                .map(DefinitionLabel::as_str),
            first
        );
    }
}

#[test]
fn the_synthetic_version_three_fixture_round_trips_intact() {
    let scratch = scratch("reveal-facts.sqlite");
    let (mut store, outcome) =
        import_new_json(&scratch.path, REVEAL_FIXTURE.as_bytes()).expect("fixture imports");
    assert_eq!(outcome.inserted, 1 + 3 + 6 + 8 + 12);
    assert_eq!(outcome.unchanged, 0);

    let expected: Json = serde_json::from_str(REVEAL_FIXTURE).expect("fixture document");
    let exported = export_json(&mut store).expect("export");
    let actual: Json = serde_json::from_slice(&exported).expect("exported document");
    assert_eq!(actual, expected, "the fixture is not a stable round trip");
    assert_eq!(actual["format_version"], 3);

    let counts = store.counts().expect("counts");
    assert_eq!(counts.pages, 1);
    assert_eq!(counts.types, 3);
    assert_eq!(counts.entities, 6);
    assert_eq!(counts.definitions, 8);
    assert_eq!(counts.assertions, 12);

    // The duplicate label is legal and the stable id is the identity: two
    // definitions on the same owner share "member of" and stay distinct.
    let first = store
        .definition(&definition_id("00000000-0000-4000-8000-000000000031"))
        .expect("member of");
    let duplicate = store
        .definition(&definition_id("00000000-0000-4000-8000-000000000038"))
        .expect("duplicate member of");
    assert_ne!(first.id, duplicate.id);
    assert_eq!(first.owner, duplicate.owner);
    assert_eq!(first.label, duplicate.label);
    assert_eq!(
        first.label.as_ref().map(DefinitionLabel::as_str),
        Some("member of")
    );

    let unnamed = store
        .definition(&definition_id("00000000-0000-4000-8000-000000000037"))
        .expect("unnamed definition");
    assert_eq!(unnamed.label, None);

    let owned = store
        .definition(&definition_id("00000000-0000-4000-8000-000000000036"))
        .expect("entity-owned definition");
    assert!(owned.archived);
    assert_eq!(
        owned.owner,
        Owner::Entity(EntityId::parse("00000000-0000-4000-8000-000000000011").expect("entity"))
    );

    // Both competing claims are present, so the fixture can drive the reveal
    // rules later without inventing data.
    let subject = EntityId::parse("00000000-0000-4000-8000-000000000011").expect("entity");
    let claims = store.assertions_from_subject(&subject).expect("claims");
    assert!(claims.len() >= 5);
    let canons: Vec<Canon> = claims.iter().map(|assertion| assertion.canon).collect();
    assert!(canons.contains(&Canon::Provisional));
    assert!(canons.contains(&Canon::Abierto));
    assert!(canons.contains(&Canon::Acordado));
    drop(store);

    // The fixture must also pass logical verification, not only SQLite's own
    // checks: it is the reviewable v3 sample.
    let report = verify_current(&scratch.path).expect("fixture verifies");
    assert_eq!(report.counts, counts);
    assert_eq!(report.mentions, 2);
}

#[test]
fn a_version_two_document_imports_with_null_labels_and_never_writes_v2() {
    let document = json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [],
        "types": [{"id": TYPE_ONE, "label": "Person"}],
        "entities": [{"id": ENTITY_ONE, "label": "Mara Vey", "type_id": TYPE_ONE, "page_id": null}],
        "definitions": [{
            "id": DEFINITION_ONE,
            "owner": {"type": "type", "id": TYPE_ONE},
            "value_class": "text",
            "cardinality": "singular",
            "archived": false
        }],
        "assertions": [{
            "id": ASSERTION_ONE,
            "subject_id": ENTITY_ONE,
            "definition_id": DEFINITION_ONE,
            "value": {"kind": "text", "text": "ford"},
            "canon": "ACORDADO",
            "source_status_raw": null,
            "provenance": {},
            "scope_note": null,
            "archived": false
        }]
    });
    let scratch = scratch("v2-document.sqlite");
    let (mut store, outcome) = import_new_json(
        &scratch.path,
        &serde_json::to_vec(&document).expect("document"),
    )
    .expect("version 2 document");
    assert_eq!(outcome.inserted, 4);
    assert_eq!(
        store
            .definition(&definition_id(DEFINITION_ONE))
            .expect("definition")
            .label,
        None
    );

    let exported: Json =
        serde_json::from_slice(&export_json(&mut store).expect("export")).expect("document");
    assert_eq!(exported["format_version"], 3);
    assert_eq!(exported["definitions"][0]["label"], Json::Null);
    assert_eq!(
        exported["assertions"][0]["value"],
        json!({"kind": "text", "text": "ford"})
    );
}

#[test]
fn a_stored_invalid_label_is_rejected_by_reads_and_verification() {
    let tampered = [" trailing", "\ttrailing", "\u{00a0}trailing"];
    for (index, stored) in tampered.into_iter().enumerate() {
        let (scratch, mut store) = with_owner(&format!("stored-invalid-{index}.sqlite"));
        store
            .create_definition(definition(Some("member of")))
            .expect("definition");
        drop(store);

        {
            let conn = Connection::open(&scratch.path).expect("raw");
            conn.execute_batch("PRAGMA ignore_check_constraints = ON")
                .expect("disable checks");
            conn.execute(
                "UPDATE definitions SET label = ?1 WHERE id = ?2",
                [stored, DEFINITION_ONE],
            )
            .expect("tamper");
        }

        let store = Store::open(&scratch.path).expect("open");
        let err = store
            .definition(&definition_id(DEFINITION_ONE))
            .expect_err("a stored label that breaks the rule is not readable");
        let rendered = format!("{err}");
        assert!(
            rendered.contains("invalid stored definition label"),
            "{stored:?}: {rendered}"
        );
        assert!(
            !rendered.contains("trailing"),
            "{stored:?}: the rejected label must not be echoed: {rendered}"
        );
        drop(store);

        let err = verify_current(&scratch.path).expect_err("verify rejects the stored label");
        assert!(
            format!("{err}").contains("invalid stored definition label"),
            "{stored:?}: {err}"
        );
    }
}

#[test]
fn a_preserved_version_two_database_is_refused_intact_with_its_own_message() {
    let scratch = scratch("preserved-v2.sqlite");
    {
        let conn = Connection::open(&scratch.path).expect("raw");
        conn.execute_batch(V2_SCHEMA).expect("version 2 schema");
        conn.execute_batch("PRAGMA user_version = 2")
            .expect("version 2 header");
        conn.execute(
            "INSERT INTO metadata (singleton, universe_id) VALUES (1, ?1)",
            [UNIVERSE],
        )
        .expect("metadata");
    }
    let before = bytes(&scratch.path);
    let modified = std::fs::metadata(&scratch.path)
        .and_then(|meta| meta.modified())
        .expect("mtime");

    let err = match Store::open(&scratch.path) {
        Ok(_) => panic!("a version 2 database must not open"),
        Err(err) => err,
    };
    match &err {
        Error::UnsupportedDatabase { found, supported } => {
            assert_eq!(*found, 2);
            assert_eq!(*supported, 3);
        }
        other => panic!("expected UnsupportedDatabase, got {other}"),
    }
    let refusal = format!("{err}");
    assert!(refusal.contains("schema version 2"), "{refusal}");
    assert!(refusal.contains("migrates version 1 only"), "{refusal}");

    assert_eq!(bytes(&scratch.path), before, "the file changed");
    assert!(!scratch.path.with_extension("sqlite-journal").exists());

    // `verify` reads the current version only. A known preserved version gets
    // the same distinct refusal, never a claim that an in-place upgrade exists.
    let err = verify_current(&scratch.path).expect_err("verify refuses version 2");
    assert!(
        matches!(
            err.cause,
            Error::UnsupportedDatabase {
                found: 2,
                supported: 3
            }
        ),
        "{err}"
    );
    let verified = format!("{err}");
    assert!(
        verified.contains("not supported by this build"),
        "{verified}"
    );
    assert!(!verified.contains("needs a migration"), "{verified}");
    assert_eq!(bytes(&scratch.path), before);
    assert_eq!(
        std::fs::metadata(&scratch.path)
            .and_then(|meta| meta.modified())
            .expect("mtime"),
        modified,
        "both refusals must leave the file's modification time alone"
    );
}

#[test]
fn a_file_that_only_claims_version_two_is_foreign_not_preserved() {
    // A current database relabelled to version 2: it has the label column, so
    // it is not a preserved version 2 universe.
    let (scratch, store) = created("forged-v2.sqlite");
    drop(store);
    {
        let conn = Connection::open(&scratch.path).expect("raw");
        conn.execute_batch("PRAGMA user_version = 2")
            .expect("relabel");
    }
    let err = Store::open(&scratch.path).expect_err("forged version 2");
    assert!(
        matches!(
            err,
            Error::Validation {
                reason: ValidationReason::ForeignDatabase,
                ..
            }
        ),
        "{err}"
    );

    // A genuine version 2 header on a schema that is not ours is foreign too.
    let scratch = support::scratch("foreign-v2.sqlite");
    {
        let conn = Connection::open(&scratch.path).expect("raw");
        conn.execute_batch(
            "CREATE TABLE metadata (
                 singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                 universe_id TEXT NOT NULL
             );
             INSERT INTO metadata (singleton, universe_id) VALUES (1, 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa');
             PRAGMA user_version = 2;",
        )
        .expect("partial schema");
    }
    let err =
        Store::open(&scratch.path).expect_err("a stock table set is not a version 2 universe");
    assert!(
        matches!(
            err,
            Error::Validation {
                reason: ValidationReason::ForeignDatabase,
                ..
            }
        ),
        "{err}"
    );
}

/// A schema impostor carries the version 2 header and all seven version 2 table
/// names, or the version 2 DDL with one object added or missing. None of them
/// is a preserved version 2 universe, and the refusal leaves each file intact.
#[test]
fn a_version_two_impostor_with_every_table_name_is_foreign() {
    let skeletal = "CREATE TABLE metadata (singleton INTEGER PRIMARY KEY, universe_id TEXT);
         CREATE TABLE pages (id TEXT PRIMARY KEY);
         CREATE TABLE types (id TEXT PRIMARY KEY);
         CREATE TABLE entities (id TEXT PRIMARY KEY);
         CREATE TABLE mentions (id TEXT PRIMARY KEY);
         CREATE TABLE definitions (id TEXT PRIMARY KEY, owner_type_id TEXT);
         CREATE TABLE assertions (id TEXT PRIMARY KEY, subject_id TEXT);"
        .to_string();
    let impostors = [
        ("skeletal-v2.sqlite", skeletal),
        (
            "extra-table-v2.sqlite",
            format!(
                "{V2_SCHEMA}
CREATE TABLE notes (id TEXT PRIMARY KEY);"
            ),
        ),
        (
            "missing-index-v2.sqlite",
            format!(
                "{V2_SCHEMA}
DROP INDEX idx_assertions_target;"
            ),
        ),
    ];
    for (name, ddl) in impostors {
        let scratch = support::scratch(name);
        {
            let conn = Connection::open(&scratch.path).expect("raw");
            conn.execute_batch(&ddl).expect("impostor schema");
            conn.execute(
                "INSERT INTO metadata (singleton, universe_id) VALUES (1, ?1)",
                [UNIVERSE],
            )
            .expect("metadata");
            conn.execute_batch("PRAGMA user_version = 2")
                .expect("version 2 header");
        }
        let before = bytes(&scratch.path);

        let err = match Store::open(&scratch.path) {
            Ok(_) => panic!("{name} must not open"),
            Err(err) => err,
        };
        assert!(
            matches!(
                err,
                Error::Validation {
                    reason: ValidationReason::ForeignDatabase,
                    ..
                }
            ),
            "{name}: {err}"
        );
        assert_eq!(bytes(&scratch.path), before, "{name} changed");
    }
}

/// Exporting the fixture, importing that export into a new file, and exporting
/// again yields the same bytes, so the export is a fixed point of the import.
#[test]
fn the_synthetic_fixture_export_is_a_byte_stable_fixed_point() {
    let source = scratch("reveal-facts-source.sqlite");
    let (mut first, outcome) =
        import_new_json(&source.path, REVEAL_FIXTURE.as_bytes()).expect("fixture imports");
    assert_eq!(outcome.inserted, 1 + 3 + 6 + 8 + 12);
    let first_export = export_json(&mut first).expect("first export");
    let expected: Json = serde_json::from_str(REVEAL_FIXTURE).expect("fixture document");
    let first_document: Json = serde_json::from_slice(&first_export).expect("first export");
    assert_eq!(
        first_document, expected,
        "the first export is not the fixture"
    );

    let copy = scratch("reveal-facts-copy.sqlite");
    let (mut second, outcome) =
        import_new_json(&copy.path, &first_export).expect("import of the first export");
    assert_eq!(outcome.inserted, 1 + 3 + 6 + 8 + 12);
    assert_eq!(outcome.unchanged, 0);
    let second_export = export_json(&mut second).expect("second export");
    assert_eq!(
        second_export, first_export,
        "the second export differs from the first"
    );
}

/// One page aggregate keeps its body and its occurrences when the store is
/// reopened, so the fixture's label work did not disturb page reading.
#[test]
fn the_fixture_page_and_occurrences_survive_a_reopen() {
    let scratch = scratch("reveal-page.sqlite");
    let (store, _outcome) =
        import_new_json(&scratch.path, REVEAL_FIXTURE.as_bytes()).expect("fixture imports");
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    let page = store
        .page_document(&PageId::parse("00000000-0000-4000-8000-000000000001").expect("page"))
        .expect("page document");
    assert_eq!(
        page.body.body,
        "Mara Vey crossed the ford with the Ashfall Order."
    );
    assert_eq!(page.body.mentions.len(), 2);
    assert_eq!(page.body.mentions[0].start_utf16, 0);
    assert_eq!(page.body.mentions[1].start_utf16, 35);
    assert_eq!(page.body.mentions[1].end_utf16, 48);
}
