#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Instant;

use narrative_universe::{
    import_new_json, Assertion, AssertionId, Canon, Cardinality, Definition, DefinitionId,
    DefinitionLabel, Entity, EntityId, EntityType, Error, Owner, Provenance, Snapshot, Store,
    TypeId, UniverseId, ValidationReason, Value, ValueClass, MAX_CONTEXT_BYTES, MAX_CONTEXT_GROUPS,
    MAX_CONTEXT_UNIQUE_ASSERTIONS,
};
use rusqlite::Connection;

fn id(end: &str) -> EntityId {
    EntityId::parse(&format!("00000000-0000-4000-8000-0000000000{end}")).unwrap()
}

fn def_id(end: &str) -> DefinitionId {
    DefinitionId::parse(&format!("00000000-0000-4000-8000-0000000000{end}")).unwrap()
}

fn ass_id(end: &str) -> AssertionId {
    AssertionId::parse(&format!("00000000-0000-4000-8000-0000000000{end}")).unwrap()
}

fn uuid_n(prefix: &str, n: usize) -> String {
    format!("{prefix}-0000-4000-8000-{n:012x}")
}

fn fixture(path: &std::path::Path) -> Store {
    import_new_json(path, include_bytes!("fixtures/reveal-facts.json"))
        .unwrap()
        .0
}

#[test]
fn reveal_fixture_groups_applicable_definitions_absent_claims_and_archives() {
    let scratch = support::scratch("context-fixture.sqlite");
    let store = fixture(&scratch.path);
    let before = support::bytes(&scratch.path);

    let mara = store.entity_context(&id("11")).unwrap();
    assert_eq!(mara.entity.label, "Mara Vey");
    assert_eq!(mara.type_label.as_deref(), Some("Person"));

    // 7 active Person definitions in reveal-facts.json: 0031, 0032, 0033, 0034, 0035, 0037, 0038.
    let active_ids: Vec<&str> = mara
        .outgoing_groups
        .iter()
        .map(|g| g.definition.id.as_str())
        .collect();
    assert_eq!(
        active_ids,
        vec![
            def_id("31").as_str(),
            def_id("32").as_str(),
            def_id("33").as_str(),
            def_id("34").as_str(),
            def_id("35").as_str(),
            def_id("37").as_str(),
            def_id("38").as_str(),
        ]
    );

    // 0031 member of: 0041 (Ashfall Order, PROVISIONAL) and 0042 (Tidewardens, ABIERTO).
    assert_eq!(mara.outgoing_groups[0].claims.len(), 2);
    assert_eq!(mara.outgoing_groups[0].claims[0].assertion.id, ass_id("41"));
    assert_eq!(
        mara.outgoing_groups[0].claims[0]
            .target
            .as_ref()
            .unwrap()
            .label,
        "Ashfall Order"
    );
    assert_eq!(
        mara.outgoing_groups[0].claims[1]
            .target
            .as_ref()
            .unwrap()
            .label,
        "Tidewardens"
    );

    // 0032 born in (singular): both ACORDADO claims preserved, no incompatibility filtering.
    assert_eq!(mara.outgoing_groups[1].claims.len(), 2);
    assert_eq!(
        mara.outgoing_groups[1].claims[0].assertion.canon,
        Canon::Acordado
    );
    assert_eq!(
        mara.outgoing_groups[1].claims[1].assertion.canon,
        Canon::Acordado
    );

    // 0033 oath sworn: Mara has 0 assertions (Corin's false does not leak onto Mara).
    assert!(mara.outgoing_groups[2].claims.is_empty());
    assert!(!mara.outgoing_groups[2].has_archived_claims);

    // 0034 epithet: "the ford-walker".
    assert_eq!(mara.outgoing_groups[3].claims.len(), 1);
    assert_eq!(
        mara.outgoing_groups[3].claims[0].assertion.value,
        Value::Text("the ford-walker".to_string())
    );

    // 0035 true name: Mara has 0 assertions.
    assert!(mara.outgoing_groups[4].claims.is_empty());
    assert!(!mara.outgoing_groups[4].has_archived_claims);

    // 0037 unnamed: recorded Unknown with source_revision rev-9.
    assert!(mara.outgoing_groups[5].definition.label.is_none());
    assert_eq!(mara.outgoing_groups[5].claims.len(), 1);
    assert_eq!(
        mara.outgoing_groups[5].claims[0].assertion.value,
        Value::Unknown
    );
    assert_eq!(
        mara.outgoing_groups[5].claims[0]
            .assertion
            .provenance
            .source_revision
            .as_deref(),
        Some("rev-9")
    );

    // 0038 distinct member of: Mara DOES have active assertion 004c -> Ashfall Order.
    assert_eq!(
        mara.outgoing_groups[6]
            .definition
            .label
            .as_ref()
            .unwrap()
            .as_str(),
        "member of"
    );
    assert_eq!(mara.outgoing_groups[6].claims.len(), 1);
    assert_eq!(mara.outgoing_groups[6].claims[0].assertion.id, ass_id("4c"));
    assert_eq!(
        mara.outgoing_groups[6].claims[0]
            .target
            .as_ref()
            .unwrap()
            .label,
        "Ashfall Order"
    );
    assert_eq!(
        mara.outgoing_groups[6].claims[0]
            .assertion
            .provenance
            .external_ref
            .as_deref(),
        Some("witness-2")
    );

    // 0036 archived sigil (entity-owned by Mara, archived: true) -> archived_outgoing_groups only.
    assert_eq!(mara.archived_outgoing_groups.len(), 1);
    assert_eq!(mara.archived_outgoing_groups[0].definition.id, def_id("36"));
    assert!(mara.archived_outgoing_groups[0].definition.archived);
    assert_eq!(mara.archived_outgoing_groups[0].claims.len(), 1);
    assert!(
        mara.archived_outgoing_groups[0].claims[0]
            .assertion
            .archived
    );
    assert!(mara.incoming_groups.is_empty());
    assert!(mara.archived_incoming_groups.is_empty());

    // Corin has false on 0033 and intentional mystery Unknown on 0035; no entity-owned 0036.
    let corin = store.entity_context(&id("12")).unwrap();
    let corin_oath = corin
        .outgoing_groups
        .iter()
        .find(|g| g.definition.id == def_id("33"))
        .unwrap();
    assert_eq!(corin_oath.claims.len(), 1);
    assert_eq!(corin_oath.claims[0].assertion.value, Value::Boolean(false));
    assert_eq!(
        corin_oath.claims[0].assertion.provenance,
        Provenance::absent()
    );
    let corin_true_name = corin
        .outgoing_groups
        .iter()
        .find(|g| g.definition.id == def_id("35"))
        .unwrap();
    assert_eq!(
        corin_true_name.claims[0].assertion.canon,
        Canon::MisterioIntencional
    );
    assert_eq!(
        corin_true_name.claims[0].assertion.scope_note.as_deref(),
        Some("sealed until the second flood")
    );
    assert!(corin.archived_outgoing_groups.is_empty());

    // Ashfall Order: 0 applicable definitions, 3 incoming groups grouped by (subject_id, definition_id).
    let ashfall = store.entity_context(&id("13")).unwrap();
    assert!(ashfall.outgoing_groups.is_empty());
    assert_eq!(ashfall.incoming_groups.len(), 3);
    assert_eq!(ashfall.incoming_groups[0].subject.id, id("11"));
    assert_eq!(ashfall.incoming_groups[0].definition.id, def_id("31"));
    assert_eq!(
        ashfall.incoming_groups[0].claims[0].assertion.id,
        ass_id("41")
    );
    assert_eq!(ashfall.incoming_groups[1].subject.id, id("11"));
    assert_eq!(ashfall.incoming_groups[1].definition.id, def_id("38"));
    assert_eq!(
        ashfall.incoming_groups[1].claims[0].assertion.id,
        ass_id("4c")
    );
    assert_eq!(ashfall.incoming_groups[2].subject.id, id("12"));
    assert_eq!(ashfall.incoming_groups[2].definition.id, def_id("31"));
    assert_eq!(
        ashfall.incoming_groups[2].claims[0].assertion.id,
        ass_id("43")
    );

    // Deterministic repeatability, missing entity fails, no bytes changed.
    assert_eq!(mara, store.entity_context(&id("11")).unwrap());
    assert!(matches!(
        store.entity_context(&id("99")),
        Err(Error::Validation {
            reason: ValidationReason::NotFound,
            ..
        })
    ));
    drop(store);
    assert_eq!(support::bytes(&scratch.path), before);
    assert_eq!(
        Store::open(&scratch.path)
            .unwrap()
            .entity_context(&id("11"))
            .unwrap(),
        mara
    );
}

