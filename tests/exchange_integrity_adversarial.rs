//! Adversarial probes that go past `tests/exchange_integrity.rs`: duplicate
//! keys in the positions that are easiest to get wrong, probe tricks, the
//! shapes of the import-new preflight (overlap is not its only failure), the
//! in-transaction referential checks that the preflight must not replace, and
//! an export that has to refuse a universe where only one page is corrupt.
//!
//! Every probe is deterministic, runs against a real SQLite file, asserts
//! literal values, and fails when the guard it covers is removed.

#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use narrative_universe::{
    export_json, import_json, import_new_json, Entity, EntityId, Error, MentionId, Page, PageId,
    PageRevision, Store, UniverseId, ValidationReason,
};
use rusqlite::Connection;
use std::path::Path;
use support::{bytes, scratch};

const UNIVERSE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const PAGE_ONE: &str = "22222222-2222-4222-8222-222222222221";
const PAGE_TWO: &str = "22222222-2222-4222-8222-222222222222";
const ENTITY_ONE: &str = "11111111-1111-4111-8111-111111111111";
const ENTITY_TWO: &str = "33333333-3333-4333-8333-333333333333";
const MENTION_ONE: &str = "dddddddd-1111-4ddd-8ddd-ddddddddddd1";
const MENTION_TWO: &str = "dddddddd-2222-4ddd-8ddd-ddddddddddd2";
const DEFINITION_ONE: &str = "ffffffff-ffff-4fff-8fff-ffffffffffff";
const ASSERTION_ONE: &str = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";

fn universe() -> UniverseId {
    UniverseId::parse(UNIVERSE).expect("universe")
}

fn page_id(raw: &str) -> PageId {
    PageId::parse(raw).expect("page")
}

fn entity(raw: &str) -> EntityId {
    EntityId::parse(raw).expect("entity")
}

fn new_page(raw: &str, title: &str, body: &str) -> Page {
    Page {
        id: page_id(raw),
        title: title.to_string(),
        body: body.to_string(),
    }
}

fn created(name: &str) -> (support::Scratch, Store) {
    let scratch = scratch(name);
    let store = Store::create(&scratch.path, universe()).expect("create");
    (scratch, store)
}

fn invalid_document(err: Error, needle: &str) {
    match err {
        Error::InvalidDocument { message } => {
            assert!(message.contains(needle), "want {needle:?} in {message:?}")
        }
        other => panic!("expected invalid document, got {other}"),
    }
}

