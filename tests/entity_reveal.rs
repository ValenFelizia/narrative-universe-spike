#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;
use narrative_universe::{import_new_json, Canon, EntityId, Store, Value};
use rusqlite::Connection;
fn id(end: &str) -> EntityId {
    EntityId::parse(&format!("00000000-0000-4000-8000-0000000000{end}")).unwrap()
}
fn fixture(path: &std::path::Path) -> Store {
    import_new_json(path, include_bytes!("fixtures/reveal-facts.json"))
        .unwrap()
        .0
}
#[test]
fn persisted_facts_preserve_alternatives_orientation_sources_and_bytes() {
    let scratch = support::scratch("reveal.sqlite");
    let store = fixture(&scratch.path);
    let before = support::bytes(&scratch.path);
    let mara = store.entity_reveal(&id("11")).unwrap();
    assert_eq!(mara.entity.label, "Mara Vey");
    assert_eq!(mara.type_label.as_deref(), Some("Person"));
    assert_eq!(mara.outgoing.len(), 7);
    let first = &mara.outgoing[0];
    assert_eq!(
        first.definition.label.as_ref().unwrap().as_str(),
        "member of"
    );
    assert_eq!(first.target.as_ref().unwrap().label, "Ashfall Order");
    assert_eq!(first.assertion.canon, Canon::Provisional);
    assert_eq!(
        first.assertion.provenance.locator.as_deref(),
        Some("folio 3")
    );
    assert_eq!(
        first.assertion.source_status_raw.as_deref(),
        Some("draft chapter 3")
    );
    assert_eq!(
        mara.outgoing[1].target.as_ref().unwrap().label,
        "Tidewardens"
    );
    assert_eq!(mara.outgoing[1].assertion.canon, Canon::Abierto);
    assert!(mara
        .outgoing
        .iter()
        .any(|c| c.definition.label.is_none() && c.assertion.value == Value::Unknown));
    let same_labels: Vec<_> = mara
        .outgoing
        .iter()
        .filter(|c| {
            c.definition
                .label
                .as_ref()
                .is_some_and(|l| l.as_str() == "member of")
        })
        .collect();
    assert_eq!(same_labels.len(), 3);
    assert_ne!(same_labels[0].definition.id, same_labels[2].definition.id);
    let ashfall = store.entity_reveal(&id("13")).unwrap();
    assert!(ashfall.outgoing.is_empty());
    assert_eq!(ashfall.incoming.len(), 3);
    assert_eq!(ashfall.incoming[0], *first);
    assert_eq!(ashfall.incoming[1].subject.label, "Corin Hale");
    assert!(ashfall
        .incoming
        .iter()
        .all(|c| c.target.as_ref().unwrap().id == id("13")));
    let corin = store.entity_reveal(&id("12")).unwrap();
    assert!(corin
        .outgoing
        .iter()
        .any(|c| c.assertion.value == Value::Boolean(false)
            && c.assertion.provenance == narrative_universe::Provenance::absent()));
    assert!(corin
        .outgoing
        .iter()
        .any(|c| c.assertion.value == Value::Unknown
            && c.assertion.scope_note.as_deref() == Some("sealed until the second flood")));
    assert!(store.entity_reveal(&id("15")).unwrap().outgoing.is_empty());
    assert_eq!(mara, store.entity_reveal(&id("11")).unwrap());
    assert!(store.entity_reveal(&id("99")).is_err());
    assert!(EntityId::parse("invalid").is_err());
    drop(store);
    assert_eq!(support::bytes(&scratch.path), before);
    assert_eq!(
        Store::open(&scratch.path)
            .unwrap()
            .entity_reveal(&id("11"))
            .unwrap(),
        mara
    );
}
#[test]
fn archived_claims_and_definitions_are_excluded_independently() {
    let scratch = support::scratch("archives.sqlite");
    let mut store = fixture(&scratch.path);
    store
        .archive_definition(
            &narrative_universe::DefinitionId::parse("00000000-0000-4000-8000-000000000031")
                .unwrap(),
        )
        .unwrap();
    store
        .archive_assertion(
            &narrative_universe::AssertionId::parse("00000000-0000-4000-8000-00000000004c")
                .unwrap(),
        )
        .unwrap();
    assert!(store.entity_reveal(&id("13")).unwrap().incoming.is_empty());
    assert_eq!(store.entity_reveal(&id("11")).unwrap().outgoing.len(), 4);
}
#[test]
fn busy_and_malformed_stored_records_fail_without_writes() {
    let scratch = support::scratch("errors.sqlite");
    let store = fixture(&scratch.path);
    let other = Connection::open(&scratch.path).unwrap();
    other.execute_batch("BEGIN EXCLUSIVE").unwrap();
    assert!(matches!(
        store.entity_reveal(&id("11")),
        Err(narrative_universe::Error::Busy)
    ));
    other.execute_batch("ROLLBACK; PRAGMA ignore_check_constraints=ON; UPDATE assertions SET bool_value=2 WHERE id='00000000-0000-4000-8000-000000000047'").unwrap();
    let before = support::bytes(&scratch.path);
    assert!(store.entity_reveal(&id("12")).is_err());
    assert_eq!(support::bytes(&scratch.path), before);
    other
        .execute_batch(
            "UPDATE definitions SET label=' bad ' WHERE id='00000000-0000-4000-8000-000000000031'",
        )
        .unwrap();
    assert!(store.entity_reveal(&id("11")).is_err());
}
