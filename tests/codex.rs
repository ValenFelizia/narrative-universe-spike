#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use narrative_universe::{
    validate_codex_label, CodexEntitySummary, CodexIndex, Entity, EntityId, EntityType, Error,
    Page, PageId, Store, TypeId, UniverseId, ValidationReason, MAX_CODEX_LABEL_SCALARS,
};
use rusqlite::Connection;
use support::scratch;

const UNIVERSE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const PAGE_A: &str = "22222222-2222-4222-8222-222222222222";
const TYPE_HOUSE_A: &str = "33333333-3333-4333-8333-333333333331";
const TYPE_HOUSE_B: &str = "33333333-3333-4333-8333-333333333332";
const TYPE_UNUSED: &str = "33333333-3333-4333-8333-333333333333";
const ENT_MARA: &str = "55555555-5555-4555-8555-555555555551";
const ENT_MARA_DUP: &str = "55555555-5555-4555-8555-555555555552";
const ENT_UNTYPED: &str = "55555555-5555-4555-8555-555555555553";
const ENT_ACCENT: &str = "55555555-5555-4555-8555-555555555554";

fn entity(id: &str, label: &str, type_id: Option<&str>, page_id: Option<&str>) -> Entity {
    Entity {
        id: EntityId::parse(id).expect("entity id"),
        label: label.to_string(),
        type_id: type_id.map(|raw| TypeId::parse(raw).expect("type id")),
        page_id: page_id.map(|raw| PageId::parse(raw).expect("page id")),
    }
}

fn entity_type(id: &str, label: &str) -> EntityType {
    EntityType {
        id: TypeId::parse(id).expect("type id"),
        label: label.to_string(),
    }
}

#[test]
fn codex_index_reads_empty_types_and_entities_in_binary_order_and_persists_reopen() {
    let scratch = scratch("codex-index.sqlite");
    let mut store = Store::create(
        &scratch.path,
        UniverseId::parse(UNIVERSE).expect("universe"),
    )
    .expect("create");

    let empty = store.codex_index().expect("empty codex index");
    assert!(empty.types.is_empty());
    assert!(empty.entities.is_empty());
    assert!(store.type_index().expect("empty types").is_empty());
    assert!(store
        .codex_entity_index()
        .expect("empty entities")
        .is_empty());

    store
        .create_page(Page {
            id: PageId::parse(PAGE_A).expect("page"),
            title: "Red Harbor Ledger".to_string(),
            body: "Unrelated prose that must stay untouched.".to_string(),
        })
        .expect("page");
    let snapshot_before = store.export_snapshot().expect("snapshot before");

    // Insert duplicate type labels in reverse ID order plus an unused type.
    store
        .create_entity_type(entity_type(TYPE_HOUSE_B, "House"))
        .expect("house b");
    store
        .create_entity_type(entity_type(TYPE_HOUSE_A, "House"))
        .expect("house a");
    store
        .create_entity_type(entity_type(TYPE_UNUSED, "Artifact"))
        .expect("unused");

    // Insert entities with duplicate labels, typed, untyped, and optional primary page.
    store
        .create_entity(entity(ENT_MARA_DUP, "Mara Vey", Some(TYPE_HOUSE_B), None))
        .expect("mara dup");
    store
        .create_entity(entity(
            ENT_MARA,
            "Mara Vey",
            Some(TYPE_HOUSE_A),
            Some(PAGE_A),
        ))
        .expect("mara");
    store
        .create_entity(entity(ENT_UNTYPED, "Ashfall Bell", None, None))
        .expect("untyped");
    store
        .create_entity(entity(ENT_ACCENT, "\u{c9}lan", None, Some(PAGE_A)))
        .expect("accent");

    let expected = CodexIndex {
        types: vec![
            entity_type(TYPE_UNUSED, "Artifact"),
            entity_type(TYPE_HOUSE_A, "House"),
            entity_type(TYPE_HOUSE_B, "House"),
        ],
        entities: vec![
            CodexEntitySummary {
                id: EntityId::parse(ENT_UNTYPED).unwrap(),
                label: "Ashfall Bell".to_string(),
                type_id: None,
                type_label: None,
                primary_page_title: None,
            },
            CodexEntitySummary {
                id: EntityId::parse(ENT_MARA).unwrap(),
                label: "Mara Vey".to_string(),
                type_id: Some(TypeId::parse(TYPE_HOUSE_A).unwrap()),
                type_label: Some("House".to_string()),
                primary_page_title: Some("Red Harbor Ledger".to_string()),
            },
            CodexEntitySummary {
                id: EntityId::parse(ENT_MARA_DUP).unwrap(),
                label: "Mara Vey".to_string(),
                type_id: Some(TypeId::parse(TYPE_HOUSE_B).unwrap()),
                type_label: Some("House".to_string()),
                primary_page_title: None,
            },
            CodexEntitySummary {
                id: EntityId::parse(ENT_ACCENT).unwrap(),
                label: "\u{c9}lan".to_string(),
                type_id: None,
                type_label: None,
                primary_page_title: Some("Red Harbor Ledger".to_string()),
            },
        ],
    };

    assert_eq!(store.codex_index().expect("codex index"), expected);
    assert_eq!(store.type_index().expect("type index"), expected.types);
    assert_eq!(
        store.codex_entity_index().expect("codex entity index"),
        expected.entities
    );

    // Existing page document was not touched by type or entity creations.
    let page_after = store
        .page_document(&PageId::parse(PAGE_A).unwrap())
        .expect("page after");
    assert_eq!(page_after.body.body, snapshot_before.pages[0].page.body);
    assert!(page_after.body.mentions.is_empty());
    assert_eq!(page_after.title, "Red Harbor Ledger");

    drop(store);
    let reopened = Store::open(&scratch.path).expect("reopen");
    assert_eq!(reopened.codex_index().expect("reopened codex"), expected);
}