#[test]
fn independent_archive_bits_zero_claim_archived_defs_and_self_reference_work() {
    let scratch = support::scratch("context-archives-selfref.sqlite");
    let universe = UniverseId::parse("11111111-1111-4111-8111-111111111111").unwrap();
    let mut store = Store::create(&scratch.path, universe).unwrap();

    let kind = TypeId::parse(&uuid_n("22222222", 1)).unwrap();
    store
        .create_entity_type(EntityType {
            id: kind.clone(),
            label: "Person".into(),
        })
        .unwrap();
    let e_self = EntityId::parse(&uuid_n("33333333", 1)).unwrap();
    let e_other = EntityId::parse(&uuid_n("33333333", 2)).unwrap();
    let e_untyped = EntityId::parse(&uuid_n("33333333", 3)).unwrap();
    for (eid, label, tid) in [
        (&e_self, "Rowan", Some(kind.clone())),
        (&e_other, "Kael", Some(kind.clone())),
        (&e_untyped, "Relic", None),
    ] {
        store
            .create_entity(Entity {
                id: eid.clone(),
                label: label.into(),
                type_id: tid,
                page_id: None,
            })
            .unwrap();
    }

    // 1: active def with archived-only claim
    let d_active_archived_only = DefinitionId::parse(&uuid_n("44444444", 1)).unwrap();
    // 2: active def with both active and archived claims
    let d_active_mixed = DefinitionId::parse(&uuid_n("44444444", 2)).unwrap();
    // 3: active def for self-reference
    let d_self_ref = DefinitionId::parse(&uuid_n("44444444", 3)).unwrap();
    // 4: archived def with active-bit claim (definition archived after claim created)
    let d_arch_def_active_claim = DefinitionId::parse(&uuid_n("44444444", 4)).unwrap();
    // 5: archived def with zero claims
    let d_arch_empty = DefinitionId::parse(&uuid_n("44444444", 5)).unwrap();

    for (did, owner, vc, label, archived_init) in [
        (
            &d_active_archived_only,
            Owner::Type(kind.clone()),
            ValueClass::Text,
            Some("old title"),
            false,
        ),
        (
            &d_active_mixed,
            Owner::Entity(e_self.clone()),
            ValueClass::Text,
            Some("alias"),
            false,
        ),
        (
            &d_self_ref,
            Owner::Type(kind.clone()),
            ValueClass::Reference,
            Some("mentor"),
            false,
        ),
        (
            &d_arch_def_active_claim,
            Owner::Type(kind.clone()),
            ValueClass::Text,
            None,
            false,
        ),
        (
            &d_arch_empty,
            Owner::Entity(e_self.clone()),
            ValueClass::Boolean,
            Some("unused archived"),
            true,
        ),
    ] {
        store
            .create_definition(Definition {
                id: did.clone(),
                owner,
                value_class: vc,
                cardinality: Cardinality::Multiple,
                label: label.map(|l| DefinitionLabel::parse(l).unwrap()),
                archived: archived_init,
            })
            .unwrap();
    }

    let a1 = AssertionId::parse(&uuid_n("55555555", 1)).unwrap();
    let a2 = AssertionId::parse(&uuid_n("55555555", 2)).unwrap();
    let a3 = AssertionId::parse(&uuid_n("55555555", 3)).unwrap();
    let a4 = AssertionId::parse(&uuid_n("55555555", 4)).unwrap();
    let a5 = AssertionId::parse(&uuid_n("55555555", 5)).unwrap();
    let a6 = AssertionId::parse(&uuid_n("55555555", 6)).unwrap();

    for a in [
        Assertion {
            id: a1.clone(),
            subject_id: e_self.clone(),
            definition_id: d_active_archived_only.clone(),
            value: Value::Text("former warden".into()),
            canon: Canon::Acordado,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: true,
        },
        Assertion {
            id: a2.clone(),
            subject_id: e_self.clone(),
            definition_id: d_active_mixed.clone(),
            value: Value::Text("Grey".into()),
            canon: Canon::Provisional,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        },
        Assertion {
            id: a3.clone(),
            subject_id: e_self.clone(),
            definition_id: d_active_mixed.clone(),
            value: Value::Text("Ash".into()),
            canon: Canon::Abierto,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: true,
        },
        // Self-reference: Rowan -> mentor -> Rowan (active)
        Assertion {
            id: a4.clone(),
            subject_id: e_self.clone(),
            definition_id: d_self_ref.clone(),
            value: Value::Reference(e_self.clone()),
            canon: Canon::Acordado,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        },
        // Archived incoming from Kael -> mentor -> Rowan
        Assertion {
            id: a5.clone(),
            subject_id: e_other.clone(),
            definition_id: d_self_ref.clone(),
            value: Value::Reference(e_self.clone()),
            canon: Canon::Provisional,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: true,
        },
        // Active-bit assertion under a soon-to-be-archived unnamed definition
        Assertion {
            id: a6.clone(),
            subject_id: e_self.clone(),
            definition_id: d_arch_def_active_claim.clone(),
            value: Value::Text("active-bit claim".into()),
            canon: Canon::Acordado,
            source_status_raw: None,
            provenance: Provenance::absent(),
            scope_note: None,
            archived: false,
        },
    ] {
        store.create_assertion(a).unwrap();
    }
    store.archive_definition(&d_arch_def_active_claim).unwrap();

    let ctx = store.entity_context(&e_self).unwrap();
    // Active outgoing has 3 active definitions (1, 2, 3), NOT 4 or 5.
    assert_eq!(ctx.outgoing_groups.len(), 3);
    assert_eq!(ctx.outgoing_groups[0].definition.id, d_active_archived_only);
    assert!(ctx.outgoing_groups[0].claims.is_empty());
    assert!(ctx.outgoing_groups[0].has_archived_claims);

    assert_eq!(ctx.outgoing_groups[1].definition.id, d_active_mixed);
    assert_eq!(ctx.outgoing_groups[1].claims.len(), 1);
    assert_eq!(ctx.outgoing_groups[1].claims[0].assertion.id, a2);
    assert!(ctx.outgoing_groups[1].has_archived_claims);

    assert_eq!(ctx.outgoing_groups[2].definition.id, d_self_ref);
    assert_eq!(ctx.outgoing_groups[2].claims.len(), 1);
    assert_eq!(ctx.outgoing_groups[2].claims[0].assertion.id, a4);

    // Self-reference also appears in active incoming_groups!
    assert_eq!(ctx.incoming_groups.len(), 1);
    assert_eq!(ctx.incoming_groups[0].subject.id, e_self);
    assert_eq!(ctx.incoming_groups[0].definition.id, d_self_ref);
    assert_eq!(ctx.incoming_groups[0].claims[0].assertion.id, a4);

    // Archived outgoing groups: defs 1, 2, 4 (active-bit claim!), 5 (0 claims!).
    assert_eq!(ctx.archived_outgoing_groups.len(), 4);
    assert_eq!(
        ctx.archived_outgoing_groups[0].definition.id,
        d_active_archived_only
    );
    assert_eq!(ctx.archived_outgoing_groups[0].claims[0].assertion.id, a1);
    assert!(!ctx.archived_outgoing_groups[0].definition.archived);
    assert!(ctx.archived_outgoing_groups[0].claims[0].assertion.archived);

    assert_eq!(
        ctx.archived_outgoing_groups[1].definition.id,
        d_active_mixed
    );
    assert_eq!(ctx.archived_outgoing_groups[1].claims[0].assertion.id, a3);

    assert_eq!(
        ctx.archived_outgoing_groups[2].definition.id,
        d_arch_def_active_claim
    );
    assert!(ctx.archived_outgoing_groups[2].definition.archived);
    assert_eq!(ctx.archived_outgoing_groups[2].claims.len(), 1);
    assert!(!ctx.archived_outgoing_groups[2].claims[0].assertion.archived);

    assert_eq!(ctx.archived_outgoing_groups[3].definition.id, d_arch_empty);
    assert!(ctx.archived_outgoing_groups[3].definition.archived);
    assert!(ctx.archived_outgoing_groups[3].claims.is_empty());

    // Archived incoming: Kael's archived claim a5
    assert_eq!(ctx.archived_incoming_groups.len(), 1);
    assert_eq!(ctx.archived_incoming_groups[0].subject.id, e_other);
    assert_eq!(ctx.archived_incoming_groups[0].claims[0].assertion.id, a5);

    // Untyped entity with zero applicable definitions
    let untyped = store.entity_context(&e_untyped).unwrap();
    assert!(untyped.type_label.is_none());
    assert!(untyped.outgoing_groups.is_empty());
    assert!(untyped.incoming_groups.is_empty());
    assert!(untyped.archived_outgoing_groups.is_empty());
    assert!(untyped.archived_incoming_groups.is_empty());
}

