//! Adversarial tests for the version 2 page aggregate.
//!
//! Every test runs against a real SQLite file and checks literal values.
//! Nothing here compares a result with the method that produced it.

#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use narrative_universe::{
    export_json, AnnotatedBody, Entity, EntityId, Error, Mention, MentionEdit, MentionId, Page,
    PageEdit, PageId, PageRevision, Store, UniverseId, ValidationReason,
};
use rusqlite::Connection;
use support::{bytes, scratch};

const UNIVERSE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const PAGE_ONE: &str = "22222222-2222-4222-8222-222222222221";
const PAGE_TWO: &str = "22222222-2222-4222-8222-222222222222";
const ENTITY_ONE: &str = "11111111-1111-4111-8111-111111111111";
const ENTITY_TWO: &str = "33333333-3333-4333-8333-333333333333";
const MENTION_ONE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const MENTION_TWO: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const MENTION_THREE: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const MENTION_FOUR: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const V1_SCHEMA: &str = include_str!("fixtures/v1_universe.sql");

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

#[test]
fn two_occurrences_with_the_same_words_keep_their_own_identity() {
    let (scratch, mut store) = created("occurrences.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara met Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    store
        .create_entity(new_entity(ENTITY_TWO, "Second"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");

    let saved = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[
                link(mention(MENTION_ONE, ENTITY_ONE, 0, 5)),
                link(mention(MENTION_TWO, ENTITY_TWO, 10, 15)),
            ],
        )
        .expect("link");
    assert_eq!(saved.body.body, "Elara met Elara");
    assert_eq!(saved.body.mentions.len(), 2);
    assert_eq!(saved.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(saved.body.mentions[0].entity_id.as_str(), ENTITY_ONE);
    assert_eq!(saved.body.mentions[0].start_utf16, 0);
    assert_eq!(saved.body.mentions[0].end_utf16, 5);
    assert_eq!(saved.body.mentions[1].id.as_str(), MENTION_TWO);
    assert_eq!(saved.body.mentions[1].entity_id.as_str(), ENTITY_TWO);
    assert_eq!(saved.body.mentions[1].start_utf16, 10);
    assert_eq!(saved.body.mentions[1].end_utf16, 15);
    assert_eq!(saved.revision, PageRevision::new(1).expect("revision"));
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.body.body, "Elara met Elara");
    assert_eq!(document.body.mentions.len(), 2);
    assert_eq!(document.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(document.body.mentions[1].id.as_str(), MENTION_TWO);
    assert_eq!(document.revision, PageRevision::new(1).expect("revision"));
}

#[test]
fn an_unlink_changes_the_mentions_and_the_revision_but_not_a_character() {
    let (scratch, mut store) = created("unlink.sqlite");
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
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect("link");

    let unlinked = store
        .save_page_edits(&page_id(PAGE_ONE), linked.revision, &[unlink(MENTION_ONE)])
        .expect("unlink");
    assert_eq!(unlinked.body.body, "Elara met Elara");
    assert!(unlinked.body.mentions.is_empty());
    assert_eq!(unlinked.revision, PageRevision::new(2).expect("revision"));
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.body.body, "Elara met Elara");
    assert!(document.body.mentions.is_empty());
    assert_eq!(document.revision, PageRevision::new(2).expect("revision"));
}

#[test]
fn a_stale_link_only_save_is_refused() {
    let (scratch, mut store) = created("stale-link.sqlite");
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

    let mut a = Store::open(&scratch.path).expect("open a");
    let mut b = Store::open(&scratch.path).expect("open b");
    let seen_by_a = a.page_document(&page_id(PAGE_ONE)).expect("a read");
    let seen_by_b = b.page_document(&page_id(PAGE_ONE)).expect("b read");
    assert_eq!(seen_by_a.revision, seen_by_b.revision);
    assert_eq!(seen_by_a.body.body, seen_by_b.body.body);

    let committed = b
        .save_page_edits(
            &page_id(PAGE_ONE),
            seen_by_b.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect("b link");
    assert_eq!(committed.revision, PageRevision::new(1).expect("revision"));

    let stale = a
        .save_page_edits(
            &page_id(PAGE_ONE),
            seen_by_a.revision,
            &[link(mention(MENTION_TWO, ENTITY_TWO, 10, 15))],
        )
        .expect_err("stale");
    assert_stale(stale, PAGE_ONE, 0, 1);
    drop(a);
    drop(b);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.body.mentions.len(), 1);
    assert_eq!(document.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(document.revision, PageRevision::new(1).expect("revision"));
}

#[test]
fn a_link_and_an_unlink_in_two_saves_invalidate_the_first_token() {
    let (scratch, mut store) = created("aba.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let original = store.page_document(&page_id(PAGE_ONE)).expect("document");

    let linked = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            original.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect("link");
    let unlinked = store
        .save_page_edits(&page_id(PAGE_ONE), linked.revision, &[unlink(MENTION_ONE)])
        .expect("unlink");

    // The aggregate is the one the caller started from, and the token is not.
    assert_eq!(unlinked.body.body, original.body.body);
    assert!(unlinked.body.mentions.is_empty());
    assert_eq!(unlinked.revision, PageRevision::new(2).expect("revision"));

    let stale = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            original.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect_err("stale");
    assert_stale(stale, PAGE_ONE, 0, 2);
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert!(document.body.mentions.is_empty());
    assert_eq!(document.revision, PageRevision::new(2).expect("revision"));
}

#[test]
fn a_link_to_a_missing_entity_is_refused() {
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
    assert_validation(err, ValidationReason::UnknownEntity, "entity_id");
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert!(document.body.mentions.is_empty());
    assert_eq!(document.body.body, "Elara");
    assert_eq!(document.revision, PageRevision::ZERO);
}

#[test]
fn a_failure_late_in_a_batch_changes_nothing() {
    let (scratch, mut store) = created("late-failure.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara met Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");

    let err = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[
                link(mention(MENTION_ONE, ENTITY_ONE, 0, 5)),
                replace(0, 5, "Z"),
                link(mention(MENTION_TWO, ENTITY_ONE, 10, 15)),
            ],
        )
        .expect_err("stale range");
    assert_validation(err, ValidationReason::InvalidMentionRange, "end_utf16");
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.body.body, "Elara met Elara");
    assert!(document.body.mentions.is_empty());
    assert_eq!(document.revision, PageRevision::ZERO);
}

#[test]
fn a_sql_failure_after_the_page_update_rolls_the_whole_aggregate_back() {
    let (scratch, mut store) = created("trigger-failure.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara met Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    drop(store);

    let conn = Connection::open(&scratch.path).expect("raw");
    conn.execute_batch(&format!(
        "CREATE TRIGGER fail_second_mention BEFORE INSERT ON mentions
         WHEN NEW.id = '{MENTION_TWO}'
         BEGIN
             SELECT RAISE(ABORT, 'injected mention failure');
         END;"
    ))
    .expect("trigger");
    drop(conn);

    let mut store = Store::open(&scratch.path).expect("open");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    let err = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[
                PageEdit::SetTitle {
                    title: "Changed".to_string(),
                },
                link(mention(MENTION_ONE, ENTITY_ONE, 0, 5)),
                link(mention(MENTION_TWO, ENTITY_ONE, 10, 15)),
            ],
        )
        .expect_err("injected failure");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.title, "Meeting");
    assert_eq!(document.body.body, "Elara met Elara");
    assert!(document.body.mentions.is_empty());
    assert_eq!(document.revision, PageRevision::ZERO);
}

#[test]
fn backlinks_report_occurrences_and_not_relations() {
    let (_scratch, mut store) = created("backlinks.sqlite");
    store
        .create_page(new_page(PAGE_TWO, "Second", "Elara"))
        .expect("page");
    store
        .create_page(new_page(PAGE_ONE, "First", "Elara and Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    store
        .create_entity(new_entity(ENTITY_TWO, "Second"))
        .expect("entity");

    let second = store.page_document(&page_id(PAGE_TWO)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_TWO),
            second.revision,
            &[link(mention(MENTION_THREE, ENTITY_ONE, 0, 5))],
        )
        .expect("link");
    let first = store.page_document(&page_id(PAGE_ONE)).expect("document");
    store
        .save_page_edits(
            &page_id(PAGE_ONE),
            first.revision,
            &[
                link(mention(MENTION_ONE, ENTITY_ONE, 0, 5)),
                link(mention(MENTION_TWO, ENTITY_ONE, 10, 15)),
            ],
        )
        .expect("link");

    let occurrences = store
        .mentions_for_entity(&entity(ENTITY_ONE))
        .expect("backlinks");
    assert_eq!(occurrences.len(), 3);
    assert_eq!(occurrences[0].page_id.as_str(), PAGE_ONE);
    assert_eq!(occurrences[0].mention.id.as_str(), MENTION_ONE);
    assert_eq!(occurrences[0].mention.start_utf16, 0);
    assert_eq!(occurrences[0].mention.end_utf16, 5);
    assert_eq!(
        occurrences[0].page_revision,
        PageRevision::new(1).expect("revision")
    );
    assert_eq!(occurrences[1].page_id.as_str(), PAGE_ONE);
    assert_eq!(occurrences[1].mention.id.as_str(), MENTION_TWO);
    assert_eq!(occurrences[1].mention.start_utf16, 10);
    assert_eq!(occurrences[2].page_id.as_str(), PAGE_TWO);
    assert_eq!(occurrences[2].mention.id.as_str(), MENTION_THREE);
    assert_eq!(
        occurrences[2].page_revision,
        PageRevision::new(1).expect("revision")
    );

    let without_links = store
        .mentions_for_entity(&entity(ENTITY_TWO))
        .expect("backlinks");
    assert!(without_links.is_empty());

    let err = store
        .mentions_for_entity(&entity("99999999-9999-4999-8999-999999999999"))
        .expect_err("absent");
    assert_validation(err, ValidationReason::NotFound, "id");
}

#[test]
fn the_prose_keeps_its_exact_bytes_after_a_link() {
    let (scratch, mut store) = created("exact-bytes.sqlite");
    let body = "  \"Elara\" \t– naïve\r\n第二行 👩‍👧 ";
    store
        .create_page(new_page(PAGE_ONE, "Title", body))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    let saved = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 2, 7))],
        )
        .expect("link");
    assert_eq!(saved.body.body, body);
    assert_eq!(saved.body.body.len(), body.len());
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.body.body, body);
}