/// A v2 envelope whose page member and trailing members are given raw, so
/// every probe can place its duplicate at its own position.
fn envelope(pages: &str, tail: &str) -> Vec<u8> {
    let head = format!(r#"{{"format_version":2,"universe_id":"{UNIVERSE}",{pages},"#);
    format!("{head}{tail}").into_bytes()
}

fn page_member(id: &str, title: &str, raw_body: &str, mentions: &str) -> String {
    let page =
        format!(r#"{{"id":"{id}","title":"{title}","body":{raw_body},"mentions":[{mentions}]}}"#);
    let mut member = String::from(r#""pages":["#);
    member.push_str(&page);
    member.push(']');
    member
}

fn occurrence_member(id: &str, entity_raw: &str, start_utf16: u32, end_utf16: u32) -> String {
    format!(
        r#"{{"id":"{id}","entity_id":"{entity_raw}","start_utf16":{start_utf16},"end_utf16":{end_utf16}}}"#
    )
}

fn assert_no_destination(destination: &Path) {
    assert!(
        !destination.exists(),
        "a refused document must not leave {} behind",
        destination.display()
    );
}

fn stored_mentions(path: &Path) -> i64 {
    let conn = Connection::open(path).expect("raw connection");
    let count = conn
        .query_row("SELECT COUNT(*) FROM mentions", [], |row| row.get(0))
        .expect("mention count");
    drop(conn);
    count
}

#[test]
fn an_entity_that_declares_a_label_twice_is_refused() {
    let (scratch, mut store) = created("duplicate-entity-label.sqlite");
    let before = bytes(&scratch.path);
    let document = envelope(
        &page_member(PAGE_ONE, "Ente", r#""Elara""#, ""),
        r#""types":[],"entities":[{"id":"11111111-1111-4111-8111-111111111111","label":"Elara","label":"Otra"}],"definitions":[],"assertions":[]}"#,
    );

    let err = import_json(&mut store, &document).expect_err("duplicate entity label");
    invalid_document(err, "duplicate field `label`");
    assert_eq!(bytes(&scratch.path), before);
}

#[test]
fn a_type_that_declares_a_label_twice_is_refused() {
    let (scratch, mut store) = created("duplicate-type-label.sqlite");
    let before = bytes(&scratch.path);
    let document = envelope(
        r#""pages":[]"#,
        r#""types":[{"id":"cccccccc-cccc-4ccc-8ccc-cccccccccccc","label":"Vara","label":"Basta"}],"entities":[],"definitions":[],"assertions":[]}"#,
    );

    let err = import_json(&mut store, &document).expect_err("duplicate type label");
    invalid_document(err, "duplicate field `label`");
    assert_eq!(bytes(&scratch.path), before);
}

#[test]
fn a_page_with_two_differing_non_empty_occurrence_lists_is_refused() {
    let (scratch, mut store) = created("duplicate-mentions-differ.sqlite");
    let before = bytes(&scratch.path);
    let first = format!(
        r#""mentions":[{}]"#,
        occurrence_member(MENTION_ONE, ENTITY_ONE, 0, 5)
    );
    let second = format!(
        r#""mentions":[{}]"#,
        occurrence_member(MENTION_TWO, ENTITY_TWO, 0, 5)
    );
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[{{"id":"{PAGE_ONE}","title":"Distinta","body":"Elara",{first},{second}}}],"types":[],"entities":[{{"id":"{ENTITY_ONE}","label":"Elara"}},{{"id":"{ENTITY_TWO}","label":"Otra"}}],"definitions":[],"assertions":[]}}"#,
    )
    .into_bytes();

    let err = import_json(&mut store, &document).expect_err("differing duplicate mentions");
    invalid_document(err, "duplicate field `mentions`");
    assert_eq!(bytes(&scratch.path), before);
}

#[test]
fn an_assertion_that_declares_a_field_twice_is_refused() {
    let (scratch, mut store) = created("duplicate-assertion-field.sqlite");
    let before = bytes(&scratch.path);
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[],"types":[],"entities":[{{"id":"{ENTITY_ONE}","label":"Elara"}}],"definitions":[{{"id":"{DEFINITION_ONE}","owner":{{"type":"entity","id":"{ENTITY_ONE}"}},"value_class":"text","cardinality":"singular","archived":false}}],"assertions":[{{"id":"{ASSERTION_ONE}","subject_id":"{ENTITY_ONE}","definition_id":"{DEFINITION_ONE}","value":{{"kind":"text","text":"a"}},"value":{{"kind":"boolean","value":true}},"canon":"ACORDADO","provenance":{{}},"archived":false}}]}}"#
    )
    .into_bytes();

    let err = import_json(&mut store, &document).expect_err("duplicate assertion field");
    invalid_document(err, "duplicate field `value`");
    assert_eq!(bytes(&scratch.path), before);
}

#[test]
fn a_value_that_declares_its_payload_twice_is_refused() {
    let (scratch, mut store) = created("duplicate-value-payload.sqlite");
    let before = bytes(&scratch.path);
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[],"types":[],"entities":[{{"id":"{ENTITY_ONE}","label":"Elara"}}],"definitions":[{{"id":"{DEFINITION_ONE}","owner":{{"type":"entity","id":"{ENTITY_ONE}"}},"value_class":"text","cardinality":"singular","archived":false}}],"assertions":[{{"id":"{ASSERTION_ONE}","subject_id":"{ENTITY_ONE}","definition_id":"{DEFINITION_ONE}","value":{{"kind":"text","text":"a","text":"b"}},"canon":"ACORDADO","provenance":{{}},"archived":false}}]}}"#
    )
    .into_bytes();

    let err = import_json(&mut store, &document).expect_err("duplicate value payload");
    invalid_document(err, "duplicate field `text`");
    assert_eq!(bytes(&scratch.path), before);
}

