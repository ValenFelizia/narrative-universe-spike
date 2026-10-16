#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use narrative_universe::{
    Assertion, AssertionId, Canon, Cardinality, Definition, DefinitionId, Entity, EntityId,
    EntityType, Error, Owner, PossibleIncompatibility, Provenance, Store, TypeId, UniverseId,
    ValidationReason, Value, ValueClass,
};
use support::scratch;

fn universe() -> UniverseId {
    UniverseId::parse("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").expect("universe")
}

fn created(name: &str) -> (support::Scratch, Store) {
    let scratch = scratch(name);
    let store = Store::create(&scratch.path, universe()).expect("create");
    (scratch, store)
}

fn world(store: &mut Store) -> (Entity, Definition) {
    let entity_type = store
        .create_entity_type(EntityType {
            id: TypeId::parse("33333333-3333-4333-8333-333333333333").expect("type"),
            label: "Personaje".to_string(),
        })
        .expect("type");
    let entity = store
        .create_entity(Entity {
            id: EntityId::parse("55555555-5555-4555-8555-555555555555").expect("entity"),
            label: "Narrador sintético".to_string(),
            type_id: Some(entity_type.id.clone()),
            page_id: None,
        })
        .expect("entity");
    let definition = store
        .create_definition(Definition {
            id: DefinitionId::parse("77777777-7777-4777-8777-777777777777").expect("definition"),
            owner: Owner::Type(entity_type.id),
            value_class: ValueClass::Text,
            cardinality: Cardinality::Singular,
            archived: false,
        })
        .expect("definition");
    (entity, definition)
}

#[allow(clippy::too_many_arguments)]
fn assertion(
    id: &str,
    subject: &EntityId,
    definition: &DefinitionId,
    text: &str,
    canon: Canon,
    source: &str,
    scope: &str,
    external_ref: &str,
) -> Assertion {
    Assertion {
        id: AssertionId::parse(id).expect("assertion"),
        subject_id: subject.clone(),
        definition_id: definition.clone(),
        value: Value::Text(text.to_string()),
        canon,
        source_status_raw: Some(source.to_string()),
        provenance: Provenance {
            external_ref: Some(external_ref.to_string()),
            locator: Some("section-1".to_string()),
            fragment: Some(text.to_string()),
            source_revision: Some("rev-1".to_string()),
        },
        scope_note: Some(scope.to_string()),
        archived: false,
    }
}

#[test]
fn t05_promoting_one_hypothesis_keeps_the_other() {
    let (scratch, mut store) = created("t05.sqlite");
    let (subject, definition) = world(&mut store);
    let first = assertion(
        "88888888-8888-4888-8888-888888888881",
        &subject.id,
        &definition.id,
        "una",
        Canon::Provisional,
        "FUENTE A",
        "al inicio",
        "source-a",
    );
    let second = assertion(
        "88888888-8888-4888-8888-888888888882",
        &subject.id,
        &definition.id,
        "otra",
        Canon::Abierto,
        "FUENTE B",
        "después",
        "source-b",
    );
    store.create_assertion(first.clone()).expect("first");
    store.create_assertion(second.clone()).expect("second");
    let promoted = store
        .set_canon(&first.id, Canon::Acordado)
        .expect("promote");
    assert_eq!(promoted.id, first.id);
    assert_eq!(promoted.value, Value::Text("una".to_string()));
    assert_eq!(promoted.canon, Canon::Acordado);
    assert_eq!(promoted.source_status_raw.as_deref(), Some("FUENTE A"));
    assert_eq!(promoted.scope_note.as_deref(), Some("al inicio"));
    assert_eq!(promoted.provenance, first.provenance);
    assert!(!promoted.archived);
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen");
    let stored_first = store.assertion(&first.id).expect("first");
    let stored_second = store.assertion(&second.id).expect("second");
    assert_eq!(stored_first.canon, Canon::Acordado);
    assert_eq!(stored_first.value, first.value);
    assert_eq!(stored_first.source_status_raw, first.source_status_raw);
    assert_eq!(stored_first.scope_note, first.scope_note);
    assert_eq!(stored_first.provenance, first.provenance);
    assert_eq!(stored_second, second);
}

#[test]
fn t06_possible_incompatibilities_return_only_the_active_pair() {
    let (scratch, mut store) = created("t06-pair.sqlite");
    let (subject, definition) = world(&mut store);
    let first = assertion(
        "88888888-8888-4888-8888-888888888881",
        &subject.id,
        &definition.id,
        "una",
        Canon::Acordado,
        "FUENTE A",
        "al inicio",
        "source-a",
    );
    let second = assertion(
        "88888888-8888-4888-8888-888888888882",
        &subject.id,
        &definition.id,
        "otra",
        Canon::Acordado,
        "FUENTE B",
        "después",
        "source-b",
    );
    store.create_assertion(first.clone()).expect("first");
    store.create_assertion(second.clone()).expect("second");
    let before = store.counts().expect("counts");
    assert_eq!(
        store.possible_incompatibilities().expect("pairs"),
        vec![PossibleIncompatibility {
            first: first.clone(),
            second: second.clone(),
        }]
    );
    assert_eq!(store.counts().expect("counts"), before);
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(
        store.possible_incompatibilities().expect("pairs"),
        vec![PossibleIncompatibility { first, second }]
    );
}

