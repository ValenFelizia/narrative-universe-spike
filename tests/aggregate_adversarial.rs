//! Adversarial tests for the version 2 page aggregate.
//!
//! Every test runs against a real SQLite file, checks literal values, and
//! reads the stored rows back through a raw connection so a test never
//! compares a result with the method that produced it.

#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use narrative_universe::{
    export_json, Entity, EntityId, Error, Mention, MentionEdit, MentionId, Page, PageEdit, PageId,
    PageRevision, Store, UniverseId, ValidationReason,
};
use rusqlite::Connection;
use support::scratch;

const UNIVERSE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const PAGE_ONE: &str = "22222222-2222-4222-8222-222222222221";
const PAGE_TWO: &str = "22222222-2222-4222-8222-222222222222";
const ENTITY_ONE: &str = "11111111-1111-4111-8111-111111111111";
const ENTITY_TWO: &str = "33333333-3333-4333-8333-333333333333";
const MENTION_ONE: &str = "11111111-2222-4333-8444-555555555555";
const MENTION_TWO: &str = "22222222-3333-4444-8555-666666666666";
const MENTION_THREE: &str = "33333333-4444-4555-8666-777777777777";
const MENTION_FOUR: &str = "44444444-5555-4666-8777-888888888888";

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

fn new_entity(raw: &str, label: &str) -> Entity {
    Entity {
        id: entity(raw),
        label: label.to_string(),
        type_id: None,
        page_id: None,
    }
}

fn link(occurrence: Mention) -> PageEdit {
    PageEdit::Mention(MentionEdit::Link {
        mention: occurrence,
    })
}

fn unlink(raw: &str) -> PageEdit {
    PageEdit::Mention(MentionEdit::Unlink {
        mention_id: MentionId::parse(raw).expect("mention id"),
    })
}

fn replace(start_utf16: u32, end_utf16: u32, inserted_text: &str) -> PageEdit {
    PageEdit::Mention(MentionEdit::ReplaceText {
        start_utf16,
        end_utf16,
        inserted_text: inserted_text.to_string(),
    })
}

fn assert_validation(err: Error, reason: ValidationReason, field: &str, id: Option<&str>) {
    match err {
        Error::Validation {
            reason: got_reason,
            field: got_field,
            id: got_id,
        } => {
            assert_eq!(got_reason, reason);
            assert_eq!(got_field, field);
            assert_eq!(got_id.as_deref(), id);
        }
        other => panic!("expected validation {reason:?}, got {other}"),
    }
}

fn assert_stale(err: Error, page: &str, expected: i64, found: i64) {
    match err {
        Error::StaleRevision {
            page_id,
            expected: got_expected,
            found: got_found,
        } => {
            assert_eq!(page_id, page);
            assert_eq!(got_expected, expected);
            assert_eq!(got_found, found);
        }
        other => panic!("expected stale revision, got {other}"),
    }
}

/// The stored occurrence rows, read back without the store.
fn stored_mentions(path: &std::path::Path) -> Vec<(String, String, String, i64, i64)> {
    let conn = Connection::open(path).expect("raw");
    let mut statement = conn
        .prepare("SELECT id, page_id, entity_id, start_utf16, end_utf16 FROM mentions ORDER BY id")
        .expect("prepare");
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .expect("query");
    rows.map(|row| row.expect("row")).collect()
}

/// The stored page row, read back without the store.
fn stored_page(path: &std::path::Path, page: &str) -> (String, String, i64) {
    let conn = Connection::open(path).expect("raw");
    conn.query_row(
        "SELECT title, body, revision FROM pages WHERE id = ?1",
        [page],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        },
    )
    .expect("page row")
}