#[test]
fn busy_and_corrupt_active_or_archived_records_fail_closed_without_writes() {
    let scratch = support::scratch("context-errors.sqlite");
    let store = fixture(&scratch.path);
    let other = Connection::open(&scratch.path).unwrap();

    other.execute_batch("BEGIN EXCLUSIVE").unwrap();
    assert!(matches!(store.entity_context(&id("11")), Err(Error::Busy)));
    other.execute_batch("ROLLBACK").unwrap();

    // Tamper with an archived assertion's canon (004a is archived under archived def 0036).
    let before = support::bytes(&scratch.path);
    other
        .execute_batch(
            "PRAGMA ignore_check_constraints=ON;
             UPDATE assertions SET canon='CORRUPT_CANON' WHERE id='00000000-0000-4000-8000-00000000004a'",
        )
        .unwrap();
    let after_tamper = support::bytes(&scratch.path);
    assert_ne!(before, after_tamper);
    assert!(matches!(
        store.entity_context(&id("11")),
        Err(Error::Storage { .. })
    ));
    assert_eq!(support::bytes(&scratch.path), after_tamper);

    // Fix canon, then tamper with definition label whitespace.
    other
        .execute_batch(
            "UPDATE assertions SET canon='ACORDADO' WHERE id='00000000-0000-4000-8000-00000000004a';
             UPDATE definitions SET label=' bad ' WHERE id='00000000-0000-4000-8000-000000000036'",
        )
        .unwrap();
    assert!(matches!(
        store.entity_context(&id("11")),
        Err(Error::Storage { .. })
    ));
}

