#[path = "support/mod.rs"]
mod support;

use narrative_universe::{
    import_json, import_new_json, Assertion, AssertionId, Canon, Cardinality, Definition,
    DefinitionId, Entity, EntityId, EntityType, Error, Owner, Page, PageId, Provenance, Store,
    TypeId, UniverseId, ValidationReason, Value, ValueClass,
};
use support::{bytes, scratch};

fn universe() -> UniverseId {
    UniverseId::parse("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").expect("universe")
}

fn created(name: &str) -> (support::Scratch, Store) {
    let scratch = scratch(name);
    let store = Store::create(&scratch.path, universe()).expect("create");
    (scratch, store)
}

fn id_page(raw: &str) -> PageId {
    PageId::parse(raw).expect(raw)
}

fn id_type(raw: &str) -> TypeId {
    TypeId::parse(raw).expect(raw)
}

fn id_entity(raw: &str) -> EntityId {
    EntityId::parse(raw).expect(raw)
}

fn id_definition(raw: &str) -> DefinitionId {
    DefinitionId::parse(raw).expect(raw)
}

fn id_assertion(raw: &str) -> AssertionId {
    AssertionId::parse(raw).expect(raw)
}

struct Fixture {
    page: Page,
    linked_page: Page,
    entity_type: EntityType,
    incomplete: Entity,
    narrator: Entity,
    house: Entity,
    archived_definition: Definition,
    boolean_definition: Definition,
    reference_definition: Definition,
    archived_assertion: Assertion,
    false_assertion: Assertion,
    unknown_assertion: Assertion,
    reference_assertion: Assertion,
}

fn fixture() -> Fixture {
    let page = Page {
        id: id_page("22222222-2222-4222-8222-222222222221"),
        title: "Página libre".to_string(),
        body: "Texto con acento áéíóú ñ y emoji 📚".to_string(),
    };
    let linked_page = Page {
        id: id_page("22222222-2222-4222-8222-222222222222"),
        title: "Ficha".to_string(),
        body: String::new(),
    };
    let entity_type = EntityType {
        id: id_type("33333333-3333-4333-8333-333333333333"),
        label: "Personaje".to_string(),
    };
    let incomplete = Entity {
        id: id_entity("55555555-5555-4555-8555-555555555551"),
        label: "Ficha incompleta".to_string(),
        type_id: None,
        page_id: None,
    };
    let narrator = Entity {
        id: id_entity("55555555-5555-4555-8555-555555555555"),
        label: "Narrador sintético".to_string(),
        type_id: Some(entity_type.id.clone()),
        page_id: Some(linked_page.id.clone()),
    };
    let house = Entity {
        id: id_entity("66666666-6666-4666-8666-666666666666"),
        label: "Casa sintética".to_string(),
        type_id: Some(entity_type.id.clone()),
        page_id: None,
    };
    let archived_definition = Definition {
        id: id_definition("77777777-7777-4777-8777-777777777771"),
        owner: Owner::Type(entity_type.id.clone()),
        value_class: ValueClass::Text,
        cardinality: Cardinality::Singular,
        archived: true,
    };
    let boolean_definition = Definition {
        id: id_definition("77777777-7777-4777-8777-777777777772"),
        owner: Owner::Type(entity_type.id.clone()),
        value_class: ValueClass::Boolean,
        cardinality: Cardinality::Singular,
        archived: false,
    };
    let reference_definition = Definition {
        id: id_definition("77777777-7777-4777-8777-777777777773"),
        owner: Owner::Entity(narrator.id.clone()),
        value_class: ValueClass::Reference,
        cardinality: Cardinality::Singular,
        archived: false,
    };
    let archived_assertion = Assertion {
        id: id_assertion("88888888-8888-4888-8888-888888888881"),
        subject_id: narrator.id.clone(),
        definition_id: archived_definition.id.clone(),
        value: Value::Text("histórico".to_string()),
        canon: Canon::Provisional,
        source_status_raw: Some("ETIQUETA LITERAL".to_string()),
        provenance: Provenance {
            external_ref: Some("source-a".to_string()),
            locator: Some("section-1".to_string()),
            fragment: Some("histórico".to_string()),
            source_revision: Some("rev-1".to_string()),
        },
        scope_note: Some("al inicio".to_string()),
        archived: true,
    };
    let false_assertion = Assertion {
        id: id_assertion("88888888-8888-4888-8888-888888888882"),
        subject_id: narrator.id.clone(),
        definition_id: boolean_definition.id.clone(),
        value: Value::Boolean(false),
        canon: Canon::Abierto,
        source_status_raw: None,
        provenance: Provenance {
            external_ref: None,
            locator: Some(String::new()),
            fragment: None,
            source_revision: None,
        },
        scope_note: None,
        archived: false,
    };
    let unknown_assertion = Assertion {
        id: id_assertion("88888888-8888-4888-8888-888888888883"),
        subject_id: narrator.id.clone(),
        definition_id: boolean_definition.id.clone(),
        value: Value::Unknown,
        canon: Canon::MisterioIntencional,
        source_status_raw: Some("NO SE SABE".to_string()),
        provenance: Provenance::absent(),
        scope_note: Some("al inicio".to_string()),
        archived: false,
    };
    let reference_assertion = Assertion {
        id: id_assertion("88888888-8888-4888-8888-888888888884"),
        subject_id: narrator.id.clone(),
        definition_id: reference_definition.id.clone(),
        value: Value::Reference(house.id.clone()),
        canon: Canon::Acordado,
        source_status_raw: Some("FUENTE".to_string()),
        provenance: Provenance {
            external_ref: Some("source-b".to_string()),
            locator: None,
            fragment: None,
            source_revision: None,
        },
        scope_note: Some("después".to_string()),
        archived: false,
    };
    Fixture {
        page,
        linked_page,
        entity_type,
        incomplete,
        narrator,
        house,
        archived_definition,
        boolean_definition,
        reference_definition,
        archived_assertion,
        false_assertion,
        unknown_assertion,
        reference_assertion,
    }
}

