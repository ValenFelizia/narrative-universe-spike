//! Adversarial probes against the version 2 JSON exchange.
//!
//! Every probe is deterministic and runs against a real SQLite file. A probe
//! fails when the guard it checks is removed, so each one asserts literal
//! values and not only counts.

#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use narrative_universe::{
    export_json, import_json, import_new_json, Entity, EntityId, Error, Mention, MentionEdit,
    MentionId, Page, PageEdit, PageId, PageRevision, Store, UniverseId, ValidationReason,
};
use rusqlite::Connection;
use support::scratch;

const UNIVERSE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const PAGE_ONE: &str = "22222222-2222-4222-8222-222222222221";
const PAGE_TWO: &str = "22222222-2222-4222-8222-222222222222";
const PAGE_THREE: &str = "22222222-2222-4222-8222-222222222223";
const ENTITY_ONE: &str = "11111111-1111-4111-8111-111111111111";
const ENTITY_TWO: &str = "33333333-3333-4333-8333-333333333333";

fn universe() -> UniverseId {
    UniverseId::parse(UNIVERSE).expect("universe")
}

fn page_id(raw: &str) -> PageId {
    PageId::parse(raw).expect("page")
}

fn entity(raw: &str) -> EntityId {
    EntityId::parse(raw).expect("entity")
}

/// A canonical occurrence id built from a number.
fn occurrence_id(index: u32) -> String {
    format!("dddddddd-dddd-4ddd-8ddd-{index:012x}")
}

fn mention(id: &str, entity_raw: &str, start_utf16: u32, end_utf16: u32) -> Mention {
    Mention {
        id: MentionId::parse(id).expect("mention id"),
        entity_id: entity(entity_raw),
        start_utf16,
        end_utf16,
    }
}

fn link(occurrence: Mention) -> PageEdit {
    PageEdit::Mention(MentionEdit::Link {
        mention: occurrence,
    })
}

fn created(name: &str) -> (support::Scratch, Store) {
    let scratch = scratch(name);
    let store = Store::create(&scratch.path, universe()).expect("create");
    (scratch, store)
}

fn new_page(raw: &str, title: &str, body: &str) -> Page {
    Page {
        id: page_id(raw),
        title: title.to_string(),
        body: body.to_string(),
    }
}

fn create_entity(store: &mut Store, raw: &str, label: &str) {
    store
        .create_entity(Entity {
            id: entity(raw),
            label: label.to_string(),
            type_id: None,
            page_id: None,
        })
        .expect("entity");
}

fn assert_validation(err: Error, reason: ValidationReason, field: &str) {
    match err {
        Error::Validation {
            reason: got_reason,
            field: got_field,
            ..
        } => {
            assert_eq!(got_reason, reason);
            assert_eq!(got_field, field);
        }
        other => panic!("expected validation {reason:?}, got {other}"),
    }
}

fn assert_conflict(err: Error, class: &str, id: &str) {
    match err {
        Error::ImportConflict {
            class: got_class,
            id: got_id,
        } => {
            assert_eq!(got_class, class);
            assert_eq!(got_id, id);
        }
        other => panic!("expected import conflict {class} {id}, got {other}"),
    }
}

/// The number of occurrence rows the database really holds.
fn stored_occurrences(path: &std::path::Path) -> i64 {
    let conn = Connection::open(path).expect("raw");
    conn.query_row("SELECT COUNT(*) FROM mentions", [], |row| row.get(0))
        .expect("count")
}

/// No key of the exported document is a local revision.
fn assert_no_revision(value: &serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                assert_ne!(key, "revision", "the export carries a local revision");
                assert_no_revision(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                assert_no_revision(item);
            }
        }
        _ => {}
    }
}

#[test]
fn a_version_1_page_with_an_empty_occurrence_array_is_refused() {
    let (_scratch, mut store) = created("v1-empty-mentions.sqlite");
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 1,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Con el arreglo vacío",
            "body": "Elara",
            "mentions": []
        }],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");

    let err = import_json(&mut store, &document).expect_err("malformed");
    assert!(matches!(err, Error::InvalidDocument { .. }), "{err}");
    // The page was never read as an empty occurrence set.
    assert_eq!(store.counts().expect("counts").pages, 0);
    assert!(store
        .page(&page_id(PAGE_ONE))
        .expect_err("absent")
        .to_string()
        .contains("NotFound"));
}