#[test]
fn synchronized_concurrent_writer_observes_atomic_snapshot_and_releases_read_lock() {
    use std::sync::mpsc;

    let scratch = support::scratch("context-concurrent.sqlite");
    let mut writer_store = fixture(&scratch.path);
    let universe = writer_store.export_snapshot().unwrap().universe_id;
    let reader_store = Store::open(&scratch.path).unwrap();

    // Part A: Deterministic overlap with an in-flight uncommitted single transaction.
    let def_a = DefinitionId::parse(&uuid_n("44444444", 98)).unwrap();
    let ass_a = AssertionId::parse(&uuid_n("55555555", 98)).unwrap();
    let (def_inserted_tx, def_inserted_rx) = mpsc::channel();
    let (reader_checked_tx, reader_checked_rx) = mpsc::channel();
    let path_for_writer = scratch.path.clone();

    let def_a_writer = def_a.clone();
    let ass_a_writer = ass_a.clone();
    let writer_a = thread::spawn(move || {
        let conn = Connection::open(&path_for_writer).unwrap();
        conn.execute_batch("BEGIN IMMEDIATE").unwrap();
        conn.execute(
            "INSERT INTO definitions (id, owner_type_id, owner_entity_id, value_class, cardinality, archived, label)
             VALUES (?1, NULL, ?2, 'text', 'singular', 0, 'in-flight')",
            rusqlite::params![def_a_writer.as_str(), id("11").as_str()],
        )
        .unwrap();
        def_inserted_tx.send(()).unwrap();
        reader_checked_rx.recv().unwrap();
        conn.execute(
            "INSERT INTO assertions (id, subject_id, definition_id, value_kind, text_value, canon, archived)
             VALUES (?1, ?2, ?3, 'text', 'committed-together', 'ACORDADO', 0)",
            rusqlite::params![ass_a_writer.as_str(), id("11").as_str(), def_a_writer.as_str()],
        )
        .unwrap();
        conn.execute_batch("COMMIT").unwrap();
    });

    def_inserted_rx.recv().unwrap();
    // While the single write tx holds RESERVED lock with def_a uncommitted and ass_a not yet inserted,
    // reader observes strictly all-old state (neither def_a nor ass_a).
    let pre_ctx = reader_store.entity_context(&id("11")).unwrap();
    assert!(!pre_ctx
        .outgoing_groups
        .iter()
        .any(|g| g.definition.id == def_a));
    assert!(!pre_ctx
        .outgoing_groups
        .iter()
        .flat_map(|g| g.claims.iter())
        .any(|c| c.assertion.id == ass_a));
    reader_checked_tx.send(()).unwrap();
    writer_a.join().unwrap();

    let post_a_ctx = reader_store.entity_context(&id("11")).unwrap();
    assert!(post_a_ctx
        .outgoing_groups
        .iter()
        .any(|g| g.definition.id == def_a && g.claims.iter().any(|c| c.assertion.id == ass_a)));

    // Part B: Single atomic Store::import_snapshot batch against synchronized concurrent reader.
    let def_b = DefinitionId::parse(&uuid_n("44444444", 99)).unwrap();
    let ass_b = AssertionId::parse(&uuid_n("55555555", 99)).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let reader_barrier = Arc::clone(&barrier);
    let def_b_reader = def_b.clone();
    let path_for_reader = scratch.path.clone();

    let reader_handle = thread::spawn(move || {
        let mut snapshots = Vec::new();
        reader_barrier.wait();
        loop {
            match reader_store.entity_context(&id("11")) {
                Ok(ctx) => {
                    let committed = ctx
                        .outgoing_groups
                        .iter()
                        .any(|g| g.definition.id == def_b_reader);
                    snapshots.push(ctx);
                    if committed && snapshots.len() >= 2 {
                        break;
                    }
                    if committed && snapshots.len() == 1 {
                        // Read one more snapshot post-commit and stop.
                        continue;
                    }
                }
                Err(Error::Busy) => {}
                Err(other) => panic!("unexpected reader error: {other}"),
            }
        }
        // Confirm reader store holds NO open transaction after entity_context returns.
        let checker = Connection::open(&path_for_reader).unwrap();
        checker.execute_batch("BEGIN IMMEDIATE; ROLLBACK;").unwrap();
        snapshots
    });

    barrier.wait();
    // Publish def_b and ass_b together in ONE atomic Store transaction.
    let outcome = writer_store
        .import_snapshot(Snapshot {
            universe_id: universe,
            pages: vec![],
            types: vec![],
            entities: vec![],
            definitions: vec![Definition {
                id: def_b.clone(),
                owner: Owner::Entity(id("11")),
                value_class: ValueClass::Text,
                cardinality: Cardinality::Singular,
                label: Some(DefinitionLabel::parse("callsign").unwrap()),
                archived: false,
            }],
            assertions: vec![Assertion {
                id: ass_b.clone(),
                subject_id: id("11"),
                definition_id: def_b.clone(),
                value: Value::Text("Lantern".into()),
                canon: Canon::Acordado,
                source_status_raw: None,
                provenance: Provenance::absent(),
                scope_note: None,
                archived: false,
            }],
        })
        .unwrap();
    assert_eq!(outcome.inserted, 2);

    let snapshots = reader_handle.join().unwrap();
    assert!(
        !snapshots.is_empty(),
        "concurrent reader must capture at least one successful snapshot"
    );
    for ctx in &snapshots {
        let has_def = ctx.outgoing_groups.iter().any(|g| g.definition.id == def_b);
        let has_ass = ctx
            .outgoing_groups
            .iter()
            .flat_map(|g| g.claims.iter())
            .any(|c| c.assertion.id == ass_b);
        // Every observed snapshot is strictly all-old or all-new (zero partial commit visibility).
        assert_eq!(
            has_def, has_ass,
            "partial snapshot: def={has_def} ass={has_ass}"
        );
        assert_eq!(ctx.entity.id, id("11"));
    }
    assert!(snapshots.iter().any(|ctx| {
        ctx.outgoing_groups
            .iter()
            .any(|g| g.definition.id == def_b && g.claims.iter().any(|c| c.assertion.id == ass_b))
    }));

    // Confirm no lingering read lock remains on the file.
    let conn = Connection::open(&scratch.path).unwrap();
    conn.execute_batch("BEGIN EXCLUSIVE; ROLLBACK;").unwrap();
}