fn persist(store: &mut Store, fixture: &Fixture) {
    store.create_page(fixture.page.clone()).expect("free page");
    store
        .create_page(fixture.linked_page.clone())
        .expect("linked page");
    store
        .create_entity_type(fixture.entity_type.clone())
        .expect("type");
    store
        .create_entity(fixture.incomplete.clone())
        .expect("incomplete");
    store
        .create_entity(fixture.narrator.clone())
        .expect("narrator");
    store.create_entity(fixture.house.clone()).expect("house");
    let mut live_definition = fixture.archived_definition.clone();
    live_definition.archived = false;
    store
        .create_definition(live_definition)
        .expect("definition");
    let mut live_assertion = fixture.archived_assertion.clone();
    live_assertion.archived = false;
    store
        .create_assertion(live_assertion)
        .expect("historical assertion");
    store
        .archive_assertion(&fixture.archived_assertion.id)
        .expect("archive assertion");
    store
        .archive_definition(&fixture.archived_definition.id)
        .expect("archive definition");
    store
        .create_definition(fixture.boolean_definition.clone())
        .expect("boolean definition");
    store
        .create_definition(fixture.reference_definition.clone())
        .expect("reference definition");
    store
        .create_assertion(fixture.false_assertion.clone())
        .expect("false");
    store
        .create_assertion(fixture.unknown_assertion.clone())
        .expect("unknown");
    store
        .create_assertion(fixture.reference_assertion.clone())
        .expect("reference");
}

