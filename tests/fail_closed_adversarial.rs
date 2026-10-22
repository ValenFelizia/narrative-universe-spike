//! Adversarial probes for the three fail-closed repairs of the Astra review.
//!
//! Each probe pins one guard of the delta: a page id whose occurrence rows
//! survived its page row cannot come back, an import refuses to reconcile a
//! page whose stored revision the aggregate read refuses, and a positional
//! array is never read as a record. Unrelated and healthy paths must stay
//! open, every refusal must leave the corrupt state untouched, and a batch
//! must roll back as a whole.
//!
//! Every probe runs against a real SQLite file and asserts literal state.

#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use std::path::Path;

use narrative_universe::{
    export_json, import_json, import_new_json, Entity, EntityId, Error, Mention, MentionEdit,
    MentionId, Page, PageEdit, PageId, Store, UniverseId,
};
use rusqlite::Connection;
use support::scratch;

const UNIVERSE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const PAGE_ONE: &str = "22222222-2222-4222-8222-222222222221";
const PAGE_TWO: &str = "22222222-2222-4222-8222-222222222222";
const TYPE_ONE: &str = "44444444-4444-4444-8444-444444444444";
const ENTITY_ONE: &str = "11111111-1111-4111-8111-111111111111";
const ENTITY_TWO: &str = "33333333-3333-4333-8333-333333333333";
const DEFINITION_ONE: &str = "77777777-7777-4777-8777-777777777777";
const ASSERTION_ONE: &str = "88888888-8888-4888-8888-888888888888";
const MENTION_ONE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const MENTION_TWO: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

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

/// A universe whose page row was deleted outside SQLite while one of its
/// occurrence rows survived, which foreign keys only allow when disabled.
fn orphaned(
    name: &str,
    page_raw: &str,
    title: &str,
    body: &str,
    occurrence: Mention,
) -> (support::Scratch, Store) {
    let (scratch, mut store) = created(name);
    create_entity(&mut store, ENTITY_ONE, "Elara");
    link_page(&mut store, page_raw, title, body, occurrence);
    drop(store);
    let conn = Connection::open(&scratch.path).expect("raw");
    conn.execute_batch("PRAGMA foreign_keys = OFF").expect("fk");
    conn.execute("DELETE FROM pages WHERE id = ?1", [page_raw])
        .expect("orphan");
    drop(conn);
    let store = Store::open(&scratch.path).expect("open");
    (scratch, store)
}

fn raw_count(path: &Path, sql: &str) -> i64 {
    let conn = Connection::open(path).expect("raw");
    conn.query_row(sql, [], |row| row.get(0)).expect("count")
}

/// The one surviving occurrence of the missing page, read raw.
fn stored_occurrence(path: &Path, mention_raw: &str) -> (String, String, i64, i64) {
    let conn = Connection::open(path).expect("raw");
    conn.query_row(
        "SELECT page_id, entity_id, start_utf16, end_utf16 FROM mentions WHERE id = ?1",
        [mention_raw],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        },
    )
    .expect("occurrence row")
}

/// The raw column type and value of a stored revision.
fn raw_real_revision(path: &Path) -> (String, f64) {
    let conn = Connection::open(path).expect("raw");
    conn.query_row(
        "SELECT typeof(revision), revision FROM pages WHERE id = ?1",
        [PAGE_ONE],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?)),
    )
    .expect("revision row")
}

fn raw_integer_revision(path: &Path) -> i64 {
    let conn = Connection::open(path).expect("raw");
    conn.query_row(
        "SELECT revision FROM pages WHERE id = ?1",
        [PAGE_ONE],
        |row| row.get(0),
    )
    .expect("revision row")
}

fn corrupt_revision(path: &Path, sql: &str) {
    let conn = Connection::open(path).expect("raw");
    conn.execute_batch("PRAGMA ignore_check_constraints = ON")
        .expect("checks");
    conn.execute(sql, [PAGE_ONE]).expect("corrupt");
}

fn assert_storage(err: Error) {
    assert!(matches!(err, Error::Storage { .. }), "{err}");
}

