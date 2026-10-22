//! Regression probes for the two review blockers and the export integrity
//! question of PR #15.
//!
//! A duplicate JSON object key is not expressible through `serde_json::json!`,
//! so these documents are literal strings. Each probe asserts literal values,
//! checks that nothing was written, and fails when the guard it covers is
//! removed.

#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use std::path::Path;

use narrative_universe::{
    export_json, import_json, import_new_json, Entity, EntityId, Error, Mention, MentionEdit,
    MentionId, Page, PageEdit, PageId, PageRevision, Store, UniverseId, ValidationReason,
};
use rusqlite::Connection;
use support::scratch;

const UNIVERSE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const PAGE_ONE: &str = "22222222-2222-4222-8222-222222222221";
const PAGE_TWO: &str = "22222222-2222-4222-8222-222222222222";
const ENTITY_ONE: &str = "11111111-1111-4111-8111-111111111111";
const ENTITY_TWO: &str = "33333333-3333-4333-8333-333333333333";
const MENTION_ONE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const MENTION_TWO: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

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

fn create_entity(store: &mut Store, raw: &str, label: &str) {
    store
        .create_entity(Entity {
            id: entity(raw),
            label: label.to_string(),
            page_id: None,
            type_id: None,
        })
        .expect("entity");
}

fn link_page(store: &mut Store, raw: &str, body: &str, occurrence: Mention) {
    store
        .create_page(new_page(raw, "Encuentro", body))
        .expect("page");
    let document = store.page_document(&page_id(raw)).expect("document");
    store
        .save_page_edits(&page_id(raw), document.revision, &[link(occurrence)])
        .expect("link");
}

fn occurrence_member(id: &str, entity_raw: &str, start_utf16: u32, end_utf16: u32) -> String {
    format!(
        r#"{{"id":"{id}","entity_id":"{entity_raw}","start_utf16":{start_utf16},"end_utf16":{end_utf16}}}"#
    )
}

/// A version 2 document whose single page declares its occurrences through two
/// `mentions` members, in the given order.
fn two_mentions_members(first: &str, second: &str) -> Vec<u8> {
    format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[{{"id":"{PAGE_ONE}","title":"Repetido","body":"Elara",{first},{second}}}],"types":[],"entities":[{{"id":"{ENTITY_ONE}","label":"Elara"}}],"definitions":[],"assertions":[]}}"#
    )
    .into_bytes()
}

fn assert_invalid_document(err: Error) {
    assert!(matches!(err, Error::InvalidDocument { .. }), "{err}");
}

/// The store must be untouched: no page, no occurrence, and the same bytes.
fn assert_untouched(scratch: &support::Scratch, store: &Store, before: &[u8]) {
    let err = store.page(&page_id(PAGE_ONE)).expect_err("page absent");
    assert!(matches!(err, Error::Validation { .. }), "{err}");
    let err = store
        .mentions_for_entity(&entity(ENTITY_ONE))
        .expect_err("entity absent");
    assert!(matches!(err, Error::Validation { .. }), "{err}");
    let counts = store.counts().expect("counts");
    assert_eq!(counts.pages, 0);
    assert_eq!(counts.entities, 0);
    let conn = Connection::open(&scratch.path).expect("raw");
    let mentions: i64 = conn
        .query_row("SELECT COUNT(*) FROM mentions", [], |row| row.get(0))
        .expect("mentions");
    drop(conn);
    assert_eq!(mentions, 0);
    assert_eq!(support::bytes(&scratch.path), before);
}

#[test]
fn a_page_that_declares_its_occurrences_and_then_an_empty_list_is_refused() {
    let (scratch, mut store) = created("duplicate-mentions-populated-first.sqlite");
    let before = support::bytes(&scratch.path);
    let document = two_mentions_members(
        &format!(
            r#""mentions":[{}]"#,
            occurrence_member(MENTION_ONE, ENTITY_ONE, 0, 5)
        ),
        r#""mentions":[]"#,
    );

    let err = import_json(&mut store, &document).expect_err("duplicate mentions");
    assert_invalid_document(err);
    assert_untouched(&scratch, &store, &before);
}

#[test]
fn a_page_that_declares_an_empty_list_and_then_its_occurrences_is_refused() {
    let (scratch, mut store) = created("duplicate-mentions-empty-first.sqlite");
    let before = support::bytes(&scratch.path);
    let document = two_mentions_members(
        r#""mentions":[]"#,
        &format!(
            r#""mentions":[{}]"#,
            occurrence_member(MENTION_ONE, ENTITY_ONE, 0, 5)
        ),
    );

    let err = import_json(&mut store, &document).expect_err("duplicate mentions");
    assert_invalid_document(err);
    assert_untouched(&scratch, &store, &before);
}