#[test]
fn oversized_non_projected_owner_type_label_does_not_oom_or_block_small_context() {
    let scratch = support::scratch("context-oversized-owner-type.sqlite");
    let universe = UniverseId::parse("11111111-1111-4111-8111-111111111111").unwrap();
    let t_huge = TypeId::parse(&uuid_n("22222222", 1)).unwrap();
    let e_subject = EntityId::parse(&uuid_n("33333333", 1)).unwrap();
    let e_target = EntityId::parse(&uuid_n("33333333", 2)).unwrap();
    let d_rel = DefinitionId::parse(&uuid_n("44444444", 1)).unwrap();
    let a_rel = AssertionId::parse(&uuid_n("55555555", 1)).unwrap();

    // Type label is 3 MiB (> MAX_CONTEXT_BYTES), but it belongs ONLY to the incoming
    // claim's subject type and is NOT projected into `e_target`'s EntityContext.
    let store = Store::import_new(
        &scratch.path,
        Snapshot {
            universe_id: universe,
            pages: vec![],
            types: vec![EntityType {
                id: t_huge.clone(),
                label: "T".repeat(3 * 1024 * 1024),
            }],
            entities: vec![
                Entity {
                    id: e_subject.clone(),
                    label: "Courier".into(),
                    type_id: Some(t_huge.clone()),
                    page_id: None,
                },
                Entity {
                    id: e_target.clone(),
                    label: "Harbor".into(),
                    type_id: None,
                    page_id: None,
                },
            ],
            definitions: vec![Definition {
                id: d_rel.clone(),
                owner: Owner::Type(t_huge),
                value_class: ValueClass::Reference,
                cardinality: Cardinality::Multiple,
                label: Some(DefinitionLabel::parse("docked at").unwrap()),
                archived: false,
            }],
            assertions: vec![Assertion {
                id: a_rel.clone(),
                subject_id: e_subject.clone(),
                definition_id: d_rel.clone(),
                value: Value::Reference(e_target.clone()),
                canon: Canon::Acordado,
                source_status_raw: None,
                provenance: Provenance::absent(),
                scope_note: None,
                archived: false,
            }],
        },
    )
    .unwrap()
    .0;

    let ctx = store.entity_context(&e_target).unwrap();
    assert_eq!(ctx.entity.id, e_target);
    assert!(ctx.type_label.is_none());
    assert_eq!(ctx.incoming_groups.len(), 1);
    assert_eq!(ctx.incoming_groups[0].subject.id, e_subject);
    assert_eq!(ctx.incoming_groups[0].definition.id, d_rel);
    assert_eq!(ctx.incoming_groups[0].claims[0].assertion.id, a_rel);
}

