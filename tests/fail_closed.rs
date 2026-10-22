//! Regressions for the three fail-closed defects of the Astra review.
//!
//! 1. A page id that still has occurrence rows while its page is gone cannot
//!    be created again, through any entry point.
//! 2. An import refuses to reconcile a page whose stored revision the
//!    aggregate read already refuses, and rolls the whole batch back.
//! 3. A positional JSON array is never read as a record, in either version.
//!
//! Every probe runs against a real SQLite file and asserts literal state.

#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use std::path::Path;

use narrative_universe::{
    export_json, import_json, import_new_json, AssertionId, Entity, EntityId, Error, Mention,
    MentionEdit, MentionId, Page, PageEdit, PageId, PageSnapshot, Snapshot, Store, UniverseId,
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

fn universe() -> UniverseId {
    UniverseId::parse(UNIVERSE).expect("universe")
}

fn page_id(raw: &str) -> PageId {
    PageId::parse(raw).expect("page")
}

fn entity_id(raw: &str) -> EntityId {
    EntityId::parse(raw).expect("entity")
}

fn mention(raw: &str, entity_raw: &str, start_utf16: u32, end_utf16: u32) -> Mention {
    Mention {
        id: MentionId::parse(raw).expect("mention id"),
        entity_id: entity_id(entity_raw),
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

fn create_entity(store: &mut Store, raw: &str, label: &str) {
    store
        .create_entity(Entity {
            id: entity_id(raw),
            label: label.to_string(),
            type_id: None,
            page_id: None,
        })
        .expect("entity");
}

fn link_page(store: &mut Store, raw: &str, title: &str, body: &str, occurrence: Mention) {
    store.create_page(new_page(raw, title, body)).expect("page");
    let document = store.page_document(&page_id(raw)).expect("document");
    store
        .save_page_edits(
            &page_id(raw),
            document.revision,
            &[PageEdit::Mention(MentionEdit::Link {
                mention: occurrence,
            })],
        )
        .expect("link");
}

fn raw_count(path: &Path, sql: &str) -> i64 {
    let conn = Connection::open(path).expect("raw");
    conn.query_row(sql, [], |row| row.get(0)).expect("count")
}

fn orphan_rows(path: &Path) -> i64 {
    let conn = Connection::open(path).expect("raw");
    conn.query_row(
        "SELECT COUNT(*) FROM mentions WHERE page_id = ?1",
        [PAGE_ONE],
        |row| row.get(0),
    )
    .expect("orphans")
}

fn stored_orphan(path: &Path) -> (String, String, i64, i64) {
    let conn = Connection::open(path).expect("raw");
    conn.query_row(
        "SELECT page_id, entity_id, start_utf16, end_utf16 FROM mentions WHERE id = ?1",
        [MENTION_ONE],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        },
    )
    .expect("orphan row")
}

/// A universe whose page row was deleted outside SQLite while its occurrence
/// row survived, which foreign keys only allow when they are disabled.
fn orphaned(name: &str) -> (support::Scratch, Store) {
    let (scratch, mut store) = created(name);
    create_entity(&mut store, ENTITY_ONE, "Elara");
    link_page(
        &mut store,
        PAGE_ONE,
        "Encuentro",
        "Elara",
        mention(MENTION_ONE, ENTITY_ONE, 0, 5),
    );
    drop(store);
    let conn = Connection::open(&scratch.path).expect("raw");
    conn.execute_batch("PRAGMA foreign_keys = OFF").expect("fk");
    conn.execute("DELETE FROM pages WHERE id = ?1", [PAGE_ONE])
        .expect("orphan");
    drop(conn);
    let store = Store::open(&scratch.path).expect("open");
    (scratch, store)
}

fn assert_storage(err: Error) {
    assert!(matches!(err, Error::Storage { .. }), "{err}");
}

fn assert_invalid_document_at(err: Error, path: &str) {
    match err {
        Error::InvalidDocument { message } => {
            assert!(
                message.contains(path),
                "expected the message to name {path}, got {message}"
            );
        }
        other => panic!("expected an invalid document, got {other}"),
    }
}

fn v2_document(pages: &str, entities: &str) -> Vec<u8> {
    format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[{pages}],"types":[],"entities":[{entities}],"definitions":[],"assertions":[]}}"#
    )
    .into_bytes()
}

