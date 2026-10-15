#[path = "support/mod.rs"]
mod support;

use narrative_universe::{
    Assertion, AssertionId, Canon, Cardinality, Counts, Definition, DefinitionId, Entity, EntityId,
    EntityType, Error, Owner, Page, PageId, Provenance, Store, TypeId, UniverseId,
    ValidationReason, Value, ValueClass,
};
use rusqlite::Connection;
use support::{bytes, scratch};

fn universe() -> UniverseId {
    UniverseId::parse("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").expect("universe")
}

fn created(name: &str) -> (support::Scratch, Store) {
    let scratch = scratch(name);
    let store = Store::create(&scratch.path, universe()).expect("create");
    (scratch, store)
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

fn user_version(path: &std::path::Path) -> i32 {
    let bytes = bytes(path);
    i32::from_be_bytes(bytes[60..64].try_into().expect("header"))
}

#[test]
fn create_reopen_reads_empty_universe_and_schema_version() {
    let (scratch, store) = created("create.sqlite");
    assert_eq!(store.universe_id(), &universe());
    assert_eq!(
        store.counts().expect("counts"),
        Counts {
            pages: 0,
            types: 0,
            entities: 0,
            definitions: 0,
            assertions: 0,
        }
    );
    drop(store);
    assert_eq!(user_version(&scratch.path), 1);
    let store = Store::open(&scratch.path).expect("open");
    assert_eq!(store.universe_id().as_str(), universe().as_str());
    let conn = Connection::open(&scratch.path).expect("raw");
    let mut names = Vec::new();
    let mut statement = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .expect("prepare");
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query");
    for row in rows {
        names.push(row.expect("name"));
    }
    assert_eq!(
        names,
        vec![
            "assertions",
            "definitions",
            "entities",
            "metadata",
            "pages",
            "types"
        ]
    );
}

#[test]
fn open_missing_file_does_not_create_it() {
    let scratch = scratch("missing.sqlite");
    let err = Store::open(&scratch.path).expect_err("missing");
    match err {
        Error::Validation {
            reason: ValidationReason::PathMissing,
            field: "path",
            id: None,
        } => {}
        other => panic!("expected path missing, got {other}"),
    }
    assert!(!scratch.path.exists());
}

#[test]
fn create_refuses_existing_bytes() {
    let scratch = scratch("existing.sqlite");
    std::fs::write(&scratch.path, b"keep-me").expect("write");
    let before = bytes(&scratch.path);
    let err = Store::create(&scratch.path, universe()).expect_err("exists");
    match err {
        Error::Validation {
            reason: ValidationReason::PathExists,
            field: "path",
            id: None,
        } => {}
        other => panic!("expected path exists, got {other}"),
    }
    assert_eq!(bytes(&scratch.path), before);
}

#[test]
fn create_parent_directory_missing_leaves_no_file() {
    let scratch = scratch("nested");
    let path = scratch.path.join("missing").join("universe.sqlite");
    let err = Store::create(&path, universe()).expect_err("missing parent");
    assert!(
        matches!(err, Error::Storage { .. } | Error::Io { .. }),
        "{err}"
    );
    assert!(!path.exists());
}

#[test]
fn foreign_text_file_is_rejected_without_changes() {
    let scratch = scratch("text.sqlite");
    std::fs::write(&scratch.path, b"not a database").expect("write");
    let before = bytes(&scratch.path);
    let err = Store::open(&scratch.path).expect_err("text");
    match err {
        Error::Validation {
            reason: ValidationReason::NotADatabase,
            field: "path",
            id: None,
        } => {}
        other => panic!("expected not a database, got {other}"),
    }
    assert_eq!(bytes(&scratch.path), before);
}

#[test]
fn open_directory_is_not_a_database() {
    let scratch = scratch("as-directory");
    std::fs::create_dir(&scratch.path).expect("dir");
    let err = Store::open(&scratch.path).expect_err("directory");
    match err {
        Error::Validation {
            reason: ValidationReason::NotADatabase,
            field: "path",
            id: None,
        } => {}
        other => panic!("expected not a database, got {other}"),
    }
    assert!(scratch.path.is_dir());
}

#[test]
fn unknown_user_version_is_rejected_without_changes() {
    let scratch = scratch("version.sqlite");
    let conn = Connection::open(&scratch.path).expect("create foreign");
    conn.execute_batch("CREATE TABLE other (id INTEGER); PRAGMA user_version = 7;")
        .expect("foreign schema");
    drop(conn);
    let before = bytes(&scratch.path);
    let err = Store::open(&scratch.path).expect_err("version");
    match err {
        Error::UnsupportedVersion { found: 7 } => {}
        other => panic!("expected version 7, got {other}"),
    }
    assert_eq!(bytes(&scratch.path), before);
    assert_eq!(user_version(&scratch.path), 7);
}

#[test]
fn version_one_without_metadata_is_rejected_without_changes() {
    let scratch = scratch("foreign.sqlite");
    let conn = Connection::open(&scratch.path).expect("create foreign");
    conn.execute_batch("CREATE TABLE other (id INTEGER); PRAGMA user_version = 1;")
        .expect("foreign schema");
    drop(conn);
    let before = bytes(&scratch.path);
    let err = Store::open(&scratch.path).expect_err("foreign");
    match err {
        Error::Validation {
            reason: ValidationReason::ForeignDatabase,
            ..
        } => {}
        other => panic!("expected foreign database, got {other}"),
    }
    assert_eq!(bytes(&scratch.path), before);
}

#[test]
fn invalid_ids_are_rejected() {
    let err = PageId::parse("not-a-uuid").expect_err("invalid");
    assert_validation(err, ValidationReason::InvalidId, "id", "not-a-uuid");
    let upper = "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA";
    let err = PageId::parse(upper).expect_err("upper");
    assert_validation(err, ValidationReason::InvalidId, "id", upper);
    let bare = "aaaaaaaaaaaa4aaa8aaaaaaaaaaaaaaa";
    let err = EntityId::parse(bare).expect_err("bare");
    assert_validation(err, ValidationReason::InvalidId, "id", bare);
}

#[test]
fn t01_free_page_and_incomplete_entity_reopen() {
    let (scratch, mut store) = created("t01.sqlite");
    let page = Page {
        id: PageId::parse("22222222-2222-4222-8222-222222222222").expect("page"),
        title: "Página libre".to_string(),
        body: "Texto con acento áéíóú ñ y emoji 📚".to_string(),
    };
    assert_eq!(store.create_page(page.clone()).expect("page"), page);
    let entity = Entity {
        id: EntityId::parse("55555555-5555-4555-8555-555555555555").expect("entity"),
        label: "Ficha incompleta".to_string(),
        type_id: None,
        page_id: None,
    };
    assert_eq!(store.create_entity(entity.clone()).expect("entity"), entity);
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.page(&page.id).expect("page"), page);
    assert_eq!(store.entity(&entity.id).expect("entity"), entity);
    assert_eq!(
        store.counts().expect("counts"),
        Counts {
            pages: 1,
            types: 0,
            entities: 1,
            definitions: 0,
            assertions: 0,
        }
    );
}

