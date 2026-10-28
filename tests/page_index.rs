#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use narrative_universe::{Page, PageId, Store, UniverseId};
use rusqlite::Connection;
use support::scratch;

const UNIVERSE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const PAGE_ONE: &str = "22222222-2222-4222-8222-222222222221";
const PAGE_TWO: &str = "22222222-2222-4222-8222-222222222222";
const PAGE_THREE: &str = "22222222-2222-4222-8222-222222222223";

fn page(id: &str, title: &str, body: &str) -> Page {
    Page {
        id: PageId::parse(id).expect("page id"),
        title: title.to_string(),
        body: body.to_string(),
    }
}

#[test]
fn page_index_is_empty_then_stably_sorted_and_survives_reopen() {
    let scratch = scratch("page-index.sqlite");
    let mut store = Store::create(
        &scratch.path,
        UniverseId::parse(UNIVERSE).expect("universe id"),
    )
    .expect("create");

    assert!(store.page_index().expect("empty index").is_empty());
    store
        .create_page(page(PAGE_THREE, "Zeta", "third body"))
        .expect("page");
    store
        .create_page(page(PAGE_TWO, "Alpha", "second body"))
        .expect("page");
    store
        .create_page(page(PAGE_ONE, "Alpha", "first body"))
        .expect("page");

    let expected = vec![
        (PAGE_ONE, "Alpha"),
        (PAGE_TWO, "Alpha"),
        (PAGE_THREE, "Zeta"),
    ];
    let index = store.page_index().expect("index");
    assert_eq!(
        index
            .iter()
            .map(|entry| (entry.id.as_str(), entry.title.as_str()))
            .collect::<Vec<_>>(),
        expected
    );
    drop(store);

    let reopened = Store::open(&scratch.path).expect("reopen");
    let index = reopened.page_index().expect("reopened index");
    assert_eq!(
        index
            .iter()
            .map(|entry| (entry.id.as_str(), entry.title.as_str()))
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn page_index_does_not_read_page_bodies() {
    let scratch = scratch("page-index-body.sqlite");
    let mut store = Store::create(
        &scratch.path,
        UniverseId::parse(UNIVERSE).expect("universe id"),
    )
    .expect("create");
    store
        .create_page(page(PAGE_ONE, "Indexed", "valid body"))
        .expect("page");
    drop(store);

    let connection = Connection::open(&scratch.path).expect("raw connection");
    connection
        .execute("UPDATE pages SET body = X'80' WHERE id = ?1", [PAGE_ONE])
        .expect("make body unreadable as UTF-8");
    drop(connection);

    let store = Store::open(&scratch.path).expect("open");
    let index = store
        .page_index()
        .expect("title index must not decode body");
    assert_eq!(index.len(), 1);
    assert_eq!(index[0].id.as_str(), PAGE_ONE);
    assert_eq!(index[0].title, "Indexed");
}

#[test]
fn page_index_rejects_invalid_stored_ids_without_echoing_them() {
    let scratch = scratch("page-index-invalid-id.sqlite");
    let mut store = Store::create(&scratch.path, UniverseId::parse(UNIVERSE).unwrap()).unwrap();
    store.create_page(page(PAGE_ONE, "Page", "body")).unwrap();
    drop(store);
    let connection = Connection::open(&scratch.path).unwrap();
    connection
        .execute(
            "UPDATE pages SET id = ?1",
            ["SECRET_MARKER_1234567890123456789012"],
        )
        .unwrap();
    drop(connection);
    let store = Store::open(&scratch.path).unwrap();
    let error = store.page_index().unwrap_err().to_string();
    assert!(!error.contains("SECRET_MARKER"));
}