#[test]
fn a_batch_link_edit_link_keeps_the_surviving_occurrence() {
    let (scratch, mut store) = created("batch-link-edit-link.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara met Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");

    // The first occurrence is covered by the text edit and disappears. The
    // second one is measured in the body the edit produced.
    let saved = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[
                link(mention(MENTION_ONE, ENTITY_ONE, 0, 5)),
                replace(0, 5, "Z"),
                link(mention(MENTION_TWO, ENTITY_ONE, 6, 11)),
            ],
        )
        .expect("batch");
    assert_eq!(saved.body.body, "Z met Elara");
    assert_eq!(saved.body.mentions.len(), 1);
    assert_eq!(saved.body.mentions[0].id.as_str(), MENTION_TWO);
    assert_eq!(saved.body.mentions[0].entity_id.as_str(), ENTITY_ONE);
    assert_eq!(saved.body.mentions[0].start_utf16, 6);
    assert_eq!(saved.body.mentions[0].end_utf16, 11);
    // One batch is one committed change.
    assert_eq!(saved.revision, PageRevision::new(1).expect("revision"));
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.body.body, "Z met Elara");
    assert_eq!(document.body.mentions.len(), 1);
    assert_eq!(document.body.mentions[0].id.as_str(), MENTION_TWO);
    assert_eq!(document.body.mentions[0].start_utf16, 6);
    assert_eq!(document.body.mentions[0].end_utf16, 11);
    assert_eq!(document.revision, PageRevision::new(1).expect("revision"));

    let rows = stored_mentions(&scratch.path);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, MENTION_TWO);
    assert_eq!(rows[0].1, PAGE_ONE);
    assert_eq!(rows[0].2, ENTITY_ONE);
    assert_eq!(rows[0].3, 6);
    assert_eq!(rows[0].4, 11);
}

#[test]
fn a_batch_that_inserts_text_shifts_the_link_and_accepts_the_new_range() {
    let (scratch, mut store) = created("batch-shift.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara met Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");

    // The second occurrence is measured in the longer body. In the body the
    // batch started from, 15..20 was outside the text.
    let saved = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[
                link(mention(MENTION_ONE, ENTITY_ONE, 0, 5)),
                replace(0, 0, "Dear "),
                link(mention(MENTION_TWO, ENTITY_ONE, 15, 20)),
            ],
        )
        .expect("batch");
    assert_eq!(saved.body.body, "Dear Elara met Elara");
    assert_eq!(saved.body.mentions.len(), 2);
    assert_eq!(saved.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(saved.body.mentions[0].start_utf16, 5);
    assert_eq!(saved.body.mentions[0].end_utf16, 10);
    assert_eq!(saved.body.mentions[1].id.as_str(), MENTION_TWO);
    assert_eq!(saved.body.mentions[1].start_utf16, 15);
    assert_eq!(saved.body.mentions[1].end_utf16, 20);
    assert_eq!(saved.revision, PageRevision::new(1).expect("revision"));
    drop(store);

    let rows = stored_mentions(&scratch.path);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, MENTION_ONE);
    assert_eq!(rows[0].3, 5);
    assert_eq!(rows[0].4, 10);
    assert_eq!(rows[1].0, MENTION_TWO);
    assert_eq!(rows[1].3, 15);
    assert_eq!(rows[1].4, 20);
}