#[test]
fn a_page_that_declares_two_non_empty_occurrence_lists_is_refused() {
    let (scratch, mut store) = created("duplicate-mentions-both-full.sqlite");
    let before = support::bytes(&scratch.path);
    let document = two_mentions_members(
        &format!(
            r#""mentions":[{}]"#,
            occurrence_member(MENTION_ONE, ENTITY_ONE, 0, 5)
        ),
        &format!(
            r#""mentions":[{}]"#,
            occurrence_member(MENTION_TWO, ENTITY_ONE, 0, 5)
        ),
    );

    let err = import_json(&mut store, &document).expect_err("duplicate mentions");
    assert_invalid_document(err);
    assert_untouched(&scratch, &store, &before);
}

#[test]
fn a_document_that_declares_its_pages_twice_is_refused() {
    let (scratch, mut store) = created("duplicate-pages.sqlite");
    let before = support::bytes(&scratch.path);
    let page = format!(r#"{{"id":"{PAGE_ONE}","title":"Repetido","body":"Elara","mentions":[]}}"#);
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[{page}],"pages":[],"types":[],"entities":[],"definitions":[],"assertions":[]}}"#
    )
    .into_bytes();

    let err = import_json(&mut store, &document).expect_err("duplicate pages");
    assert_invalid_document(err);
    assert_untouched(&scratch, &store, &before);
}

#[test]
fn a_document_that_declares_its_version_twice_is_refused() {
    let (scratch, mut store) = created("duplicate-version.sqlite");
    let before = support::bytes(&scratch.path);
    let tail = format!(
        r#""universe_id":"{UNIVERSE}","pages":[],"types":[],"entities":[],"definitions":[],"assertions":[]}}"#
    );
    let version_two_then_one =
        format!(r#"{{"format_version":2,"format_version":1,{tail}"#).into_bytes();
    let version_one_then_two =
        format!(r#"{{"format_version":1,"format_version":2,{tail}"#).into_bytes();

    for document in [version_two_then_one, version_one_then_two] {
        let err = import_json(&mut store, &document).expect_err("duplicate version");
        assert_invalid_document(err);
    }
    assert_untouched(&scratch, &store, &before);
}

#[test]
fn a_mention_that_declares_a_field_twice_is_refused() {
    let (scratch, mut store) = created("duplicate-mention-field.sqlite");
    let before = support::bytes(&scratch.path);
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[{{"id":"{PAGE_ONE}","title":"Repetido","body":"Elara","mentions":[{{"id":"{MENTION_ONE}","entity_id":"{ENTITY_ONE}","entity_id":"{ENTITY_TWO}","start_utf16":0,"end_utf16":5}}]}}],"types":[],"entities":[{{"id":"{ENTITY_ONE}","label":"Elara"}},{{"id":"{ENTITY_TWO}","label":"Otra"}}],"definitions":[],"assertions":[]}}"#
    )
    .into_bytes();

    let err = import_json(&mut store, &document).expect_err("duplicate field");
    assert_invalid_document(err);
    assert_untouched(&scratch, &store, &before);
}

#[test]
fn an_occurrence_that_ends_beyond_its_body_reserves_no_destination() {
    let scratch = scratch("import-new-out-of-range.sqlite");
    let destination = scratch.path.with_file_name("malformed.sqlite");
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[{{"id":"{PAGE_ONE}","title":"Fuera","body":"Elara","mentions":[{{"id":"{MENTION_ONE}","entity_id":"{ENTITY_ONE}","start_utf16":0,"end_utf16":99}}]}}],"types":[],"entities":[{{"id":"{ENTITY_ONE}","label":"Elara"}}],"definitions":[],"assertions":[]}}"#
    )
    .into_bytes();

    let err = import_new_json(&destination, &document).expect_err("out of range");
    match err {
        Error::Validation { reason, field, .. } => {
            assert_eq!(reason, ValidationReason::InvalidMentionRange);
            assert_eq!(field, "end_utf16");
        }
        other => panic!("expected invalid range, got {other}"),
    }
    assert!(
        !destination.exists(),
        "a malformed document must not reserve a destination"
    );
}