#[test]
fn a_version_2_page_with_a_duplicate_occurrence_id_is_refused() {
    let (_scratch, mut store) = created("duplicate-inside.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Repetido",
            "body": "Elara",
            "mentions": [
                {
                    "id": occurrence_id(1),
                    "entity_id": ENTITY_ONE,
                    "start_utf16": 0,
                    "end_utf16": 5
                },
                {
                    "id": occurrence_id(1),
                    "entity_id": ENTITY_ONE,
                    "start_utf16": 0,
                    "end_utf16": 5
                }
            ]
        }],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");

    let err = import_json(&mut store, &document).expect_err("duplicate");
    assert_validation(err, ValidationReason::MentionIdConflict, "mention_id");
    assert_eq!(store.counts().expect("counts").pages, 0);
    assert_eq!(stored_occurrences(&_scratch.path), 0);
}

#[test]
fn two_pages_that_carry_the_same_occurrence_id_are_refused() {
    let (_scratch, mut store) = created("duplicate-across.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [
            {
                "id": PAGE_ONE,
                "title": "Primera",
                "body": "Elara",
                "mentions": [{
                    "id": occurrence_id(2),
                    "entity_id": ENTITY_ONE,
                    "start_utf16": 0,
                    "end_utf16": 5
                }]
            },
            {
                "id": PAGE_TWO,
                "title": "Segunda",
                "body": "Elara",
                "mentions": [{
                    "id": occurrence_id(2),
                    "entity_id": ENTITY_ONE,
                    "start_utf16": 0,
                    "end_utf16": 5
                }]
            }
        ],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");

    let err = import_json(&mut store, &document).expect_err("duplicate");
    assert_validation(err, ValidationReason::MentionIdConflict, "mention_id");
    assert_eq!(store.counts().expect("counts").pages, 0);
    assert_eq!(stored_occurrences(&_scratch.path), 0);
}

#[test]
fn an_occurrence_may_point_at_an_entity_that_only_exists_in_the_destination() {
    let (scratch, mut store) = created("destination-entity.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara del destino");
    store
        .create_page(new_page(PAGE_TWO, "Otra", "Elara"))
        .expect("page");
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Nueva",
            "body": "Elara",
            "mentions": [{
                "id": occurrence_id(3),
                "entity_id": ENTITY_ONE,
                "start_utf16": 0,
                "end_utf16": 5
            }]
        }],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");

    let outcome = import_json(&mut store, &document).expect("import");
    assert_eq!(outcome.inserted, 1);
    assert_eq!(outcome.unchanged, 0);
    let page = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(page.body.mentions.len(), 1);
    assert_eq!(page.body.mentions[0].id.as_str(), occurrence_id(3));
    assert_eq!(page.body.mentions[0].entity_id.as_str(), ENTITY_ONE);
    assert_eq!(page.body.mentions[0].start_utf16, 0);
    assert_eq!(page.body.mentions[0].end_utf16, 5);
    assert_eq!(stored_occurrences(&scratch.path), 1);
    // The page that the document did not carry keeps its own content.
    let untouched = store.page_document(&page_id(PAGE_TWO)).expect("document");
    assert_eq!(untouched.title, "Otra");
    assert!(untouched.body.mentions.is_empty());
}

#[test]
fn an_occurrence_that_splits_a_grapheme_cluster_is_refused() {
    let (_scratch, mut store) = created("split-cluster.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Cluster",
            "body": "e\u{301}lara",
            "mentions": [{
                "id": occurrence_id(4),
                "entity_id": ENTITY_ONE,
                "start_utf16": 0,
                "end_utf16": 1
            }]
        }],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");

    let err = import_json(&mut store, &document).expect_err("cluster");
    assert_validation(err, ValidationReason::SplitGrapheme, "end_utf16");
    assert_eq!(store.counts().expect("counts").pages, 0);
    assert_eq!(stored_occurrences(&_scratch.path), 0);
}

#[test]
fn an_occurrence_that_starts_after_it_ends_is_refused() {
    let (_scratch, mut store) = created("inverted-range.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Invertido",
            "body": "Elara",
            "mentions": [{
                "id": occurrence_id(5),
                "entity_id": ENTITY_ONE,
                "start_utf16": 4,
                "end_utf16": 1
            }]
        }],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");

    let err = import_json(&mut store, &document).expect_err("range");
    assert_validation(err, ValidationReason::InvalidMentionRange, "end_utf16");
    assert_eq!(store.counts().expect("counts").pages, 0);
}