#[test]
fn missing_definition_owner_fails_closed_without_writing_bytes() {
    // 1. Missing Owner::Type fails closed with UnknownType and mutates nothing.
    {
        let scratch = support::scratch("context-missing-owner-type.sqlite");
        let _store = fixture(&scratch.path);
        let conn = Connection::open(&scratch.path).unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = OFF;
             DELETE FROM types WHERE id = '00000000-0000-4000-8000-000000000021';
             UPDATE entities SET type_id = NULL WHERE id = '00000000-0000-4000-8000-000000000013';",
        )
        .unwrap();
        let before = support::bytes(&scratch.path);
        let store = Store::open(&scratch.path).unwrap();
        // Inspecting Ashfall Order (0013, untyped) loads incoming claim 0041 from Mara Vey,
        // whose definition 0031 is owned by missing type 0021.
        let err = store.entity_context(&id("13")).unwrap_err();
        assert!(
            matches!(
                err,
                Error::Validation {
                    reason: ValidationReason::UnknownType,
                    field: "owner_type_id",
                    ..
                }
            ),
            "expected UnknownType, got {err:?}"
        );
        assert_eq!(support::bytes(&scratch.path), before);
    }

    // 2. Missing Owner::Entity fails closed with UnknownEntity and mutates nothing.
    {
        let scratch = support::scratch("context-missing-owner-entity.sqlite");
        let universe = UniverseId::parse("11111111-1111-4111-8111-111111111111").unwrap();
        let e1 = id("11");
        let e2 = id("12");
        let d_ent = def_id("39");
        let mut store = Store::import_new(
            &scratch.path,
            Snapshot {
                universe_id: universe,
                pages: vec![],
                types: vec![],
                entities: vec![
                    Entity {
                        id: e1.clone(),
                        label: "One".into(),
                        type_id: None,
                        page_id: None,
                    },
                    Entity {
                        id: e2.clone(),
                        label: "Two".into(),
                        type_id: None,
                        page_id: None,
                    },
                ],
                definitions: vec![Definition {
                    id: d_ent.clone(),
                    owner: Owner::Entity(e2.clone()),
                    value_class: ValueClass::Text,
                    cardinality: Cardinality::Singular,
                    label: Some(DefinitionLabel::parse("tag").unwrap()),
                    archived: false,
                }],
                assertions: vec![],
            },
        )
        .unwrap()
        .0;
        drop(store);
        let conn = Connection::open(&scratch.path).unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = OFF;
             UPDATE definitions SET owner_entity_id = '00000000-0000-4000-8000-000000000099'
             WHERE id = '00000000-0000-4000-8000-000000000039';",
        )
        .unwrap();
        // Trigger lookup via a dangling outgoing assertion referencing d_ent.
        conn.execute(
            "INSERT INTO assertions (id, subject_id, definition_id, value_kind, text_value, canon, archived)
             VALUES (?1, ?2, ?3, 'text', 'v', 'ACORDADO', 0)",
            rusqlite::params![ass_id("51").as_str(), e1.as_str(), d_ent.as_str()],
        )
        .unwrap();
        drop(conn);
        let before = support::bytes(&scratch.path);
        store = Store::open(&scratch.path).unwrap();
        let err = store.entity_context(&e1).unwrap_err();
        assert!(
            matches!(
                err,
                Error::Validation {
                    reason: ValidationReason::UnknownEntity,
                    field: "owner_entity_id",
                    ..
                }
            ),
            "expected UnknownEntity, got {err:?}"
        );
        assert_eq!(support::bytes(&scratch.path), before);
    }
}

