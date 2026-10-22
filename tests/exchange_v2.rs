//! Tests for the version 2 JSON exchange.
//!
//! The round trip compares literal bodies, occurrence ids, entity ids and
//! ranges. Every test runs against a real SQLite file.

#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use narrative_universe::{
    export_json, import_json, import_new_json, Assertion, AssertionId, Canon, Cardinality,
    Definition, DefinitionId, Entity, EntityId, EntityType, Error, Mention, MentionEdit, MentionId,
    Owner, Page, PageEdit, PageId, PageRevision, Provenance, Store, TypeId, UniverseId,
    ValidationReason, Value, ValueClass,
};
use rusqlite::Connection;
use support::scratch;

const UNIVERSE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const PAGE_ONE: &str = "22222222-2222-4222-8222-222222222221";
const PAGE_TWO: &str = "22222222-2222-4222-8222-222222222222";
const ENTITY_ONE: &str = "11111111-1111-4111-8111-111111111111";
const ENTITY_TWO: &str = "33333333-3333-4333-8333-333333333333";
const TYPE_ONE: &str = "44444444-4444-4444-8444-444444444444";
const DEFINITION_ONE: &str = "77777777-7777-4777-8777-777777777777";
const ASSERTION_ONE: &str = "88888888-8888-4888-8888-888888888888";
const MENTION_ONE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const MENTION_TWO: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const MENTION_THREE: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

fn universe() -> UniverseId {
    UniverseId::parse(UNIVERSE).expect("universe")
}

fn page_id(raw: &str) -> PageId {
    PageId::parse(raw).expect("page")
}

fn entity(raw: &str) -> EntityId {
    EntityId::parse(raw).expect("entity")
}

fn mention(raw: &str, entity_raw: &str, start_utf16: u32, end_utf16: u32) -> Mention {
    Mention {
        id: MentionId::parse(raw).expect("mention id"),
        entity_id: entity(entity_raw),
        start_utf16,
        end_utf16,
    }
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

fn link(occurrence: Mention) -> PageEdit {
    PageEdit::Mention(MentionEdit::Link {
        mention: occurrence,
    })
}

/// A universe with two pages, two entities, a type, a definition and an
/// assertion, and occurrences that point at repeated words.
fn populated(store: &mut Store) {
    store
        .create_entity_type(EntityType {
            id: TypeId::parse(TYPE_ONE).expect("type"),
            label: "Personaje".to_string(),
        })
        .expect("type");
    store
        .create_entity(Entity {
            id: entity(ENTITY_ONE),
            label: "Elara".to_string(),
            type_id: Some(TypeId::parse(TYPE_ONE).expect("type")),
            page_id: None,
        })
        .expect("entity");
    store
        .create_entity(Entity {
            id: entity(ENTITY_TWO),
            label: "Elara del otro lado".to_string(),
            type_id: Some(TypeId::parse(TYPE_ONE).expect("type")),
            page_id: None,
        })
        .expect("entity");
    store
        .create_page(new_page(PAGE_ONE, "Encuentro", "Elara遇见 Elara 📚"))
        .expect("page");
    store
        .create_page(new_page(PAGE_TWO, "Segundo", "Elara sola"))
        .expect("page");
    store
        .create_definition(Definition {
            id: DefinitionId::parse(DEFINITION_ONE).expect("definition"),
            owner: Owner::Type(TypeId::parse(TYPE_ONE).expect("type")),
            value_class: ValueClass::Text,
            cardinality: Cardinality::Singular,
            archived: false,
        })
        .expect("definition");
    store
        .create_assertion(Assertion {
            id: AssertionId::parse(ASSERTION_ONE).expect("assertion"),
            subject_id: entity(ENTITY_ONE),
            definition_id: DefinitionId::parse(DEFINITION_ONE).expect("definition"),
            value: Value::Text("nota".to_string()),
            canon: Canon::Provisional,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: Some("al inicio".to_string()),
            archived: false,
        })
        .expect("assertion");

    let first = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            first.revision,
            &[
                link(mention(MENTION_ONE, ENTITY_ONE, 0, 5)),
                link(mention(MENTION_TWO, ENTITY_TWO, 7, 12)),
            ],
        )
        .expect("link");
    let second = store.page_document(&page_id(PAGE_TWO)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_TWO),
            second.revision,
            &[link(mention(MENTION_THREE, ENTITY_ONE, 0, 5))],
        )
        .expect("link");
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