#[test]
fn a_document_that_is_a_json_array_is_refused() {
    let (scratch, mut store) = created("document-as-array.sqlite");
    let before = bytes(&scratch.path);

    let err = import_json(&mut store, b"[]").expect_err("array");
    invalid_document(err, "the document has no format_version");
    assert_eq!(bytes(&scratch.path), before);
}

#[test]
fn a_format_version_that_is_a_nested_object_is_refused() {
    let (scratch, mut store) = created("object-version.sqlite");
    let before = bytes(&scratch.path);
    let document = envelope(
        r#""format_version":{"v":2},"pages":[]"#,
        r#""types":[],"entities":[],"definitions":[],"assertions":[]}"#,
    );

    let err = import_json(&mut store, &document).expect_err("object version");
    invalid_document(err, "the format_version of the document is not an integer");
    assert_eq!(bytes(&scratch.path), before);
}

#[test]
fn a_format_version_that_is_not_an_integer_in_any_other_shape_is_refused() {
    let (scratch, mut store) = created("odd-version-shapes.sqlite");
    let before = bytes(&scratch.path);
    for version in [r#""2""#, "2.5", "null", "123456789012345678901234567890"] {
        let document = format!(
            r#"{{"format_version":{version},"universe_id":"{UNIVERSE}","pages":[],"types":[],"entities":[],"definitions":[],"assertions":[]}}"#
        )
        .into_bytes();
        let err = import_json(&mut store, &document)
            .expect_err("a version that is not an integer must be refused");
        invalid_document(err, "the format_version of the document is not an integer");
    }
    assert_eq!(bytes(&scratch.path), before);
}

#[test]
fn a_version_repeated_with_an_unknown_last_value_is_refused() {
    let (scratch, mut store) = created("duplicated-unknown-version.sqlite");
    let before = bytes(&scratch.path);
    let document = format!(
        r#"{{"format_version":2,"format_version":99,"universe_id":"{UNIVERSE}","pages":[],"types":[],"entities":[],"definitions":[],"assertions":[]}}"#
    )
    .into_bytes();

    match import_json(&mut store, &document).expect_err("duplicated unknown version") {
        Error::UnsupportedVersion { found } => assert_eq!(found, 99),
        other => panic!("expected unsupported version, got {other}"),
    }
    assert_eq!(bytes(&scratch.path), before);
}

#[test]
fn an_import_new_with_overlapping_occurrences_reserves_no_destination() {
    let scratch = scratch("import-new-overlap.sqlite");
    let destination = scratch.path.with_file_name("overlap.sqlite");
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[{{"id":"{PAGE_ONE}","title":"Solapado","body":"qwertyuiopasdfgh","mentions":[{first},{second}]}}],"types":[],"entities":[{{"id":"{ENTITY_ONE}","label":"Elara"}}],"definitions":[],"assertions":[]}}"#,
        first = occurrence_member(MENTION_ONE, ENTITY_ONE, 0, 10),
        second = occurrence_member(MENTION_TWO, ENTITY_ONE, 5, 15),
    )
    .into_bytes();

    let err = import_new_json(&destination, &document).expect_err("overlap");
    match err {
        Error::Validation { reason, field, id } => {
            assert_eq!(
                reason,
                ValidationReason::OverlappingMentions,
                "field {field:?}"
            );
            assert_eq!(field, "mentions");
            assert_eq!(id, Some(MENTION_TWO.to_string()));
        }
        other => panic!("expected overlapping mentions, got {other}"),
    }
    assert_no_destination(&destination);
}

