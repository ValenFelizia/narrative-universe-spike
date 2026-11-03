#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use std::collections::HashSet;

use narrative_universe::{
    apply_mention_edit, validate_mentions, AnnotatedBody, Entity, EntityId, Error, Mention,
    MentionEdit, MentionId, Page, PageEdit, PageId, PageRevision, Store, UniverseId,
    ValidationReason,
};
use serde::Deserialize;

const UNIVERSE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const PAGE: &str = "22222222-2222-4222-8222-222222222222";
const ENTITY: &str = "11111111-1111-4111-8111-111111111111";

#[derive(Deserialize)]
struct FixtureFile {
    cases: Vec<FixtureCase>,
}

#[derive(Deserialize)]
struct FixtureCase {
    id: String,
    base: String,
    mentions: Vec<FixtureMention>,
    expected_body: String,
    expected_mentions: Vec<FixtureMention>,
    #[serde(default)]
    expected_edits: Vec<(u32, u32, String)>,
    expected_ops: Option<Vec<FixtureOp>>,
    expected_dropped: Option<Vec<String>>,
    expected_changed: bool,
    expected_revision: i64,
}

/// One ordered save operation, in the coordinates of the body that the
/// previous operation produced.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum FixtureOp {
    Replace(u32, u32, String),
    Link(FixtureMention),
    Unlink(String),
}

#[derive(Deserialize)]
struct GraphemeFile {
    cases: Vec<GraphemeCase>,
}

#[derive(Deserialize)]
struct GraphemeCase {
    id: String,
    text: String,
    boundaries: Vec<u32>,
}

#[derive(Deserialize)]
struct FixtureMention {
    id: String,
    entity_id: String,
    start_utf16: u32,
    end_utf16: u32,
}

fn fixture() -> FixtureFile {
    serde_json::from_str(include_str!("fixtures/bridge_cases.json")).expect("fixture json")
}

fn universe() -> UniverseId {
    UniverseId::parse(UNIVERSE).expect("universe")
}

fn page_id() -> PageId {
    PageId::parse(PAGE).expect("page")
}

fn mention(raw: &FixtureMention) -> Mention {
    Mention {
        id: MentionId::parse(&raw.id).expect("mention id"),
        entity_id: EntityId::parse(&raw.entity_id).expect("entity id"),
        start_utf16: raw.start_utf16,
        end_utf16: raw.end_utf16,
    }
}

fn ids(mentions: &[Mention]) -> Vec<(String, String, u32, u32)> {
    mentions
        .iter()
        .map(|held| {
            (
                held.id.as_str().to_string(),
                held.entity_id.as_str().to_string(),
                held.start_utf16,
                held.end_utf16,
            )
        })
        .collect()
}

fn expected_ids(mentions: &[FixtureMention]) -> Vec<(String, String, u32, u32)> {
    mentions
        .iter()
        .map(|held| {
            (
                held.id.clone(),
                held.entity_id.clone(),
                held.start_utf16,
                held.end_utf16,
            )
        })
        .collect()
}

fn seed(store: &mut Store, case: &FixtureCase) -> PageRevision {
    let id = page_id();
    store
        .create_page(Page {
            id: id.clone(),
            title: "Bridge".to_string(),
            body: case.base.clone(),
        })
        .expect("page");
    let mut seen: HashSet<&str> = HashSet::new();
    for entity in entity_ids(case) {
        if seen.insert(entity) {
            store
                .create_entity(Entity {
                    id: EntityId::parse(entity).expect("entity id"),
                    label: "Synthetic".to_string(),
                    type_id: None,
                    page_id: None,
                })
                .expect("entity");
        }
    }
    let links: Vec<PageEdit> = case
        .mentions
        .iter()
        .map(|held| {
            PageEdit::Mention(MentionEdit::Link {
                mention: mention(held),
            })
        })
        .collect();
    if links.is_empty() {
        return PageRevision::ZERO;
    }
    store
        .save_page_edits(&id, PageRevision::ZERO, &links)
        .expect("seed links")
        .revision
}

fn entity_ids(case: &FixtureCase) -> Vec<&str> {
    let mut ids: Vec<&str> = case
        .mentions
        .iter()
        .map(|held| held.entity_id.as_str())
        .collect();
    for op in case.expected_ops.iter().flatten() {
        if let FixtureOp::Link(held) = op {
            ids.push(held.entity_id.as_str());
        }
    }
    ids
}