#[test]
fn an_occurrence_id_that_is_not_a_canonical_uuid_is_refused() {
    let (_scratch, mut store) = created("bad-occurrence-id.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Identidad mala",
            "body": "Elara",
            "mentions": [{
                "id": "not-a-uuid",
                "entity_id": ENTITY_ONE,
                "start_utf16": 0,
                "end_utf16": 5
            }]
        }],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");

    let err = import_json(&mut store, &document).expect_err("id");
    assert_validation(err, ValidationReason::InvalidId, "mention_id");
    assert_eq!(store.counts().expect("counts").pages, 0);

    let uppercase = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Identidad mayúscula",
            "body": "Elara",
            "mentions": [{
                "id": "DDDDDDDD-DDDD-4DDD-8DDD-DDDDDDDDDDDD",
                "entity_id": ENTITY_ONE,
                "start_utf16": 0,
                "end_utf16": 5
            }]
        }],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");
    let err = import_json(&mut store, &uppercase).expect_err("canonical id");
    assert_validation(err, ValidationReason::InvalidId, "mention_id");
    assert_eq!(store.counts().expect("counts").pages, 0);
}

#[test]
fn a_document_without_an_integer_format_version_is_refused() {
    let (_scratch, mut store) = created("bad-version.sqlite");
    for version in [
        serde_json::json!("2"),
        serde_json::json!(2.0),
        serde_json::json!(2.5),
    ] {
        let mut document = serde_json::json!({
            "format_version": version,
            "universe_id": UNIVERSE,
            "pages": [],
            "types": [],
            "entities": [],
            "definitions": [],
            "assertions": []
        });
        if version.is_null() {
            document
                .as_object_mut()
                .expect("object")
                .remove("format_version");
        }
        let bytes = serde_json::to_vec(&document).expect("document");
        let err = import_json(&mut store, &bytes).expect_err("version");
        assert!(matches!(err, Error::InvalidDocument { .. }), "{err}");
        assert_eq!(store.counts().expect("counts").pages, 0);
    }

    let missing = serde_json::to_vec(&serde_json::json!({
        "universe_id": UNIVERSE,
        "pages": [],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");
    let err = import_json(&mut store, &missing).expect_err("version");
    assert!(matches!(err, Error::InvalidDocument { .. }), "{err}");
    assert_eq!(store.counts().expect("counts").pages, 0);
}

#[test]
fn a_version_2_page_with_a_null_occurrence_array_is_refused() {
    let (_scratch, mut store) = created("null-mentions.sqlite");
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Nulo",
            "body": "Elara",
            "mentions": serde_json::Value::Null
        }],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");

    let err = import_json(&mut store, &document).expect_err("malformed");
    assert!(matches!(err, Error::InvalidDocument { .. }), "{err}");
    assert_eq!(store.counts().expect("counts").pages, 0);
}

#[test]
fn two_overlapping_occurrences_are_refused() {
    let (_scratch, mut store) = created("overlapping.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Superpuestas",
            "body": "Elara",
            "mentions": [
                {
                    "id": occurrence_id(6),
                    "entity_id": ENTITY_ONE,
                    "start_utf16": 0,
                    "end_utf16": 4
                },
                {
                    "id": occurrence_id(7),
                    "entity_id": ENTITY_ONE,
                    "start_utf16": 2,
                    "end_utf16": 5
                }
            ]
        }],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");

    let err = import_json(&mut store, &document).expect_err("overlap");
    assert_validation(err, ValidationReason::OverlappingMentions, "mentions");
    assert_eq!(store.counts().expect("counts").pages, 0);
}

#[test]
fn an_occurrence_that_splits_a_surrogate_pair_is_refused() {
    let (_scratch, mut store) = created("split-scalar.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Escalar",
            "body": "📚",
            "mentions": [{
                "id": occurrence_id(19),
                "entity_id": ENTITY_ONE,
                "start_utf16": 0,
                "end_utf16": 1
            }]
        }],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");

    let err = import_json(&mut store, &document).expect_err("scalar");
    assert_validation(err, ValidationReason::SplitScalar, "end_utf16");
    assert_eq!(store.counts().expect("counts").pages, 0);
    assert_eq!(stored_occurrences(&_scratch.path), 0);
}