fn assert_not_found(err: Error) {
    assert!(
        err.to_string().contains("NotFound"),
        "expected NotFound, got {err}"
    );
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

fn v2_entity_on_page(id: &str, label: &str, page_raw: &str) -> String {
    format!(r#"{{"id":"{id}","label":"{label}","page_id":"{page_raw}"}}"#)
}

/// A new page and a new entity plus the reconcile page itself, so a refusal
/// on the reconciliation has to roll the new records back, too.
fn reconcile_with_new_records() -> Vec<u8> {
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

/// A new page whose entity points at the corrupt page only indirectly.
fn new_page_pointing_at_corrupt_page() -> Vec<u8> {
    v2_document(
        &v2_page(PAGE_TWO, "Nueva", "texto", ""),
        &v2_entity_on_page(ENTITY_TWO, "Nueva entidad", PAGE_ONE),
    )
}

// --------------------------------------------------------------- orphan reuse

#[test]
fn an_orphan_blocks_its_page_while_an_unrelated_page_is_still_created() {
    let (scratch, mut store) = orphaned(
        "adv-orphan-unrelated.sqlite",
        PAGE_ONE,
        "Encuentro",
        "Elara",
        mention(MENTION_ONE, ENTITY_ONE, 0, 5),
    );

    // The unrelated page never carried occurrences, so its id stays
    // creatable in the same store: the guard is not a store-wide refusal.
    store
        .create_page(new_page(PAGE_TWO, "Prosas nuevas", "Texto nuevo"))
        .expect("unrelated page");

    let err = store
        .create_page(new_page(PAGE_ONE, "Otra prosa", "Elara"))
        .expect_err("orphan");
    assert_storage(err);
    drop(store);

    assert_eq!(raw_count(&scratch.path, "SELECT COUNT(*) FROM pages"), 1);
    // The row still points at the missing page and keeps its literal
    // identity: it was not adopted, relinked or deleted.
    assert_eq!(
        stored_occurrence(&scratch.path, MENTION_ONE),
        (PAGE_ONE.to_string(), ENTITY_ONE.to_string(), 0, 5)
    );
}

#[test]
fn an_orphan_that_belongs_to_a_second_page_id_blocks_only_that_page() {
    let (scratch, mut store) = orphaned(
        "adv-orphan-second-id.sqlite",
        PAGE_TWO,
        "Aria nocturna",
        "Aria nocturna",
        mention(MENTION_TWO, ENTITY_ONE, 6, 10),
    );

    let err = store
        .create_page(new_page(PAGE_TWO, "Reprise", "Otra vez"))
        .expect_err("orphan");
    assert_storage(err);

    // In the same store the unrelated id that never had occurrences
    // (because its rows belong to a different set) is still creatable.
    store
        .create_page(new_page(PAGE_ONE, "Prosas nuevas", "Texto nuevo"))
        .expect("unrelated page");
    drop(store);

    assert_eq!(raw_count(&scratch.path, "SELECT COUNT(*) FROM pages"), 1);
    assert_eq!(
        stored_occurrence(&scratch.path, MENTION_TWO),
        (PAGE_TWO.to_string(), ENTITY_ONE.to_string(), 6, 10)
    );
}

#[test]
fn a_batch_that_first_inserts_a_page_then_hits_the_orphan_guard_fully_rolls_back() {
    let (scratch, store) = orphaned(
        "adv-orphan-rollback.sqlite",
        PAGE_ONE,
        "Encuentro",
        "Elara",
        mention(MENTION_ONE, ENTITY_ONE, 0, 5),
    );
    drop(store);
    let before = support::bytes(&scratch.path);
    let mut store = Store::open(&scratch.path).expect("open");
    // The batch inserts the fresh page first, then reaches the corrupt id
    // carrying no occurrences in the document at all, so the orphan guard
    // is the only thing between the batch and the commit.
    let document = reconcile_with_new_records();

    let err = import_json(&mut store, &document).expect_err("orphan");
    drop(store);
    assert_storage(err);

    let store = Store::open(&scratch.path).expect("reopen");
    assert_not_found(store.page(&page_id(PAGE_TWO)).expect_err("page"));
    assert_not_found(store.entity(&entity_id(ENTITY_TWO)).expect_err("entity"));
    drop(store);
    assert_eq!(raw_count(&scratch.path, "SELECT COUNT(*) FROM pages"), 0);
    assert_eq!(raw_count(&scratch.path, "SELECT COUNT(*) FROM entities"), 1);
    assert_eq!(support::bytes(&scratch.path), before);
}

// ---------------------------------------------------------- corrupt revisions

#[test]
fn a_revision_stored_as_a_float_refuses_the_import_batch() {
    let (scratch, mut store) = created("adv-revision-float.sqlite");
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
        "UPDATE pages SET revision = 1.5 WHERE id = ?1",
    );
    let before = support::bytes(&scratch.path);
    let mut store = Store::open(&scratch.path).expect("open");

    let err = import_json(&mut store, &reconcile_with_new_records()).expect_err("revision");
    drop(store);
    assert_storage(err);

    let store = Store::open(&scratch.path).expect("reopen");
    assert_not_found(store.page(&page_id(PAGE_TWO)).expect_err("page"));
    assert_not_found(store.entity(&entity_id(ENTITY_TWO)).expect_err("entity"));
    drop(store);
    // The corrupt value is still there as a float. It was not truncated,
    // reset or repaired.
    assert_eq!(raw_real_revision(&scratch.path), ("real".to_string(), 1.5));
    assert_eq!(support::bytes(&scratch.path), before);
}

#[test]
fn a_stored_revision_above_i64_refuses_the_import_batch() {
    let (scratch, mut store) = created("adv-revision-above-i64.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    link_page(
        &mut store,
        PAGE_ONE,
        "Encuentro",
        "Elara",
        mention(MENTION_ONE, ENTITY_ONE, 0, 5),
    );
    drop(store);
    // SQLite keeps an integer that does not fit in i64 as a REAL, so this
    // corrupts the check on the typed i64 read the import uses.
    corrupt_revision(
        &scratch.path,
        "UPDATE pages SET revision = 9223372036854775808 WHERE id = ?1",
    );
    let before = support::bytes(&scratch.path);
    let mut store = Store::open(&scratch.path).expect("open");

    let err = import_json(&mut store, &reconcile_with_new_records()).expect_err("revision");
    drop(store);
    assert_storage(err);

    let store = Store::open(&scratch.path).expect("reopen");
    assert_not_found(store.page(&page_id(PAGE_TWO)).expect_err("page"));
    assert_not_found(store.entity(&entity_id(ENTITY_TWO)).expect_err("entity"));
    drop(store);
    assert_eq!(
        raw_real_revision(&scratch.path),
        ("real".to_string(), 9223372036854775808.0)
    );
    assert_eq!(support::bytes(&scratch.path), before);
}

#[test]
fn a_corrupt_page_referenced_only_indirectly_refuses_the_import_batch() {
    let (scratch, mut store) = created("adv-revision-indirect.sqlite");
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
        "UPDATE pages SET revision = -3 WHERE id = ?1",
    );
    let before = support::bytes(&scratch.path);
    let mut store = Store::open(&scratch.path).expect("open");
    // The corrupt page is absent from the document itself; the batch only
    // points at through the new entity's page_id.
    let document = new_page_pointing_at_corrupt_page();

    let err = import_json(&mut store, &document).expect_err("revision");
    drop(store);
    assert_storage(err);

    let store = Store::open(&scratch.path).expect("reopen");
    assert_not_found(store.page(&page_id(PAGE_TWO)).expect_err("page"));
    assert_not_found(store.entity(&entity_id(ENTITY_TWO)).expect_err("entity"));
    drop(store);
    assert_eq!(raw_integer_revision(&scratch.path), -3);
    assert_eq!(support::bytes(&scratch.path), before);
}

// --------------------------------------------------------- positional records

#[test]
fn an_assertion_provenance_sent_as_an_array_is_refused() {
    let (_scratch, mut store) = created("adv-shape-provenance.sqlite");
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[],"types":[],"entities":[],"definitions":[{{"id":"{DEFINITION_ONE}","owner":{{"type":"type","id":"{TYPE_ONE}"}},"value_class":"text","cardinality":"singular","archived":false}}],"assertions":[{{"id":"{ASSERTION_ONE}","subject_id":"{ENTITY_ONE}","definition_id":"{DEFINITION_ONE}","value":{{"kind":"unknown"}},"canon":"PROVISIONAL","source_status_raw":null,"provenance":[],"scope_note":null,"archived":false}}]}}"#
    )
    .into_bytes();

    let err = import_json(&mut store, &document).expect_err("array provenance");
    assert_invalid_document_at(err, "assertions[0].provenance");
    let counts = store.counts().expect("counts");
    assert_eq!(counts.pages, 0);
    assert_eq!(counts.entities, 0);
    assert_eq!(counts.definitions, 0);
}