#[test]
fn context_bounds_enforce_group_unique_assertion_and_byte_edges() {
    // 1. Group limit: MAX_CONTEXT_GROUPS (128) succeeds, +1 (129) fails closed.
    {
        let scratch = support::scratch("context-limit-groups.sqlite");
        let universe = UniverseId::parse("11111111-1111-4111-8111-111111111111").unwrap();
        let e1 = EntityId::parse(&uuid_n("33333333", 1)).unwrap();
        let defs: Vec<Definition> = (1..=MAX_CONTEXT_GROUPS)
            .map(|i| Definition {
                id: DefinitionId::parse(&uuid_n("44444444", i)).unwrap(),
                owner: Owner::Entity(e1.clone()),
                value_class: ValueClass::Text,
                cardinality: Cardinality::Singular,
                label: Some(DefinitionLabel::parse(&format!("prop-{i}")).unwrap()),
                archived: false,
            })
            .collect();
        let mut store = Store::import_new(
            &scratch.path,
            Snapshot {
                universe_id: universe,
                pages: vec![],
                types: vec![],
                entities: vec![Entity {
                    id: e1.clone(),
                    label: "Target".into(),
                    type_id: None,
                    page_id: None,
                }],
                definitions: defs,
                assertions: vec![],
            },
        )
        .unwrap()
        .0;

        let ok = store.entity_context(&e1).unwrap();
        assert_eq!(ok.outgoing_groups.len(), MAX_CONTEXT_GROUPS);

        // +1 archived definition pushes groups to MAX_CONTEXT_GROUPS + 1 -> ContextTooLarge.
        store
            .create_definition(Definition {
                id: DefinitionId::parse(&uuid_n("44444444", MAX_CONTEXT_GROUPS + 1)).unwrap(),
                owner: Owner::Entity(e1.clone()),
                value_class: ValueClass::Text,
                cardinality: Cardinality::Singular,
                label: Some(DefinitionLabel::parse("prop-overflow").unwrap()),
                archived: true,
            })
            .unwrap();
        let err = store.entity_context(&e1).unwrap_err();
        assert!(
            matches!(
                err,
                Error::Validation {
                    reason: ValidationReason::ContextTooLarge,
                    ..
                }
            ),
            "expected ContextTooLarge, got {err:?}"
        );
    }

    // 2. Unique assertion limit: MAX_CONTEXT_UNIQUE_ASSERTIONS (500) self-references succeed, +1 fails.
    {
        let scratch = support::scratch("context-limit-assertions.sqlite");
        let universe = UniverseId::parse("11111111-1111-4111-8111-111111111111").unwrap();
        let e1 = EntityId::parse(&uuid_n("33333333", 1)).unwrap();
        let d1 = DefinitionId::parse(&uuid_n("44444444", 1)).unwrap();
        let assertions: Vec<Assertion> = (1..=MAX_CONTEXT_UNIQUE_ASSERTIONS)
            .map(|i| Assertion {
                id: AssertionId::parse(&uuid_n("55555555", i)).unwrap(),
                subject_id: e1.clone(),
                definition_id: d1.clone(),
                value: Value::Reference(e1.clone()),
                canon: Canon::Abierto,
                source_status_raw: None,
                provenance: Provenance::absent(),
                scope_note: None,
                archived: false,
            })
            .collect();
        let mut store = Store::import_new(
            &scratch.path,
            Snapshot {
                universe_id: universe,
                pages: vec![],
                types: vec![],
                entities: vec![Entity {
                    id: e1.clone(),
                    label: "SelfRef".into(),
                    type_id: None,
                    page_id: None,
                }],
                definitions: vec![Definition {
                    id: d1.clone(),
                    owner: Owner::Entity(e1.clone()),
                    value_class: ValueClass::Reference,
                    cardinality: Cardinality::Multiple,
                    label: Some(DefinitionLabel::parse("mirrors").unwrap()),
                    archived: false,
                }],
                assertions,
            },
        )
        .unwrap()
        .0;

        let ok = store.entity_context(&e1).unwrap();
        assert_eq!(
            ok.outgoing_groups[0].claims.len(),
            MAX_CONTEXT_UNIQUE_ASSERTIONS
        );
        assert_eq!(
            ok.incoming_groups[0].claims.len(),
            MAX_CONTEXT_UNIQUE_ASSERTIONS
        );

        store
            .create_assertion(Assertion {
                id: AssertionId::parse(&uuid_n("55555555", MAX_CONTEXT_UNIQUE_ASSERTIONS + 1))
                    .unwrap(),
                subject_id: e1.clone(),
                definition_id: d1,
                value: Value::Reference(e1.clone()),
                canon: Canon::Abierto,
                source_status_raw: None,
                provenance: Provenance::absent(),
                scope_note: None,
                archived: false,
            })
            .unwrap();
        let err = store.entity_context(&e1).unwrap_err();
        assert!(
            matches!(
                err,
                Error::Validation {
                    reason: ValidationReason::ContextTooLarge,
                    ..
                }
            ),
            "expected ContextTooLarge, got {err:?}"
        );
    }

    // 3. Byte limit & oversized single field + JSON escaping overhead.
    {
        let scratch = support::scratch("context-limit-bytes.sqlite");
        let universe = UniverseId::parse("11111111-1111-4111-8111-111111111111").unwrap();
        let e1 = EntityId::parse(&uuid_n("33333333", 1)).unwrap();
        let d1 = DefinitionId::parse(&uuid_n("44444444", 1)).unwrap();
        let mut store = Store::import_new(
            &scratch.path,
            Snapshot {
                universe_id: universe,
                pages: vec![],
                types: vec![],
                entities: vec![Entity {
                    id: e1.clone(),
                    label: "Bounded".into(),
                    type_id: None,
                    page_id: None,
                }],
                definitions: vec![Definition {
                    id: d1.clone(),
                    owner: Owner::Entity(e1.clone()),
                    value_class: ValueClass::Text,
                    cardinality: Cardinality::Multiple,
                    label: Some(DefinitionLabel::parse("notes").unwrap()),
                    archived: false,
                }],
                assertions: vec![],
            },
        )
        .unwrap()
        .0;

        // Oversized single text value (> 2 MiB).
        let huge = "a".repeat(MAX_CONTEXT_BYTES);
        let a_huge = AssertionId::parse(&uuid_n("55555555", 1)).unwrap();
        store
            .create_assertion(Assertion {
                id: a_huge.clone(),
                subject_id: e1.clone(),
                definition_id: d1.clone(),
                value: Value::Text(huge),
                canon: Canon::Acordado,
                source_status_raw: None,
                provenance: Provenance::absent(),
                scope_note: None,
                archived: false,
            })
            .unwrap();
        assert!(matches!(
            store.entity_context(&e1),
            Err(Error::Validation {
                reason: ValidationReason::ContextTooLarge,
                ..
            })
        ));

        // Escaping amplification: 1.1 MiB of quotes (`"`) escapes to > 2.2 MiB JSON.
        store.archive_assertion(&a_huge).unwrap();
        drop(store);
        let conn = Connection::open(&scratch.path).unwrap();
        let quoted = "\"".repeat(1_100_000);
        conn.execute(
            "UPDATE assertions SET text_value = ?1, archived = 0 WHERE id = ?2",
            rusqlite::params![quoted, a_huge.as_str()],
        )
        .unwrap();
        drop(conn);
        let store = Store::open(&scratch.path).unwrap();
        assert!(matches!(
            store.entity_context(&e1),
            Err(Error::Validation {
                reason: ValidationReason::ContextTooLarge,
                ..
            })
        ));
    }
}