#[test]
fn duplicate_page_keeps_the_first_body() {
    let (scratch, mut store) = created("duplicate.sqlite");
    let id = PageId::parse("22222222-2222-4222-8222-222222222222").expect("page");
    store
        .create_page(Page {
            id: id.clone(),
            title: "Primera".to_string(),
            body: "cuerpo".to_string(),
        })
        .expect("first");
    let err = store
        .create_page(Page {
            id: id.clone(),
            title: "Segunda".to_string(),
            body: "otro".to_string(),
        })
        .expect_err("duplicate");
    assert_validation(err, ValidationReason::DuplicateId, "id", id.as_str());
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.page(&id).expect("page").body, "cuerpo");
}

#[test]
fn entity_type_and_page_roundtrip() {
    let (scratch, mut store) = created("linked.sqlite");
    let page = store
        .create_page(Page {
            id: PageId::parse("22222222-2222-4222-8222-222222222222").expect("page"),
            title: "Ficha".to_string(),
            body: "".to_string(),
        })
        .expect("page");
    let entity_type = store
        .create_entity_type(EntityType {
            id: TypeId::parse("33333333-3333-4333-8333-333333333333").expect("type"),
            label: "Personaje".to_string(),
        })
        .expect("type");
    let entity = Entity {
        id: EntityId::parse("55555555-5555-4555-8555-555555555555").expect("entity"),
        label: "Con ficha".to_string(),
        type_id: Some(entity_type.id.clone()),
        page_id: Some(page.id.clone()),
    };
    assert_eq!(store.create_entity(entity.clone()).expect("entity"), entity);
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.entity(&entity.id).expect("entity"), entity);
    assert_eq!(store.page(&page.id).expect("page").body, "");
}