fn v2_page(id: &str, title: &str, body: &str, mentions: &str) -> String {
    format!(r#"{{"id":"{id}","title":"{title}","body":"{body}","mentions":[{mentions}]}}"#)
}

fn v2_mention(id: &str, entity_raw: &str, start_utf16: u32, end_utf16: u32) -> String {
    format!(
        r#"{{"id":"{id}","entity_id":"{entity_raw}","start_utf16":{start_utf16},"end_utf16":{end_utf16}}}"#
    )
}

fn v2_entity(id: &str, label: &str) -> String {
    format!(r#"{{"id":"{id}","label":"{label}"}}"#)
}

// ---------------------------------------------------------------- orphan rows

#[test]
fn a_page_id_with_orphan_occurrences_cannot_be_created_again() {
    let (scratch, mut store) = orphaned("orphan-create.sqlite");
    let before = support::bytes(&scratch.path);

    let err = store
        .create_page(new_page(PAGE_ONE, "Otra prosa", "Elara"))
        .expect_err("orphan");
    assert_storage(err);
    drop(store);

    assert_eq!(raw_count(&scratch.path, "SELECT COUNT(*) FROM pages"), 0);
    assert_eq!(orphan_rows(&scratch.path), 1);
    // The row still points at the missing page and at the same entity and
    // range. It was not adopted, relinked or deleted.
    assert_eq!(
        stored_orphan(&scratch.path),
        (PAGE_ONE.to_string(), ENTITY_ONE.to_string(), 0, 5)
    );
    assert_eq!(support::bytes(&scratch.path), before);
}

#[test]
fn a_page_id_with_orphan_occurrences_is_refused_even_with_the_same_text() {
    let (scratch, mut store) = orphaned("orphan-same-body.sqlite");
    let err = store
        .create_page(new_page(PAGE_ONE, "Encuentro", "Elara"))
        .expect_err("orphan");
    assert_storage(err);
    drop(store);
    assert_eq!(raw_count(&scratch.path, "SELECT COUNT(*) FROM pages"), 0);
    assert_eq!(orphan_rows(&scratch.path), 1);
}

#[test]
fn a_version_2_import_cannot_reattach_orphan_occurrences() {
    let (scratch, mut store) = orphaned("orphan-import-v2.sqlite");
    let before = support::bytes(&scratch.path);
    let document = v2_document(
        &v2_page(
            PAGE_ONE,
            "Otra prosa",
            "Elara",
            &v2_mention(MENTION_ONE, ENTITY_ONE, 0, 5),
        ),
        &v2_entity(ENTITY_ONE, "Elara"),
    );

    let err = import_json(&mut store, &document).expect_err("orphan");
    assert_storage(err);
    drop(store);
    assert_eq!(raw_count(&scratch.path, "SELECT COUNT(*) FROM pages"), 0);
    assert_eq!(orphan_rows(&scratch.path), 1);
    assert_eq!(support::bytes(&scratch.path), before);
}

#[test]
fn a_version_1_import_cannot_reattach_orphan_occurrences() {
    let (scratch, mut store) = orphaned("orphan-import-v1.sqlite");
    let before = support::bytes(&scratch.path);
    let document = format!(
        r#"{{"format_version":1,"universe_id":"{UNIVERSE}","pages":[{{"id":"{PAGE_ONE}","title":"Otra prosa","body":"Elara"}}],"types":[],"entities":[],"definitions":[],"assertions":[]}}"#
    )
    .into_bytes();

    let err = import_json(&mut store, &document).expect_err("orphan");
    assert_storage(err);
    drop(store);
    assert_eq!(raw_count(&scratch.path, "SELECT COUNT(*) FROM pages"), 0);
    assert_eq!(orphan_rows(&scratch.path), 1);
    assert_eq!(support::bytes(&scratch.path), before);
}

#[test]
fn a_snapshot_import_cannot_reattach_orphan_occurrences() {
    let (scratch, mut store) = orphaned("orphan-snapshot.sqlite");
    let before = support::bytes(&scratch.path);
    let snapshot = Snapshot {
        universe_id: universe(),
        pages: vec![PageSnapshot {
            page: new_page(PAGE_ONE, "Otra prosa", "Elara"),
            mentions: Vec::new(),
        }],
        types: Vec::new(),
        entities: Vec::new(),
        definitions: Vec::new(),
        assertions: Vec::new(),
    };

    let err = store.import_snapshot(snapshot).expect_err("orphan");
    assert_storage(err);
    drop(store);
    assert_eq!(raw_count(&scratch.path, "SELECT COUNT(*) FROM pages"), 0);
    assert_eq!(orphan_rows(&scratch.path), 1);
    assert_eq!(support::bytes(&scratch.path), before);
}

#[test]
fn the_same_document_still_imports_into_a_fresh_path() {
    // The guard is scoped to the destination database, so a document that
    // reuses an id in a new file must not be over-rejected.
    let scratch = scratch("orphan-fresh-path.sqlite");
    let destination = scratch.path.with_file_name("fresh.sqlite");
    let document = v2_document(
        &v2_page(
            PAGE_ONE,
            "Encuentro",
            "Elara",
            &v2_mention(MENTION_ONE, ENTITY_ONE, 0, 5),
        ),
        &v2_entity(ENTITY_ONE, "Elara"),
    );

    let (store, outcome) = import_new_json(&destination, &document).expect("import");
    assert_eq!(outcome.inserted, 2);
    let page = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(page.body.body, "Elara");
    assert_eq!(page.body.mentions.len(), 1);
    assert_eq!(page.body.mentions[0].id.as_str(), MENTION_ONE);
}

// ------------------------------------------------------- corrupt revision

fn corrupt_revision(path: &Path, sql: &str) {
    let conn = Connection::open(path).expect("raw");
    conn.execute_batch("PRAGMA ignore_check_constraints = ON")
        .expect("checks");
    conn.execute(sql, [PAGE_ONE]).expect("corrupt");
}

/// The document inserts a new page and a new entity first, so a refusal on the
/// corrupt page has to roll those writes back.
fn import_with_new_records() -> Vec<u8> {
    v2_document(
        &format!(
            "{},{}",
            v2_page(PAGE_TWO, "Nueva", "texto", ""),
            v2_page(
                PAGE_ONE,
                "Encuentro",
                "Elara",
                &v2_mention(MENTION_ONE, ENTITY_ONE, 0, 5)
            )
        ),
        &v2_entity(ENTITY_TWO, "Nueva entidad"),
    )
}

fn assert_import_refused_by_revision(scratch: &support::Scratch, store: &Store, before: &[u8]) {
    let err = store
        .page(&page_id(PAGE_TWO))
        .expect_err("page rolled back");
    assert!(
        err.to_string().contains("NotFound"),
        "the new page must not survive the rollback: {err}"
    );
    let err = store
        .entity(&entity_id(ENTITY_TWO))
        .expect_err("entity rolled back");
    assert!(
        err.to_string().contains("NotFound"),
        "the new entity must not survive the rollback: {err}"
    );
    let err = store
        .assertion(&AssertionId::parse(ASSERTION_ONE).expect("assertion"))
        .expect_err("absent");
    assert!(err.to_string().contains("NotFound"), "{err}");
    assert_eq!(orphan_rows(&scratch.path), 1);
    assert_eq!(support::bytes(&scratch.path), before);
}

#[test]
fn an_import_refuses_to_reconcile_a_negative_stored_revision() {
    let (scratch, mut store) = created("reconcile-negative.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    link_page(
        &mut store,
        PAGE_ONE,
        "Encuentro",
        "Elara",
        mention(MENTION_ONE, ENTITY_ONE, 0, 5),
    );
    drop(store);
    corrupt_revision(
        &scratch.path,
        "UPDATE pages SET revision = -5 WHERE id = ?1",
    );
    let before = support::bytes(&scratch.path);

    let mut store = Store::open(&scratch.path).expect("open");
    let err = import_json(&mut store, &import_with_new_records()).expect_err("revision");
    assert_storage(err);
    assert_import_refused_by_revision(&scratch, &store, &before);
    // The corrupt value is still there. It was not reset or repaired.
    let conn = Connection::open(&scratch.path).expect("raw");
    let revision: i64 = conn
        .query_row(
            "SELECT revision FROM pages WHERE id = ?1",
            [PAGE_ONE],
            |row| row.get(0),
        )
        .expect("revision");
    drop(conn);
    assert_eq!(revision, -5);
}

#[test]
fn an_import_refuses_to_reconcile_a_text_stored_revision() {
    let (scratch, mut store) = created("reconcile-text.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    link_page(
        &mut store,
        PAGE_ONE,
        "Encuentro",
        "Elara",
        mention(MENTION_ONE, ENTITY_ONE, 0, 5),
    );
    drop(store);
    corrupt_revision(
        &scratch.path,
        "UPDATE pages SET revision = 'seven' WHERE id = ?1",
    );
    let before = support::bytes(&scratch.path);

    let mut store = Store::open(&scratch.path).expect("open");
    let err = import_json(&mut store, &import_with_new_records()).expect_err("revision");
    assert_storage(err);
    assert_import_refused_by_revision(&scratch, &store, &before);
}

#[test]
fn an_import_that_reconciles_a_healthy_page_still_succeeds() {
    // Positive control for the revision check: the same document over a
    // healthy page imports the new page and the new entity.
    let (_scratch, mut store) = created("reconcile-healthy.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    link_page(
        &mut store,
        PAGE_ONE,
        "Encuentro",
        "Elara",
        mention(MENTION_ONE, ENTITY_ONE, 0, 5),
    );

    let outcome = import_json(&mut store, &import_with_new_records()).expect("import");
    assert_eq!(outcome.inserted, 2);
    assert_eq!(outcome.unchanged, 1);
    assert_eq!(
        store.page(&page_id(PAGE_TWO)).expect("new page").body,
        "texto"
    );
    assert_eq!(
        store
            .entity(&entity_id(ENTITY_TWO))
            .expect("new entity")
            .label,
        "Nueva entidad"
    );
}

// ------------------------------------------------------- positional records

#[test]
fn a_version_1_page_sent_as_an_array_is_refused() {
    let (_scratch, mut store) = created("shape-v1-page.sqlite");
    let document = format!(
        r#"{{"format_version":1,"universe_id":"{UNIVERSE}","pages":[["{PAGE_ONE}","Titulo","Elara"]],"types":[],"entities":[],"definitions":[],"assertions":[]}}"#
    )
    .into_bytes();

    let err = import_json(&mut store, &document).expect_err("array page");
    assert_invalid_document_at(err, "pages[0]");
    assert_eq!(store.counts().expect("counts").pages, 0);
}

#[test]
fn a_version_2_page_sent_as_an_array_is_refused() {
    let (_scratch, mut store) = created("shape-v2-page.sqlite");
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[["{PAGE_ONE}","Titulo","Elara",[]]],"types":[],"entities":[],"definitions":[],"assertions":[]}}"#
    )
    .into_bytes();

    let err = import_json(&mut store, &document).expect_err("array page");
    assert_invalid_document_at(err, "pages[0]");
    assert_eq!(store.counts().expect("counts").pages, 0);
}