#[test]
fn an_import_new_that_splits_a_surrogate_pair_reserves_no_destination() {
    let scratch = scratch("import-new-surrogate.sqlite");
    let destination = scratch.path.with_file_name("surrogate.sqlite");
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[{{"id":"{PAGE_ONE}","title":"Par","body":"a\ud83d\udcbeb","mentions":[{mention}]}}],"types":[],"entities":[{{"id":"{ENTITY_ONE}","label":"Elara"}}],"definitions":[],"assertions":[]}}"#,
        mention = occurrence_member(MENTION_ONE, ENTITY_ONE, 2, 4),
    )
    .into_bytes();

    let err = import_new_json(&destination, &document).expect_err("split surrogate pair");
    match err {
        Error::Validation { reason, field, id } => {
            assert_eq!(reason, ValidationReason::SplitScalar, "field {field:?}");
            assert_eq!(field, "start_utf16");
            assert_eq!(id, Some(MENTION_ONE.to_string()));
        }
        other => panic!("expected split scalar, got {other}"),
    }
    assert_no_destination(&destination);
}

#[test]
fn an_import_new_with_an_empty_body_and_an_occurrence_reserves_no_destination() {
    let scratch = scratch("import-new-empty-body.sqlite");
    let destination = scratch.path.with_file_name("empty.sqlite");
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[{{"id":"{PAGE_ONE}","title":"Vacia","body":"","mentions":[{mention}]}}],"types":[],"entities":[{{"id":"{ENTITY_ONE}","label":"Elara"}}],"definitions":[],"assertions":[]}}"#,
        mention = occurrence_member(MENTION_ONE, ENTITY_ONE, 0, 1),
    )
    .into_bytes();

    let err = import_new_json(&destination, &document).expect_err("empty body");
    match err {
        Error::Validation { reason, field, id } => {
            assert_eq!(
                reason,
                ValidationReason::InvalidMentionRange,
                "field {field:?}"
            );
            assert_eq!(field, "end_utf16");
            assert_eq!(id, Some(MENTION_ONE.to_string()));
        }
        other => panic!("expected invalid range, got {other}"),
    }
    assert_no_destination(&destination);
}

#[test]
fn a_healthy_v2_document_imports_into_a_new_path_and_round_trips() {
    let scratch = scratch("import-new-round-trip.sqlite");
    let destination = scratch.path.with_file_name("first.sqlite");
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[{{"id":"{PAGE_ONE}","title":"Sana","body":"Elara","mentions":[{mention}]}}],"types":[],"entities":[{{"id":"{ENTITY_ONE}","label":"Elara"}}],"definitions":[],"assertions":[]}}"#,
        mention = occurrence_member(MENTION_ONE, ENTITY_ONE, 0, 5),
    )
    .into_bytes();

    let (mut store, outcome) = import_new_json(&destination, &document).expect("import");
    assert_eq!(outcome.inserted, 2);

    let exported = export_json(&mut store).expect("export");
    let exported_text = String::from_utf8(exported.clone()).expect("utf8");
    assert!(exported_text.contains(MENTION_ONE));
    assert!(!exported_text.contains("revision"));

    let second = scratch.path.with_file_name("second.sqlite");
    let (round, round_outcome) = import_new_json(&second, &exported).expect("reimport");
    assert_eq!(round_outcome.inserted, 2);
    assert_eq!(round_outcome.unchanged, 0);
    let read = round.page_document(&page_id(PAGE_ONE)).expect("document");
    assert_eq!(read.body.body, "Elara");
    assert_eq!(read.body.mentions.len(), 1);
    assert_eq!(read.body.mentions[0].id.as_str(), MENTION_ONE);
    assert_eq!(read.body.mentions[0].entity_id.as_str(), ENTITY_ONE);
    assert_eq!(read.body.mentions[0].start_utf16, 0);
    assert_eq!(read.body.mentions[0].end_utf16, 5);
    assert_eq!(read.revision, PageRevision::new(0).expect("revision"));
}