#[test]
fn two_pages_with_the_same_body_keep_their_own_occurrence_ids() {
    let (scratch, mut store) = created("same-body.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "First", "Elara met Elara"))
        .expect("page");
    store
        .create_page(new_page(PAGE_TWO, "Second", "Elara met Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let first = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            first.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect("link");
    let second = store.page_document(&page_id(PAGE_TWO)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_TWO),
            second.revision,
            &[link(mention(MENTION_TWO, ENTITY_ONE, 10, 15))],
        )
        .expect("link");

    let first = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(first.body.body, "Elara met Elara");
    assert_eq!(first.body.mentions.len(), 1);
    assert_eq!(first.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(first.body.mentions[0].start_utf16, 0);
    assert_eq!(first.body.mentions[0].end_utf16, 5);
    let second = store.page_document(&page_id(PAGE_TWO)).expect("document");
    assert_eq!(second.body.body, "Elara met Elara");
    assert_eq!(second.body.mentions.len(), 1);
    assert_eq!(second.body.mentions[0].id.as_str(), MENTION_TWO);
    assert_eq!(second.body.mentions[0].start_utf16, 10);
    assert_eq!(second.body.mentions[0].end_utf16, 15);

    let rows = stored_mentions(&scratch.path);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, MENTION_ONE);
    assert_eq!(rows[0].1, PAGE_ONE);
    assert_eq!(rows[0].3, 0);
    assert_eq!(rows[0].4, 5);
    assert_eq!(rows[1].0, MENTION_TWO);
    assert_eq!(rows[1].1, PAGE_TWO);
    assert_eq!(rows[1].3, 10);
    assert_eq!(rows[1].4, 15);

    // The two pages are told apart by their occurrences, not by their text.
    let occurrences = store
        .mentions_for_entity(&entity(ENTITY_ONE))
        .expect("backlinks");
    assert_eq!(occurrences.len(), 2);
    assert_eq!(occurrences[0].page_id.as_str(), PAGE_ONE);
    assert_eq!(occurrences[0].mention.id.as_str(), MENTION_ONE);
    assert_eq!(occurrences[1].page_id.as_str(), PAGE_TWO);
    assert_eq!(occurrences[1].mention.id.as_str(), MENTION_TWO);

    // A no-op save on the first page leaves the second page alone.
    let first = store.page_document(&page_id(PAGE_ONE)).expect("document");
    let saved = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            first.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect("noop");
    assert_eq!(saved.revision, PageRevision::new(1).expect("revision"));
    let second = store.page_document(&page_id(PAGE_TWO)).expect("document");
    assert_eq!(second.revision, PageRevision::new(1).expect("revision"));
    assert_eq!(second.body.mentions[0].id.as_str(), MENTION_TWO);

    // An import of the two pages keeps the two occurrences apart as well.
    let snapshot = store.export_snapshot().expect("snapshot");
    drop(store);
    let destination = scratch.path.with_file_name("same-body-imported.sqlite");
    let (imported, outcome) = Store::import_new(&destination, snapshot).expect("import");
    assert_eq!(outcome.inserted, 3);
    let first = imported
        .page_document(&page_id(PAGE_ONE))
        .expect("document");
    assert_eq!(first.body.mentions.len(), 1);
    assert_eq!(first.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(first.body.mentions[0].start_utf16, 0);
    assert_eq!(first.body.mentions[0].end_utf16, 5);
    let second = imported
        .page_document(&page_id(PAGE_TWO))
        .expect("document");
    assert_eq!(second.body.mentions.len(), 1);
    assert_eq!(second.body.mentions[0].id.as_str(), MENTION_TWO);
    assert_eq!(second.body.mentions[0].start_utf16, 10);
    assert_eq!(second.body.mentions[0].end_utf16, 15);
    assert_eq!(first.revision, PageRevision::ZERO);
    assert_eq!(second.revision, PageRevision::ZERO);
}

#[test]
fn a_link_that_reuses_a_stored_occurrence_id_with_another_range_is_refused() {
    let (scratch, mut store) = created("same-id-other-range.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara met Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect("link");

    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    let err = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 10, 15))],
        )
        .expect_err("conflict");
    assert_validation(
        err,
        ValidationReason::MentionIdConflict,
        "mention_id",
        Some(MENTION_ONE),
    );
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.body.body, "Elara met Elara");
    assert_eq!(document.body.mentions.len(), 1);
    assert_eq!(document.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(document.body.mentions[0].start_utf16, 0);
    assert_eq!(document.body.mentions[0].end_utf16, 5);
    assert_eq!(document.revision, PageRevision::new(1).expect("revision"));

    let rows = stored_mentions(&scratch.path);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, MENTION_ONE);
    assert_eq!(rows[0].3, 0);
    assert_eq!(rows[0].4, 5);
}