#[test]
fn an_occurrence_id_that_the_destination_already_owns_is_refused() {
    let (scratch, mut store) = created("owned-occurrence.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    store
        .create_page(new_page(PAGE_TWO, "Dueña", "Elara"))
        .expect("page");
    let document = store.page_document(&page_id(PAGE_TWO)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_TWO),
            document.revision,
            &[link(mention(&occurrence_id(20), ENTITY_ONE, 0, 5))],
        )
        .expect("link");
    assert_eq!(stored_occurrences(&scratch.path), 1);

    // The document wants the same occurrence id on another page.
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Otra",
            "body": "Elara",
            "mentions": [{
                "id": occurrence_id(20),
                "entity_id": ENTITY_ONE,
                "start_utf16": 0,
                "end_utf16": 5
            }]
        }],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");

    let err = import_json(&mut store, &document).expect_err("owned");
    assert_validation(err, ValidationReason::MentionIdConflict, "mention_id");
    assert_eq!(store.counts().expect("counts").pages, 1);
    assert_eq!(stored_occurrences(&scratch.path), 1);
    let owner = store.page_document(&page_id(PAGE_TWO)).expect("document");
    assert_eq!(owner.body.mentions.len(), 1);
    assert_eq!(owner.body.mentions[0].id.as_str(), occurrence_id(20));
}

#[test]
fn an_entity_that_the_document_renames_rolls_the_whole_batch_back() {
    let (scratch, mut store) = created("entity-label.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    store
        .create_page(new_page(PAGE_TWO, "Dueña", "Elara"))
        .expect("page");
    let before = support::bytes(&scratch.path);

    // The document carries a new page that mentions the entity and declares
    // the same entity under another label.
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Nueva",
            "body": "Elara",
            "mentions": [{
                "id": occurrence_id(21),
                "entity_id": ENTITY_ONE,
                "start_utf16": 0,
                "end_utf16": 5
            }]
        }],
        "types": [],
        "entities": [{
            "id": ENTITY_ONE,
            "label": "Elara renombrada"
        }],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");

    let err = import_json(&mut store, &document).expect_err("conflict");
    assert_conflict(err, "entity", ENTITY_ONE);
    assert_eq!(store.counts().expect("counts").pages, 1);
    assert_eq!(stored_occurrences(&scratch.path), 0);
    assert!(store
        .page(&page_id(PAGE_ONE))
        .expect_err("absent")
        .to_string()
        .contains("NotFound"));
    assert_eq!(
        store.entity(&entity(ENTITY_ONE)).expect("entity").label,
        "Elara"
    );
    drop(store);
    assert_eq!(support::bytes(&scratch.path), before);
}

#[test]
fn an_export_of_a_universe_whose_entity_vanished_fails_closed() {
    let (scratch, mut store) = created("vanished-entity.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Elara", "Elara"))
        .expect("page");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[link(mention(&occurrence_id(8), ENTITY_ONE, 0, 5))],
        )
        .expect("link");
    drop(store);

    // The entity disappears without SQLite knowing about it.
    let conn = Connection::open(&scratch.path).expect("raw");
    conn.execute_batch("PRAGMA foreign_keys = OFF").expect("fk");
    conn.execute("DELETE FROM entities WHERE id = ?1", [ENTITY_ONE])
        .expect("delete");
    drop(conn);
    let before = support::bytes(&scratch.path);

    let mut store = Store::open(&scratch.path).expect("open");
    let err = export_json(&mut store).expect_err("vanished entity");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
    assert_eq!(stored_occurrences(&scratch.path), 1);
    drop(store);
    assert_eq!(support::bytes(&scratch.path), before);
}

#[test]
fn an_export_of_a_universe_with_a_corrupt_occurrence_fails_closed() {
    let (scratch, mut store) = created("corrupt-range.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Elara", "Elara"))
        .expect("page");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[link(mention(&occurrence_id(9), ENTITY_ONE, 0, 5))],
        )
        .expect("link");
    drop(store);

    let conn = Connection::open(&scratch.path).expect("raw");
    conn.execute("UPDATE mentions SET start_utf16 = 10, end_utf16 = 20", [])
        .expect("corrupt");
    drop(conn);

    let mut store = Store::open(&scratch.path).expect("open");
    let err = export_json(&mut store).expect_err("corrupt range");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
}

