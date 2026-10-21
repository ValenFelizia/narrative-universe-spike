#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use std::path::Path;

use narrative_universe::{Counts, Error, Page, PageId, Store, UniverseId, ValidationReason};
use support::scratch;

const KEPT: &str = "22222222-2222-4222-8222-222222222222";
const OTHER: &str = "44444444-4444-4444-8444-444444444444";
const ABSENT: &str = "99999999-9999-4999-8999-999999999999";

fn universe() -> UniverseId {
    UniverseId::parse("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").expect("universe")
}

fn created(name: &str) -> (support::Scratch, Store) {
    let scratch = scratch(name);
    let store = Store::create(&scratch.path, universe()).expect("create");
    (scratch, store)
}

fn make_page(id: &str, title: &str, body: &str) -> Page {
    Page {
        id: PageId::parse(id).expect("page id"),
        title: title.to_string(),
        body: body.to_string(),
    }
}

fn persisted(path: &Path, id: &PageId) -> Page {
    let store = Store::open(path).expect("reopen");
    store.page(id).expect("page")
}

fn assert_validation(err: Error, reason: ValidationReason, field: &str, id: &str) {
    match err {
        Error::Validation {
            reason: got_reason,
            field: got_field,
            id: got_id,
        } => {
            assert_eq!(got_reason, reason);
            assert_eq!(got_field, field);
            assert_eq!(got_id.as_deref(), Some(id));
        }
        other => panic!("expected validation {reason:?}, got {other}"),
    }
}

fn assert_stale(err: Error, id: &str) {
    match err {
        Error::StalePage { id: got } => assert_eq!(got, id),
        other => panic!("expected stale page {id}, got {other}"),
    }
}

fn assert_stale_is_distinct_from_missing(stale: &Error, missing: &Error) {
    match (stale, missing) {
        (
            Error::StalePage { .. },
            Error::Validation {
                reason: ValidationReason::NotFound,
                ..
            },
        ) => {}
        _ => panic!("stale and missing-page failures must differ, got {stale} and {missing}"),
    }
}

fn one_page() -> Counts {
    Counts {
        pages: 1,
        types: 0,
        entities: 0,
        definitions: 0,
        assertions: 0,
    }
}

fn round_trip(path: &Path, expected: &Page, replacement: Page, unrelated: &Page) -> Page {
    let mut store = Store::open(path).expect("open");
    let saved = store.save_page(expected, replacement).expect("save");
    let id = saved.id.clone();
    drop(store);
    let store = Store::open(path).expect("reopen");
    let loaded = store.page(&id).expect("page");
    assert_eq!(loaded, saved);
    assert_eq!(store.page(&unrelated.id).expect("other"), *unrelated);
    assert_eq!(
        store.counts().expect("counts"),
        Counts {
            pages: 2,
            types: 0,
            entities: 0,
            definitions: 0,
            assertions: 0,
        }
    );
    loaded
}

#[test]
fn pa_t01_save_persists_and_reopens() {
    let (scratch, mut store) = created("pa-t01.sqlite");
    let created_page = store
        .create_page(make_page(KEPT, "Original", "original body"))
        .expect("create");
    let original = store.page(&created_page.id).expect("read");
    assert_eq!(original, created_page);

    let saved = store
        .save_page(&original, make_page(KEPT, "Saved title", "Saved body"))
        .expect("save");
    assert_eq!(saved.id.as_str(), KEPT);
    assert_eq!(saved.title, "Saved title");
    assert_eq!(saved.body, "Saved body");
    assert_eq!(store.page(&original.id).expect("read back"), saved);
    drop(store);

    let loaded = persisted(&scratch.path, &original.id);
    assert_eq!(loaded.id.as_str(), KEPT);
    assert_eq!(loaded.title, "Saved title");
    assert_eq!(loaded.body, "Saved body");
    assert_eq!(loaded, saved);
}