#[test]
fn a_version_2_occurrence_sent_as_an_array_is_refused() {
    let (_scratch, mut store) = created("shape-v2-mention.sqlite");
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[{{"id":"{PAGE_ONE}","title":"Titulo","body":"Elara","mentions":[["{MENTION_ONE}","{ENTITY_ONE}",0,5]]}}],"types":[],"entities":[{{"id":"{ENTITY_ONE}","label":"Elara"}}],"definitions":[],"assertions":[]}}"#
    )
    .into_bytes();

    let err = import_json(&mut store, &document).expect_err("array mention");
    assert_invalid_document_at(err, "pages[0].mentions[0]");
    assert_eq!(store.counts().expect("counts").pages, 0);
}

#[test]
fn an_entity_or_a_type_sent_as_an_array_is_refused() {
    let (_scratch, mut store) = created("shape-entity-type.sqlite");
    let entities = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[],"types":[],"entities":[["{ENTITY_ONE}","Elara",null,null]],"definitions":[],"assertions":[]}}"#
    )
    .into_bytes();
    let err = import_json(&mut store, &entities).expect_err("array entity");
    assert_invalid_document_at(err, "entities[0]");

    let types = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[],"types":[["{TYPE_ONE}","Personaje"]],"entities":[],"definitions":[],"assertions":[]}}"#
    )
    .into_bytes();
    let err = import_json(&mut store, &types).expect_err("array type");
    assert_invalid_document_at(err, "types[0]");
    assert_eq!(store.counts().expect("counts").entities, 0);
}