fn mention_edits(case: &FixtureCase) -> Vec<MentionEdit> {
    match &case.expected_ops {
        Some(ops) => ops
            .iter()
            .map(|op| match op {
                FixtureOp::Replace(start, end, inserted) => MentionEdit::ReplaceText {
                    start_utf16: *start,
                    end_utf16: *end,
                    inserted_text: inserted.clone(),
                },
                FixtureOp::Link(held) => MentionEdit::Link {
                    mention: mention(held),
                },
                FixtureOp::Unlink(id) => MentionEdit::Unlink {
                    mention_id: MentionId::parse(id).expect("mention id"),
                },
            })
            .collect(),
        None => case
            .expected_edits
            .iter()
            .map(|(start, end, inserted)| MentionEdit::ReplaceText {
                start_utf16: *start,
                end_utf16: *end,
                inserted_text: inserted.clone(),
            })
            .collect(),
    }
}

fn edits(case: &FixtureCase) -> Vec<PageEdit> {
    mention_edits(case)
        .into_iter()
        .map(PageEdit::Mention)
        .collect()
}

#[test]
fn bridge_cases_replay_through_the_real_store_and_survive_reopen() {
    for case in fixture().cases {
        let scratch = support::scratch(&format!("bridge-{}.sqlite", case.id));
        let mut store = Store::create(&scratch.path, universe()).expect("create");
        let revision = seed(&mut store, &case);
        assert_eq!(
            revision,
            PageRevision::new(if case.mentions.is_empty() { 0 } else { 1 })
                .expect("literal seed revision"),
            "{} seed revision",
            case.id
        );
        assert_eq!(
            case.expected_changed,
            case.expected_revision != revision.as_i64(),
            "{} changed flag",
            case.id
        );
        let id = page_id();

        let saved = store
            .save_page_edits(&id, revision, &edits(&case))
            .unwrap_or_else(|err| panic!("{}: {err}", case.id));

        assert_eq!(saved.body.body, case.expected_body, "{} body", case.id);
        assert_eq!(
            ids(&saved.body.mentions),
            expected_ids(&case.expected_mentions),
            "{} mentions",
            case.id
        );
        let expected_revision =
            PageRevision::new(case.expected_revision).expect("literal expected revision");
        assert_eq!(saved.revision, expected_revision, "{} revision", case.id);
        drop(store);

        let store = Store::open(&scratch.path).expect("reopen");
        let reloaded = store.page_document(&id).expect("document");
        assert_eq!(reloaded.body.body, case.expected_body, "{} reopen", case.id);
        assert_eq!(
            ids(&reloaded.body.mentions),
            expected_ids(&case.expected_mentions),
            "{} reopen mentions",
            case.id
        );
        assert_eq!(
            reloaded.revision, expected_revision,
            "{} reopen revision",
            case.id
        );
    }
}

#[test]
fn a_net_diff_noop_keeps_the_link_that_the_ledger_removes() {
    let scratch = support::scratch("bridge-net-noop.sqlite");
    let mut store = Store::create(&scratch.path, universe()).expect("create");
    store
        .create_page(Page {
            id: page_id(),
            title: "Counterexample".to_string(),
            body: "Elara".to_string(),
        })
        .expect("page");
    store
        .create_entity(Entity {
            id: EntityId::parse(ENTITY).expect("entity"),
            label: "Elara".to_string(),
            type_id: None,
            page_id: None,
        })
        .expect("entity");
    let seeded = store
        .save_page_edits(
            &page_id(),
            PageRevision::ZERO,
            &[PageEdit::Mention(MentionEdit::Link {
                mention: Mention {
                    id: MentionId::parse("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").expect("mention"),
                    entity_id: EntityId::parse(ENTITY).expect("entity"),
                    start_utf16: 0,
                    end_utf16: 5,
                },
            })],
        )
        .expect("link");

    let noop = store
        .save_page_edits(&page_id(), seeded.revision, &[])
        .expect("noop");
    assert_eq!(noop.body.body, "Elara");
    assert_eq!(noop.body.mentions.len(), 1);
    assert_eq!(noop.revision, seeded.revision);

    let ledger = store
        .save_page_edits(
            &page_id(),
            seeded.revision,
            &[
                PageEdit::Mention(MentionEdit::ReplaceText {
                    start_utf16: 2,
                    end_utf16: 2,
                    inserted_text: "x".to_string(),
                }),
                PageEdit::Mention(MentionEdit::ReplaceText {
                    start_utf16: 2,
                    end_utf16: 3,
                    inserted_text: String::new(),
                }),
            ],
        )
        .expect("ledger");
    assert_eq!(ledger.body.body, "Elara");
    assert!(ledger.body.mentions.is_empty());
    assert_eq!(
        ledger.revision,
        seeded.revision.checked_next().expect("revision")
    );
}