#[test]
fn a_body_edit_that_shrinks_the_text_keeps_the_link_inside_the_text() {
    let (scratch, mut store) = created("shrink.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara met Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    let linked = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 10, 15))],
        )
        .expect("link");

    // The text shrinks by ten units. An endpoint that was not moved would sit
    // at 15, outside a body of five units.
    let saved = store
        .save_page_edits(&page_id(PAGE_ONE), linked.revision, &[replace(0, 10, "")])
        .expect("shrink");
    assert_eq!(saved.body.body, "Elara");
    assert_eq!(saved.body.mentions.len(), 1);
    assert_eq!(saved.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(saved.body.mentions[0].start_utf16, 0);
    assert_eq!(saved.body.mentions[0].end_utf16, 5);
    assert_eq!(saved.revision, PageRevision::new(2).expect("revision"));
    drop(store);

    let rows = stored_mentions(&scratch.path);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, MENTION_ONE);
    assert_eq!(rows[0].3, 0);
    assert_eq!(rows[0].4, 5);
    let (title, body, revision) = stored_page(&scratch.path, PAGE_ONE);
    assert_eq!(title, "Meeting");
    assert_eq!(body, "Elara");
    assert_eq!(revision, 2);
}

#[test]
fn a_body_edit_that_covers_the_link_drops_it() {
    let (scratch, mut store) = created("covered.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara met Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    let linked = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 10, 15))],
        )
        .expect("link");

    let saved = store
        .save_page_edits(&page_id(PAGE_ONE), linked.revision, &[replace(0, 11, "")])
        .expect("cover");
    assert_eq!(saved.body.body, "lara");
    assert!(saved.body.mentions.is_empty());
    assert_eq!(saved.revision, PageRevision::new(2).expect("revision"));
    drop(store);

    let rows = stored_mentions(&scratch.path);
    assert!(rows.is_empty());
    let (_title, body, revision) = stored_page(&scratch.path, PAGE_ONE);
    assert_eq!(body, "lara");
    assert_eq!(revision, 2);
}

#[test]
fn a_link_on_an_empty_body_is_refused() {
    let (scratch, mut store) = created("empty-body.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", ""))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");

    let err = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect_err("out of bounds");
    assert_validation(
        err,
        ValidationReason::InvalidMentionRange,
        "end_utf16",
        Some(MENTION_ONE),
    );
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.body.body, "");
    assert!(document.body.mentions.is_empty());
    assert_eq!(document.revision, PageRevision::ZERO);
    assert!(stored_mentions(&scratch.path).is_empty());
}

#[test]
fn a_corrupted_mention_row_is_refused_by_both_read_paths() {
    let (scratch, mut store) = created("corrupt-mention.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    drop(store);

    // The row is inside the schema, but its end is outside the body.
    let conn = Connection::open(&scratch.path).expect("raw");
    conn.execute(
        "INSERT INTO mentions (id, page_id, entity_id, start_utf16, end_utf16)
         VALUES (?1, ?2, ?3, 0, 99)",
        [MENTION_ONE, PAGE_ONE, ENTITY_ONE],
    )
    .expect("corrupt");
    drop(conn);

    let store = Store::open(&scratch.path).expect("open");
    let err = store
        .page_document(&page_id(PAGE_ONE))
        .expect_err("invalid mention");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
    let err = store
        .mentions_for_entity(&entity(ENTITY_ONE))
        .expect_err("invalid mention");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
    // The corrupt row is still there. Nothing was dropped or repaired.
    let rows = stored_mentions(&scratch.path);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, MENTION_ONE);
    assert_eq!(rows[0].3, 0);
    assert_eq!(rows[0].4, 99);
}