#[test]
fn a_spent_revision_refuses_a_change_and_keeps_an_unchanged_aggregate() {
    let (scratch, mut store) = created("revision-exhausted.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara"))
        .expect("page");
    drop(store);

    let conn = Connection::open(&scratch.path).expect("raw");
    conn.execute(
        "UPDATE pages SET revision = 9223372036854775807 WHERE id = ?1",
        [PAGE_ONE],
    )
    .expect("revision");
    drop(conn);

    let mut store = Store::open(&scratch.path).expect("open");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(
        document.revision,
        PageRevision::new(i64::MAX).expect("revision")
    );

    let unchanged = store
        .save_page_edits(&page_id(PAGE_ONE), document.revision, &[])
        .expect("unchanged");
    assert_eq!(unchanged.revision, document.revision);

    let err = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[PageEdit::SetTitle {
                title: "Changed".to_string(),
            }],
        )
        .expect_err("exhausted");
    assert_validation(err, ValidationReason::RevisionExhausted, "revision");
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.title, "Meeting");
    assert_eq!(
        document.revision,
        PageRevision::new(i64::MAX).expect("revision")
    );
}

#[test]
fn a_negative_stored_revision_is_refused_instead_of_read() {
    let (scratch, mut store) = created("bad-revision.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara"))
        .expect("page");
    drop(store);

    let conn = Connection::open(&scratch.path).expect("raw");
    conn.execute_batch("PRAGMA ignore_check_constraints = ON")
        .expect("ignore checks");
    conn.execute("UPDATE pages SET revision = -5 WHERE id = ?1", [PAGE_ONE])
        .expect("corrupt");
    drop(conn);

    let store = Store::open(&scratch.path).expect("open");
    let err = store
        .page_document(&page_id(PAGE_ONE))
        .expect_err("invalid revision");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
}