fn assert_fixture(store: &Store, fixture: &Fixture) {
    assert_eq!(store.page(&fixture.page.id).expect("page"), fixture.page);
    assert_eq!(
        store.page(&fixture.linked_page.id).expect("linked"),
        fixture.linked_page
    );
    assert_eq!(
        store.entity_type(&fixture.entity_type.id).expect("type"),
        fixture.entity_type
    );
    assert_eq!(
        store.entity(&fixture.incomplete.id).expect("incomplete"),
        fixture.incomplete
    );
    assert_eq!(
        store.entity(&fixture.narrator.id).expect("narrator"),
        fixture.narrator
    );
    assert_eq!(
        store.entity(&fixture.house.id).expect("house"),
        fixture.house
    );
    assert_eq!(
        store
            .definition(&fixture.archived_definition.id)
            .expect("archived definition"),
        fixture.archived_definition
    );
    assert_eq!(
        store
            .definition(&fixture.boolean_definition.id)
            .expect("boolean definition"),
        fixture.boolean_definition
    );
    assert_eq!(
        store
            .definition(&fixture.reference_definition.id)
            .expect("reference definition"),
        fixture.reference_definition
    );
    assert_eq!(
        store
            .assertion(&fixture.archived_assertion.id)
            .expect("archived assertion"),
        fixture.archived_assertion
    );
    assert_eq!(
        store.assertion(&fixture.false_assertion.id).expect("false"),
        fixture.false_assertion
    );
    assert_eq!(
        store
            .assertion(&fixture.unknown_assertion.id)
            .expect("unknown"),
        fixture.unknown_assertion
    );
    assert_eq!(
        store
            .assertion(&fixture.reference_assertion.id)
            .expect("reference"),
        fixture.reference_assertion
    );
}

#[test]
fn t08_roundtrip_matches_literal_fields_and_json_shape() {
    let (scratch, mut store) = created("t08.sqlite");
    let fixture = fixture();
    persist(&mut store, &fixture);
    let bytes = narrative_universe::export_json(&mut store).expect("export");
    let document: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(document["format_version"], 1);
    assert_eq!(document["universe_id"], universe().as_str());
    assert_eq!(
        document["pages"][0]["id"],
        "22222222-2222-4222-8222-222222222221"
    );
    assert_eq!(document["pages"][0]["body"], fixture.page.body);
    assert_eq!(document["entities"][0]["type_id"], serde_json::Value::Null);
    assert_eq!(document["entities"][0]["page_id"], serde_json::Value::Null);
    let assertions = document["assertions"].as_array().expect("assertions");
    let unknown = assertions
        .iter()
        .find(|row| row["id"] == "88888888-8888-4888-8888-888888888883")
        .expect("unknown row");
    assert_eq!(unknown["value"], serde_json::json!({"kind": "unknown"}));
    assert_eq!(unknown["canon"], "MISTERIO_INTENCIONAL");
    assert_eq!(
        unknown["provenance"]["external_ref"],
        serde_json::Value::Null
    );
    let falsy = assertions
        .iter()
        .find(|row| row["id"] == "88888888-8888-4888-8888-888888888882")
        .expect("false row");
    assert_eq!(
        falsy["value"],
        serde_json::json!({"kind": "boolean", "value": false})
    );
    assert_eq!(falsy["provenance"]["locator"], "");
    assert_eq!(falsy["provenance"]["external_ref"], serde_json::Value::Null);
    let archived = assertions
        .iter()
        .find(|row| row["id"] == "88888888-8888-4888-8888-888888888881")
        .expect("archived row");
    assert_eq!(archived["archived"], true);
    assert_eq!(archived["canon"], "PROVISIONAL");
    assert_eq!(archived["scope_note"], "al inicio");
    assert_eq!(archived["source_status_raw"], "ETIQUETA LITERAL");
    assert_eq!(archived["value"]["text"], "histórico");
    let reference = assertions
        .iter()
        .find(|row| row["id"] == "88888888-8888-4888-8888-888888888884")
        .expect("reference row");
    assert_eq!(reference["canon"], "ACORDADO");
    assert_eq!(
        reference["value"]["target_id"],
        "66666666-6666-4666-8666-666666666666"
    );
    drop(store);

    let imported = scratch.path.with_file_name("t08-imported.sqlite");
    let (imported_store, outcome) = import_new_json(&imported, &bytes).expect("import-new");
    assert_eq!(outcome.inserted, 13);
    assert_eq!(outcome.unchanged, 0);
    assert_fixture(&imported_store, &fixture);
    let err = {
        let mut imported_store = imported_store;
        imported_store
            .create_assertion(Assertion {
                id: id_assertion("88888888-8888-4888-8888-888888888885"),
                subject_id: fixture.narrator.id.clone(),
                definition_id: fixture.archived_definition.id.clone(),
                value: Value::Text("nueva".to_string()),
                canon: Canon::Abierto,
                source_status_raw: None,
                provenance: Provenance::absent(),
                scope_note: None,
                archived: false,
            })
            .expect_err("live create")
    };
    match err {
        Error::Validation {
            reason: ValidationReason::ArchivedDefinition,
            ..
        } => {}
        other => panic!("expected archived definition, got {other}"),
    }
    let _ = scratch;
}