#[test]
fn a_definition_owner_or_an_assertion_value_sent_as_an_array_is_refused() {
    let (_scratch, mut store) = created("shape-nested.sqlite");
    let definitions = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[],"types":[{{"id":"{TYPE_ONE}","label":"Personaje"}}],"entities":[],"definitions":[{{"id":"{DEFINITION_ONE}","owner":["type","{TYPE_ONE}"],"value_class":"text","cardinality":"singular","archived":false}}],"assertions":[]}}"#
    )
    .into_bytes();
    let err = import_json(&mut store, &definitions).expect_err("array owner");
    assert_invalid_document_at(err, "definitions[0].owner");

    let assertions = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[],"types":[],"entities":[],"definitions":[],"assertions":[{{"id":"{ASSERTION_ONE}","subject_id":"{ENTITY_ONE}","definition_id":"{DEFINITION_ONE}","value":[],"canon":"PROVISIONAL","source_status_raw":null,"provenance":{{}},"scope_note":null,"archived":false}}]}}"#
    )
    .into_bytes();
    let err = import_json(&mut store, &assertions).expect_err("array value");
    assert_invalid_document_at(err, "assertions[0].value");

    let provenance = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[],"types":[],"entities":[],"definitions":[],"assertions":[{{"id":"{ASSERTION_ONE}","subject_id":"{ENTITY_ONE}","definition_id":"{DEFINITION_ONE}","value":{{"kind":"unknown"}},"canon":"PROVISIONAL","source_status_raw":null,"provenance":[],"scope_note":null,"archived":false}}]}}"#
    )
    .into_bytes();
    let err = import_json(&mut store, &provenance).expect_err("array provenance");
    assert_invalid_document_at(err, "assertions[0].provenance");
    assert_eq!(store.counts().expect("counts").definitions, 0);
}