#[test]
fn a_stored_mention_outside_the_body_is_refused_instead_of_read() {
    let (scratch, mut store) = created("bad-offset.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    drop(store);

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
    let err = export_json(&mut { store }).expect_err("invalid mention");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
}

#[test]
fn a_stored_mention_that_splits_a_cluster_is_refused_instead_of_read() {
    let (scratch, mut store) = created("bad-grapheme.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "e\u{0301}"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    drop(store);

    let conn = Connection::open(&scratch.path).expect("raw");
    conn.execute(
        "INSERT INTO mentions (id, page_id, entity_id, start_utf16, end_utf16)
         VALUES (?1, ?2, ?3, 0, 1)",
        [MENTION_ONE, PAGE_ONE, ENTITY_ONE],
    )
    .expect("corrupt");
    drop(conn);

    let store = Store::open(&scratch.path).expect("open");
    let err = store
        .page_document(&page_id(PAGE_ONE))
        .expect_err("invalid mention");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
}

#[test]
fn a_version_1_database_needs_a_migration_and_is_not_touched() {
    let scratch = scratch("v1.sqlite");
    let conn = Connection::open(&scratch.path).expect("raw");
    conn.execute_batch(V1_SCHEMA).expect("v1 schema");
    conn.execute(
        "INSERT INTO metadata (singleton, universe_id, format_version) VALUES (1, ?1, 1)",
        [UNIVERSE],
    )
    .expect("metadata");
    conn.execute_batch("PRAGMA user_version = 1")
        .expect("version");
    drop(conn);
    let before = bytes(&scratch.path);

    let err = Store::open(&scratch.path).expect_err("migration");
    match err {
        Error::MigrationRequired { found } => assert_eq!(found, 1),
        other => panic!("expected migration required, got {other}"),
    }
    assert_eq!(bytes(&scratch.path), before);
}

#[test]
fn a_new_database_is_always_the_current_version() {
    let (scratch, store) = created("new-version.sqlite");
    drop(store);
    let raw = bytes(&scratch.path);
    let version = i32::from_be_bytes(raw[60..64].try_into().expect("header"));
    assert_eq!(version, 2);
}

#[test]
fn the_same_text_replacement_removes_the_link() {
    let (_scratch, mut store) = created("same-text.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara left"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    let linked = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect("link");

    let saved = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            linked.revision,
            &[replace(0, 5, "Elara")],
        )
        .expect("same text");
    assert_eq!(saved.body.body, "Elara left");
    assert!(saved.body.mentions.is_empty());
    assert_eq!(saved.revision, PageRevision::new(2).expect("revision"));
}

#[test]
fn a_crlf_body_keeps_its_line_endings_across_a_link() {
    let (_scratch, mut store) = created("crlf.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "A\r\nElara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    let saved = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[link(mention(MENTION_ONE, ENTITY_ONE, 3, 8))],
        )
        .expect("link");
    assert_eq!(saved.body.body, "A\r\nElara");
    assert_eq!(saved.body.mentions.len(), 1);
}

#[test]
fn an_edited_body_keeps_the_links_that_still_cover_whole_characters() {
    let (_scratch, mut store) = created("kept-links.sqlite");
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
            &[
                link(mention(MENTION_ONE, ENTITY_ONE, 0, 5)),
                link(mention(MENTION_TWO, ENTITY_ONE, 10, 15)),
            ],
        )
        .expect("link");

    let saved = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            linked.revision,
            &[replace(0, 0, "Dear ")],
        )
        .expect("insert");
    assert_eq!(saved.body.body, "Dear Elara met Elara");
    assert_eq!(saved.body.mentions.len(), 2);
    assert_eq!(saved.body.mentions[0].start_utf16, 5);
    assert_eq!(saved.body.mentions[0].end_utf16, 10);
    assert_eq!(saved.body.mentions[1].start_utf16, 15);
    assert_eq!(saved.body.mentions[1].end_utf16, 20);
}