#[test]
fn t09_reimport_inserts_nothing_and_keeps_distinct_ids() {
    let (scratch, mut store) = created("t09.sqlite");
    let fixture = fixture();
    persist(&mut store, &fixture);
    let bytes = narrative_universe::export_json(&mut store).expect("export");
    let first = import_json(&mut store, &bytes).expect("first import");
    assert_eq!(first.inserted, 0);
    assert_eq!(first.unchanged, 13);
    let second = import_json(&mut store, &bytes).expect("second import");
    assert_eq!(second.inserted, 0);
    assert_eq!(second.unchanged, 13);
    let twin = Entity {
        id: id_entity("55555555-5555-4555-8555-555555555552"),
        label: fixture.narrator.label.clone(),
        type_id: None,
        page_id: None,
    };
    let mut document: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    document["entities"]
        .as_array_mut()
        .expect("entities")
        .push(serde_json::json!({
            "id": twin.id.as_str(),
            "label": twin.label,
            "type_id": null,
            "page_id": null
        }));
    let extra = serde_json::to_vec(&document).expect("encode");
    let added = import_json(&mut store, &extra).expect("add twin");
    assert_eq!(added.inserted, 1);
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(
        store.entity(&fixture.narrator.id).expect("original"),
        fixture.narrator
    );
    assert_eq!(store.entity(&twin.id).expect("twin"), twin);
}

#[test]
fn t10_late_conflict_rolls_back_the_new_page() {
    let (scratch, mut store) = created("t10.sqlite");
    let original = store
        .create_page(Page {
            id: id_page("22222222-2222-4222-8222-222222222222"),
            title: "Vieja".to_string(),
            body: "queda".to_string(),
        })
        .expect("page");
    let bytes = serde_json::to_vec(&serde_json::json!({
        "format_version": 1,
        "universe_id": universe().as_str(),
        "pages": [
            {"id": "22222222-2222-4222-8222-222222222221", "title": "Nueva", "body": "no queda"},
            {"id": "22222222-2222-4222-8222-222222222222", "title": "Cambiada", "body": "cambia"}
        ],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    }))
    .expect("json");
    let err = import_json(&mut store, &bytes).expect_err("conflict");
    match err {
        Error::ImportConflict { class, id } => {
            assert_eq!(class, "page");
            assert_eq!(id, original.id.as_str());
        }
        other => panic!("expected import conflict, got {other}"),
    }
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.page(&original.id).expect("page"), original);
    match store
        .page(&id_page("22222222-2222-4222-8222-222222222221"))
        .expect_err("new page")
    {
        Error::Validation {
            reason: ValidationReason::NotFound,
            ..
        } => {}
        other => panic!("expected not found, got {other}"),
    }
}

