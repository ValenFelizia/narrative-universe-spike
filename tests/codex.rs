#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use narrative_universe::{
    validate_codex_label, Assertion, AssertionId, Canon, Cardinality, CodexEntitySummary,
    CodexIndex, Definition, DefinitionId, DefinitionLabel, Entity, EntityId, EntityType, Error,
    KnowledgeRecordProbe, Owner, Page, PageId, Provenance, Store, TypeId, UniverseId,
    ValidationReason, Value, ValueClass, MAX_CODEX_LABEL_SCALARS, MAX_CONTEXT_GROUPS,
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

#[test]
fn create_definition_with_assertion_commits_atomically_and_rolls_back_second_step_failures() {
    let scratch = scratch("ka2-atomic.sqlite");
    let mut store = Store::create(
        &scratch.path,
        UniverseId::parse(UNIVERSE).expect("universe"),
    )
    .expect("create");

    store
        .create_entity_type(entity_type(TYPE_HOUSE_A, "Order"))
        .expect("type a");
    store
        .create_entity_type(entity_type(TYPE_HOUSE_B, "Guild"))
        .expect("type b");
    store
        .create_entity(entity(ENT_MARA, "Mara Vey", Some(TYPE_HOUSE_A), None))
        .expect("mara");
    store
        .create_entity(entity(ENT_UNTYPED, "Ashfall Bell", None, None))
        .expect("untyped");

    let def_ok = Definition {
        id: DefinitionId::parse("44444444-4444-4444-8444-000000000001").unwrap(),
        owner: Owner::Type(TypeId::parse(TYPE_HOUSE_A).unwrap()),
        value_class: ValueClass::Text,
        cardinality: Cardinality::Singular,
        label: Some(DefinitionLabel::parse("Motto").unwrap()),
        archived: false,
    };
    let ass_ok = Assertion {
        id: AssertionId::parse("66666666-6666-4666-8666-000000000001").unwrap(),
        subject_id: EntityId::parse(ENT_MARA).unwrap(),
        definition_id: def_ok.id.clone(),
        value: Value::Text("  Hold the lamp\n\tuntil dawn  ".to_string()),
        canon: Canon::Provisional,
        source_status_raw: Some("draft note".to_string()),
        provenance: Provenance {
            external_ref: Some("Ledger v1".to_string()),
            locator: Some("p. 12".to_string()),
            fragment: Some("line 1\nline 2".to_string()),
            source_revision: Some("rev-a".to_string()),
        },
        scope_note: Some("Early chapters".to_string()),
        archived: false,
    };

    let (saved_def, saved_ass) = store
        .create_definition_with_assertion(def_ok.clone(), ass_ok.clone())
        .expect("atomic commit");
    assert_eq!(saved_def, def_ok);
    assert_eq!(saved_ass, ass_ok);

    let baseline = store.export_snapshot().expect("baseline snapshot");

    // 1. Missing reference target rolls back both definition and assertion.
    let def_missing_target = Definition {
        id: DefinitionId::parse("44444444-4444-4444-8444-000000000002").unwrap(),
        owner: Owner::Entity(EntityId::parse(ENT_MARA).unwrap()),
        value_class: ValueClass::Reference,
        cardinality: Cardinality::Multiple,
        label: Some(DefinitionLabel::parse("Ally").unwrap()),
        archived: false,
    };
    let ass_missing_target = Assertion {
        id: AssertionId::parse("66666666-6666-4666-8666-000000000002").unwrap(),
        subject_id: EntityId::parse(ENT_MARA).unwrap(),
        definition_id: def_missing_target.id.clone(),
        value: Value::Reference(EntityId::parse("55555555-5555-4555-8555-999999999999").unwrap()),
        canon: Canon::Provisional,
        source_status_raw: None,
        provenance: Provenance::absent(),
        scope_note: None,
        archived: false,
    };
    assert!(matches!(
        store.create_definition_with_assertion(def_missing_target.clone(), ass_missing_target),
        Err(Error::Validation {
            reason: ValidationReason::UnknownTarget,
            ..
        })
    ));

    // 2. Wrong value class rolls back both.
    let def_wrong_class = Definition {
        id: DefinitionId::parse("44444444-4444-4444-8444-000000000003").unwrap(),
        owner: Owner::Entity(EntityId::parse(ENT_MARA).unwrap()),
        value_class: ValueClass::Boolean,
        cardinality: Cardinality::Singular,
        label: Some(DefinitionLabel::parse("Alive").unwrap()),
        archived: false,
    };
    let ass_wrong_class = Assertion {
        id: AssertionId::parse("66666666-6666-4666-8666-000000000003").unwrap(),
        subject_id: EntityId::parse(ENT_MARA).unwrap(),
        definition_id: def_wrong_class.id.clone(),
        value: Value::Text("yes".to_string()),
        canon: Canon::Acordado,
        source_status_raw: None,
        provenance: Provenance::absent(),
        scope_note: None,
        archived: false,
    };
    assert!(matches!(
        store.create_definition_with_assertion(def_wrong_class, ass_wrong_class),
        Err(Error::Validation {
            reason: ValidationReason::ValueMismatch,
            ..
        })
    ));

    // 3. Wrong owner (not applicable to subject) rolls back both.
    let def_wrong_owner = Definition {
        id: DefinitionId::parse("44444444-4444-4444-8444-000000000004").unwrap(),
        owner: Owner::Type(TypeId::parse(TYPE_HOUSE_B).unwrap()),
        value_class: ValueClass::Text,
        cardinality: Cardinality::Singular,
        label: Some(DefinitionLabel::parse("Rank").unwrap()),
        archived: false,
    };
    let ass_wrong_owner = Assertion {
        id: AssertionId::parse("66666666-6666-4666-8666-000000000004").unwrap(),
        subject_id: EntityId::parse(ENT_MARA).unwrap(),
        definition_id: def_wrong_owner.id.clone(),
        value: Value::Text("Initiate".to_string()),
        canon: Canon::Provisional,
        source_status_raw: None,
        provenance: Provenance::absent(),
        scope_note: None,
        archived: false,
    };
    assert!(matches!(
        store.create_definition_with_assertion(def_wrong_owner, ass_wrong_owner),
        Err(Error::Validation {
            reason: ValidationReason::NotApplicable,
            ..
        })
    ));

    // 4. Duplicate assertion ID rolls back the newly inserted definition too.
    let def_dup_ass = Definition {
        id: DefinitionId::parse("44444444-4444-4444-8444-000000000005").unwrap(),
        owner: Owner::Entity(EntityId::parse(ENT_MARA).unwrap()),
        value_class: ValueClass::Text,
        cardinality: Cardinality::Singular,
        label: Some(DefinitionLabel::parse("Alias").unwrap()),
        archived: false,
    };
    let ass_dup = Assertion {
        id: ass_ok.id.clone(),
        subject_id: EntityId::parse(ENT_MARA).unwrap(),
        definition_id: def_dup_ass.id.clone(),
        value: Value::Text("Lantern".to_string()),
        canon: Canon::Provisional,
        source_status_raw: None,
        provenance: Provenance::absent(),
        scope_note: None,
        archived: false,
    };
    assert!(matches!(
        store.create_definition_with_assertion(def_dup_ass, ass_dup),
        Err(Error::Validation {
            reason: ValidationReason::DuplicateId,
            ..
        })
    ));

    // 5. Association mismatch (assertion.definition_id != definition.id) rejected with zero writes.
    let def_mismatch = Definition {
        id: DefinitionId::parse("44444444-4444-4444-8444-000000000006").unwrap(),
        owner: Owner::Entity(EntityId::parse(ENT_MARA).unwrap()),
        value_class: ValueClass::Text,
        cardinality: Cardinality::Singular,
        label: Some(DefinitionLabel::parse("Note").unwrap()),
        archived: false,
    };
    let ass_mismatch = Assertion {
        id: AssertionId::parse("66666666-6666-4666-8666-000000000006").unwrap(),
        subject_id: EntityId::parse(ENT_MARA).unwrap(),
        definition_id: def_ok.id.clone(),
        value: Value::Text("Mismatch".to_string()),
        canon: Canon::Provisional,
        source_status_raw: None,
        provenance: Provenance::absent(),
        scope_note: None,
        archived: false,
    };
    assert!(matches!(
        store.create_definition_with_assertion(def_mismatch, ass_mismatch),
        Err(Error::Validation {
            reason: ValidationReason::NotApplicable,
            ..
        })
    ));

    // 6. Archived definition rejected and rolled back.
    let def_archived = Definition {
        id: DefinitionId::parse("44444444-4444-4444-8444-000000000007").unwrap(),
        owner: Owner::Entity(EntityId::parse(ENT_MARA).unwrap()),
        value_class: ValueClass::Text,
        cardinality: Cardinality::Singular,
        label: Some(DefinitionLabel::parse("Retired").unwrap()),
        archived: true,
    };
    let ass_archived = Assertion {
        id: AssertionId::parse("66666666-6666-4666-8666-000000000007").unwrap(),
        subject_id: EntityId::parse(ENT_MARA).unwrap(),
        definition_id: def_archived.id.clone(),
        value: Value::Text("Nope".to_string()),
        canon: Canon::Provisional,
        source_status_raw: None,
        provenance: Provenance::absent(),
        scope_note: None,
        archived: false,
    };
    assert!(matches!(
        store.create_definition_with_assertion(def_archived, ass_archived),
        Err(Error::Validation {
            reason: ValidationReason::ArchivedDefinition,
            ..
        })
    ));

    // Zero partial rows and unrelated rows untouched.
    assert_eq!(store.export_snapshot().expect("after rollbacks"), baseline);

    // Reopen preserves exact committed rows and exact bytes.
    drop(store);
    let reopened = Store::open(&scratch.path).expect("reopen");
    assert_eq!(reopened.definition(&def_ok.id).expect("def"), def_ok);
    assert_eq!(reopened.assertion(&ass_ok.id).expect("ass"), ass_ok);
}