#[test]
fn validate_codex_label_enforces_nonblank_and_preserves_exact_bytes() {
    assert_eq!(
        validate_codex_label("Mara Vey").expect("normal"),
        "Mara Vey"
    );
    // Exact scalar boundary and byte-for-byte preservation (no case or Unicode normalization).
    let decomposed = "Cafe\u{0301}";
    assert_eq!(validate_codex_label(decomposed).expect("nfd"), decomposed);
    let max_120 = "\u{1f409}".repeat(MAX_CODEX_LABEL_SCALARS);
    assert_eq!(
        validate_codex_label(&max_120).expect("120 scalars"),
        max_120
    );

    let too_long = "a".repeat(MAX_CODEX_LABEL_SCALARS + 1);
    for invalid in [
        "",
        " ",
        "\t",
        "\n",
        "\u{00a0}",
        " Mara",
        "Mara ",
        "Mara\nVey",
        "Mara\0Vey",
        "\tMara",
        too_long.as_str(),
    ] {
        assert!(
            matches!(
                validate_codex_label(invalid),
                Err(Error::Validation {
                    reason: ValidationReason::InvalidLabel,
                    ..
                })
            ),
            "expected invalid label for {invalid:?}"
        );
    }
}

#[test]
fn codex_index_fails_closed_on_corrupted_rows_or_dangling_foreign_keys() {
    for corrupt_sql in [
        "INSERT INTO types (id, label) VALUES ('not-a-valid-type-xxxxxxxxxxxxxxxxxxx', 'House')",
        "INSERT INTO types (id, label) VALUES ('33333333-3333-4333-8333-333333333339', X'deadbeef')",
        "INSERT INTO entities (id, label, type_id, page_id) VALUES ('not-a-valid-entity-xxxxxxxxxxxxxxxxx', 'X', NULL, NULL)",
        "INSERT INTO entities (id, label, type_id, page_id) VALUES ('55555555-5555-4555-8555-555555555559', X'deadbeef', NULL, NULL)",
        "INSERT INTO entities (id, label, type_id, page_id) VALUES ('55555555-5555-4555-8555-555555555559', 'Dangling Type', '33333333-3333-4333-8333-333333333399', NULL)",
        "INSERT INTO entities (id, label, type_id, page_id) VALUES ('55555555-5555-4555-8555-555555555559', 'Dangling Page', NULL, '22222222-2222-4222-8222-222222222299')",
    ] {
        let scratch = scratch("codex-corrupt.sqlite");
        let store = Store::create(
            &scratch.path,
            UniverseId::parse(UNIVERSE).expect("universe"),
        )
        .expect("create");
        drop(store);

        let raw = Connection::open(&scratch.path).expect("raw open");
        raw.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
        raw.execute_batch(corrupt_sql).unwrap();
        drop(raw);

        let reopened = Store::open(&scratch.path).expect("reopen");
        assert!(
            reopened.codex_index().is_err(),
            "expected failure for {corrupt_sql}"
        );
    }
}