#[test]
fn a_corrupted_offset_that_is_not_an_integer_is_refused() {
    let (scratch, mut store) = created("corrupt-offset.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    drop(store);

    let conn = Connection::open(&scratch.path).expect("raw");
    conn.execute_batch("PRAGMA ignore_check_constraints = ON")
        .expect("ignore checks");
    conn.execute(
        "INSERT INTO mentions (id, page_id, entity_id, start_utf16, end_utf16)
         VALUES (?1, ?2, ?3, 'x', 5)",
        [MENTION_ONE, PAGE_ONE, ENTITY_ONE],
    )
    .expect("corrupt");
    drop(conn);

    let store = Store::open(&scratch.path).expect("open");
    let err = store
        .page_document(&page_id(PAGE_ONE))
        .expect_err("invalid offset");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
    let err = store
        .mentions_for_entity(&entity(ENTITY_ONE))
        .expect_err("invalid offset");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
}

#[test]
fn a_corrupted_revision_that_is_not_an_integer_is_refused() {
    let (scratch, mut store) = created("corrupt-revision.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect("link");
    drop(store);

    let conn = Connection::open(&scratch.path).expect("raw");
    conn.execute_batch("PRAGMA ignore_check_constraints = ON")
        .expect("ignore checks");
    conn.execute(
        "UPDATE pages SET revision = 'seven' WHERE id = ?1",
        [PAGE_ONE],
    )
    .expect("corrupt");
    drop(conn);

    let mut store = Store::open(&scratch.path).expect("open");
    let err = store
        .page_document(&page_id(PAGE_ONE))
        .expect_err("invalid revision");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
    let err = store
        .mentions_for_entity(&entity(ENTITY_ONE))
        .expect_err("invalid revision");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
    // A save refuses the corrupt token instead of resetting it.
    let err = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            PageRevision::new(1).expect("revision"),
            &[PageEdit::SetTitle {
                title: "Changed".to_string(),
            }],
        )
        .expect_err("invalid revision");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
    // The corrupt token is still there. It was not reset to zero.
    let conn = Connection::open(&scratch.path).expect("raw");
    let revision: rusqlite::types::Value = conn
        .query_row(
            "SELECT revision FROM pages WHERE id = ?1",
            [PAGE_ONE],
            |row| row.get(0),
        )
        .expect("revision");
    assert_eq!(revision, rusqlite::types::Value::Text("seven".to_string()));
    let body: String = conn
        .query_row("SELECT body FROM pages WHERE id = ?1", [PAGE_ONE], |row| {
            row.get(0)
        })
        .expect("body");
    assert_eq!(body, "Elara");
}

#[test]
fn two_handles_one_body_change_one_link_only() {
    let (scratch, mut store) = created("two-handles.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara met Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    store
        .create_entity(new_entity(ENTITY_TWO, "Second"))
        .expect("entity");
    drop(store);

    let mut first = Store::open(&scratch.path).expect("open first");
    let mut second = Store::open(&scratch.path).expect("open second");
    let seen_by_first = first.page_document(&page_id(PAGE_ONE)).expect("first read");
    let seen_by_second = second
        .page_document(&page_id(PAGE_ONE))
        .expect("second read");
    assert_eq!(seen_by_first.revision, PageRevision::ZERO);
    assert_eq!(seen_by_second.revision, PageRevision::ZERO);

    let committed = second
        .save_page_edits(
            &page_id(PAGE_ONE),
            seen_by_second.revision,
            &[replace(0, 5, "Zelda")],
        )
        .expect("second save");
    assert_eq!(committed.body.body, "Zelda met Elara");
    assert_eq!(committed.revision, PageRevision::new(1).expect("revision"));

    let stale = first
        .save_page_edits(
            &page_id(PAGE_ONE),
            seen_by_first.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 10, 15))],
        )
        .expect_err("stale");
    assert_stale(stale, PAGE_ONE, 0, 1);
    drop(first);
    drop(second);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.body.body, "Zelda met Elara");
    assert!(document.body.mentions.is_empty());
    assert_eq!(document.revision, PageRevision::new(1).expect("revision"));
    assert!(stored_mentions(&scratch.path).is_empty());
}