#[test]
fn a_mention_id_used_on_another_page_is_refused_atomically() {
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
            &[link(mention(MENTION_ONE, ENTITY_ONE, 0, 5))],
        )
        .expect_err("collision");
    assert_validation(err, ValidationReason::MentionIdConflict, "mention_id");
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let first = store.page_document(&page_id(PAGE_ONE)).expect("first");
    assert_eq!(first.body.mentions.len(), 1);
    assert_eq!(first.body.mentions[0].id.as_str(), MENTION_ONE);
    let second = store.page_document(&page_id(PAGE_TWO)).expect("second");
    assert!(second.body.mentions.is_empty());
    assert_eq!(second.revision, PageRevision::ZERO);
    assert_eq!(second.body.body, "Elara");
}

#[test]
fn an_unknown_unlink_keeps_the_aggregate_and_the_token() {
    let (_scratch, mut store) = created("unknown-unlink.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    let saved = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[unlink(MENTION_FOUR)],
        )
        .expect("unlink");
    assert_eq!(saved.body.body, "Elara");
    assert!(saved.body.mentions.is_empty());
    assert_eq!(saved.revision, PageRevision::ZERO);
}

#[test]
fn a_busy_database_does_not_report_success() {
    let (scratch, mut store) = created("busy.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    drop(store);

    let holder = Connection::open(&scratch.path).expect("holder");
    holder.execute_batch("BEGIN IMMEDIATE").expect("lock");
    let mut store = Store::open(&scratch.path).expect("open");
    let err = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[PageEdit::SetTitle {
                title: "Changed".to_string(),
            }],
        )
        .expect_err("busy");
    assert!(matches!(err, Error::Busy), "{err}");
    drop(holder);
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.title, "Meeting");
    assert_eq!(document.revision, PageRevision::ZERO);
}