#[test]
fn the_export_is_version_2_and_every_page_carries_its_occurrences() {
    let (_scratch, mut store) = created("export-shape.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Sin enlaces", "texto"))
        .expect("page");
    store
        .create_page(new_page(PAGE_TWO, "Con enlaces", "Elara"))
        .expect("page");
    store
        .create_entity(Entity {
            id: entity(ENTITY_ONE),
            label: "Elara".to_string(),
            type_id: None,
            page_id: None,
        })
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_TWO)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_TWO),
            document.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect("link");

    let bytes = export_json(&mut store).expect("export");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(json["format_version"], 2);
    let pages = json["pages"].as_array().expect("pages");
    assert_eq!(pages.len(), 2);
    for page in pages {
        assert!(page["mentions"].is_array(), "{page}");
    }
    let with_link = pages
        .iter()
        .find(|page| page["id"] == PAGE_TWO)
        .expect("page");
    assert_eq!(with_link["mentions"].as_array().expect("mentions").len(), 1);
    assert_eq!(with_link["mentions"][0]["id"], MENTION_ONE);
    assert_eq!(with_link["mentions"][0]["entity_id"], ENTITY_ONE);
    assert_eq!(with_link["mentions"][0]["start_utf16"], 0);
    assert_eq!(with_link["mentions"][0]["end_utf16"], 5);
    let without_link = pages
        .iter()
        .find(|page| page["id"] == PAGE_ONE)
        .expect("page");
    assert_eq!(without_link["mentions"], serde_json::json!([]));
    assert!(!String::from_utf8(bytes).expect("utf8").contains("revision"));
}

#[test]
fn a_round_trip_restores_the_occurrences_and_the_domain_records() {
    let (scratch, mut store) = created("round-trip.sqlite");
    populated(&mut store);
    let bytes = export_json(&mut store).expect("export");

    let destination = scratch.path.with_file_name("restored.sqlite");
    let (restored, outcome) = import_new_json(&destination, &bytes).expect("import");
    assert_eq!(outcome.inserted, 7);
    drop(restored);

    let restored = Store::open(&destination).expect("reopen");
    let first = restored
        .page_document(&page_id(PAGE_ONE))
        .expect("document");
    assert_eq!(first.title, "Encuentro");
    assert_eq!(first.body.body, "Elara遇见 Elara 📚");
    assert_eq!(first.body.mentions.len(), 2);
    assert_eq!(first.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(first.body.mentions[0].entity_id.as_str(), ENTITY_ONE);
    assert_eq!(first.body.mentions[0].start_utf16, 0);
    assert_eq!(first.body.mentions[0].end_utf16, 5);
    assert_eq!(first.body.mentions[1].id.as_str(), MENTION_TWO);
    assert_eq!(first.body.mentions[1].entity_id.as_str(), ENTITY_TWO);
    assert_eq!(first.body.mentions[1].start_utf16, 7);
    assert_eq!(first.body.mentions[1].end_utf16, 12);
    assert_eq!(first.revision, PageRevision::ZERO);

    let second = restored
        .page_document(&page_id(PAGE_TWO))
        .expect("document");
    assert_eq!(second.body.body, "Elara sola");
    assert_eq!(second.body.mentions.len(), 1);
    assert_eq!(second.body.mentions[0].id.as_str(), MENTION_THREE);
    assert_eq!(second.revision, PageRevision::ZERO);

    assert_eq!(restored.counts().expect("counts").pages, 2);
    assert_eq!(restored.counts().expect("counts").entities, 2);
    assert_eq!(restored.counts().expect("counts").types, 1);
    assert_eq!(restored.counts().expect("counts").definitions, 1);
    assert_eq!(restored.counts().expect("counts").assertions, 1);
    let assertion = restored
        .assertion(&AssertionId::parse(ASSERTION_ONE).expect("assertion"))
        .expect("assertion");
    assert_eq!(assertion.canon, Canon::Provisional);
    assert_eq!(assertion.scope_note.as_deref(), Some("al inicio"));
    assert_eq!(
        restored.entity(&entity(ENTITY_TWO)).expect("entity").label,
        "Elara del otro lado"
    );
}

#[test]
fn an_identical_import_is_idempotent_and_keeps_the_revision() {
    let (_scratch, mut store) = created("idempotent.sqlite");
    populated(&mut store);
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[PageEdit::SetTitle {
                title: "Otro titulo".to_string(),
            }],
        )
        .expect("title");
    let revision = store
        .page_document(&page_id(PAGE_ONE))
        .expect("document")
        .revision;
    let bytes = export_json(&mut store).expect("export");

    let outcome = import_json(&mut store, &bytes).expect("import");
    assert_eq!(outcome.inserted, 0);
    assert_eq!(outcome.unchanged, 7);
    let after = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(after.revision, revision);
    assert_eq!(after.title, "Otro titulo");
    assert_eq!(after.body.mentions.len(), 2);
}