#[test]
fn t11_rejected_documents_do_not_write() {
    let (scratch, mut store) = created("t11.sqlite");
    let page = store
        .create_page(Page {
            id: id_page("22222222-2222-4222-8222-222222222222"),
            title: "Queda".to_string(),
            body: "intacta".to_string(),
        })
        .expect("page");
    drop(store);
    let before = bytes(&scratch.path);
    let base = serde_json::json!({
        "format_version": 1,
        "universe_id": universe().as_str(),
        "pages": [],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": []
    });
    let mut version = base.clone();
    version["format_version"] = serde_json::json!(2);
    expect_unchanged(
        &scratch.path,
        &before,
        &serde_json::to_vec(&version).unwrap(),
        |err| matches!(err, Error::UnsupportedVersion { found: 2 }),
    );
    let mut other = base.clone();
    other["universe_id"] = serde_json::json!("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
    expect_unchanged(
        &scratch.path,
        &before,
        &serde_json::to_vec(&other).unwrap(),
        |err| matches!(err, Error::WrongUniverse { .. }),
    );
    let mut duplicate = base.clone();
    duplicate["pages"] = serde_json::json!([
        {"id": "22222222-2222-4222-8222-222222222221", "title": "A", "body": "a"},
        {"id": "22222222-2222-4222-8222-222222222221", "title": "B", "body": "b"}
    ]);
    expect_unchanged(
        &scratch.path,
        &before,
        &serde_json::to_vec(&duplicate).unwrap(),
        |err| {
            matches!(
                err,
                Error::Validation {
                    reason: ValidationReason::DuplicateId,
                    field: "page",
                    ..
                }
            )
        },
    );
    let mut unknown_field = base.clone();
    unknown_field["extra"] = serde_json::json!(true);
    expect_unchanged(
        &scratch.path,
        &before,
        &serde_json::to_vec(&unknown_field).unwrap(),
        |err| matches!(err, Error::InvalidDocument { .. }),
    );
    let mut unknown_kind = base.clone();
    unknown_kind["assertions"] = serde_json::json!([{
        "id": "88888888-8888-4888-8888-888888888881",
        "subject_id": "55555555-5555-4555-8555-555555555555",
        "definition_id": "77777777-7777-4777-8777-777777777777",
        "value": {"kind": "number", "value": 1},
        "canon": "ABIERTO",
        "provenance": {},
        "archived": false
    }]);
    expect_unchanged(
        &scratch.path,
        &before,
        &serde_json::to_vec(&unknown_kind).unwrap(),
        |err| matches!(err, Error::InvalidDocument { .. }),
    );
    let mut bad_payload = base.clone();
    bad_payload["assertions"] = serde_json::json!([{
        "id": "88888888-8888-4888-8888-888888888881",
        "subject_id": "55555555-5555-4555-8555-555555555555",
        "definition_id": "77777777-7777-4777-8777-777777777777",
        "value": {"kind": "text", "text": 4},
        "canon": "ABIERTO",
        "provenance": {},
        "archived": false
    }]);
    expect_unchanged(
        &scratch.path,
        &before,
        &serde_json::to_vec(&bad_payload).unwrap(),
        |err| matches!(err, Error::InvalidDocument { .. }),
    );
    expect_unchanged(&scratch.path, &before, b"{", |err| {
        matches!(err, Error::InvalidDocument { .. })
    });
    let mut huge = vec![b' '; 10 * 1024 * 1024 + 1];
    huge[0] = b'{';
    expect_unchanged(&scratch.path, &before, &huge, |err| {
        matches!(
            err,
            Error::Validation {
                reason: ValidationReason::InputTooLarge,
                field: "json",
                id: None,
            }
        )
    });
    let missing = scratch.path.with_file_name("t11-new.sqlite");
    let err = import_new_json(&missing, b"{").expect_err("import-new");
    assert!(matches!(err, Error::InvalidDocument { .. }), "{err}");
    assert!(!missing.exists());
    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.page(&page.id).expect("page"), page);
}