#[test]
fn a_mention_array_in_a_page_alongside_a_valid_page_is_refused() {
    let (_scratch, mut store) = created("adv-shape-mention-valid-page.sqlite");
    let document = v2_document(
        &format!(
            "{},{}",
            // The healthy control: a proper object page in the same document.
            v2_page(PAGE_TWO, "Valida", "Aria nocturna", ""),
            v2_page(
                PAGE_ONE,
                "Rota",
                "Elara",
                &format!(r#"["{MENTION_TWO}","{ENTITY_ONE}",0,5]"#)
            )
        ),
        &v2_entity(ENTITY_ONE, "Elara"),
    );

    let err = import_json(&mut store, &document).expect_err("array mention");
    assert_invalid_document_at(err, "pages[1].mentions[0]");
    let counts = store.counts().expect("counts");
    assert_eq!(counts.pages, 0);
    assert_eq!(counts.entities, 0);
}

#[test]
fn a_version_1_page_with_three_members_is_refused() {
    let (_scratch, mut store) = created("adv-shape-v1-three.sqlite");
    // A three element array matches the three fields of the version 1 page
    // record by position.
    let document = format!(
        r#"{{"format_version":1,"universe_id":"{UNIVERSE}","pages":[["{PAGE_ONE}","Titulo mezclado","Elara"]],"types":[],"entities":[],"definitions":[],"assertions":[]}}"#
    )
    .into_bytes();

    let err = import_json(&mut store, &document).expect_err("array page");
    assert_invalid_document_at(err, "pages[0]");
    assert_eq!(store.counts().expect("counts").pages, 0);
}

#[test]
fn a_version_2_page_with_four_members_is_refused() {
    let (_scratch, mut store) = created("adv-shape-v2-four.sqlite");
    // A four element array matches the four fields of the version 2 page
    // record by position, including the empty occurrence list.
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[["{PAGE_ONE}","Titulo mezclado","Elara",[]]],"types":[],"entities":[],"definitions":[],"assertions":[]}}"#
    )
    .into_bytes();

    let err = import_json(&mut store, &document).expect_err("array page");
    assert_invalid_document_at(err, "pages[0]");
    assert_eq!(store.counts().expect("counts").pages, 0);
}