fn person_type(store: &mut Store) -> EntityType {
    store
        .create_entity_type(EntityType {
            id: TypeId::parse("33333333-3333-4333-8333-333333333333").expect("type"),
            label: "Personaje".to_string(),
        })
        .expect("type")
}

fn house_type(store: &mut Store) -> EntityType {
    store
        .create_entity_type(EntityType {
            id: TypeId::parse("44444444-4444-4444-8444-444444444444").expect("type"),
            label: "Casa".to_string(),
        })
        .expect("type")
}

fn narrator(store: &mut Store, type_id: Option<TypeId>) -> Entity {
    let entity = Entity {
        id: EntityId::parse("55555555-5555-4555-8555-555555555555").expect("entity"),
        label: "Narrador sintético".to_string(),
        type_id,
        page_id: None,
    };
    store.create_entity(entity).expect("entity")
}

#[test]
fn t03_unknown_subject_persists_nothing() {
    let (scratch, mut store) = created("t03-subject.sqlite");
    let entity_type = person_type(&mut store);
    let definition = store
        .create_definition(Definition {
            id: DefinitionId::parse("77777777-7777-4777-8777-777777777777").expect("definition"),
            owner: Owner::Type(entity_type.id),
            value_class: ValueClass::Text,
            cardinality: Cardinality::Singular,
            archived: false,
        })
        .expect("definition");
    let before = store.counts().expect("before");
    let assertion_id =
        AssertionId::parse("88888888-8888-4888-8888-888888888888").expect("assertion");
    let subject = EntityId::parse("55555555-5555-4555-8555-555555555555").expect("subject");
    let err = store
        .create_assertion(Assertion {
            id: assertion_id.clone(),
            subject_id: subject.clone(),
            definition_id: definition.id,
            value: Value::Text("ausente".to_string()),
            canon: Canon::Abierto,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        })
        .expect_err("subject");
    assert_validation(
        err,
        ValidationReason::UnknownSubject,
        "subject_id",
        subject.as_str(),
    );
    assert_eq!(store.counts().expect("after"), before);
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.counts().expect("reopen counts"), before);
    match store
        .assertion(&assertion_id)
        .expect_err("missing assertion")
    {
        Error::Validation {
            reason: ValidationReason::NotFound,
            field: "id",
            id: Some(id),
        } => assert_eq!(id, assertion_id.as_str()),
        other => panic!("expected not found, got {other}"),
    }
}

#[test]
fn t03_unknown_target_persists_nothing() {
    let (scratch, mut store) = created("t03-target.sqlite");
    let entity_type = person_type(&mut store);
    let subject = narrator(&mut store, Some(entity_type.id.clone()));
    let definition = store
        .create_definition(Definition {
            id: DefinitionId::parse("77777777-7777-4777-8777-777777777777").expect("definition"),
            owner: Owner::Type(entity_type.id),
            value_class: ValueClass::Reference,
            cardinality: Cardinality::Singular,
            archived: false,
        })
        .expect("definition");
    let before = store.counts().expect("before");
    let target = EntityId::parse("66666666-6666-4666-8666-666666666666").expect("target");
    let assertion_id =
        AssertionId::parse("88888888-8888-4888-8888-888888888888").expect("assertion");
    let err = store
        .create_assertion(Assertion {
            id: assertion_id.clone(),
            subject_id: subject.id,
            definition_id: definition.id,
            value: Value::Reference(target.clone()),
            canon: Canon::Provisional,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        })
        .expect_err("target");
    assert_validation(
        err,
        ValidationReason::UnknownTarget,
        "target_entity_id",
        target.as_str(),
    );
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.counts().expect("counts"), before);
    assert!(matches!(
        store.assertion(&assertion_id).expect_err("missing"),
        Error::Validation {
            reason: ValidationReason::NotFound,
            ..
        }
    ));
}