#[test]
fn query_plans_and_scale_measurement_report() {
    let scratch = support::scratch("context-query-plan.sqlite");
    let universe = UniverseId::parse("11111111-1111-4111-8111-111111111111").unwrap();
    let t_person = TypeId::parse(&uuid_n("22222222", 1)).unwrap();
    let t_other = TypeId::parse(&uuid_n("22222222", 2)).unwrap();
    let e1 = EntityId::parse(&uuid_n("33333333", 1)).unwrap();
    let e2 = EntityId::parse(&uuid_n("33333333", 2)).unwrap();

    // 2,000 unrelated definitions + 40 applicable definitions + 300 assertions.
    let mut defs = Vec::with_capacity(2040);
    for i in 1..=2000 {
        defs.push(Definition {
            id: DefinitionId::parse(&uuid_n("44444444", i)).unwrap(),
            owner: Owner::Type(t_other.clone()),
            value_class: ValueClass::Text,
            cardinality: Cardinality::Singular,
            label: Some(DefinitionLabel::parse(&format!("other-{i}")).unwrap()),
            archived: false,
        });
    }
    for i in 2001..=2040 {
        defs.push(Definition {
            id: DefinitionId::parse(&uuid_n("44444444", i)).unwrap(),
            owner: if i % 2 == 0 {
                Owner::Type(t_person.clone())
            } else {
                Owner::Entity(e1.clone())
            },
            value_class: ValueClass::Reference,
            cardinality: Cardinality::Multiple,
            label: Some(DefinitionLabel::parse(&format!("rel-{i}")).unwrap()),
            archived: false,
        });
    }
    let mut assertions = Vec::with_capacity(300);
    for i in 1..=300 {
        let def_idx = 2001 + (i % 40);
        assertions.push(Assertion {
            id: AssertionId::parse(&uuid_n("55555555", i)).unwrap(),
            subject_id: e1.clone(),
            definition_id: DefinitionId::parse(&uuid_n("44444444", def_idx)).unwrap(),
            value: Value::Reference(e2.clone()),
            canon: Canon::Provisional,
            source_status_raw: Some("draft".into()),
            provenance: Provenance {
                external_ref: Some("ref".into()),
                locator: Some(format!("folio {i}")),
                fragment: None,
                source_revision: Some("rev-1".into()),
            },
            scope_note: Some("synthetic scale check".into()),
            archived: i % 10 == 0,
        });
    }

    let store = Store::import_new(
        &scratch.path,
        Snapshot {
            universe_id: universe,
            pages: vec![],
            types: vec![
                EntityType {
                    id: t_person.clone(),
                    label: "Person".into(),
                },
                EntityType {
                    id: t_other.clone(),
                    label: "Other".into(),
                },
            ],
            entities: vec![
                Entity {
                    id: e1.clone(),
                    label: "Mara".into(),
                    type_id: Some(t_person),
                    page_id: None,
                },
                Entity {
                    id: e2,
                    label: "Target".into(),
                    type_id: Some(t_other),
                    page_id: None,
                },
            ],
            definitions: defs,
            assertions,
        },
    )
    .unwrap()
    .0;

    let start = Instant::now();
    let ctx = store.entity_context(&e1).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(ctx.outgoing_groups.len(), 40);
    assert!(elapsed.as_millis() < 250, "context read took {elapsed:?}");

    drop(store);
    let conn = Connection::open(&scratch.path).unwrap();
    let plan_for = |sql: &str| -> Vec<String> {
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        rows
    };

    let def_plan = plan_for(
        "SELECT id, owner_type_id, owner_entity_id, value_class, cardinality, archived, label \
         FROM definitions WHERE owner_entity_id = 'a' OR owner_type_id = 'b' ORDER BY id",
    );
    let out_plan =
        plan_for("SELECT id, subject_id, definition_id FROM assertions WHERE subject_id = 'a'");
    let in_plan = plan_for(
        "SELECT id, subject_id, definition_id FROM assertions WHERE target_entity_id = 'a'",
    );
    assert!(
        out_plan
            .iter()
            .any(|p| p.contains("idx_assertions_subject_definition")),
        "outgoing plan: {out_plan:?}"
    );
    assert!(
        !out_plan.iter().any(|p| p.contains("TEMP B-TREE")),
        "unexpected temp b-tree in outgoing plan: {out_plan:?}"
    );
    assert!(
        in_plan.iter().any(|p| p.contains("idx_assertions_target")),
        "incoming plan: {in_plan:?}"
    );
    assert!(
        !in_plan.iter().any(|p| p.contains("TEMP B-TREE")),
        "unexpected temp b-tree in incoming plan: {in_plan:?}"
    );
    eprintln!(
        "QUERY PLANS:\n  defs: {def_plan:?}\n  out:  {out_plan:?}\n  in:   {in_plan:?}\n  2040 defs + 300 claims elapsed: {elapsed:?}"
    );
}