#[test]
fn a_version_1_document_cannot_erase_the_occurrences_of_a_stored_page() {
    let (scratch, mut store) = created("v1-over-linked.sqlite");
    populated(&mut store);
    let before = support::bytes(&scratch.path);
    let legacy = serde_json::to_vec(&serde_json::json!({
        "format_version": 1,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Encuentro",
            "body": "Elara遇见 Elara 📚"
        }],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");

    let err = import_json(&mut store, &legacy).expect_err("conflict");
    match err {
        Error::ImportConflict { class, id } => {
            assert_eq!(class, "page");
            assert_eq!(id, PAGE_ONE);
        }
        other => panic!("expected import conflict, got {other}"),
    }
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.body.mentions.len(), 2);
    drop(store);
    assert_eq!(support::bytes(&scratch.path), before);
}

#[test]
fn an_older_snapshot_cannot_resurrect_a_removed_occurrence() {
    let (_scratch, mut store) = created("no-resurrection.sqlite");
    populated(&mut store);
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    let unlinked = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[PageEdit::Mention(MentionEdit::Unlink {
                mention_id: MentionId::parse(MENTION_TWO).expect("mention id"),
            })],
        )
        .expect("unlink");
    assert_eq!(unlinked.body.mentions.len(), 1);

    // The snapshot before the unlink still carries the removed occurrence.
    let mut stale = store.export_snapshot().expect("snapshot");
    let page = stale
        .pages
        .iter_mut()
        .find(|page| page.page.id.as_str() == PAGE_ONE)
        .expect("page");
    page.mentions.push(mention(MENTION_TWO, ENTITY_TWO, 7, 12));
    let err = store.import_snapshot(stale).expect_err("conflict");
    match err {
        Error::ImportConflict { class, id } => {
            assert_eq!(class, "page");
            assert_eq!(id, PAGE_ONE);
        }
        other => panic!("expected import conflict, got {other}"),
    }
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.body.mentions.len(), 1);
    assert_eq!(document.body.mentions[0].id.as_str(), MENTION_ONE);
}

#[test]
fn a_version_1_page_with_an_occurrence_field_is_malformed() {
    let (_scratch, mut store) = created("v1-with-mentions.sqlite");
    let before = store.counts().expect("counts");
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 1,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Con menciones",
            "body": "Elara",
            "mentions": [{
                "id": MENTION_ONE,
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

    let err = import_json(&mut store, &document).expect_err("malformed");
    assert!(matches!(err, Error::InvalidDocument { .. }), "{err}");
    assert_eq!(store.counts().expect("counts"), before);
}

#[test]
fn a_version_2_page_without_the_occurrence_array_is_malformed() {
    let (_scratch, mut store) = created("v2-without-mentions.sqlite");
    let before = store.counts().expect("counts");
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Sin el arreglo",
            "body": "Elara"
        }],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");

    let err = import_json(&mut store, &document).expect_err("malformed");
    assert!(matches!(err, Error::InvalidDocument { .. }), "{err}");
    assert_eq!(store.counts().expect("counts"), before);
}