#[test]
fn an_array_shaped_document_reserves_no_destination() {
    let scratch = scratch("shape-destination.sqlite");
    let destination = scratch.path.with_file_name("shape.sqlite");
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[["{PAGE_ONE}","Titulo","Elara",[]]],"types":[],"entities":[],"definitions":[],"assertions":[]}}"#
    )
    .into_bytes();

    let err = import_new_json(&destination, &document).expect_err("array page");
    assert_invalid_document_at(err, "pages[0]");
    assert!(
        !destination.exists(),
        "no destination after a malformed shape"
    );
}

#[test]
fn a_document_of_proper_objects_still_imports_and_round_trips() {
    let scratch = scratch("shape-positive.sqlite");
    let destination = scratch.path.with_file_name("proper.sqlite");
    let document = v2_document(
        &v2_page(
            PAGE_ONE,
            "Encuentro",
            "Elara",
            &v2_mention(MENTION_ONE, ENTITY_ONE, 0, 5),
        ),
        &v2_entity(ENTITY_ONE, "Elara"),
    );

    let (mut store, outcome) = import_new_json(&destination, &document).expect("import");
    assert_eq!(outcome.inserted, 2);
    let page = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(page.body.mentions.len(), 1);
    assert_eq!(page.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(page.body.mentions[0].start_utf16, 0);
    assert_eq!(page.body.mentions[0].end_utf16, 5);
    let bytes = export_json(&mut store).expect("export");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(json["format_version"], 2);
    assert_eq!(json["pages"][0]["mentions"][0]["id"], MENTION_ONE);
    assert!(!String::from_utf8(bytes).expect("utf8").contains("revision"));
}