#[test]
fn a_link_to_a_missing_entity_is_refused_even_when_a_later_unlink_removes_it() {
    let (scratch, mut store) = created("unlinked-missing.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara"))
        .expect("page");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");

    let err = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[
                link(mention(MENTION_ONE, ENTITY_TWO, 0, 5)),
                unlink(MENTION_ONE),
            ],
        )
        .expect_err("missing entity");
    assert_validation(err, ValidationReason::UnknownEntity, "entity_id");
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert!(document.body.mentions.is_empty());
    assert_eq!(document.revision, PageRevision::ZERO);
}

#[test]
fn a_batch_that_ends_where_it_started_keeps_the_revision() {
    let (scratch, mut store) = created("same-aggregate.sqlite");
    store
        .create_page(new_page(PAGE_ONE, "Meeting", "Elara"))
        .expect("page");
    store
        .create_entity(new_entity(ENTITY_ONE, "First"))
        .expect("entity");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");

    let saved = store
        .save_page_edits(
            &page_id(PAGE_ONE),
            document.revision,
            &[
                link(mention(MENTION_ONE, ENTITY_ONE, 0, 5)),
                unlink(MENTION_ONE),
            ],
        )
        .expect("batch");
    assert_eq!(saved.body.body, "Elara");
    assert!(saved.body.mentions.is_empty());
    assert_eq!(saved.revision, PageRevision::ZERO);
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.revision, PageRevision::ZERO);
}

#[test]
fn the_export_refuses_a_page_with_mentions() {
    let (scratch, mut store) = created("export-guard.sqlite");
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

    // The JSON format 2 exchange does not exist yet. The export must not
    // produce a document that drops the occurrence.
    let err = export_json(&mut store).expect_err("mentions");
    assert!(matches!(err, Error::MentionsNotExported), "{err}");
}

#[test]
fn an_import_restores_the_occurrences_and_starts_the_revision_at_zero() {
    let (scratch, mut store) = created("import-source.sqlite");
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
    let snapshot = store.export_snapshot().expect("snapshot");
    drop(store);

    let destination = scratch.path.with_file_name("imported.sqlite");
    let (imported, outcome) = Store::import_new(&destination, snapshot).expect("import");
    // The page and the entity are counted. The occurrence belongs to its
    // page and is not counted on its own.
    assert_eq!(outcome.inserted, 2);
    let document = imported
        .page_document(&page_id(PAGE_ONE))
        .expect("document");
    assert_eq!(document.body.body, "Elara met Elara");
    assert_eq!(document.body.mentions.len(), 1);
    assert_eq!(document.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(document.body.mentions[0].entity_id.as_str(), ENTITY_ONE);
    assert_eq!(document.body.mentions[0].start_utf16, 0);
    assert_eq!(document.body.mentions[0].end_utf16, 5);
    assert_eq!(document.revision, PageRevision::ZERO);
    drop(imported);

    let reopened = Store::open(&destination).expect("reopen");
    let document = reopened
        .page_document(&page_id(PAGE_ONE))
        .expect("document");
    assert_eq!(document.body.mentions.len(), 1);
    assert_eq!(document.revision, PageRevision::ZERO);
}

#[test]
fn an_import_without_occurrences_cannot_erase_the_links() {
    let (scratch, mut store) = created("import-conflict.sqlite");
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

    // A document without occurrences would erase the link if it were applied.
    let snapshot = narrative_universe::Snapshot {
        universe_id: universe(),
        pages: vec![narrative_universe::PageSnapshot {
            page: new_page(PAGE_ONE, "Meeting", "Elara"),
            mentions: Vec::new(),
        }],
        types: Vec::new(),
        entities: Vec::new(),
        definitions: Vec::new(),
        assertions: Vec::new(),
    };
    let err = store.import_snapshot(snapshot).expect_err("conflict");
    match err {
        Error::ImportConflict { class, id } => {
            assert_eq!(class, "page");
            assert_eq!(id, PAGE_ONE);
        }
        other => panic!("expected import conflict, got {other}"),
    }
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(document.body.mentions.len(), 1);
    assert_eq!(document.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(document.revision, PageRevision::new(1).expect("revision"));
}

#[test]
fn the_document_of_a_page_never_holds_a_mention_of_another_page() {
    let (_scratch, mut store) = created("no-foreign-mentions.sqlite");
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
            &[link(mention(MENTION_THREE, ENTITY_ONE, 0, 5))],
        )
        .expect("link");

    let first = store.page_document(&page_id(PAGE_ONE)).expect("document");
    assert!(first.body.mentions.is_empty());
    let annotated = AnnotatedBody {
        body: first.body.body.clone(),
        mentions: first.body.mentions.clone(),
    };
    assert!(narrative_universe::validate_mentions(&annotated).is_ok());
}