#[test]
fn a_save_refused_while_another_connection_holds_the_lock_changes_nothing() {
    let (scratch, mut store) = created("locked.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara met Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect("link");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    drop(store);

    let holder = Connection::open(&scratch.path).expect("holder");
    holder.execute_batch("BEGIN IMMEDIATE").expect("lock");
    let mut store = Store::open(&scratch.path).expect("open");
    let err = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[
                PageEdit::SetTitle {
                    title: "Changed".to_string(),
                },
                link(mention(MENTION_TWO, ENTITY_ONE, 10, 15)),
            ],
        )
        .expect_err("busy");
    assert!(matches!(err, Error::Busy), "{err}");
    drop(holder);
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.title, "Meeting");
    assert_eq!(document.body.body, "Elara met Elara");
    assert_eq!(document.body.mentions.len(), 1);
    assert_eq!(document.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(document.revision, PageRevision::new(1).expect("revision"));
    let rows = stored_mentions(&scratch.path);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, MENTION_ONE);
    assert_eq!(rows[0].3, 0);
    assert_eq!(rows[0].4, 5);
}

#[test]
fn the_revision_advances_twice_for_a_then_b_then_a() {
    let (scratch, mut store) = created("aba-twice.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let original = store.page_document(&page_id(PAGE_ONE)).expect("document");

    let first = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            original.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect("a");
    assert_eq!(first.revision, PageRevision::new(1).expect("revision"));
    let second = store
        .save_page_edits(&page_id(PAGE_ONE), first.revision, &[unlink(MENTION_ONE)])
        .expect("b");
    assert_eq!(second.revision, PageRevision::new(2).expect("revision"));
    let third = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            second.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect("a again");
    assert_eq!(third.revision, PageRevision::new(3).expect("revision"));

    // The aggregate is the one the caller started from, the token is not.
    assert_eq!(third.body.body, "Elara");
    assert_eq!(third.body.mentions.len(), 1);
    assert_eq!(third.body.mentions[0].id.as_str(), MENTION_ONE);

    let stale = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            original.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect_err("stale");
    assert_stale(stale, PAGE_ONE, 0, 3);
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.revision, PageRevision::new(3).expect("revision"));
    assert_eq!(document.body.mentions.len(), 1);
    assert_eq!(document.body.mentions[0].id.as_str(), MENTION_ONE);
}

