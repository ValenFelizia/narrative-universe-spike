#[path = "support/mod.rs"]
mod support;

use narrative_universe::{import_new_json, Store};

#[test]
fn connect_smoke_imports_without_seed_changes_and_reopens_exactly() {
    let scratch = support::scratch("connect-smoke.sqlite");
    let fixture = include_bytes!("fixtures/connect-smoke.json");
    let (mut store, _) = import_new_json(&scratch.path, fixture).expect("synthetic import-new");
    let before = store.export_snapshot().expect("snapshot");
    let entities = store.entity_index().expect("entity index");
    assert_eq!(entities.len(), 3);
    assert!(entities.iter().all(|e| e.label == "Valdoria"));
    assert!(entities
        .iter()
        .any(|e| e.type_label.as_deref() == Some("House")));
    assert_eq!(
        entities
            .iter()
            .filter(|e| e.type_label.as_deref() == Some("Place"))
            .count(),
        2
    );
    assert_eq!(before.pages.len(), 3);
    assert!(before.pages.iter().any(|p| p.page.body.len() > 4000));
    let stored = support::bytes(&scratch.path);
    drop(store);
    let mut reopened = Store::open(&scratch.path).expect("reopen");
    assert_eq!(
        before,
        reopened.export_snapshot().expect("reopened snapshot")
    );
    assert_eq!(stored, support::bytes(&scratch.path));
}
