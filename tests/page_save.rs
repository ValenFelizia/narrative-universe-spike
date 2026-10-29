#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use narrative_universe::{
    apply_mention_edit, AnnotatedBody, EntityId, Error, Mention, MentionEdit, MentionId, PageEdit,
    PageRevision, Store, UniverseId, ValidationReason,
};
use support::scratch;

const KEPT: &str = "22222222-2222-4222-8222-222222222222";
const OTHER: &str = "44444444-4444-4444-8444-444444444444";
const ABSENT: &str = "99999999-9999-4999-8999-999999999999";
const ENTITY_ONE: &str = "11111111-1111-4111-8111-111111111111";
const MENTION_ONE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

fn universe() -> UniverseId {
    UniverseId::parse("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").expect("universe")
}

fn created(name: &str) -> (support::Scratch, Store) {
    let scratch = scratch(name);
    let store = Store::create(&scratch.path, universe()).expect("create");
    (scratch, store)
}

fn page_id(raw: &str) -> narrative_universe::PageId {
    narrative_universe::PageId::parse(raw).expect("page")
}

fn mention(raw: &str, entity_raw: &str, start_utf16: u32, end_utf16: u32) -> Mention {
    Mention {
        id: MentionId::parse(raw).expect("mention id"),
        entity_id: EntityId::parse(entity_raw).expect("entity"),
        start_utf16,
        end_utf16,
    }
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

#[test]
fn stale_title_and_body_roll_back_and_unicode_reopens_exactly() {
    let (scratch, mut store) = created("unicode-stale.sqlite");
    let id = page_id(KEPT);
    store
        .create_page(narrative_universe::Page {
            id: id.clone(),
            title: "before".into(),
            body: "before".into(),
        })
        .unwrap();
    let title = "Árbol 🐉 e\u{301}";
    let body = "漢字\n🦊 e\u{301} fin";
    store
        .save_page_edits(
            &id,
            PageRevision::ZERO,
            &[
                PageEdit::SetTitle {
                    title: title.into(),
                },
                replace(0, 6, body),
            ],
        )
        .unwrap();
    let err = store
        .save_page_edits(
            &id,
            PageRevision::ZERO,
            &[
                PageEdit::SetTitle {
                    title: "lost".into(),
                },
                replace(0, 6, "lost"),
            ],
        )
        .unwrap_err();
    assert_stale(err, KEPT, 0, 1);
    drop(store);
    let store = Store::open(&scratch.path).unwrap();
    let page = store.page_document(&id).unwrap();
    assert_eq!(page.title, title);
    assert_eq!(page.body.body, body);
    assert_eq!(page.revision.as_i64(), 1);
}

#[test]
fn pa_t01_save_persists_and_reopens() {
    let (scratch, mut store) = created("pa-t01.sqlite");
    let id = page_id(KEPT);
    store
        .create_page(narrative_universe::Page {
            id: id.clone(),
            title: "Original".to_string(),
            body: "original body".to_string(),
        })
        .expect("page");
    let document = store.page_document(&id).expect("document");
    assert_eq!(document.revision, PageRevision::ZERO);
    assert_eq!(document.title, "Original");
    assert_eq!(document.body.body, "original body");

    let saved = store
        .save_page_edits(
            &id,
            document.revision,
            &[
                PageEdit::SetTitle {
                    title: "Saved title".to_string(),
                },
                replace(0, 13, "Saved body"),
            ],
        )
        .expect("save");
    assert_eq!(saved.title, "Saved title");
    assert_eq!(saved.body.body, "Saved body");
    assert_eq!(saved.revision, PageRevision::new(1).expect("revision"));
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let reloaded = store.page_document(&id).expect("document");
    assert_eq!(reloaded.title, "Saved title");
    assert_eq!(reloaded.body.body, "Saved body");
    assert_eq!(reloaded.revision, PageRevision::new(1).expect("revision"));
    assert_eq!(reloaded.id, id);
}

#[test]
fn pa_t02_stale_save_keeps_the_committed_page() {
    let (scratch, mut store) = created("pa-t02.sqlite");
    let id = page_id(KEPT);
    store
        .create_page(narrative_universe::Page {
            id: id.clone(),
            title: "Original".to_string(),
            body: "original body".to_string(),
        })
        .expect("page");
    drop(store);

    let mut a = Store::open(&scratch.path).expect("open a");
    let mut b = Store::open(&scratch.path).expect("open b");
    let seen_by_a = a.page_document(&id).expect("a read");
    let seen_by_b = b.page_document(&id).expect("b read");
    assert_eq!(seen_by_a.revision, seen_by_b.revision);

    let committed = b
        .save_page_edits(&id, seen_by_b.revision, &[replace(0, 13, "committed by b")])
        .expect("b save");
    assert_eq!(committed.body.body, "committed by b");

    let stale = a
        .save_page_edits(&id, seen_by_a.revision, &[replace(0, 13, "lost update")])
        .expect_err("stale");
    assert_stale(stale, KEPT, 0, 1);
    drop(a);
    drop(b);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&id).expect("document");
    assert_eq!(document.body.body, "committed by b");
    assert_eq!(document.revision, PageRevision::new(1).expect("revision"));
}