#[test]
fn pa_t02_stale_save_keeps_the_committed_page() {
    let (scratch, mut store) = created("pa-t02.sqlite");
    let original = store
        .create_page(make_page(KEPT, "Original", "original body"))
        .expect("create");
    drop(store);

    let mut a = Store::open(&scratch.path).expect("open a");
    let mut b = Store::open(&scratch.path).expect("open b");
    let seen_by_a = a.page(&original.id).expect("a read");
    let seen_by_b = b.page(&original.id).expect("b read");
    assert_eq!(seen_by_a, original);
    assert_eq!(seen_by_b, original);

    let committed = b
        .save_page(&seen_by_b, make_page(KEPT, "From B", "committed by b"))
        .expect("b save");
    assert_eq!(committed, make_page(KEPT, "From B", "committed by b"));

    let stale = a
        .save_page(&seen_by_a, make_page(KEPT, "From A", "lost update"))
        .expect_err("stale");
    let absent = make_page(ABSENT, "Ghost", "not stored");
    let missing = a.save_page(&absent, absent.clone()).expect_err("missing");
    assert_stale_is_distinct_from_missing(&stale, &missing);
    assert_stale(stale, original.id.as_str());
    assert_validation(
        missing,
        ValidationReason::NotFound,
        "id",
        absent.id.as_str(),
    );

    assert_eq!(a.page(&original.id).expect("a sees commit"), committed);
    assert_eq!(b.page(&original.id).expect("b unchanged"), committed);
    drop(a);
    drop(b);

    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.page(&original.id).expect("persisted"), committed);
    assert_eq!(store.counts().expect("counts"), one_page());
    assert_validation(
        store.page(&absent.id).expect_err("absent"),
        ValidationReason::NotFound,
        "id",
        ABSENT,
    );
}

#[test]
fn pa_t02_stale_detects_a_body_only_change() {
    let (scratch, mut store) = created("pa-t02-body.sqlite");
    let original = store
        .create_page(make_page(KEPT, "Original", "original body"))
        .expect("create");
    drop(store);

    let mut a = Store::open(&scratch.path).expect("open a");
    let mut b = Store::open(&scratch.path).expect("open b");
    let seen_by_a = a.page(&original.id).expect("a read");
    let seen_by_b = b.page(&original.id).expect("b read");

    let committed = b
        .save_page(&seen_by_b, make_page(KEPT, "Original", "body changed by b"))
        .expect("b save");
    assert_eq!(committed.body, "body changed by b");
    assert_eq!(committed.title, "Original");

    let stale = a
        .save_page(&seen_by_a, make_page(KEPT, "Original", "body lost update"))
        .expect_err("stale");
    assert_stale(stale, original.id.as_str());
    assert_eq!(a.page(&original.id).expect("b commit kept"), committed);
    drop(a);
    drop(b);

    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.page(&original.id).expect("persisted"), committed);
}

#[test]
fn pa_t02_stale_detects_a_title_only_change() {
    let (scratch, mut store) = created("pa-t02-title.sqlite");
    let original = store
        .create_page(make_page(KEPT, "Original", "original body"))
        .expect("create");
    drop(store);

    let mut a = Store::open(&scratch.path).expect("open a");
    let mut b = Store::open(&scratch.path).expect("open b");
    let seen_by_a = a.page(&original.id).expect("a read");
    let seen_by_b = b.page(&original.id).expect("b read");

    let committed = b
        .save_page(
            &seen_by_b,
            make_page(KEPT, "title changed by b", "original body"),
        )
        .expect("b save");
    assert_eq!(committed.title, "title changed by b");
    assert_eq!(committed.body, "original body");

    let stale = a
        .save_page(
            &seen_by_a,
            make_page(KEPT, "title lost update", "original body"),
        )
        .expect_err("stale");
    assert_stale(stale, original.id.as_str());
    assert_eq!(a.page(&original.id).expect("b commit kept"), committed);
    drop(a);
    drop(b);

    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.page(&original.id).expect("persisted"), committed);
}

#[test]
fn pa_t03_identical_save_is_a_noop() {
    let (scratch, mut store) = created("pa-t03.sqlite");
    let created_page = store
        .create_page(make_page(KEPT, "Same", "same body"))
        .expect("create");
    let original = store.page(&created_page.id).expect("read");
    assert_eq!(original, created_page);

    let first = store
        .save_page(&original, original.clone())
        .expect("first noop");
    assert_eq!(first, original);
    assert_eq!(store.page(&original.id).expect("after first"), original);

    let second = store
        .save_page(&original, original.clone())
        .expect("second noop");
    assert_eq!(second, original);
    assert_eq!(store.page(&original.id).expect("after second"), original);
    drop(store);

    assert_eq!(persisted(&scratch.path, &original.id), original);
}