#[test]
fn t03_definition_owned_by_other_entity_persists_nothing() {
    let (scratch, mut store) = created("t03-owner-entity.sqlite");
    let subject = narrator(&mut store, None);
    let other = store
        .create_entity(Entity {
            id: EntityId::parse("66666666-6666-4666-8666-666666666666").expect("other"),
            label: "Otra".to_string(),
            type_id: None,
            page_id: None,
        })
        .expect("other");
    let definition = store
        .create_definition(Definition {
            id: DefinitionId::parse("77777777-7777-4777-8777-777777777777").expect("definition"),
            owner: Owner::Entity(other.id),
            value_class: ValueClass::Boolean,
            cardinality: Cardinality::Singular,
            archived: false,
        })
        .expect("definition");
    let before = store.counts().expect("before");
    let err = store
        .create_assertion(Assertion {
            id: AssertionId::parse("88888888-8888-4888-8888-888888888888").expect("assertion"),
            subject_id: subject.id,
            definition_id: definition.id.clone(),
            value: Value::Boolean(true),
            canon: Canon::Acordado,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        })
        .expect_err("owner");
    assert_validation(
        err,
        ValidationReason::NotApplicable,
        "definition_id",
        definition.id.as_str(),
    );
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.counts().expect("counts"), before);
}

#[test]
fn t03_definition_owned_by_other_type_persists_nothing() {
    let (scratch, mut store) = created("t03-owner-type.sqlite");
    let character = person_type(&mut store);
    let house = house_type(&mut store);
    let subject = narrator(&mut store, Some(character.id));
    let definition = store
        .create_definition(Definition {
            id: DefinitionId::parse("77777777-7777-4777-8777-777777777777").expect("definition"),
            owner: Owner::Type(house.id),
            value_class: ValueClass::Text,
            cardinality: Cardinality::Singular,
            archived: false,
        })
        .expect("definition");
    let before = store.counts().expect("before");
    let err = store
        .create_assertion(Assertion {
            id: AssertionId::parse("88888888-8888-4888-8888-888888888888").expect("assertion"),
            subject_id: subject.id,
            definition_id: definition.id.clone(),
            value: Value::Text("no aplica".to_string()),
            canon: Canon::MisterioIntencional,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        })
        .expect_err("type");
    assert_validation(
        err,
        ValidationReason::NotApplicable,
        "definition_id",
        definition.id.as_str(),
    );
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.counts().expect("counts"), before);
}

#[test]
fn unknown_type_and_page_persist_no_entity() {
    let (scratch, mut store) = created("unknown-links.sqlite");
    let before = store.counts().expect("before");
    let type_id = TypeId::parse("33333333-3333-4333-8333-333333333333").expect("type");
    let entity_id = EntityId::parse("55555555-5555-4555-8555-555555555555").expect("entity");
    let err = store
        .create_entity(Entity {
            id: entity_id.clone(),
            label: "Sin tipo".to_string(),
            type_id: Some(type_id.clone()),
            page_id: None,
        })
        .expect_err("type");
    assert_validation(
        err,
        ValidationReason::UnknownType,
        "type_id",
        type_id.as_str(),
    );
    let page_id = PageId::parse("22222222-2222-4222-8222-222222222222").expect("page");
    let err = store
        .create_entity(Entity {
            id: entity_id.clone(),
            label: "Sin página".to_string(),
            type_id: None,
            page_id: Some(page_id.clone()),
        })
        .expect_err("page");
    assert_validation(
        err,
        ValidationReason::UnknownPage,
        "page_id",
        page_id.as_str(),
    );
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.counts().expect("counts"), before);
    assert!(matches!(
        store.entity(&entity_id).expect_err("entity"),
        Error::Validation {
            reason: ValidationReason::NotFound,
            ..
        }
    ));
}