#[test]
fn t06_identical_values_are_not_a_pair() {
    let (_scratch, mut store) = created("t06-same.sqlite");
    let (subject, definition) = world(&mut store);
    store
        .create_assertion(assertion(
            "88888888-8888-4888-8888-888888888881",
            &subject.id,
            &definition.id,
            "una",
            Canon::Acordado,
            "FUENTE A",
            "al inicio",
            "source-a",
        ))
        .expect("first");
    store
        .create_assertion(assertion(
            "88888888-8888-4888-8888-888888888882",
            &subject.id,
            &definition.id,
            "una",
            Canon::Acordado,
            "FUENTE B",
            "después",
            "source-b",
        ))
        .expect("second");
    assert_eq!(
        store.possible_incompatibilities().expect("pairs"),
        Vec::<PossibleIncompatibility>::new()
    );
}

#[test]
fn t06_unknown_is_not_a_known_conflict() {
    let (_scratch, mut store) = created("t06-unknown.sqlite");
    let (subject, definition) = world(&mut store);
    let known = assertion(
        "88888888-8888-4888-8888-888888888881",
        &subject.id,
        &definition.id,
        "una",
        Canon::Acordado,
        "FUENTE A",
        "al inicio",
        "source-a",
    );
    let mut unknown = known.clone();
    unknown.id = AssertionId::parse("88888888-8888-4888-8888-888888888882").expect("id");
    unknown.value = Value::Unknown;
    unknown.source_status_raw = Some("FUENTE B".to_string());
    store.create_assertion(known).expect("known");
    store.create_assertion(unknown).expect("unknown");
    assert_eq!(
        store.possible_incompatibilities().expect("pairs"),
        Vec::<PossibleIncompatibility>::new()
    );
}

#[test]
fn t06_multiple_cardinality_does_not_conflict() {
    let (_scratch, mut store) = created("t06-multiple.sqlite");
    let entity_type = store
        .create_entity_type(EntityType {
            id: TypeId::parse("33333333-3333-4333-8333-333333333333").expect("type"),
            label: "Personaje".to_string(),
        })
        .expect("type");
    let subject = store
        .create_entity(Entity {
            id: EntityId::parse("55555555-5555-4555-8555-555555555555").expect("entity"),
            label: "Narrador sintético".to_string(),
            type_id: Some(entity_type.id.clone()),
            page_id: None,
        })
        .expect("entity");
    let definition = store
        .create_definition(Definition {
            id: DefinitionId::parse("77777777-7777-4777-8777-777777777777").expect("definition"),
            owner: Owner::Type(entity_type.id),
            value_class: ValueClass::Text,
            cardinality: Cardinality::Multiple,
            archived: false,
        })
        .expect("definition");
    store
        .create_assertion(assertion(
            "88888888-8888-4888-8888-888888888881",
            &subject.id,
            &definition.id,
            "una",
            Canon::Acordado,
            "FUENTE A",
            "al inicio",
            "source-a",
        ))
        .expect("first");
    store
        .create_assertion(assertion(
            "88888888-8888-4888-8888-888888888882",
            &subject.id,
            &definition.id,
            "otra",
            Canon::Acordado,
            "FUENTE B",
            "después",
            "source-b",
        ))
        .expect("second");
    assert_eq!(
        store.possible_incompatibilities().expect("pairs"),
        Vec::<PossibleIncompatibility>::new()
    );
}

#[test]
fn t06_archived_assertion_or_definition_is_omitted() {
    let (_scratch, mut store) = created("t06-archived.sqlite");
    let (subject, definition) = world(&mut store);
    let first = assertion(
        "88888888-8888-4888-8888-888888888881",
        &subject.id,
        &definition.id,
        "una",
        Canon::Acordado,
        "FUENTE A",
        "al inicio",
        "source-a",
    );
    let second = assertion(
        "88888888-8888-4888-8888-888888888882",
        &subject.id,
        &definition.id,
        "otra",
        Canon::Acordado,
        "FUENTE B",
        "después",
        "source-b",
    );
    store.create_assertion(first.clone()).expect("first");
    store.create_assertion(second).expect("second");
    store
        .archive_assertion(&first.id)
        .expect("archive assertion");
    assert_eq!(
        store
            .possible_incompatibilities()
            .expect("archived assertion"),
        Vec::<PossibleIncompatibility>::new()
    );
    store
        .restore_assertion(&first.id)
        .expect("restore assertion");
    store
        .archive_definition(&definition.id)
        .expect("archive definition");
    assert_eq!(
        store
            .possible_incompatibilities()
            .expect("archived definition"),
        Vec::<PossibleIncompatibility>::new()
    );
}