#[test]
fn a_stale_save_rejects_the_ledger_without_losing_the_committed_body() {
    let scratch = support::scratch("bridge-stale.sqlite");
    let mut writer = Store::create(&scratch.path, universe()).expect("create");
    writer
        .create_page(Page {
            id: page_id(),
            title: "Stale".to_string(),
            body: "original".to_string(),
        })
        .expect("page");
    let seen = writer.page_document(&page_id()).expect("document");
    drop(writer);

    let mut first = Store::open(&scratch.path).expect("first");
    let mut second = Store::open(&scratch.path).expect("second");
    first
        .save_page_edits(
            &page_id(),
            seen.revision,
            &[PageEdit::Mention(MentionEdit::ReplaceText {
                start_utf16: 0,
                end_utf16: 8,
                inserted_text: "winner".to_string(),
            })],
        )
        .expect("first save");

    let error = second
        .save_page_edits(
            &page_id(),
            seen.revision,
            &[PageEdit::Mention(MentionEdit::ReplaceText {
                start_utf16: 0,
                end_utf16: 8,
                inserted_text: "draft".to_string(),
            })],
        )
        .expect_err("stale");
    match error {
        Error::StaleRevision {
            expected, found, ..
        } => {
            assert_eq!(expected, 0);
            assert_eq!(found, 1);
        }
        other => panic!("expected stale revision, got {other}"),
    }
    drop(first);
    drop(second);

    let store = Store::open(&scratch.path).expect("reopen");
    let stored = store.page_document(&page_id()).expect("document");
    assert_eq!(stored.body.body, "winner");
    assert_eq!(stored.revision, PageRevision::new(1).expect("revision"));
}

#[test]
fn bridge_cases_replay_through_the_pure_reducer_with_their_dropped_ids() {
    for case in fixture().cases {
        let mut body = AnnotatedBody {
            body: case.base.clone(),
            mentions: case.mentions.iter().map(mention).collect(),
        };
        let mut dropped: Vec<String> = Vec::new();
        for edit in mention_edits(&case) {
            let (next, removed) =
                apply_mention_edit(&body, &edit).unwrap_or_else(|err| panic!("{}: {err}", case.id));
            if matches!(edit, MentionEdit::ReplaceText { .. }) {
                dropped.extend(removed.iter().map(|id| id.as_str().to_string()));
            }
            body = next;
        }
        dropped.retain(|id| !body.mentions.iter().any(|held| held.id.as_str() == id));
        dropped.sort();
        dropped.dedup();
        assert_eq!(body.body, case.expected_body, "{} body", case.id);
        assert_eq!(
            ids(&body.mentions),
            expected_ids(&case.expected_mentions),
            "{} mentions",
            case.id
        );
        if let Some(expected) = &case.expected_dropped {
            assert_eq!(&dropped, expected, "{} dropped", case.id);
        }
    }
}

#[test]
fn grapheme_boundaries_match_the_shared_corpus() {
    let corpus: GraphemeFile =
        serde_json::from_str(include_str!("fixtures/grapheme_boundaries.json"))
            .expect("grapheme json");
    let entity = EntityId::parse(ENTITY).expect("entity");
    let id = MentionId::parse("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").expect("mention");
    for case in corpus.cases {
        let length = case.text.encode_utf16().count() as u32;
        let mut found = vec![0];
        for end in 1..=length {
            let body = AnnotatedBody {
                body: case.text.clone(),
                mentions: vec![Mention {
                    id: id.clone(),
                    entity_id: entity.clone(),
                    start_utf16: 0,
                    end_utf16: end,
                }],
            };
            match validate_mentions(&body) {
                Ok(()) => found.push(end),
                Err(Error::Validation {
                    reason: ValidationReason::SplitGrapheme | ValidationReason::SplitScalar,
                    ..
                }) => {}
                Err(other) => panic!("{}: unexpected {other}", case.id),
            }
        }
        assert_eq!(found, case.boundaries, "{} boundaries", case.id);
    }
}