#[test]
fn an_export_of_a_universe_with_a_corrupt_occurrence_id_fails_closed() {
    let (scratch, mut store) = created("corrupt-id.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Elara", "Elara"))
        .expect("page");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[link(mention(&occurrence_id(10), ENTITY_ONE, 0, 5))],
        )
        .expect("link");
    drop(store);

    let conn = Connection::open(&scratch.path).expect("raw");
    conn.execute(
        "UPDATE mentions SET id = 'zzzzzzzz-zzzz-zzzz-zzzz-zzzzzzzzzzzz'",
        [],
    )
    .expect("corrupt");
    drop(conn);

    let mut store = Store::open(&scratch.path).expect("open");
    let err = export_json(&mut store).expect_err("corrupt id");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
}

#[test]
fn the_same_body_on_two_pages_keeps_the_occurrences_apart() {
    let (scratch, mut store) = created("same-body.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    create_entity(&mut store, ENTITY_TWO, "Elara otra");
    store
        .create_page(new_page(PAGE_ONE, "Primera", "Elara"))
        .expect("page");
    store
        .create_page(new_page(PAGE_TWO, "Segunda", "Elara"))
        .expect("page");
    let first = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            first.revision,
            &[link(mention(&occurrence_id(11), ENTITY_ONE, 0, 5))],
        )
        .expect("link");
    let second = store.page_document(&page_id(PAGE_TWO)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_TWO),
            second.revision,
            &[link(mention(&occurrence_id(12), ENTITY_TWO, 0, 5))],
        )
        .expect("link");

    let bytes = export_json(&mut store).expect("export");
    let destination = scratch.path.with_file_name("same-body-restored.sqlite");
    let (restored, _) = import_new_json(&destination, &bytes).expect("import");
    let one = restored
        .page_document(&page_id(PAGE_ONE))
        .expect("document");
    let two = restored
        .page_document(&page_id(PAGE_TWO))
        .expect("document");
    assert_eq!(one.body.body, "Elara");
    assert_eq!(two.body.body, "Elara");
    assert_eq!(one.body.mentions.len(), 1);
    assert_eq!(two.body.mentions.len(), 1);
    assert_eq!(one.body.mentions[0].id.as_str(), occurrence_id(11));
    assert_eq!(one.body.mentions[0].entity_id.as_str(), ENTITY_ONE);
    assert_eq!(two.body.mentions[0].id.as_str(), occurrence_id(12));
    assert_eq!(two.body.mentions[0].entity_id.as_str(), ENTITY_TWO);
    assert_ne!(one.body.mentions[0].id, two.body.mentions[0].id);
    assert_eq!(stored_occurrences(&destination), 2);
}