#[test]
fn an_occurrence_that_splits_a_grapheme_reserves_no_destination() {
    let scratch = scratch("import-new-grapheme.sqlite");
    let destination = scratch.path.with_file_name("malformed.sqlite");
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[{{"id":"{PAGE_ONE}","title":"Partido","body":"e\u0301lara","mentions":[{{"id":"{MENTION_ONE}","entity_id":"{ENTITY_ONE}","start_utf16":0,"end_utf16":1}}]}}],"types":[],"entities":[{{"id":"{ENTITY_ONE}","label":"Elara"}}],"definitions":[],"assertions":[]}}"#
    )
    .into_bytes();

    let err = import_new_json(&destination, &document).expect_err("split grapheme");
    match err {
        Error::Validation { reason, field, .. } => {
            assert_eq!(reason, ValidationReason::SplitGrapheme);
            assert_eq!(field, "end_utf16");
        }
        other => panic!("expected split grapheme, got {other}"),
    }
    assert!(
        !destination.exists(),
        "no destination after a malformed body"
    );
}

#[test]
fn an_import_into_a_new_path_still_accepts_a_forward_entity_reference() {
    let scratch = scratch("import-new-forward.sqlite");
    let destination = scratch.path.with_file_name("forward.sqlite");
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[{{"id":"{PAGE_ONE}","title":"Adelante","body":"Elara","mentions":[{{"id":"{MENTION_ONE}","entity_id":"{ENTITY_ONE}","start_utf16":0,"end_utf16":5}}]}}],"types":[],"entities":[{{"id":"{ENTITY_ONE}","label":"Elara"}}],"definitions":[],"assertions":[]}}"#
    )
    .into_bytes();

    let (store, outcome) = import_new_json(&destination, &document).expect("import");
    assert_eq!(outcome.inserted, 2);
    let page = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(page.body.body, "Elara");
    assert_eq!(page.body.mentions.len(), 1);
    assert_eq!(page.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(page.body.mentions[0].entity_id.as_str(), ENTITY_ONE);
    assert_eq!(page.body.mentions[0].start_utf16, 0);
    assert_eq!(page.body.mentions[0].end_utf16, 5);
}

fn corrupt_revision(path: &Path, raw_sql: &str) {
    let conn = Connection::open(path).expect("raw");
    conn.execute_batch("PRAGMA ignore_check_constraints = ON")
        .expect("checks");
    conn.execute(raw_sql, [PAGE_ONE]).expect("corrupt");
}

#[test]
fn an_export_refuses_a_page_whose_stored_revision_is_negative() {
    let (scratch, mut store) = created("export-negative-revision.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    link_page(
        &mut store,
        PAGE_ONE,
        "Elara",
        mention(MENTION_ONE, ENTITY_ONE, 0, 5),
    );
    drop(store);
    corrupt_revision(
        &scratch.path,
        "UPDATE pages SET revision = -7 WHERE id = ?1",
    );
    let before = support::bytes(&scratch.path);

    let mut store = Store::open(&scratch.path).expect("open");
    // The aggregate read already refuses the row.
    let err = store
        .page_document(&page_id(PAGE_ONE))
        .expect_err("revision");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
    // The export refuses it too instead of writing a universe that the read
    // path considers corrupt.
    let err = export_json(&mut store).expect_err("export");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
    let err = store.export_snapshot().expect_err("snapshot");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
    assert_eq!(support::bytes(&scratch.path), before);
}

#[test]
fn an_export_refuses_a_page_whose_stored_revision_is_not_an_integer() {
    let (scratch, mut store) = created("export-text-revision.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    link_page(
        &mut store,
        PAGE_ONE,
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
    let err = store
        .page_document(&page_id(PAGE_ONE))
        .expect_err("revision");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
    let err = export_json(&mut store).expect_err("export");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
    assert_eq!(support::bytes(&scratch.path), before);
}

#[test]
fn a_healthy_universe_still_exports_both_pages_after_the_checks() {
    let (_scratch, mut store) = created("export-healthy.sqlite");
    create_entity(&mut store, ENTITY_ONE, "Elara");
    link_page(
        &mut store,
        PAGE_ONE,
        "Elara",
        mention(MENTION_ONE, ENTITY_ONE, 0, 5),
    );
    store
        .create_page(new_page(PAGE_TWO, "Sin enlaces", "texto"))
        .expect("page");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.revision, PageRevision::new(1).expect("revision"));

    let bytes = export_json(&mut store).expect("export");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(json["format_version"], 2);
    let pages = json["pages"].as_array().expect("pages");
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0]["id"], PAGE_ONE);
    assert_eq!(pages[0]["mentions"][0]["id"], MENTION_ONE);
    assert_eq!(pages[1]["id"], PAGE_TWO);
    assert_eq!(pages[1]["mentions"], serde_json::json!([]));
    assert!(!String::from_utf8(bytes).expect("utf8").contains("revision"));
}