fn expect_unchanged(
    path: &std::path::Path,
    before: &[u8],
    bytes: &[u8],
    pred: impl Fn(&Error) -> bool,
) {
    let mut store = Store::open(path).expect("open");
    let err = import_json(&mut store, bytes).expect_err("rejected");
    assert!(pred(&err), "{err}");
    drop(store);
    assert_eq!(support::bytes(path), before);
}

#[test]
fn t12_late_dangling_reference_rolls_back_and_batch_references_work() {
    let (scratch, mut store) = created("t12.sqlite");
    let kept = store
        .create_page(Page {
            id: id_page("22222222-2222-4222-8222-222222222220"),
            title: "Queda".to_string(),
            body: "intacta".to_string(),
        })
        .expect("kept");
    let broken = serde_json::to_vec(&serde_json::json!({
        "format_version": 1,
        "universe_id": universe().as_str(),
        "pages": [{"id": "22222222-2222-4222-8222-222222222221", "title": "Nueva", "body": "x"}],
        "types": [{"id": "33333333-3333-4333-8333-333333333333", "label": "Personaje"}],
        "entities": [{
            "id": "55555555-5555-4555-8555-555555555555",
            "label": "Narrador sintético",
            "type_id": "33333333-3333-4333-8333-333333333333",
            "page_id": null
        }],
        "definitions": [{
            "id": "77777777-7777-4777-8777-777777777777",
            "owner": {"type": "type", "id": "33333333-3333-4333-8333-333333333333"},
            "value_class": "reference",
            "cardinality": "singular",
            "archived": true
        }],
        "assertions": [{
            "id": "88888888-8888-4888-8888-888888888881",
            "subject_id": "55555555-5555-4555-8555-555555555555",
            "definition_id": "77777777-7777-4777-8777-777777777777",
            "value": {"kind": "reference", "target_id": "66666666-6666-4666-8666-666666666666"},
            "canon": "ACORDADO",
            "provenance": {
                "external_ref": "source-a",
                "locator": null,
                "fragment": null,
                "source_revision": null
            },
            "scope_note": "al inicio",
            "archived": true
        }]
    }))
    .expect("json");
    let err = import_json(&mut store, &broken).expect_err("dangling");
    match err {
        Error::Validation {
            reason: ValidationReason::UnknownTarget,
            field: "target_entity_id",
            id: Some(id),
        } => assert_eq!(id, "66666666-6666-4666-8666-666666666666"),
        other => panic!("expected unknown target, got {other}"),
    }
    drop(store);
    let mut store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.page(&kept.id).expect("kept"), kept);
    match store
        .page(&id_page("22222222-2222-4222-8222-222222222221"))
        .expect_err("new page")
    {
        Error::Validation {
            reason: ValidationReason::NotFound,
            ..
        } => {}
        other => panic!("expected not found, got {other}"),
    }
    let valid = serde_json::to_vec(&serde_json::json!({
        "format_version": 1,
        "universe_id": universe().as_str(),
        "pages": [],
        "types": [{"id": "33333333-3333-4333-8333-333333333333", "label": "Personaje"}],
        "entities": [
            {
                "id": "55555555-5555-4555-8555-555555555555",
                "label": "Narrador sintético",
                "type_id": "33333333-3333-4333-8333-333333333333",
                "page_id": null
            },
            {
                "id": "66666666-6666-4666-8666-666666666666",
                "label": "Casa sintética",
                "type_id": null,
                "page_id": null
            }
        ],
        "definitions": [{
            "id": "77777777-7777-4777-8777-777777777777",
            "owner": {"type": "entity", "id": "55555555-5555-4555-8555-555555555555"},
            "value_class": "reference",
            "cardinality": "singular",
            "archived": true
        }],
        "assertions": [{
            "id": "88888888-8888-4888-8888-888888888881",
            "subject_id": "55555555-5555-4555-8555-555555555555",
            "definition_id": "77777777-7777-4777-8777-777777777777",
            "value": {"kind": "reference", "target_id": "66666666-6666-4666-8666-666666666666"},
            "canon": "PROVISIONAL",
            "source_status_raw": "ETIQUETA LITERAL",
            "provenance": {
                "external_ref": "source-a",
                "locator": "",
                "fragment": null,
                "source_revision": null
            },
            "scope_note": "al inicio",
            "archived": true
        }]
    }))
    .expect("valid json");
    import_json(&mut store, &valid).expect("batch reference");
    drop(store);
    let mut store = Store::open(&scratch.path).expect("reopen valid");
    let assertion = store
        .assertion(&id_assertion("88888888-8888-4888-8888-888888888881"))
        .expect("assertion");
    assert_eq!(
        assertion.value,
        Value::Reference(id_entity("66666666-6666-4666-8666-666666666666"))
    );
    assert!(assertion.archived);
    assert_eq!(assertion.canon, Canon::Provisional);
    assert_eq!(assertion.scope_note.as_deref(), Some("al inicio"));
    assert_eq!(assertion.provenance.locator.as_deref(), Some(""));
    assert_eq!(
        assertion.provenance.external_ref.as_deref(),
        Some("source-a")
    );
    assert!(
        store
            .definition(&id_definition("77777777-7777-4777-8777-777777777777"))
            .expect("definition")
            .archived
    );
    let err = store
        .create_assertion(Assertion {
            id: id_assertion("88888888-8888-4888-8888-888888888882"),
            subject_id: id_entity("55555555-5555-4555-8555-555555555555"),
            definition_id: id_definition("77777777-7777-4777-8777-777777777777"),
            value: Value::Reference(id_entity("66666666-6666-4666-8666-666666666666")),
            canon: Canon::Abierto,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        })
        .expect_err("live create");
    assert!(matches!(
        err,
        Error::Validation {
            reason: ValidationReason::ArchivedDefinition,
            ..
        }
    ));
}