#[test]
fn a_version_1_import_into_a_fresh_database_exports_as_version_2() {
    let (scratch, _store) = created("v1-fresh.sqlite");
    let legacy = serde_json::to_vec(&serde_json::json!({
        "format_version": 1,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Antiguo",
            "body": "Elara"
        }],
        "types": [],
        "entities": [{
            "id": ENTITY_ONE,
            "label": "Elara"
        }],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");

    let destination = scratch.path.with_file_name("v1-fresh-restored.sqlite");
    let (restored, outcome) = import_new_json(&destination, &legacy).expect("import");
    assert_eq!(outcome.inserted, 2);
    assert_eq!(outcome.unchanged, 0);
    let page = restored
        .page_document(&page_id(PAGE_ONE))
        .expect("document");
    assert!(page.body.mentions.is_empty());
    assert_eq!(page.revision, PageRevision::ZERO);
    drop(restored);

    let mut restored = Store::open(&destination).expect("reopen");
    let bytes = export_json(&mut restored).expect("export");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(json["format_version"], 2);
    assert_eq!(json["pages"].as_array().expect("pages").len(), 1);
    assert_eq!(json["pages"][0]["mentions"], serde_json::json!([]));
    assert_no_revision(&json);
}

#[test]
fn an_import_keeps_the_revision_of_a_page_that_does_not_change() {
    let (scratch, mut store) = created("revision-kept.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    store
        .create_page(new_page(PAGE_ONE, "Uno", "Elara"))
        .expect("page");
    store
        .create_page(new_page(PAGE_TWO, "Dos", "Dos"))
        .expect("page");

    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    let linked = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[link(mention(&occurrence_id(13), ENTITY_ONE, 0, 5))],
        )
        .expect("link");
    assert_eq!(linked.revision.as_i64(), 1);
    let mut revision = linked.revision;
    for title in ["Uno dos", "Uno tres", "Uno cuatro"] {
        revision = store
            .save_page_edits(
                &page_id(PAGE_ONE),
                revision,
                &[PageEdit::SetTitle {
                    title: title.to_string(),
                }],
            )
            .expect("title")
            .revision;
    }
    assert_eq!(revision.as_i64(), 4);
    let other = store
        .save_page_edits(
            &page_id(PAGE_TWO),
            store
                .page_document(&page_id(PAGE_TWO))
                .expect("document")
                .revision,
            &[PageEdit::SetTitle {
                title: "Dos title".to_string(),
            }],
        )
        .expect("title");
    assert_eq!(other.revision.as_i64(), 1);

    // The document carries one page that the destination already has, with
    // the same content, and one page that the destination does not have.
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [
            {
                "id": PAGE_ONE,
                "title": "Uno cuatro",
                "body": "Elara",
                "mentions": [{
                    "id": occurrence_id(13),
                    "entity_id": ENTITY_ONE,
                    "start_utf16": 0,
                    "end_utf16": 5
                }]
            },
            {
                "id": PAGE_THREE,
                "title": "Tres",
                "body": "Tres",
                "mentions": []
            }
        ],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");

    let outcome = import_json(&mut store, &document).expect("import");
    assert_eq!(outcome.inserted, 1);
    assert_eq!(outcome.unchanged, 1);

    let kept = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(kept.revision.as_i64(), 4);
    assert_eq!(kept.title, "Uno cuatro");
    assert_eq!(kept.body.mentions.len(), 1);
    assert_eq!(kept.body.mentions[0].id.as_str(), occurrence_id(13));

    // The page that the document did not carry keeps its content and token.
    let untouched = store.page_document(&page_id(PAGE_TWO)).expect("document");
    assert_eq!(untouched.revision.as_i64(), 1);
    assert_eq!(untouched.title, "Dos title");

    // The page that only the document carried starts at zero.
    let inserted = store.page_document(&page_id(PAGE_THREE)).expect("document");
    assert_eq!(inserted.revision, PageRevision::ZERO);
    assert_eq!(inserted.title, "Tres");
    assert!(inserted.body.mentions.is_empty());
    assert_eq!(stored_occurrences(&scratch.path), 1);
}

#[test]
fn an_older_document_cannot_resurrect_a_removed_occurrence() {
    let (scratch, mut store) = created("no-resurrect-json.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    create_entity(&mut store, ENTITY_TWO, "Elara otra");
    store
        .create_page(new_page(PAGE_ONE, "Uno", "Elara y Elara"))
        .expect("page");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[
                link(mention(&occurrence_id(14), ENTITY_ONE, 0, 5)),
                link(mention(&occurrence_id(15), ENTITY_TWO, 8, 13)),
            ],
        )
        .expect("link");
    let older = export_json(&mut store).expect("export");

    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[PageEdit::Mention(MentionEdit::Unlink {
                mention_id: MentionId::parse(&occurrence_id(15)).expect("mention id"),
            })],
        )
        .expect("unlink");

    let err = import_json(&mut store, &older).expect_err("conflict");
    assert_conflict(err, "page", PAGE_ONE);
    let after = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(after.body.mentions.len(), 1);
    assert_eq!(after.body.mentions[0].id.as_str(), occurrence_id(14));
    assert_eq!(stored_occurrences(&scratch.path), 1);
}