#[test]
fn t04_absence_unknown_and_false_stay_distinct() {
    let (scratch, mut store) = created("t04.sqlite");
    let entity_type = person_type(&mut store);
    let subject = narrator(&mut store, Some(entity_type.id.clone()));
    let absent = store
        .create_definition(Definition {
            id: DefinitionId::parse("77777777-7777-4777-8777-777777777771").expect("absent"),
            owner: Owner::Type(entity_type.id.clone()),
            value_class: ValueClass::Boolean,
            cardinality: Cardinality::Singular,
            archived: false,
        })
        .expect("absent");
    let unknown_definition = store
        .create_definition(Definition {
            id: DefinitionId::parse("77777777-7777-4777-8777-777777777772").expect("unknown"),
            owner: Owner::Type(entity_type.id.clone()),
            value_class: ValueClass::Boolean,
            cardinality: Cardinality::Singular,
            archived: false,
        })
        .expect("unknown definition");
    let false_definition = store
        .create_definition(Definition {
            id: DefinitionId::parse("77777777-7777-4777-8777-777777777773").expect("false"),
            owner: Owner::Type(entity_type.id.clone()),
            value_class: ValueClass::Boolean,
            cardinality: Cardinality::Singular,
            archived: false,
        })
        .expect("false definition");
    let unknown = store
        .create_assertion(Assertion {
            id: AssertionId::parse("88888888-8888-4888-8888-888888888881").expect("unknown"),
            subject_id: subject.id.clone(),
            definition_id: unknown_definition.id.clone(),
            value: Value::Unknown,
            canon: Canon::Abierto,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        })
        .expect("unknown");
    let falsy = store
        .create_assertion(Assertion {
            id: AssertionId::parse("88888888-8888-4888-8888-888888888882").expect("false"),
            subject_id: subject.id.clone(),
            definition_id: false_definition.id.clone(),
            value: Value::Boolean(false),
            canon: Canon::Provisional,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        })
        .expect("false");
    let err = store
        .create_assertion(Assertion {
            id: AssertionId::parse("88888888-8888-4888-8888-888888888883").expect("bad"),
            subject_id: subject.id.clone(),
            definition_id: false_definition.id.clone(),
            value: Value::Text("no".to_string()),
            canon: Canon::Abierto,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        })
        .expect_err("mismatch");
    assert_validation(
        err,
        ValidationReason::ValueMismatch,
        "value",
        "88888888-8888-4888-8888-888888888883",
    );
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(
        store
            .assertions_for(&subject.id, &absent.id)
            .expect("absent"),
        Vec::<Assertion>::new()
    );
    assert_eq!(
        store
            .assertions_for(&subject.id, &unknown_definition.id)
            .expect("unknown"),
        vec![unknown.clone()]
    );
    assert_eq!(
        store
            .assertions_for(&subject.id, &false_definition.id)
            .expect("false"),
        vec![falsy.clone()]
    );
    let conn = Connection::open(&scratch.path).expect("raw");
    let unknown_row: (String, Option<String>, Option<i64>, Option<String>) = conn
        .query_row(
            "SELECT value_kind, text_value, bool_value, target_entity_id FROM assertions WHERE id = ?1",
            [unknown.id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("unknown row");
    assert_eq!(unknown_row, ("unknown".to_string(), None, None, None));
    let false_row: (String, Option<String>, Option<i64>, Option<String>) = conn
        .query_row(
            "SELECT value_kind, text_value, bool_value, target_entity_id FROM assertions WHERE id = ?1",
            [falsy.id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("false row");
    assert_eq!(false_row, ("boolean".to_string(), None, Some(0), None));
    let absent_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM assertions WHERE definition_id = ?1",
            [absent.id.as_str()],
            |row| row.get(0),
        )
        .expect("absent count");
    assert_eq!(absent_count, 0);
}

#[test]
fn absent_metadata_is_not_an_empty_string() {
    let (scratch, mut store) = created("metadata.sqlite");
    let entity_type = person_type(&mut store);
    let subject = narrator(&mut store, Some(entity_type.id.clone()));
    let definition = store
        .create_definition(Definition {
            id: DefinitionId::parse("77777777-7777-4777-8777-777777777777").expect("definition"),
            owner: Owner::Type(entity_type.id),
            value_class: ValueClass::Text,
            cardinality: Cardinality::Singular,
            archived: false,
        })
        .expect("definition");
    let assertion = Assertion {
        id: AssertionId::parse("88888888-8888-4888-8888-888888888888").expect("assertion"),
        subject_id: subject.id,
        definition_id: definition.id,
        value: Value::Text("nota".to_string()),
        canon: Canon::Provisional,
        source_status_raw: Some("ETIQUETA LITERAL".to_string()),
        provenance: Provenance {
            external_ref: None,
            locator: Some(String::new()),
            fragment: None,
            source_revision: None,
        },
        scope_note: Some(String::new()),
        archived: false,
    };
    store
        .create_assertion(assertion.clone())
        .expect("assertion");
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    let stored = store.assertion(&assertion.id).expect("assertion");
    assert_eq!(stored.canon, Canon::Provisional);
    assert_eq!(
        stored.source_status_raw.as_deref(),
        Some("ETIQUETA LITERAL")
    );
    assert_eq!(stored.provenance.external_ref, None);
    assert_eq!(stored.provenance.locator.as_deref(), Some(""));
    assert_eq!(stored.scope_note.as_deref(), Some(""));
    let conn = Connection::open(&scratch.path).expect("raw");
    let row: (Option<String>, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT provenance_external_ref, provenance_locator, scope_note FROM assertions WHERE id = ?1",
            [assertion.id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("row");
    assert_eq!(row, (None, Some(String::new()), Some(String::new())));
}

#[test]
fn four_canon_states_roundtrip() {
    let (scratch, mut store) = created("canon.sqlite");
    let entity_type = person_type(&mut store);
    let subject = narrator(&mut store, Some(entity_type.id.clone()));
    let definition = store
        .create_definition(Definition {
            id: DefinitionId::parse("77777777-7777-4777-8777-777777777777").expect("definition"),
            owner: Owner::Type(entity_type.id),
            value_class: ValueClass::Text,
            cardinality: Cardinality::Singular,
            archived: false,
        })
        .expect("definition");
    let states = [
        (
            Canon::Acordado,
            "88888888-8888-4888-8888-888888888881",
            "acordado",
        ),
        (
            Canon::Provisional,
            "88888888-8888-4888-8888-888888888882",
            "provisional",
        ),
        (
            Canon::Abierto,
            "88888888-8888-4888-8888-888888888883",
            "abierto",
        ),
        (
            Canon::MisterioIntencional,
            "88888888-8888-4888-8888-888888888884",
            "misterio",
        ),
    ];
    for (canon, id, text) in states {
        store
            .create_assertion(Assertion {
                id: AssertionId::parse(id).expect("id"),
                subject_id: subject.id.clone(),
                definition_id: definition.id.clone(),
                value: Value::Text(text.to_string()),
                canon,
                source_status_raw: None,
                provenance: Provenance::absent(),
                scope_note: None,
                archived: false,
            })
            .expect("assertion");
    }
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    let stored = store
        .assertions_for(&subject.id, &definition.id)
        .expect("assertions");
    assert_eq!(stored.len(), 4);
    assert_eq!(
        stored
            .iter()
            .map(|assertion| assertion.canon)
            .collect::<Vec<_>>(),
        vec![
            Canon::Acordado,
            Canon::Provisional,
            Canon::Abierto,
            Canon::MisterioIntencional,
        ]
    );
}

#[test]
fn singular_definition_stores_two_assertions() {
    let (scratch, mut store) = created("singular.sqlite");
    let entity_type = person_type(&mut store);
    let subject = narrator(&mut store, Some(entity_type.id.clone()));
    let definition = store
        .create_definition(Definition {
            id: DefinitionId::parse("77777777-7777-4777-8777-777777777777").expect("definition"),
            owner: Owner::Entity(subject.id.clone()),
            value_class: ValueClass::Text,
            cardinality: Cardinality::Singular,
            archived: false,
        })
        .expect("definition");
    let first = AssertionId::parse("88888888-8888-4888-8888-888888888881").expect("first");
    let second = AssertionId::parse("88888888-8888-4888-8888-888888888882").expect("second");
    store
        .create_assertion(Assertion {
            id: first.clone(),
            subject_id: subject.id.clone(),
            definition_id: definition.id.clone(),
            value: Value::Text("una".to_string()),
            canon: Canon::Provisional,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        })
        .expect("first");
    store
        .create_assertion(Assertion {
            id: second.clone(),
            subject_id: subject.id.clone(),
            definition_id: definition.id.clone(),
            value: Value::Text("otra".to_string()),
            canon: Canon::Abierto,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        })
        .expect("second");
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    let stored = store
        .assertions_for(&subject.id, &definition.id)
        .expect("assertions");
    assert_eq!(
        stored
            .iter()
            .map(|assertion| assertion.id.as_str().to_string())
            .collect::<Vec<_>>(),
        vec![first.as_str().to_string(), second.as_str().to_string()]
    );
}

fn assert_sql_error(result: rusqlite::Result<usize>, phrase: &str) {
    let err = result.expect_err("constraint");
    let message = err.to_string();
    assert!(message.contains(phrase), "{message}");
}

#[test]
fn schema_checks_reject_null_bypass_and_bad_foreign_keys() {
    let (scratch, mut store) = created("checks.sqlite");
    let entity_type = person_type(&mut store);
    let subject = narrator(&mut store, Some(entity_type.id.clone()));
    let definition = store
        .create_definition(Definition {
            id: DefinitionId::parse("77777777-7777-4777-8777-777777777777").expect("definition"),
            owner: Owner::Type(entity_type.id.clone()),
            value_class: ValueClass::Text,
            cardinality: Cardinality::Singular,
            archived: false,
        })
        .expect("definition");
    let before = store.counts().expect("before");
    drop(store);
    let conn = Connection::open(&scratch.path).expect("raw");
    conn.execute_batch("PRAGMA foreign_keys = ON").expect("fk");
    assert_sql_error(
        conn.execute(
            "INSERT INTO definitions (
                id, owner_type_id, owner_entity_id, value_class, cardinality, archived
            ) VALUES ('12121212-1212-4121-8121-121212121212', NULL, NULL, 'text', 'singular', 0)",
            [],
        ),
        "CHECK constraint failed",
    );
    assert_sql_error(
        conn.execute(
            "INSERT INTO definitions (
                id, owner_type_id, owner_entity_id, value_class, cardinality, archived
            ) VALUES ('13131313-1313-4131-8131-131313131313', ?1, ?2, 'text', 'singular', 0)",
            rusqlite::params![entity_type.id.as_str(), subject.id.as_str()],
        ),
        "CHECK constraint failed",
    );
    assert_sql_error(
        conn.execute(
            "INSERT INTO assertions (
                id, subject_id, definition_id, value_kind, text_value, bool_value,
                target_entity_id, canon, archived
            ) VALUES (
                '14141414-1414-4141-8141-141414141414', ?1, ?2, 'text', NULL, NULL, NULL,
                'PROVISIONAL', 0
            )",
            rusqlite::params![subject.id.as_str(), definition.id.as_str()],
        ),
        "CHECK constraint failed",
    );
    assert_sql_error(
        conn.execute(
            "INSERT INTO assertions (
                id, subject_id, definition_id, value_kind, text_value, bool_value,
                target_entity_id, canon, archived
            ) VALUES (
                '15151515-1515-4151-8151-151515151515', ?1, ?2, 'unknown', 'payload', NULL, NULL,
                'ABIERTO', 0
            )",
            rusqlite::params![subject.id.as_str(), definition.id.as_str()],
        ),
        "CHECK constraint failed",
    );
    assert_sql_error(
        conn.execute(
            "INSERT INTO assertions (
                id, subject_id, definition_id, value_kind, text_value, bool_value,
                target_entity_id, canon, archived
            ) VALUES (
                '16161616-1616-4161-8161-161616161616', ?1, ?2, NULL, NULL, NULL, NULL,
                'ABIERTO', 0
            )",
            rusqlite::params![subject.id.as_str(), definition.id.as_str()],
        ),
        "NOT NULL constraint failed",
    );
    assert_sql_error(
        conn.execute(
            "INSERT INTO assertions (
                id, subject_id, definition_id, value_kind, text_value, bool_value,
                target_entity_id, canon, archived
            ) VALUES (
                '17171717-1717-4171-8171-171717171717',
                '19191919-1919-4191-8191-191919191919',
                ?1, 'text', 'hola', NULL, NULL, 'PROVISIONAL', 0
            )",
            rusqlite::params![definition.id.as_str()],
        ),
        "FOREIGN KEY constraint failed",
    );
    assert_sql_error(
        conn.execute(
            "INSERT INTO assertions (
                id, subject_id, definition_id, value_kind, text_value, bool_value,
                target_entity_id, canon, archived
            ) VALUES (
                '18181818-1818-4181-8181-181818181818', ?1, ?2, 'boolean', NULL, 2, NULL,
                'PROVISIONAL', 0
            )",
            rusqlite::params![subject.id.as_str(), definition.id.as_str()],
        ),
        "CHECK constraint failed",
    );
    drop(conn);
    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.counts().expect("counts"), before);
    assert_eq!(
        store
            .definition(&definition.id)
            .expect("definition")
            .value_class,
        ValueClass::Text
    );
}

