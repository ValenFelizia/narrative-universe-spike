#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use narrative_universe::{Entity, EntityId, EntityType, Page, PageId, Store, TypeId, UniverseId};
use support::scratch;

const UNIVERSE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const ATLAS: &str = "22222222-2222-4222-8222-222222222222";
const PLACE: &str = "33333333-3333-4333-8333-333333333333";
const HOUSE: &str = "44444444-4444-4444-8444-444444444444";
const VALDORIA_HOUSE: &str = "55555555-5555-4555-8555-555555555551";
const VALDORIA_PLACE: &str = "55555555-5555-4555-8555-555555555552";
const ELARA: &str = "55555555-5555-4555-8555-555555555553";
const ACCENT: &str = "55555555-5555-4555-8555-555555555554";

type Row = (String, String, Option<String>, Option<String>);

fn rows(store: &Store) -> Vec<Row> {
    store
        .entity_index()
        .expect("entity index")
        .into_iter()
        .map(|entry| {
            (
                entry.id.as_str().to_string(),
                entry.label,
                entry.type_label,
                entry.primary_page_title,
            )
        })
        .collect()
}

fn entity(id: &str, label: &str, type_id: Option<&str>, page_id: Option<&str>) -> Entity {
    Entity {
        id: EntityId::parse(id).expect("entity id"),
        label: label.to_string(),
        type_id: type_id.map(|raw| TypeId::parse(raw).expect("type id")),
        page_id: page_id.map(|raw| PageId::parse(raw).expect("page id")),
    }
}

#[test]
fn entity_index_joins_optional_labels_in_stable_order_and_survives_reopen() {
    let scratch = scratch("entity-index.sqlite");
    let mut store = Store::create(
        &scratch.path,
        UniverseId::parse(UNIVERSE).expect("universe"),
    )
    .expect("create");
    assert!(store.entity_index().expect("empty").is_empty());

    store
        .create_page(Page {
            id: PageId::parse(ATLAS).expect("page"),
            title: "Atlas".to_string(),
            body: "synthetic body that the index never reads".to_string(),
        })
        .expect("page");
    for (id, label) in [(PLACE, "Place"), (HOUSE, "House")] {
        store
            .create_entity_type(EntityType {
                id: TypeId::parse(id).expect("type"),
                label: label.to_string(),
            })
            .expect("type");
    }
    store
        .create_entity(entity(VALDORIA_PLACE, "Valdoria", Some(PLACE), Some(ATLAS)))
        .expect("entity");
    store
        .create_entity(entity(ELARA, "Elara", None, None))
        .expect("entity");
    store
        .create_entity(entity(ACCENT, "\u{c9}lan", None, Some(ATLAS)))
        .expect("entity");
    store
        .create_entity(entity(VALDORIA_HOUSE, "Valdoria", Some(HOUSE), None))
        .expect("entity");
    let before = store.counts().expect("counts");

    let expected: Vec<Row> = vec![
        (ELARA.into(), "Elara".into(), None, None),
        (
            VALDORIA_HOUSE.into(),
            "Valdoria".into(),
            Some("House".into()),
            None,
        ),
        (
            VALDORIA_PLACE.into(),
            "Valdoria".into(),
            Some("Place".into()),
            Some("Atlas".into()),
        ),
        (
            ACCENT.into(),
            "\u{c9}lan".into(),
            None,
            Some("Atlas".into()),
        ),
    ];
    assert_eq!(rows(&store), expected);
    assert_eq!(store.counts().expect("counts"), before);
    drop(store);

    let reopened = Store::open(&scratch.path).expect("reopen");
    assert_eq!(rows(&reopened), expected);
}