#[test]
fn a_conflicting_page_rolls_the_whole_batch_back() {
    let (scratch, mut store) = created("batch-rollback.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    store
        .create_page(new_page(PAGE_TWO, "Dos", "Dos"))
        .expect("page");
    let before = support::bytes(&scratch.path);
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [
            {
                "id": PAGE_ONE,
                "title": "Nueva",
                "body": "Elara",
                "mentions": [{
                    "id": occurrence_id(16),
                    "entity_id": ENTITY_ONE,
                    "start_utf16": 0,
                    "end_utf16": 5
                }]
            },
            {
                "id": PAGE_TWO,
                "title": "Cambiada",
                "body": "Dos",
                "mentions": []
            }
        ],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");

    let err = import_json(&mut store, &document).expect_err("conflict");
    assert_conflict(err, "page", PAGE_TWO);
    assert_eq!(store.counts().expect("counts").pages, 1);
    assert_eq!(stored_occurrences(&scratch.path), 0);
    assert!(store
        .page(&page_id(PAGE_ONE))
        .expect_err("absent")
        .to_string()
        .contains("NotFound"));
    drop(store);
    assert_eq!(support::bytes(&scratch.path), before);
}

#[test]
fn the_imported_occurrence_order_does_not_depend_on_the_document_order() {
    let (scratch, _store) = created("reversed-order.sqlite");
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Desordenado",
            "body": "Elara y Elara",
            "mentions": [
                {
                    "id": occurrence_id(18),
                    "entity_id": ENTITY_TWO,
                    "start_utf16": 8,
                    "end_utf16": 13
                },
                {
                    "id": occurrence_id(17),
                    "entity_id": ENTITY_ONE,
                    "start_utf16": 0,
                    "end_utf16": 5
                }
            ]
        }],
        "types": [],
        "entities": [
            {"id": ENTITY_ONE, "label": "Elara"},
            {"id": ENTITY_TWO, "label": "Elara otra"}
        ],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");

    let destination = scratch.path.with_file_name("reversed-restored.sqlite");
    let (restored, _) = import_new_json(&destination, &document).expect("import");
    let page = restored
        .page_document(&page_id(PAGE_ONE))
        .expect("document");
    assert_eq!(page.body.mentions.len(), 2);
    assert_eq!(page.body.mentions[0].id.as_str(), occurrence_id(17));
    assert_eq!(page.body.mentions[0].start_utf16, 0);
    assert_eq!(page.body.mentions[0].end_utf16, 5);
    assert_eq!(page.body.mentions[1].id.as_str(), occurrence_id(18));
    assert_eq!(page.body.mentions[1].start_utf16, 8);
    assert_eq!(page.body.mentions[1].end_utf16, 13);
}

#[test]
fn the_size_cap_counts_the_occurrence_metadata() {
    let (_scratch, mut store) = created("cap-metadata.sqlite");
    let body = "x".repeat(narrative_universe::MAX_DOCUMENT_BYTES - 1200);
    store
        .create_page(new_page(PAGE_ONE, "Grande", &body))
        .expect("page");
    create_entity(&mut store, ENTITY_ONE, "Elara");

    // The prose alone still fits inside the cap.
    let prose = export_json(&mut store).expect("prose export");
    assert!(
        prose.len() < narrative_universe::MAX_DOCUMENT_BYTES,
        "{}",
        prose.len()
    );
    let slack = narrative_universe::MAX_DOCUMENT_BYTES - prose.len();
    // Every occurrence adds at least one hundred bytes of metadata.
    let count = slack / 100 + 2;

    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    let edits: Vec<PageEdit> = (0..count)
        .map(|index| {
            let index = index as u32;
            link(mention(
                &occurrence_id(1_000 + index),
                ENTITY_ONE,
                index * 3,
                index * 3 + 1,
            ))
        })
        .collect();
    store
        .save_page_edits(&page_id(PAGE_ONE), document.revision, &edits)
        .expect("link");

    let err = export_json(&mut store).expect_err("over the cap");
    assert_validation(err, ValidationReason::InputTooLarge, "json");
}

#[test]
fn an_oversized_export_creates_no_destination_file() {
    let (scratch, mut store) = created("over-cap-file.sqlite");
    let body = "x".repeat(narrative_universe::MAX_DOCUMENT_BYTES + 16);
    store
        .create_page(new_page(PAGE_ONE, "Grande", &body))
        .expect("page");
    drop(store);

    let json = scratch.path.with_file_name("over-cap.json");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_spike"))
        .arg("export")
        .arg(&scratch.path)
        .arg(&json)
        .output()
        .expect("run spike");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr");
    assert!(stderr.contains("InputTooLarge"), "{stderr}");
    assert!(!json.exists(), "the destination file was created");
}