#[test]
fn an_unlink_of_a_foreign_occurrence_id_changes_nothing() {
    let (scratch, mut store) = created("foreign-unlink.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "First", "Elara"))
        .expect("page");
    store
        .create_page(new_page(PAGE_TWO, "Second", "Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let second = store.page_document(&page_id(PAGE_TWO)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_TWO),
            second.revision,
            &[link(mention(MENTION_TWO, ENTITY_ONE, 0, 5))],
        )
        .expect("link");

    let first = store.page_document(&page_id(PAGE_ONE)).expect("document");
    let saved = store
        .save_page_edits(&page_id(PAGE_ONE), first.revision, &[unlink(MENTION_TWO)])
        .expect("unlink");
    assert_eq!(saved.revision, PageRevision::ZERO);
    assert!(saved.body.mentions.is_empty());
    drop(store);

    // The other page still owns its occurrence.
    let rows = stored_mentions(&scratch.path);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, MENTION_TWO);
    assert_eq!(rows[0].1, PAGE_TWO);
    assert_eq!(rows[0].3, 0);
    assert_eq!(rows[0].4, 5);
    let (_title, _body, revision) = stored_page(&scratch.path, PAGE_TWO);
    assert_eq!(revision, 1);
}

#[test]
fn a_link_that_reuses_the_occurrence_id_of_another_page_is_refused_atomically() {
    let (scratch, mut store) = created("cross-page-id.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "First", "Elara"))
        .expect("page");
    store
        .create_page(new_page(PAGE_TWO, "Second", "Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let first = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            first.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect("link");

    let second = store.page_document(&page_id(PAGE_TWO)).expect("document");
    let err = store
        .save_page_edits(
            &page_id(PAGE_TWO),
            second.revision,
            &[
                PageEdit::SetTitle {
                    title: "Changed".to_string(),
                },
                link(mention(MENTION_ONE, ENTITY_ONE, 0, 5)),
            ],
        )
        .expect_err("collision");
    assert_validation(
        err,
        ValidationReason::MentionIdConflict,
        "mention_id",
        Some(MENTION_ONE),
    );
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let second = store.page_document(&page_id(PAGE_TWO)).expect("document");
    assert_eq!(second.title, "Second");
    assert_eq!(second.body.body, "Elara");
    assert!(second.body.mentions.is_empty());
    assert_eq!(second.revision, PageRevision::ZERO);
    let rows = stored_mentions(&scratch.path);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, MENTION_ONE);
    assert_eq!(rows[0].1, PAGE_ONE);
}

#[test]
fn the_read_paths_do_not_see_an_uncommitted_write() {
    let (scratch, mut store) = created("snapshot.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara met Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect("link");

    let writer = Connection::open(&scratch.path).expect("writer");
    writer.execute_batch("BEGIN IMMEDIATE").expect("begin");
    writer
        .execute(
            "UPDATE pages SET body = 'uncommitted' WHERE id = ?1",
            [PAGE_ONE],
        )
        .expect("update");
    writer
        .execute(
            "INSERT INTO mentions (id, page_id, entity_id, start_utf16, end_utf16)
             VALUES (?1, ?2, ?3, 10, 15)",
            [MENTION_TWO, PAGE_ONE, ENTITY_ONE],
        )
        .expect("insert");

    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.body.body, "Elara met Elara");
    assert_eq!(document.body.mentions.len(), 1);
    assert_eq!(document.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(document.revision, PageRevision::new(1).expect("revision"));
    let occurrences = store
        .mentions_for_entity(&entity(ENTITY_ONE))
        .expect("backlinks");
    assert_eq!(occurrences.len(), 1);
    assert_eq!(occurrences[0].mention.id.as_str(), MENTION_ONE);

    writer.execute_batch("ROLLBACK").expect("rollback");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.body.body, "Elara met Elara");
    assert_eq!(document.body.mentions.len(), 1);
    assert_eq!(document.body.mentions[0].id.as_str(), MENTION_ONE);
    let occurrences = store
        .mentions_for_entity(&entity(ENTITY_ONE))
        .expect("backlinks");
    assert_eq!(occurrences.len(), 1);
    assert_eq!(occurrences[0].mention.id.as_str(), MENTION_ONE);
}

#[test]
fn a_no_op_save_keeps_the_stored_rows_identical() {
    let (scratch, mut store) = created("noop.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara met Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[
                link(mention(MENTION_ONE, ENTITY_ONE, 0, 5)),
                link(mention(MENTION_TWO, ENTITY_ONE, 10, 15)),
            ],
        )
        .expect("link");
    let before = stored_mentions(&scratch.path);
    assert_eq!(before.len(), 2);

    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    let saved = store
        .save_page_edits(&page_id(PAGE_ONE), document.revision, &[])
        .expect("noop");
    assert_eq!(saved.revision, PageRevision::new(1).expect("revision"));
    assert_eq!(saved.body.mentions.len(), 2);
    drop(store);

    let after = stored_mentions(&scratch.path);
    assert_eq!(after.len(), 2);
    assert_eq!(after[0].0, MENTION_ONE);
    assert_eq!(after[0].1, PAGE_ONE);
    assert_eq!(after[0].2, ENTITY_ONE);
    assert_eq!(after[0].3, 0);
    assert_eq!(after[0].4, 5);
    assert_eq!(after[1].0, MENTION_TWO);
    assert_eq!(after[1].1, PAGE_ONE);
    assert_eq!(after[1].2, ENTITY_ONE);
    assert_eq!(after[1].3, 10);
    assert_eq!(after[1].4, 15);
}

#[test]
fn the_canonical_order_of_the_stored_rows_is_the_order_of_the_document() {
    let (scratch, mut store) = created("order.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara met Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");

    // The batch links the later occurrence first.
    let saved = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[
                link(mention(MENTION_TWO, ENTITY_ONE, 10, 15)),
                link(mention(MENTION_ONE, ENTITY_ONE, 0, 5)),
            ],
        )
        .expect("batch");
    assert_eq!(saved.body.mentions.len(), 2);
    assert_eq!(saved.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(saved.body.mentions[0].start_utf16, 0);
    assert_eq!(saved.body.mentions[1].id.as_str(), MENTION_TWO);
    assert_eq!(saved.body.mentions[1].start_utf16, 10);
    drop(store);

    let conn = Connection::open(&scratch.path).expect("raw");
    let mut statement = conn
        .prepare("SELECT id FROM mentions WHERE page_id = ?1 ORDER BY start_utf16, end_utf16, id")
        .expect("prepare");
    let ids: Vec<String> = statement
        .query_map([PAGE_ONE], |row| row.get::<_, String>(0))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect();
    assert_eq!(ids, vec![MENTION_ONE.to_string(), MENTION_TWO.to_string()]);
}

#[test]
fn the_revision_never_reaches_the_json() {
    let (_scratch, mut store) = created("no-revision-json.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara"))
        .expect("page");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[PageEdit::SetTitle {
                title: "Changed".to_string(),
            }],
        )
        .expect("title");

    let bytes = export_json(&mut store).expect("export");
    let text = String::from_utf8(bytes).expect("utf8");
    assert!(!text.contains("revision"), "{text}");
    assert!(text.contains("\"format_version\":1"), "{text}");
}