#[test]
fn t07_archive_and_restore_keep_canon_and_provenance() {
    let (scratch, mut store) = created("t07-roundtrip.sqlite");
    let (subject, definition) = world(&mut store);
    let original = assertion(
        "88888888-8888-4888-8888-888888888881",
        &subject.id,
        &definition.id,
        "una",
        Canon::Provisional,
        "FUENTE A",
        "al inicio",
        "source-a",
    );
    store.create_assertion(original.clone()).expect("assertion");
    let archived_definition = store
        .archive_definition(&definition.id)
        .expect("archive definition");
    let archived_again = store
        .archive_definition(&definition.id)
        .expect("archive definition again");
    let archived_assertion = store.archive_assertion(&original.id).expect("archive");
    let archived_assertion_again = store
        .archive_assertion(&original.id)
        .expect("archive again");
    assert!(archived_definition.archived);
    assert_eq!(archived_again, archived_definition);
    assert!(archived_assertion.archived);
    assert_eq!(archived_assertion_again, archived_assertion);
    assert_eq!(archived_assertion.canon, Canon::Provisional);
    assert_eq!(archived_assertion.value, original.value);
    assert_eq!(archived_assertion.provenance, original.provenance);
    assert_eq!(archived_assertion.scope_note, original.scope_note);
    assert_eq!(
        archived_assertion.source_status_raw,
        original.source_status_raw
    );
    drop(store);

    let mut store = Store::open(&scratch.path).expect("reopen");
    let stored = store.assertion(&original.id).expect("assertion");
    assert_eq!(stored, archived_assertion);
    assert!(
        store
            .definition(&definition.id)
            .expect("definition")
            .archived
    );
    let restored_definition = store
        .restore_definition(&definition.id)
        .expect("restore definition");
    let restored_definition_again = store
        .restore_definition(&definition.id)
        .expect("restore definition again");
    let restored = store.restore_assertion(&original.id).expect("restore");
    let restored_again = store
        .restore_assertion(&original.id)
        .expect("restore again");
    assert!(!restored_definition.archived);
    assert_eq!(restored_definition_again, restored_definition);
    assert!(!restored.archived);
    assert_eq!(restored_again, restored);
    assert_eq!(restored.canon, Canon::Provisional);
    assert_eq!(restored.provenance, original.provenance);
    assert_eq!(restored.value, original.value);
    drop(store);

    let store = Store::open(&scratch.path).expect("reopen again");
    assert_eq!(store.assertion(&original.id).expect("assertion"), restored);
}

#[test]
fn t07_restored_assertion_stays_out_while_definition_is_archived() {
    let (scratch, mut store) = created("t07-definition-archived.sqlite");
    let (subject, definition) = world(&mut store);
    let first = assertion(
        "88888888-8888-4888-8888-888888888881",
        &subject.id,
        &definition.id,
        "una",
        Canon::Acordado,
        "FUENTE A",
        "al inicio",
        "source-a",
    );
    let second = assertion(
        "88888888-8888-4888-8888-888888888882",
        &subject.id,
        &definition.id,
        "otra",
        Canon::Acordado,
        "FUENTE B",
        "después",
        "source-b",
    );
    store.create_assertion(first.clone()).expect("first");
    store.create_assertion(second.clone()).expect("second");
    store
        .archive_definition(&definition.id)
        .expect("archive definition");
    store
        .archive_assertion(&first.id)
        .expect("archive assertion");
    let restored = store.restore_assertion(&first.id).expect("restore");
    let stored_definition = store.definition(&definition.id).expect("definition");
    assert!(stored_definition.archived);
    assert!(!restored.archived);
    assert_eq!(restored.canon, Canon::Acordado);
    assert_eq!(restored.provenance, first.provenance);
    assert_eq!(restored.value, first.value);
    assert_eq!(
        store.possible_incompatibilities().expect("pairs"),
        Vec::<PossibleIncompatibility>::new()
    );
    let before = store.counts().expect("counts");
    let rejected_id = AssertionId::parse("88888888-8888-4888-8888-888888888883").expect("rejected");
    let err = store
        .create_assertion(Assertion {
            id: rejected_id.clone(),
            subject_id: subject.id.clone(),
            definition_id: definition.id.clone(),
            value: Value::Text("tercera".to_string()),
            canon: Canon::Abierto,
            source_status_raw: Some("FUENTE C".to_string()),
            provenance: Provenance::absent(),
            scope_note: Some("al inicio".to_string()),
            archived: false,
        })
        .expect_err("new assertion");
    match err {
        Error::Validation {
            reason: ValidationReason::ArchivedDefinition,
            field: "definition_id",
            id: Some(id),
        } => assert_eq!(id, definition.id.as_str()),
        other => panic!("expected archived definition, got {other}"),
    }
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.counts().expect("counts"), before);
    match store.assertion(&rejected_id).expect_err("missing") {
        Error::Validation {
            reason: ValidationReason::NotFound,
            ..
        } => {}
        other => panic!("expected not found, got {other}"),
    }
    assert!(
        store
            .definition(&definition.id)
            .expect("definition")
            .archived
    );
    assert!(!store.assertion(&first.id).expect("first").archived);
    assert_eq!(store.assertion(&second.id).expect("second"), second);
}