#[test]
fn pa_t03_identical_save_is_a_noop() {
    let (scratch, mut store) = created("pa-t03.sqlite");
    let id = page_id(KEPT);
    store
        .create_page(narrative_universe::Page {
            id: id.clone(),
            title: "Same".to_string(),
            body: "same body".to_string(),
        })
        .expect("page");
    let document = store.page_document(&id).expect("document");

    let first = store
        .save_page_edits(&id, document.revision, &[])
        .expect("first noop");
    assert_eq!(first.revision, document.revision);
    assert_eq!(first.body.body, "same body");

    let second = store
        .save_page_edits(&id, first.revision, &[])
        .expect("second noop");
    assert_eq!(second.revision, document.revision);
    assert_eq!(second.body.body, "same body");
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&id).expect("document");
    assert_eq!(document.revision, PageRevision::ZERO);
    assert_eq!(document.body.body, "same body");
}

#[test]
fn pa_t04_absent_page_never_creates_a_record() {
    let (scratch, mut store) = created("pa-t04.sqlite");
    let absent = page_id(ABSENT);
    let err = store
        .save_page_edits(&absent, PageRevision::ZERO, &[replace(0, 0, "x")])
        .expect_err("missing");
    assert_validation(err, ValidationReason::NotFound, "id", Some(ABSENT));
    assert_eq!(store.counts().expect("counts").pages, 0);
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.counts().expect("counts").pages, 0);
    assert_validation(
        store.page(&absent).expect_err("absent"),
        ValidationReason::NotFound,
        "id",
        Some(ABSENT),
    );
}

#[test]
fn a_write_only_touches_the_named_page() {
    let (scratch, mut store) = created("pa-t04-other.sqlite");
    let kept = page_id(KEPT);
    let other = page_id(OTHER);
    store
        .create_page(narrative_universe::Page {
            id: kept.clone(),
            title: "Kept".to_string(),
            body: "kept body".to_string(),
        })
        .expect("page");
    store
        .create_page(narrative_universe::Page {
            id: other.clone(),
            title: "Other".to_string(),
            body: "other body".to_string(),
        })
        .expect("other");
    let document = store.page_document(&kept).expect("document");
    store
        .save_page_edits(&kept, document.revision, &[replace(0, 4, "moved")])
        .expect("save");
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.page(&kept).expect("kept").body, "moved body");
    let untouched = store.page(&other).expect("other");
    assert_eq!(untouched.title, "Other");
    assert_eq!(untouched.body, "other body");
    assert_eq!(
        store.page_document(&other).expect("document").revision,
        PageRevision::ZERO
    );
}