#[test]
fn authoring_semantics_empty_future_type_unknown_selfref_competing_and_probe_beyond_context_bounds()
{
    let scratch = scratch("ka2-semantics.sqlite");
    let mut store = Store::create(
        &scratch.path,
        UniverseId::parse(UNIVERSE).expect("universe"),
    )
    .expect("create");

    store
        .create_entity_type(entity_type(TYPE_HOUSE_A, "Order"))
        .expect("type");
    store
        .create_entity(entity(ENT_MARA, "Mara Vey", Some(TYPE_HOUSE_A), None))
        .expect("mara");
    store
        .create_entity(entity(ENT_UNTYPED, "Ashfall Bell", None, None))
        .expect("untyped");

    // Empty type-owned definition applies to current AND future entities of exact type.
    let def_empty = store
        .create_definition(Definition {
            id: DefinitionId::parse("44444444-4444-4444-8444-000000000010").unwrap(),
            owner: Owner::Type(TypeId::parse(TYPE_HOUSE_A).unwrap()),
            value_class: ValueClass::Boolean,
            cardinality: Cardinality::Singular,
            label: Some(DefinitionLabel::parse("Sworn").unwrap()),
            archived: false,
        })
        .expect("empty def");

    // Future entity of exact type automatically sees def_empty in entity_context.
    store
        .create_entity(entity(ENT_MARA_DUP, "Sable", Some(TYPE_HOUSE_A), None))
        .expect("sable");
    let sable_ctx = store
        .entity_context(&EntityId::parse(ENT_MARA_DUP).unwrap())
        .expect("sable ctx");
    assert_eq!(sable_ctx.outgoing_groups.len(), 1);
    assert_eq!(sable_ctx.outgoing_groups[0].definition.id, def_empty.id);
    assert!(sable_ctx.outgoing_groups[0].claims.is_empty());

    // Competing singular claims: Boolean(false), Boolean(true), and Unknown all coexist.
    let a_false = store
        .create_assertion(Assertion {
            id: AssertionId::parse("66666666-6666-4666-8666-000000000010").unwrap(),
            subject_id: EntityId::parse(ENT_MARA).unwrap(),
            definition_id: def_empty.id.clone(),
            value: Value::Boolean(false),
            canon: Canon::Provisional,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        })
        .expect("false");
    let a_true = store
        .create_assertion(Assertion {
            id: AssertionId::parse("66666666-6666-4666-8666-000000000011").unwrap(),
            subject_id: EntityId::parse(ENT_MARA).unwrap(),
            definition_id: def_empty.id.clone(),
            value: Value::Boolean(true),
            canon: Canon::Acordado,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        })
        .expect("true");
    let a_unknown = store
        .create_assertion(Assertion {
            id: AssertionId::parse("66666666-6666-4666-8666-000000000012").unwrap(),
            subject_id: EntityId::parse(ENT_MARA).unwrap(),
            definition_id: def_empty.id.clone(),
            value: Value::Unknown,
            canon: Canon::MisterioIntencional,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        })
        .expect("unknown bool");
    assert_eq!(
        store
            .assertions_for(&EntityId::parse(ENT_MARA).unwrap(), &def_empty.id)
            .unwrap()
            .len(),
        3
    );

    // Self-reference and incoming relation stored once.
    let def_ref = store
        .create_definition(Definition {
            id: DefinitionId::parse("44444444-4444-4444-8444-000000000011").unwrap(),
            owner: Owner::Entity(EntityId::parse(ENT_MARA).unwrap()),
            value_class: ValueClass::Reference,
            cardinality: Cardinality::Multiple,
            label: None, // Unnamed definition stays legal and usable
            archived: false,
        })
        .expect("unnamed ref def");
    let a_self = store
        .create_assertion(Assertion {
            id: AssertionId::parse("66666666-6666-4666-8666-000000000013").unwrap(),
            subject_id: EntityId::parse(ENT_MARA).unwrap(),
            definition_id: def_ref.id.clone(),
            value: Value::Reference(EntityId::parse(ENT_MARA).unwrap()),
            canon: Canon::Abierto,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        })
        .expect("self ref");
    let a_to_untyped = store
        .create_assertion(Assertion {
            id: AssertionId::parse("66666666-6666-4666-8666-000000000014").unwrap(),
            subject_id: EntityId::parse(ENT_MARA).unwrap(),
            definition_id: def_ref.id.clone(),
            value: Value::Reference(EntityId::parse(ENT_UNTYPED).unwrap()),
            canon: Canon::Provisional,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        })
        .expect("to untyped");
    let untyped_ctx = store
        .entity_context(&EntityId::parse(ENT_UNTYPED).unwrap())
        .expect("untyped ctx");
    assert_eq!(untyped_ctx.incoming_groups.len(), 1);
    assert_eq!(
        untyped_ctx.incoming_groups[0].claims[0].assertion.id,
        a_to_untyped.id
    );

    // Probe works in one snapshot for present, partial, and absent IDs.
    let missing_def = DefinitionId::parse("44444444-4444-4444-8444-999999999999").unwrap();
    let missing_ass = AssertionId::parse("66666666-6666-4666-8666-999999999999").unwrap();
    assert_eq!(
        store
            .probe_knowledge_records(Some(&def_empty.id), Some(&a_false.id))
            .unwrap(),
        KnowledgeRecordProbe {
            definition: Some(def_empty.clone()),
            assertion: Some(a_false),
        }
    );
    assert_eq!(
        store
            .probe_knowledge_records(Some(&missing_def), Some(&missing_ass))
            .unwrap(),
        KnowledgeRecordProbe {
            definition: None,
            assertion: None,
        }
    );
    let _ = (a_true, a_unknown, a_self);

    // Probe still succeeds when entity_context exceeds MAX_CONTEXT_GROUPS.
    let mut last_def_id = def_empty.id;
    for i in 0..=MAX_CONTEXT_GROUPS {
        let id = DefinitionId::parse(&format!("44444444-4444-4444-8444-00000001{i:04}")).unwrap();
        store
            .create_definition(Definition {
                id: id.clone(),
                owner: Owner::Entity(EntityId::parse(ENT_UNTYPED).unwrap()),
                value_class: ValueClass::Text,
                cardinality: Cardinality::Singular,
                label: Some(DefinitionLabel::parse("Shared").unwrap()),
                archived: false,
            })
            .expect("extra def");
        last_def_id = id;
    }
    assert!(matches!(
        store.entity_context(&EntityId::parse(ENT_UNTYPED).unwrap()),
        Err(Error::Validation {
            reason: ValidationReason::ContextTooLarge,
            ..
        })
    ));
    let probed = store
        .probe_knowledge_records(Some(&last_def_id), None)
        .expect("probe succeeds beyond context bounds");
    assert_eq!(
        probed.definition.as_ref().map(|d| &d.id),
        Some(&last_def_id)
    );
}