#[test]
fn an_unknown_version_or_field_is_refused() {
    let (_scratch, mut store) = created("unknown.sqlite");
    let before = store.counts().expect("counts");
    let unknown_version = serde_json::to_vec(&serde_json::json!({
        "format_version": 3,
        "universe_id": UNIVERSE,
        "pages": [],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");
    let err = import_json(&mut store, &unknown_version).expect_err("version");
    match err {
        Error::UnsupportedVersion { found } => assert_eq!(found, 3),
        other => panic!("expected unsupported version, got {other}"),
    }

    let unknown_field = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": [],
        "extra": true
    }))
    .expect("document");
    let err = import_json(&mut store, &unknown_field).expect_err("field");
    assert!(matches!(err, Error::InvalidDocument { .. }), "{err}");

    let unknown_mention_field = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Extra",
            "body": "Elara",
            "mentions": [{
                "id": MENTION_ONE,
                "entity_id": ENTITY_ONE,
                "start_utf16": 0,
                "end_utf16": 5,
                "revision": 3
            }]
        }],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");
    let err = import_json(&mut store, &unknown_mention_field).expect_err("field");
    assert!(matches!(err, Error::InvalidDocument { .. }), "{err}");
    assert_eq!(store.counts().expect("counts"), before);
}

#[test]
fn a_malformed_occurrence_writes_nothing() {
    let (_scratch, mut store) = created("malformed-mentions.sqlite");
    store
        .create_entity(Entity {
            id: entity(ENTITY_ONE),
            label: "Elara".to_string(),
            type_id: None,
            page_id: None,
        })
        .expect("entity");
    let before = store.counts().expect("counts");

    let out_of_range = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Fuera de rango",
            "body": "Elara",
            "mentions": [{
                "id": MENTION_ONE,
                "entity_id": ENTITY_ONE,
                "start_utf16": 0,
                "end_utf16": 99
            }]
        }],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");
    let err = import_json(&mut store, &out_of_range).expect_err("range");
    assert_validation(err, ValidationReason::InvalidMentionRange, "end_utf16");

    let unknown_entity = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Entidad desconocida",
            "body": "Elara",
            "mentions": [{
                "id": MENTION_ONE,
                "entity_id": "99999999-9999-4999-8999-999999999999",
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
    let err = import_json(&mut store, &unknown_entity).expect_err("entity");
    assert_validation(err, ValidationReason::UnknownEntity, "entity_id");

    let duplicate = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [
            {
                "id": PAGE_ONE,
                "title": "Primera",
                "body": "Elara",
                "mentions": [{
                    "id": MENTION_ONE,
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
                    "id": MENTION_ONE,
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
    let err = import_json(&mut store, &duplicate).expect_err("duplicate");
    assert_validation(err, ValidationReason::MentionIdConflict, "mention_id");

    assert_eq!(store.counts().expect("counts"), before);
    assert!(store
        .page(&page_id(PAGE_ONE))
        .expect_err("absent")
        .to_string()
        .contains("NotFound"));
}

#[test]
fn an_occurrence_may_point_at_an_entity_that_the_same_import_creates() {
    let (scratch, _store) = created("forward-reference.sqlite");
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Adelante",
            "body": "Elara",
            "mentions": [{
                "id": MENTION_ONE,
                "entity_id": ENTITY_ONE,
                "start_utf16": 0,
                "end_utf16": 5
            }]
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

    let destination = scratch.path.with_file_name("forward.sqlite");
    let (imported, _) = import_new_json(&destination, &document).expect("import");
    let page = imported
        .page_document(&page_id(PAGE_ONE))
        .expect("document");
    assert_eq!(page.body.mentions.len(), 1);
    assert_eq!(page.body.mentions[0].entity_id.as_str(), ENTITY_ONE);
}

#[test]
fn the_imported_occurrences_do_not_depend_on_the_order_of_the_document() {
    let (scratch, _store) = created("unordered.sqlite");
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Desordenado",
            "body": "Elara y Elara",
            "mentions": [
                {
                    "id": MENTION_TWO,
                    "entity_id": ENTITY_TWO,
                    "start_utf16": 7,
                    "end_utf16": 12
                },
                {
                    "id": MENTION_ONE,
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

    let destination = scratch.path.with_file_name("unordered-restored.sqlite");
    let (imported, _) = import_new_json(&destination, &document).expect("import");
    let page = imported
        .page_document(&page_id(PAGE_ONE))
        .expect("document");
    assert_eq!(page.body.mentions.len(), 2);
    assert_eq!(page.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(page.body.mentions[0].start_utf16, 0);
    assert_eq!(page.body.mentions[1].id.as_str(), MENTION_TWO);
    assert_eq!(page.body.mentions[1].start_utf16, 7);
}

#[test]
fn an_occurrence_that_belongs_to_no_page_makes_the_export_refuse_the_universe() {
    let (scratch, mut store) = created("orphan-row.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara"))
        .expect("page");
    store
        .create_entity(Entity {
            id: entity(ENTITY_ONE),
            label: "Elara".to_string(),
            type_id: None,
            page_id: None,
        })
        .expect("entity");
    drop(store);

    let conn = Connection::open(&scratch.path).expect("raw");
    conn.execute_batch("PRAGMA foreign_keys = OFF").expect("fk");
    conn.execute(
        "INSERT INTO mentions (id, page_id, entity_id, start_utf16, end_utf16)
         VALUES (?1, ?2, ?3, 0, 5)",
        [MENTION_ONE, PAGE_TWO, ENTITY_ONE],
    )
    .expect("orphan");
    drop(conn);

    let mut store = Store::open(&scratch.path).expect("open");
    let err = export_json(&mut store).expect_err("orphan");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
}

#[test]
fn a_version_1_document_still_imports_as_a_universe_without_occurrences() {
    let (scratch, mut store) = created("v1-import.sqlite");
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

    let outcome = import_json(&mut store, &legacy).expect("import");
    assert_eq!(outcome.inserted, 2);
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.title, "Antiguo");
    assert_eq!(document.body.body, "Elara");
    assert!(document.body.mentions.is_empty());
    assert_eq!(document.revision, PageRevision::ZERO);
    let exported = export_json(&mut store).expect("export");
    let json: serde_json::Value = serde_json::from_slice(&exported).expect("json");
    assert_eq!(json["format_version"], 2);
    assert_eq!(json["pages"][0]["mentions"], serde_json::json!([]));
    drop(store);
    let _ = scratch;
}

#[test]
fn the_size_cap_counts_the_occurrence_metadata() {
    let (_scratch, mut store) = created("cap.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara"))
        .expect("page");
    store
        .create_entity(Entity {
            id: entity(ENTITY_ONE),
            label: "Elara".to_string(),
            type_id: None,
            page_id: None,
        })
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect("link");

    let bytes = export_json(&mut store).expect("export");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    let mentions = json["pages"][0]["mentions"].as_array().expect("mentions");
    assert_eq!(mentions.len(), 1);
    // The occurrence metadata is part of the document and of the cap.
    assert!(bytes.len() > 200);
    assert!(bytes.len() <= narrative_universe::MAX_DOCUMENT_BYTES);
}

#[test]
fn a_document_over_the_cap_is_refused_before_anything_is_written() {
    let (scratch, mut store) = created("over-cap.sqlite");
    let before = support::bytes(&scratch.path);
    let huge = serde_json::to_vec(&serde_json::json!({
        "format_version": 2,
        "universe_id": UNIVERSE,
        "pages": [{
            "id": PAGE_ONE,
            "title": "Enorme",
            "body": "x".repeat(narrative_universe::MAX_DOCUMENT_BYTES),
            "mentions": []
        }],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("document");
    let err = import_json(&mut store, &huge).expect_err("cap");
    assert_validation(err, ValidationReason::InputTooLarge, "json");
    drop(store);
    assert_eq!(support::bytes(&scratch.path), before);
}