#[test]
fn a_token_from_the_future_is_refused_like_a_token_from_the_past() {
    let (scratch, mut store) = created("future-token.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect("link");

    // A token that was never handed out is not a match either.
    let err = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            PageRevision::new(7).expect("revision"),
            &[],
        )
        .expect_err("future token");
    assert_stale(err, PAGE_ONE, 7, 1);

    // An empty batch on a stale token is still stale.
    let err = store
        .save_page_edits(&page_id(PAGE_ONE), PageRevision::ZERO, &[])
        .expect_err("stale no-op");
    assert_stale(err, PAGE_ONE, 0, 1);
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.revision, PageRevision::new(1).expect("revision"));
    assert_eq!(document.body.mentions.len(), 1);
    assert_eq!(document.body.mentions[0].id.as_str(), MENTION_ONE);
}

#[test]
fn a_link_to_a_missing_entity_is_refused_before_any_write() {
    let (scratch, mut store) = created("missing-entity.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara"))
        .expect("page");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");

    let err = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[link(mention(MENTION_ONE, ENTITY_TWO, 0, 5))],
        )
        .expect_err("missing entity");
    assert_validation(
        err,
        ValidationReason::UnknownEntity,
        "entity_id",
        Some(ENTITY_TWO),
    );
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.body.body, "Elara");
    assert!(document.body.mentions.is_empty());
    assert_eq!(document.revision, PageRevision::ZERO);
    assert!(stored_mentions(&scratch.path).is_empty());
}

#[test]
fn a_failed_batch_keeps_the_occurrence_of_another_page() {
    let (scratch, mut store) = created("foreign-untouched.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "First", "Elara met Elara"))
        .expect("page");
    store
        .create_page(new_page(PAGE_TWO, "Second", "Elara met Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let second = store.page_document(&page_id(PAGE_TWO)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_TWO),
            second.revision,
            &[link(mention(MENTION_THREE, ENTITY_ONE, 10, 15))],
        )
        .expect("link");

    let first = store.page_document(&page_id(PAGE_ONE)).expect("document");
    let err = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            first.revision,
            &[
                link(mention(MENTION_ONE, ENTITY_ONE, 0, 5)),
                link(mention(MENTION_FOUR, ENTITY_ONE, 6, 20)),
            ],
        )
        .expect_err("out of bounds");
    assert_validation(
        err,
        ValidationReason::InvalidMentionRange,
        "end_utf16",
        Some(MENTION_FOUR),
    );
    drop(store);

    let rows = stored_mentions(&scratch.path);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, MENTION_THREE);
    assert_eq!(rows[0].1, PAGE_TWO);
    assert_eq!(rows[0].3, 10);
    assert_eq!(rows[0].4, 15);
    let (_title, body, revision) = stored_page(&scratch.path, PAGE_ONE);
    assert_eq!(body, "Elara met Elara");
    assert_eq!(revision, 0);
}