#[test]
fn the_transaction_refuses_an_occurrence_that_points_at_an_unknown_entity() {
    let (scratch, mut store) = created("import-unknown-entity.sqlite");
    let before = bytes(&scratch.path);
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[{{"id":"{PAGE_ONE}","title":"Huerfana","body":"Elara","mentions":[{mention}]}}],"types":[],"entities":[],"definitions":[],"assertions":[]}}"#,
        mention = occurrence_member(MENTION_ONE, ENTITY_TWO, 0, 5),
    )
    .into_bytes();

    // The geometry is document-local and valid, so the import-new preflight
    // passes: the in-transaction referential check against the combined
    // imported/existing entity set has to decide.
    let err = import_json(&mut store, &document).expect_err("unknown entity");
    match err {
        Error::Validation { reason, field, id } => {
            assert_eq!(reason, ValidationReason::UnknownEntity, "field {field:?}");
            assert_eq!(field, "entity_id");
            assert_eq!(id, Some(ENTITY_TWO.to_string()));
        }
        other => panic!("expected unknown entity, got {other}"),
    }
    // Nothing was stored, not even the page whose occurrence points nowhere.
    let counts = store.counts().expect("counts");
    assert_eq!(counts.pages, 0);
    assert_eq!(counts.entities, 0);
    assert_eq!(stored_mentions(&scratch.path), 0);
    assert_eq!(bytes(&scratch.path), before);
}

#[test]
fn an_occurrence_may_point_at_an_entity_the_destination_already_owns() {
    let (_scratch, mut store) = created("import-owned-entity.sqlite");
    store
        .create_entity(Entity {
            id: entity(ENTITY_ONE),
            label: "Elara".to_string(),
            page_id: None,
            type_id: None,
        })
        .expect("entity");
    let document = format!(
        r#"{{"format_version":2,"universe_id":"{UNIVERSE}","pages":[{{"id":"{PAGE_ONE}","title":"Propia","body":"Elara","mentions":[{mention}]}}],"types":[],"entities":[],"definitions":[],"assertions":[]}}"#,
        mention = occurrence_member(MENTION_ONE, ENTITY_ONE, 0, 5),
    )
    .into_bytes();

    let outcome = import_json(&mut store, &document).expect("import");
    assert_eq!(outcome.inserted, 1);
    assert_eq!(outcome.unchanged, 0);
    let mentions = store
        .mentions_for_entity(&entity(ENTITY_ONE))
        .expect("mentions");
    assert_eq!(mentions.len(), 1);
    assert_eq!(
        mentions[0].mention.id,
        MentionId::parse(MENTION_ONE).expect("mention")
    );
}

#[test]
fn an_export_refuses_the_whole_universe_when_one_page_is_corrupt() {
    let (scratch, mut store) = created("export-partial-corruption.sqlite");
    store
        .create_entity(Entity {
            id: entity(ENTITY_ONE),
            label: "Elara".to_string(),
            page_id: None,
            type_id: None,
        })
        .expect("entity");
    store
        .create_page(new_page(PAGE_ONE, "Corrupta", "Elara"))
        .expect("page");
    store
        .create_page(new_page(PAGE_TWO, "Sana", "texto"))
        .expect("page");
    drop(store);

    let conn = Connection::open(&scratch.path).expect("raw connection");
    conn.execute_batch("PRAGMA ignore_check_constraints = ON")
        .expect("checks");
    conn.execute("UPDATE pages SET revision = -3 WHERE id = ?1", [PAGE_ONE])
        .expect("corrupt");
    drop(conn);
    let before = bytes(&scratch.path);

    let mut store = Store::open(&scratch.path).expect("open");
    // The healthy page is still readable in isolation; only the corrupt page
    // refuses.
    let healthy = store.page_document(&page_id(PAGE_TWO)).expect("page");
    assert_eq!(healthy.body.body, "texto");
    let err = store
        .page_document(&page_id(PAGE_ONE))
        .expect_err("revision");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
    let err = store.export_snapshot().expect_err("snapshot");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
    let err = export_json(&mut store).expect_err("export");
    assert!(matches!(err, Error::Storage { .. }), "{err}");
    assert_eq!(bytes(&scratch.path), before);
}