// ------------------------------------------------------------- positive paths

#[test]
fn a_proper_object_document_still_round_trips_after_all_of_this() {
    // Positive control: after every shape and store guard above, a document
    // of proper objects still imports into a brand new file and exports.
    let scratch = scratch("adv-shape-positive.sqlite");
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
    assert_eq!(page.body.body, "Elara");
    assert_eq!(page.body.mentions.len(), 1);
    assert_eq!(page.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(page.body.mentions[0].start_utf16, 0);
    assert_eq!(page.body.mentions[0].end_utf16, 5);
    let bytes = export_json(&mut store).expect("export");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(json["format_version"], 2);
    assert_eq!(json["pages"][0]["title"], "Encuentro");
    assert_eq!(json["pages"][0]["mentions"][0]["id"], MENTION_ONE);
    assert!(!String::from_utf8(bytes).expect("utf8").contains("revision"));
}

#[test]
fn an_import_that_reconciles_a_healthy_page_while_a_different_page_is_new() {
    // Positive control for the revision check: a healthy reconcile page and
    // a brand new page and entity in the same document still import.
    let (_scratch, mut store) = created("adv-reconcile-healthy.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    link_page(
        &mut store,
        PAGE_ONE,
        "Encuentro",
        "Elara",
        mention(MENTION_ONE, ENTITY_ONE, 0, 5),
    );

    let outcome = import_json(&mut store, &reconcile_with_new_records()).expect("import");
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
    let page = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(page.body.mentions.len(), 1);
    assert_eq!(page.body.mentions[0].id.as_str(), MENTION_ONE);
}