#[test]
fn import_new_refuses_an_existing_path_and_a_failed_batch_is_not_a_universe() {
    let scratch = scratch("import-new-existing.sqlite");
    std::fs::write(&scratch.path, b"keep-me").expect("write");
    let before = bytes(&scratch.path);
    let document = br#"{"format_version":1,"universe_id":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa","pages":[],"types":[],"entities":[],"definitions":[],"assertions":[]}"#;
    let err = import_new_json(&scratch.path, document).expect_err("exists");
    assert!(matches!(
        err,
        Error::Validation {
            reason: ValidationReason::PathExists,
            ..
        }
    ));
    assert_eq!(bytes(&scratch.path), before);
    let broken = scratch.path.with_file_name("import-new-broken.sqlite");
    let bad = serde_json::to_vec(&serde_json::json!({
        "format_version": 1,
        "universe_id": universe().as_str(),
        "pages": [{"id": "22222222-2222-4222-8222-222222222221", "title": "Nueva", "body": "x"}],
        "types": [],
        "entities": [],
        "definitions": [],
        "assertions": [{
            "id": "88888888-8888-4888-8888-888888888881",
            "subject_id": "55555555-5555-4555-8555-555555555555",
            "definition_id": "77777777-7777-4777-8777-777777777777",
            "value": {"kind": "text", "text": "hola"},
            "canon": "ABIERTO",
            "provenance": {},
            "archived": false
        }]
    }))
    .expect("json");
    let err = import_new_json(&broken, &bad).expect_err("batch");
    assert!(
        matches!(err, Error::Validation { .. } | Error::Io { .. }),
        "{err}"
    );
    if broken.exists() {
        let open_err = Store::open(&broken).expect_err("residual open");
        assert!(
            matches!(
                open_err,
                Error::UnsupportedVersion { .. } | Error::Validation { .. }
            ),
            "{open_err}"
        );
    }
}