#[test]
fn t13_busy_write_does_not_persist() {
    let (scratch, mut store) = created("t13-busy.sqlite");
    let holder = Connection::open(&scratch.path).expect("holder");
    holder.execute_batch("BEGIN IMMEDIATE").expect("lock");
    let page_id = PageId::parse("22222222-2222-4222-8222-222222222222").expect("page");
    let err = store
        .create_page(Page {
            id: page_id.clone(),
            title: "No".to_string(),
            body: "entra".to_string(),
        })
        .expect_err("busy");
    assert!(matches!(err, Error::Busy), "{err}");
    drop(holder);
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.counts().expect("counts").pages, 0);
    match store.page(&page_id).expect_err("page") {
        Error::Validation {
            reason: ValidationReason::NotFound,
            field: "id",
            id: Some(id),
        } => assert_eq!(id, page_id.as_str()),
        other => panic!("expected not found, got {other}"),
    }
}

#[test]
fn archived_flag_roundtrips_without_changing_canon() {
    let (scratch, mut store) = created("archived-flag.sqlite");
    let entity_type = person_type(&mut store);
    let subject = narrator(&mut store, Some(entity_type.id.clone()));
    let definition = store
        .create_definition(Definition {
            id: DefinitionId::parse("77777777-7777-4777-8777-777777777777").expect("definition"),
            owner: Owner::Type(entity_type.id),
            value_class: ValueClass::Text,
            cardinality: Cardinality::Multiple,
            archived: true,
        })
        .expect("definition");
    let assertion = store
        .create_assertion(Assertion {
            id: AssertionId::parse("88888888-8888-4888-8888-888888888888").expect("assertion"),
            subject_id: subject.id,
            definition_id: definition.id.clone(),
            value: Value::Text("histórico".to_string()),
            canon: Canon::MisterioIntencional,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: true,
        })
        .expect("assertion");
    drop(store);
    let store = Store::open(&scratch.path).expect("reopen");
    let stored_definition = store.definition(&definition.id).expect("definition");
    assert!(stored_definition.archived);
    assert_eq!(stored_definition.cardinality, Cardinality::Multiple);
    let stored = store.assertion(&assertion.id).expect("assertion");
    assert!(stored.archived);
    assert_eq!(stored.canon, Canon::MisterioIntencional);
    assert_eq!(stored.value, Value::Text("histórico".to_string()));
    let conn = Connection::open(&scratch.path).expect("raw");
    let definition_flag: i64 = conn
        .query_row(
            "SELECT archived FROM definitions WHERE id = ?1",
            [definition.id.as_str()],
            |row| row.get(0),
        )
        .expect("definition flag");
    let assertion_flag: i64 = conn
        .query_row(
            "SELECT archived FROM assertions WHERE id = ?1",
            [assertion.id.as_str()],
            |row| row.get(0),
        )
        .expect("assertion flag");
    assert_eq!(definition_flag, 1);
    assert_eq!(assertion_flag, 1);
}