#[test]
fn pa_t04_missing_and_mismatched_ids_do_not_write() {
    let (scratch, mut store) = created("pa-t04.sqlite");
    let stored = store
        .create_page(make_page(KEPT, "Stored", "stored body"))
        .expect("create");
    assert_eq!(store.counts().expect("counts"), one_page());

    let absent = make_page(ABSENT, "Ghost", "not stored");
    let missing = store
        .save_page(&absent, absent.clone())
        .expect_err("missing");
    assert_validation(
        missing,
        ValidationReason::NotFound,
        "id",
        absent.id.as_str(),
    );
    assert_eq!(store.counts().expect("counts"), one_page());
    assert_eq!(store.page(&stored.id).expect("stored"), stored);
    assert_validation(
        store.page(&absent.id).expect_err("still absent"),
        ValidationReason::NotFound,
        "id",
        absent.id.as_str(),
    );

    let mismatch = store
        .save_page(&stored, make_page(OTHER, "Replacement", "should not land"))
        .expect_err("mismatch");
    assert_validation(
        mismatch,
        ValidationReason::PageIdMismatch,
        "id",
        stored.id.as_str(),
    );
    assert_eq!(store.counts().expect("counts"), one_page());
    assert_eq!(store.page(&stored.id).expect("stored"), stored);
    assert_validation(
        store
            .page(&PageId::parse(OTHER).expect("other id"))
            .expect_err("not created"),
        ValidationReason::NotFound,
        "id",
        OTHER,
    );

    let swapped = store
        .save_page(&absent, make_page(KEPT, "Swapped", "should not replace"))
        .expect_err("swapped mismatch");
    assert_validation(
        swapped,
        ValidationReason::PageIdMismatch,
        "id",
        absent.id.as_str(),
    );
    assert_eq!(store.counts().expect("counts"), one_page());
    assert_eq!(store.page(&stored.id).expect("stored"), stored);
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.page(&stored.id).expect("persisted"), stored);
    assert_eq!(store.counts().expect("counts"), one_page());
    assert_validation(
        store.page(&absent.id).expect_err("absent"),
        ValidationReason::NotFound,
        "id",
        ABSENT,
    );
}

#[test]
fn pa_t05_edits_roundtrip_without_touching_other_page() {
    let (scratch, mut store) = created("pa-t05.sqlite");
    let current = store
        .create_page(make_page(KEPT, "Initial", "Stable body"))
        .expect("page");
    let unrelated = store
        .create_page(make_page(OTHER, "Other", "Keep"))
        .expect("other");
    drop(store);

    let current = round_trip(
        &scratch.path,
        &current,
        make_page(KEPT, "Title only", "Stable body"),
        &unrelated,
    );
    let current = round_trip(
        &scratch.path,
        &current,
        make_page(KEPT, "Title only", "Body only"),
        &unrelated,
    );
    let current = round_trip(
        &scratch.path,
        &current,
        make_page(KEPT, "Title only", ""),
        &unrelated,
    );
    let unicode = "Texto con acento áéíóú ñ y emoji 📚\nsegunda línea";
    let loaded = round_trip(
        &scratch.path,
        &current,
        make_page(KEPT, "Title only", unicode),
        &unrelated,
    );
    assert_eq!(loaded.body, unicode);
    assert_eq!(loaded.title, "Title only");
    assert_eq!(loaded.id.as_str(), KEPT);

    let empty_title = round_trip(
        &scratch.path,
        &loaded,
        make_page(KEPT, "", "body under an empty title"),
        &unrelated,
    );
    assert_eq!(empty_title.title, "");

    let unicode_title = "Título con acento áéíóú ñ y emoji 📚\nsegunda línea";
    let loaded = round_trip(
        &scratch.path,
        &empty_title,
        make_page(KEPT, unicode_title, "body under a unicode title"),
        &unrelated,
    );
    assert_eq!(loaded.title, unicode_title);
    assert_eq!(loaded.body, "body under a unicode title");
}

#[test]
fn pa_t06_stale_snapshot_rejects_current_value() {
    let (scratch, mut store) = created("pa-t06.sqlite");
    let original = store
        .create_page(make_page(KEPT, "Before", "before"))
        .expect("create");
    let current = store
        .save_page(&original, make_page(KEPT, "After", "after"))
        .expect("update");
    assert_eq!(current, make_page(KEPT, "After", "after"));

    let stale = store
        .save_page(&original, make_page(KEPT, "After", "after"))
        .expect_err("stale");
    assert_stale(stale, original.id.as_str());
    assert_eq!(store.page(&original.id).expect("unchanged"), current);
    drop(store);

    assert_eq!(persisted(&scratch.path, &original.id), current);
}