#[test]
fn a_mention_id_from_another_page_cannot_be_reused() {
    let (scratch, mut store) = created("pa-t04-mention.sqlite");
    let kept = page_id(KEPT);
    let other = page_id(OTHER);
    store
        .create_page(narrative_universe::Page {
            id: kept.clone(),
            title: "Kept".to_string(),
            body: "Elara".to_string(),
        })
        .expect("page");
    store
        .create_page(narrative_universe::Page {
            id: other.clone(),
            title: "Other".to_string(),
            body: "Elara".to_string(),
        })
        .expect("other");
    store
        .create_entity(narrative_universe::Entity {
            id: EntityId::parse(ENTITY_ONE).expect("entity"),
            label: "Entity".to_string(),
            type_id: None,
            page_id: None,
        })
        .expect("entity");

    let document = store.page_document(&kept).expect("document");
    store
        .save_page_edits(
            &kept,
            document.revision,
            &[PageEdit::Mention(MentionEdit::Link {
                mention: mention(MENTION_ONE, ENTITY_ONE, 0, 5),
            })],
        )
        .expect("link");

    let other_document = store.page_document(&other).expect("document");
    let err = store
        .save_page_edits(
            &other,
            other_document.revision,
            &[PageEdit::Mention(MentionEdit::Link {
                mention: mention(MENTION_ONE, ENTITY_ONE, 0, 5),
            })],
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
    let kept_document = store.page_document(&kept).expect("kept");
    assert_eq!(kept_document.body.mentions.len(), 1);
    assert_eq!(kept_document.body.mentions[0].id.as_str(), MENTION_ONE);
    let other_document = store.page_document(&other).expect("other");
    assert!(other_document.body.mentions.is_empty());
    assert_eq!(other_document.revision, PageRevision::ZERO);
}

#[test]
fn pa_t05_edits_roundtrip_without_touching_other_page() {
    let (scratch, mut store) = created("pa-t05.sqlite");
    let kept = page_id(KEPT);
    let other = page_id(OTHER);
    store
        .create_page(narrative_universe::Page {
            id: kept.clone(),
            title: "Initial".to_string(),
            body: "Stable body".to_string(),
        })
        .expect("page");
    store
        .create_page(narrative_universe::Page {
            id: other.clone(),
            title: "Other".to_string(),
            body: "Keep".to_string(),
        })
        .expect("other");
    drop(store);

    let mut store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&kept).expect("document");
    let title_only = store
        .save_page_edits(
            &kept,
            document.revision,
            &[PageEdit::SetTitle {
                title: "Title only".to_string(),
            }],
        )
        .expect("title");
    assert_eq!(title_only.title, "Title only");
    assert_eq!(title_only.body.body, "Stable body");

    let body_only = store
        .save_page_edits(&kept, title_only.revision, &[replace(0, 11, "Body only")])
        .expect("body");
    assert_eq!(body_only.title, "Title only");
    assert_eq!(body_only.body.body, "Body only");

    let empty = store
        .save_page_edits(&kept, body_only.revision, &[replace(0, 9, "")])
        .expect("empty");
    assert_eq!(empty.body.body, "");

    let unicode = "Texto con acento áéíóú ñ y emoji 📚\nsegunda línea";
    let unicode = store
        .save_page_edits(&kept, empty.revision, &[replace(0, 0, unicode)])
        .expect("unicode");
    assert_eq!(unicode.body.body, unicode_text());
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&kept).expect("document");
    assert_eq!(document.body.body, unicode_text());
    let untouched = store.page(&other).expect("other");
    assert_eq!(untouched.title, "Other");
    assert_eq!(untouched.body, "Keep");
}

fn unicode_text() -> String {
    "Texto con acento áéíóú ñ y emoji 📚\nsegunda línea".to_string()
}

#[test]
fn pa_t06_stale_revision_rejects_the_current_value() {
    let (scratch, mut store) = created("pa-t06.sqlite");
    let id = page_id(KEPT);
    store
        .create_page(narrative_universe::Page {
            id: id.clone(),
            title: "Before".to_string(),
            body: "before".to_string(),
        })
        .expect("page");
    let original = store.page_document(&id).expect("document");
    let current = store
        .save_page_edits(&id, original.revision, &[replace(0, 6, "after")])
        .expect("update");
    assert_eq!(current.body.body, "after");

    // The desired value is the stored one, but the expectation is old.
    let stale = store
        .save_page_edits(&id, original.revision, &[replace(0, 6, "after")])
        .expect_err("stale");
    assert_stale(stale, KEPT, 0, 1);
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let document = store.page_document(&id).expect("document");
    assert_eq!(document.body.body, "after");
    assert_eq!(document.revision, PageRevision::new(1).expect("revision"));
}

#[test]
fn the_pure_reducer_still_drives_the_mentions() {
    let body = AnnotatedBody {
        body: "Elara left".to_string(),
        mentions: vec![mention(MENTION_ONE, ENTITY_ONE, 0, 5)],
    };
    let (edited, removed) = apply_mention_edit(
        &body,
        &MentionEdit::ReplaceText {
            start_utf16: 2,
            end_utf16: 2,
            inserted_text: "x".to_string(),
        },
    )
    .expect("edit");
    assert_eq!(edited.body, "Elxara left");
    assert!(edited.mentions.is_empty());
    assert_eq!(
        removed,
        vec![MentionId::parse(MENTION_ONE).expect("mention id")]
    );
}