#[test]
fn null_primary_keys_are_rejected() {
    let (scratch, mut store) = created("null-pk.sqlite");
    let entity_type = person_type(&mut store);
    let subject = narrator(&mut store, Some(entity_type.id.clone()));
    let definition = store
        .create_definition(Definition {
            id: DefinitionId::parse("77777777-7777-4777-8777-777777777777").expect("definition"),
            owner: Owner::Type(entity_type.id.clone()),
            value_class: ValueClass::Text,
            cardinality: Cardinality::Singular,
            archived: false,
        })
        .expect("definition");
    drop(store);
    let conn = Connection::open(&scratch.path).expect("raw");
    conn.execute_batch("PRAGMA foreign_keys = ON").expect("fk");
    let definition_sql = format!(
        "INSERT INTO definitions (
            id, owner_type_id, owner_entity_id, value_class, cardinality, archived
        ) VALUES (NULL, '{}', NULL, 'text', 'singular', 0)",
        entity_type.id.as_str()
    );
    let assertion_sql = format!(
        "INSERT INTO assertions (
            id, subject_id, definition_id, value_kind, text_value, bool_value,
            target_entity_id, canon, archived
        ) VALUES (NULL, '{}', '{}', 'text', 'x', NULL, NULL, 'PROVISIONAL', 0)",
        subject.id.as_str(),
        definition.id.as_str()
    );
    let statements = [
        "INSERT INTO pages (id, title, body) VALUES (NULL, 't', 'b')",
        "INSERT INTO types (id, label) VALUES (NULL, 't')",
        "INSERT INTO entities (id, label, type_id, page_id) VALUES (NULL, 'e', NULL, NULL)",
        definition_sql.as_str(),
        assertion_sql.as_str(),
    ];
    for sql in statements {
        assert_sql_error(conn.execute(sql, []), "NOT NULL constraint failed");
    }
}
