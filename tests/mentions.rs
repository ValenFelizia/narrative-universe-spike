//! Behavioral tests for the pure mention editing contract.
//!
//! Every expected value in this file is written by hand. The expected
//! offsets are never produced by calling the reducer, so the tests fail when
//! the reducer drifts away from the contract.

use narrative_universe::{
    apply_mention_edit, validate_mentions, AnnotatedBody, EntityId, Error, Mention, MentionEdit,
    MentionId, Result, ValidationReason,
};

const ENTITY_ONE: &str = "11111111-1111-4111-8111-111111111111";
const ENTITY_TWO: &str = "22222222-2222-4222-8222-222222222222";

const MENTION_ONE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const MENTION_TWO: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const MENTION_THREE: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

const COMBINING_ACUTE: &str = "\u{0301}";
const ZERO_WIDTH_JOINER: &str = "\u{200D}";
const REGIONAL_INDICATOR_A: &str = "\u{1F1E6}";
const REGIONAL_INDICATOR_B: &str = "\u{1F1E7}";
const REGIONAL_INDICATOR_C: &str = "\u{1F1E8}";
const REGIONAL_INDICATOR_D: &str = "\u{1F1E9}";
const REGIONAL_INDICATOR_E: &str = "\u{1F1EA}";
const DEVANAGARI_CONJUNCT: &str = "\u{0915}\u{094D}\u{0937}";
const WOMAN: &str = "\u{1F469}";
const GIRL: &str = "\u{1F467}";
const BOOK: &str = "\u{1F4DA}";

fn entity(raw: &str) -> EntityId {
    EntityId::parse(raw).expect("entity")
}

fn mention(raw: &str, entity_raw: &str, start_utf16: u32, end_utf16: u32) -> Mention {
    Mention {
        id: MentionId::parse(raw).expect("mention id"),
        entity_id: entity(entity_raw),
        start_utf16,
        end_utf16,
    }
}

fn annotated(body: &str, mentions: Vec<Mention>) -> AnnotatedBody {
    AnnotatedBody {
        body: body.to_string(),
        mentions,
    }
}

fn replace(start_utf16: u32, end_utf16: u32, inserted_text: &str) -> MentionEdit {
    MentionEdit::ReplaceText {
        start_utf16,
        end_utf16,
        inserted_text: inserted_text.to_string(),
    }
}

fn link(occurrence: Mention) -> MentionEdit {
    MentionEdit::Link {
        mention: occurrence,
    }
}

fn unlink(raw: &str) -> MentionEdit {
    MentionEdit::Unlink {
        mention_id: MentionId::parse(raw).expect("mention id"),
    }
}

fn assert_reason(err: Error, reason: ValidationReason) {
    match err {
        Error::Validation {
            reason: got_reason, ..
        } => assert_eq!(got_reason, reason),
        other => panic!("expected validation {reason:?}, got {other}"),
    }
}

fn assert_outcome(
    outcome: &Result<(AnnotatedBody, Vec<MentionId>)>,
    expected_body: &str,
    expected_mentions: &[(&str, &str, u32, u32)],
    expected_removed: &[&str],
) {
    let (result, removed) = outcome.as_ref().expect("edit");
    assert_eq!(result.body, expected_body);
    let expected: Vec<Mention> = expected_mentions
        .iter()
        .map(|(id, entity_raw, start, end)| mention(id, entity_raw, *start, *end))
        .collect();
    assert_eq!(result.mentions, expected);
    let expected_removed: Vec<MentionId> = expected_removed
        .iter()
        .map(|raw| MentionId::parse(raw).expect("mention id"))
        .collect();
    assert_eq!(*removed, expected_removed);
}

fn base() -> AnnotatedBody {
    annotated("Elara left", vec![mention(MENTION_ONE, ENTITY_ONE, 0, 5)])
}

/// Slices exact text by UTF-16 positions, so a test can show which words a
/// hand written range really covers.
fn slice_utf16(text: &str, start: u32, end: u32) -> String {
    let mut units: u32 = 0;
    let mut from: Option<usize> = None;
    let mut to: Option<usize> = None;
    for (byte, ch) in text.char_indices() {
        if units == start && from.is_none() {
            from = Some(byte);
        }
        if units == end && to.is_none() {
            to = Some(byte);
            break;
        }
        units += ch.len_utf16() as u32;
    }
    let from = from.unwrap_or(text.len());
    let to = to.unwrap_or(text.len());
    text[from..to].to_string()
}

#[test]
fn base_body_validates() {
    validate_mentions(&base()).expect("valid");
    assert_eq!(slice_utf16("Elara left", 0, 5), "Elara");
}

#[test]
fn a_text_edit_may_fall_inside_a_grapheme_cluster() {
    let body = annotated(
        &format!("e{COMBINING_ACUTE}x"),
        vec![mention(MENTION_ONE, ENTITY_ONE, 2, 3)],
    );
    validate_mentions(&body).expect("valid");
    let outcome = apply_mention_edit(&body, &replace(0, 1, "y"));
    assert_outcome(
        &outcome,
        &format!("y{COMBINING_ACUTE}x"),
        &[(MENTION_ONE, ENTITY_ONE, 2, 3)],
        &[],
    );
}

#[test]
fn an_indic_conjunct_is_one_extended_cluster() {
    let body = annotated(
        DEVANAGARI_CONJUNCT,
        vec![mention(MENTION_ONE, ENTITY_ONE, 0, 3)],
    );
    validate_mentions(&body).expect("valid");
    assert_eq!(slice_utf16(DEVANAGARI_CONJUNCT, 0, 3), DEVANAGARI_CONJUNCT);

    let split = annotated(
        DEVANAGARI_CONJUNCT,
        vec![mention(MENTION_ONE, ENTITY_ONE, 0, 2)],
    );
    let err = validate_mentions(&split).expect_err("split grapheme");
    assert_reason(err, ValidationReason::SplitGrapheme);
}

#[test]
fn removed_occurrences_are_sorted_by_id_not_by_position() {
    let body = annotated(
        "Elara met Elara",
        vec![
            mention(MENTION_THREE, ENTITY_ONE, 0, 5),
            mention(MENTION_ONE, ENTITY_TWO, 10, 15),
        ],
    );
    let outcome = apply_mention_edit(&body, &replace(3, 12, ""));
    assert_outcome(&outcome, "Elaara", &[], &[MENTION_ONE, MENTION_THREE]);
}

#[test]
fn a_mention_endpoint_inside_a_surrogate_pair_is_rejected() {
    let start_inside = annotated(
        &format!("A{BOOK}Elara"),
        vec![mention(MENTION_ONE, ENTITY_ONE, 2, 8)],
    );
    let err = validate_mentions(&start_inside).expect_err("split scalar");
    assert_reason(err, ValidationReason::SplitScalar);

    let end_inside = annotated(
        &format!("A{BOOK}Elara"),
        vec![mention(MENTION_ONE, ENTITY_ONE, 0, 2)],
    );
    let err = validate_mentions(&end_inside).expect_err("split scalar");
    assert_reason(err, ValidationReason::SplitScalar);
}

#[test]
fn a_reversed_text_range_is_rejected() {
    let err = apply_mention_edit(&base(), &replace(5, 2, "x")).expect_err("reversed");
    assert_reason(err, ValidationReason::InvalidTextRange);
}

#[test]
fn an_empty_body_without_mentions_is_valid() {
    let body = annotated("", Vec::new());
    validate_mentions(&body).expect("valid");
    let outcome = apply_mention_edit(&body, &replace(0, 0, "x"));
    assert_outcome(&outcome, "x", &[], &[]);
}

#[test]
fn a_join_elsewhere_leaves_an_unrelated_link_intact() {
    let body = annotated("ab", vec![mention(MENTION_ONE, ENTITY_ONE, 1, 2)]);
    let outcome = apply_mention_edit(&body, &replace(1, 1, COMBINING_ACUTE));
    assert_outcome(
        &outcome,
        &format!("a{COMBINING_ACUTE}b"),
        &[(MENTION_ONE, ENTITY_ONE, 2, 3)],
        &[],
    );
}

#[test]
fn a_successful_edit_does_not_change_the_input() {
    let body = base();
    let before = body.clone();
    let outcome = apply_mention_edit(&body, &replace(0, 0, "Dear "));
    assert_outcome(
        &outcome,
        "Dear Elara left",
        &[(MENTION_ONE, ENTITY_ONE, 5, 10)],
        &[],
    );
    assert_eq!(body, before);
}

#[test]
fn a_link_whose_range_splits_a_cluster_is_rejected() {
    let body = annotated(&format!("e{COMBINING_ACUTE}"), Vec::new());
    let err = apply_mention_edit(&body, &link(mention(MENTION_ONE, ENTITY_ONE, 0, 1)))
        .expect_err("split grapheme");
    assert_reason(err, ValidationReason::SplitGrapheme);
}

#[test]
fn a_link_may_be_adjacent_to_an_existing_mention() {
    let body = annotated("ElaraElara", vec![mention(MENTION_ONE, ENTITY_ONE, 0, 5)]);
    let outcome = apply_mention_edit(&body, &link(mention(MENTION_TWO, ENTITY_TWO, 5, 10)));
    assert_outcome(
        &outcome,
        "ElaraElara",
        &[
            (MENTION_ONE, ENTITY_ONE, 0, 5),
            (MENTION_TWO, ENTITY_TWO, 5, 10),
        ],
        &[],
    );
}

#[test]
fn two_linked_occurrences_may_point_at_the_same_entity() {
    let body = annotated("Elara met Elara", Vec::new());
    let outcome = apply_mention_edit(&body, &link(mention(MENTION_ONE, ENTITY_ONE, 0, 5)));
    let (linked, _) = outcome.expect("first link");
    let outcome = apply_mention_edit(&linked, &link(mention(MENTION_TWO, ENTITY_ONE, 10, 15)));
    assert_outcome(
        &outcome,
        "Elara met Elara",
        &[
            (MENTION_ONE, ENTITY_ONE, 0, 5),
            (MENTION_TWO, ENTITY_ONE, 10, 15),
        ],
        &[],
    );
}

#[test]
fn insert_before_the_mention_shifts_it() {
    let outcome = apply_mention_edit(&base(), &replace(0, 0, "Dear "));
    assert_outcome(
        &outcome,
        "Dear Elara left",
        &[(MENTION_ONE, ENTITY_ONE, 5, 10)],
        &[],
    );
}

#[test]
fn insert_after_the_mention_keeps_it() {
    let outcome = apply_mention_edit(&base(), &replace(5, 5, "!"));
    assert_outcome(
        &outcome,
        "Elara! left",
        &[(MENTION_ONE, ENTITY_ONE, 0, 5)],
        &[],
    );
}

#[test]
fn insert_inside_the_mention_removes_it() {
    let outcome = apply_mention_edit(&base(), &replace(2, 2, "x"));
    assert_outcome(&outcome, "Elxara left", &[], &[MENTION_ONE]);
}

#[test]
fn deletion_just_after_the_mention_keeps_it() {
    let outcome = apply_mention_edit(&base(), &replace(5, 10, ""));
    assert_outcome(&outcome, "Elara", &[(MENTION_ONE, ENTITY_ONE, 0, 5)], &[]);
}

#[test]
fn deletion_ending_at_the_mention_start_shifts_it() {
    let body = annotated("Hi Elara", vec![mention(MENTION_ONE, ENTITY_ONE, 3, 8)]);
    let outcome = apply_mention_edit(&body, &replace(0, 3, ""));
    assert_outcome(&outcome, "Elara", &[(MENTION_ONE, ENTITY_ONE, 0, 5)], &[]);
}

#[test]
fn partial_overlap_removes_the_mention() {
    let outcome = apply_mention_edit(&base(), &replace(3, 7, "!"));
    assert_outcome(&outcome, "Ela!eft", &[], &[MENTION_ONE]);
}

#[test]
fn full_deletion_removes_the_mention() {
    let outcome = apply_mention_edit(&base(), &replace(0, 5, ""));
    assert_outcome(&outcome, " left", &[], &[MENTION_ONE]);
}

#[test]
fn same_text_replacement_removes_the_mention() {
    let outcome = apply_mention_edit(&base(), &replace(0, 5, "Elara"));
    assert_outcome(&outcome, "Elara left", &[], &[MENTION_ONE]);
}

#[test]
fn empty_insertion_changes_nothing() {
    let outcome = apply_mention_edit(&base(), &replace(2, 2, ""));
    assert_outcome(
        &outcome,
        "Elara left",
        &[(MENTION_ONE, ENTITY_ONE, 0, 5)],
        &[],
    );
}

#[test]
fn repeated_names_keep_two_separate_occurrences() {
    let body = annotated(
        "Elara met Elara",
        vec![
            mention(MENTION_ONE, ENTITY_ONE, 0, 5),
            mention(MENTION_TWO, ENTITY_TWO, 10, 15),
        ],
    );
    let outcome = apply_mention_edit(&body, &replace(0, 5, "Ela"));
    assert_outcome(
        &outcome,
        "Ela met Elara",
        &[(MENTION_TWO, ENTITY_TWO, 8, 13)],
        &[MENTION_ONE],
    );
}

#[test]
fn adjacent_occurrences_both_survive() {
    let body = annotated(
        "ElaraElara",
        vec![
            mention(MENTION_ONE, ENTITY_ONE, 0, 5),
            mention(MENTION_TWO, ENTITY_TWO, 5, 10),
        ],
    );
    let outcome = apply_mention_edit(&body, &replace(5, 5, "!"));
    assert_outcome(
        &outcome,
        "Elara!Elara",
        &[
            (MENTION_ONE, ENTITY_ONE, 0, 5),
            (MENTION_TWO, ENTITY_TWO, 6, 11),
        ],
        &[],
    );
}

#[test]
fn deletion_across_two_occurrences_removes_both() {
    let body = annotated(
        "Elara met Elara",
        vec![
            mention(MENTION_ONE, ENTITY_ONE, 0, 5),
            mention(MENTION_TWO, ENTITY_TWO, 10, 15),
        ],
    );
    let outcome = apply_mention_edit(&body, &replace(3, 12, ""));
    assert_outcome(&outcome, "Elaara", &[], &[MENTION_ONE, MENTION_TWO]);
}

#[test]
fn astral_scalar_shifts_the_mention() {
    let body = annotated(
        &format!("A{BOOK}Elara"),
        vec![mention(MENTION_ONE, ENTITY_ONE, 3, 8)],
    );
    let outcome = apply_mention_edit(&body, &replace(0, 0, "!"));
    assert_outcome(
        &outcome,
        &format!("!A{BOOK}Elara"),
        &[(MENTION_ONE, ENTITY_ONE, 4, 9)],
        &[],
    );
}

#[test]
fn edit_inside_a_surrogate_pair_is_rejected() {
    let body = annotated(
        &format!("A{BOOK}Elara"),
        vec![mention(MENTION_ONE, ENTITY_ONE, 3, 8)],
    );
    let err = apply_mention_edit(&body, &replace(2, 2, "x")).expect_err("split scalar");
    assert_reason(err, ValidationReason::SplitScalar);
}

#[test]
fn decomposed_accent_accepts_the_whole_cluster() {
    let body = annotated(
        &format!("e{COMBINING_ACUTE}"),
        vec![mention(MENTION_ONE, ENTITY_ONE, 0, 2)],
    );
    validate_mentions(&body).expect("valid");
}

#[test]
fn decomposed_accent_split_inside_the_cluster_is_rejected() {
    let body = annotated(
        &format!("e{COMBINING_ACUTE}"),
        vec![mention(MENTION_ONE, ENTITY_ONE, 0, 1)],
    );
    let err = validate_mentions(&body).expect_err("split grapheme");
    assert_reason(err, ValidationReason::SplitGrapheme);
}

#[test]
fn precomposed_accent_is_a_different_exact_body() {
    let decomposed = annotated(
        &format!("e{COMBINING_ACUTE}"),
        vec![mention(MENTION_ONE, ENTITY_ONE, 0, 2)],
    );
    let precomposed = annotated("\u{00E9}", vec![mention(MENTION_ONE, ENTITY_ONE, 0, 1)]);
    validate_mentions(&precomposed).expect("valid");
    assert_ne!(decomposed.body, precomposed.body);
    assert_eq!(precomposed.body.len(), 2);
}

#[test]
fn joining_a_cluster_removes_the_link_and_keeps_the_text() {
    let body = annotated("e", vec![mention(MENTION_ONE, ENTITY_ONE, 0, 1)]);
    let outcome = apply_mention_edit(&body, &replace(1, 1, COMBINING_ACUTE));
    assert_outcome(
        &outcome,
        &format!("e{COMBINING_ACUTE}"),
        &[],
        &[MENTION_ONE],
    );
}

#[test]
fn zwj_sequence_removes_both_links() {
    let body = annotated(
        &format!("{WOMAN}{GIRL}"),
        vec![
            mention(MENTION_ONE, ENTITY_ONE, 0, 2),
            mention(MENTION_TWO, ENTITY_TWO, 2, 4),
        ],
    );
    let outcome = apply_mention_edit(&body, &replace(2, 2, ZERO_WIDTH_JOINER));
    assert_outcome(
        &outcome,
        &format!("{WOMAN}{ZERO_WIDTH_JOINER}{GIRL}"),
        &[],
        &[MENTION_ONE, MENTION_TWO],
    );
}

#[test]
fn regional_indicator_pair_removes_the_shifted_link() {
    let body = annotated(
        &format!(
            "{REGIONAL_INDICATOR_A}{REGIONAL_INDICATOR_B}{REGIONAL_INDICATOR_C}{REGIONAL_INDICATOR_D}"
        ),
        vec![mention(MENTION_ONE, ENTITY_ONE, 4, 8)],
    );
    let outcome = apply_mention_edit(&body, &replace(0, 0, REGIONAL_INDICATOR_E));
    assert_outcome(
        &outcome,
        &format!(
            "{REGIONAL_INDICATOR_E}{REGIONAL_INDICATOR_A}{REGIONAL_INDICATOR_B}{REGIONAL_INDICATOR_C}{REGIONAL_INDICATOR_D}"
        ),
        &[],
        &[MENTION_ONE],
    );
}

#[test]
fn carriage_return_and_line_feed_survive_an_unrelated_edit() {
    let body = annotated("A\r\nElara", vec![mention(MENTION_ONE, ENTITY_ONE, 3, 8)]);
    let outcome = apply_mention_edit(&body, &replace(0, 0, "!"));
    assert_outcome(
        &outcome,
        "!A\r\nElara",
        &[(MENTION_ONE, ENTITY_ONE, 4, 9)],
        &[],
    );
}

#[test]
fn mixed_line_endings_keep_their_clusters() {
    let body = annotated("a\nb\r\nc\rd", vec![mention(MENTION_ONE, ENTITY_ONE, 2, 3)]);
    validate_mentions(&body).expect("valid");
    let outcome = apply_mention_edit(&body, &replace(0, 0, "!"));
    assert_outcome(
        &outcome,
        "!a\nb\r\nc\rd",
        &[(MENTION_ONE, ENTITY_ONE, 3, 4)],
        &[],
    );
}

#[test]
fn exact_text_is_preserved() {
    let original =
        format!("\t \"quote\" \u{0} e{COMBINING_ACUTE} \u{05E9}\u{05DC}\u{05D5}\u{05DD}");
    let body = annotated(&original, vec![mention(MENTION_ONE, ENTITY_ONE, 15, 19)]);
    validate_mentions(&body).expect("valid");
    assert_eq!(
        slice_utf16(&original, 15, 19),
        "\u{05E9}\u{05DC}\u{05D5}\u{05DD}"
    );
    let outcome = apply_mention_edit(&body, &replace(19, 19, "!"));
    assert_outcome(
        &outcome,
        &format!("{original}!"),
        &[(MENTION_ONE, ENTITY_ONE, 15, 19)],
        &[],
    );
}

#[test]
fn link_leaves_the_prose_unchanged() {
    let body = annotated("Elara left", Vec::new());
    let outcome = apply_mention_edit(&body, &link(mention(MENTION_ONE, ENTITY_ONE, 0, 5)));
    assert_outcome(
        &outcome,
        "Elara left",
        &[(MENTION_ONE, ENTITY_ONE, 0, 5)],
        &[],
    );
}

#[test]
fn duplicate_link_is_a_no_op() {
    let body = base();
    let outcome = apply_mention_edit(&body, &link(mention(MENTION_ONE, ENTITY_ONE, 0, 5)));
    assert_outcome(
        &outcome,
        "Elara left",
        &[(MENTION_ONE, ENTITY_ONE, 0, 5)],
        &[],
    );
}

#[test]
fn unlink_removes_only_the_named_occurrence() {
    let body = annotated(
        "Elara met Elara",
        vec![
            mention(MENTION_ONE, ENTITY_ONE, 0, 5),
            mention(MENTION_TWO, ENTITY_TWO, 10, 15),
        ],
    );
    let outcome = apply_mention_edit(&body, &unlink(MENTION_ONE));
    assert_outcome(
        &outcome,
        "Elara met Elara",
        &[(MENTION_TWO, ENTITY_TWO, 10, 15)],
        &[MENTION_ONE],
    );
}

#[test]
fn unknown_unlink_is_a_no_op() {
    let body = base();
    let outcome = apply_mention_edit(&body, &unlink(MENTION_THREE));
    assert_outcome(
        &outcome,
        "Elara left",
        &[(MENTION_ONE, ENTITY_ONE, 0, 5)],
        &[],
    );
}

#[test]
fn invalid_states_are_rejected() {
    let empty = annotated("Elara left", vec![mention(MENTION_ONE, ENTITY_ONE, 3, 3)]);
    assert_reason(
        validate_mentions(&empty).expect_err("empty range"),
        ValidationReason::InvalidMentionRange,
    );

    let reversed = annotated("Elara left", vec![mention(MENTION_ONE, ENTITY_ONE, 5, 2)]);
    assert_reason(
        validate_mentions(&reversed).expect_err("reversed range"),
        ValidationReason::InvalidMentionRange,
    );

    let out_of_bounds = annotated("Elara left", vec![mention(MENTION_ONE, ENTITY_ONE, 0, 99)]);
    assert_reason(
        validate_mentions(&out_of_bounds).expect_err("out of bounds"),
        ValidationReason::InvalidMentionRange,
    );

    let duplicated = annotated(
        "Elara left",
        vec![
            mention(MENTION_ONE, ENTITY_ONE, 0, 5),
            mention(MENTION_ONE, ENTITY_ONE, 0, 5),
        ],
    );
    assert_reason(
        validate_mentions(&duplicated).expect_err("duplicate id"),
        ValidationReason::DuplicateMentionId,
    );

    let overlapping = annotated(
        "Elara left",
        vec![
            mention(MENTION_ONE, ENTITY_ONE, 0, 5),
            mention(MENTION_TWO, ENTITY_TWO, 3, 8),
        ],
    );
    assert_reason(
        validate_mentions(&overlapping).expect_err("overlapping"),
        ValidationReason::OverlappingMentions,
    );
}

#[test]
fn a_rejected_edit_does_not_change_the_input() {
    let body = annotated(
        "Elara left",
        vec![
            mention(MENTION_ONE, ENTITY_ONE, 0, 5),
            mention(MENTION_TWO, ENTITY_TWO, 3, 8),
        ],
    );
    let before = body.clone();
    let err = apply_mention_edit(&body, &replace(0, 1, "z")).expect_err("overlapping input");
    assert_reason(err, ValidationReason::OverlappingMentions);
    assert_eq!(body, before);
}

#[test]
fn a_reused_mention_id_conflicts() {
    let body = base();
    let err = apply_mention_edit(&body, &link(mention(MENTION_ONE, ENTITY_ONE, 0, 6)))
        .expect_err("different range");
    assert_reason(err, ValidationReason::MentionIdConflict);

    let err = apply_mention_edit(&body, &link(mention(MENTION_ONE, ENTITY_TWO, 0, 5)))
        .expect_err("different target");
    assert_reason(err, ValidationReason::MentionIdConflict);
}

#[test]
fn a_link_that_overlaps_an_existing_occurrence_fails() {
    let body = base();
    let err = apply_mention_edit(&body, &link(mention(MENTION_TWO, ENTITY_TWO, 3, 8)))
        .expect_err("overlapping link");
    assert_reason(err, ValidationReason::OverlappingMentions);
}

#[test]
fn two_occurrences_may_point_at_the_same_entity() {
    let body = annotated(
        "Elara met Elara",
        vec![
            mention(MENTION_ONE, ENTITY_ONE, 0, 5),
            mention(MENTION_TWO, ENTITY_ONE, 10, 15),
        ],
    );
    validate_mentions(&body).expect("valid");
}

#[test]
fn a_malformed_mention_id_fails_at_parse() {
    let err = MentionId::parse("not-a-uuid").expect_err("invalid id");
    assert_reason(err, ValidationReason::InvalidId);
    let upper = "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA";
    let err = MentionId::parse(upper).expect_err("not canonical");
    assert_reason(err, ValidationReason::InvalidId);
}

#[test]
fn coordinates_near_the_u32_limit_fail_without_allocating() {
    let body = base();
    let err = apply_mention_edit(&body, &replace(0, u32::MAX, "")).expect_err("out of bounds");
    assert_reason(err, ValidationReason::InvalidTextRange);

    let err =
        apply_mention_edit(&body, &replace(u32::MAX, u32::MAX, "")).expect_err("out of bounds");
    assert_reason(err, ValidationReason::InvalidTextRange);

    let err = apply_mention_edit(&body, &replace(u32::MAX - 1, u32::MAX, "x"))
        .expect_err("out of bounds");
    assert_reason(err, ValidationReason::InvalidTextRange);

    let out_of_bounds = annotated(
        "Elara left",
        vec![mention(MENTION_ONE, ENTITY_ONE, 0, u32::MAX)],
    );
    assert_reason(
        validate_mentions(&out_of_bounds).expect_err("out of bounds"),
        ValidationReason::InvalidMentionRange,
    );
}

#[test]
fn an_inverse_text_edit_does_not_resurrect_the_link() {
    let outcome = apply_mention_edit(&base(), &replace(0, 5, "X"));
    assert_outcome(&outcome, "X left", &[], &[MENTION_ONE]);

    let (edited, _) = outcome.expect("first edit");
    let outcome = apply_mention_edit(&edited, &replace(0, 1, "Elara"));
    assert_outcome(&outcome, "Elara left", &[], &[]);
}

#[test]
fn an_explicit_link_restores_the_original_occurrence() {
    let outcome = apply_mention_edit(&base(), &replace(0, 5, "X"));
    let (edited, _) = outcome.expect("first edit");
    let outcome = apply_mention_edit(&edited, &replace(0, 1, "Elara"));
    let (edited, _) = outcome.expect("second edit");

    let outcome = apply_mention_edit(&edited, &link(mention(MENTION_ONE, ENTITY_ONE, 0, 5)));
    assert_outcome(
        &outcome,
        "Elara left",
        &[(MENTION_ONE, ENTITY_ONE, 0, 5)],
        &[],
    );
}

#[test]
fn the_second_operation_uses_the_first_result_coordinates() {
    let outcome = apply_mention_edit(&base(), &replace(0, 0, "Dear "));
    assert_outcome(
        &outcome,
        "Dear Elara left",
        &[(MENTION_ONE, ENTITY_ONE, 5, 10)],
        &[],
    );

    let (edited, _) = outcome.expect("first edit");
    let outcome = apply_mention_edit(&edited, &replace(10, 10, "!"));
    assert_outcome(
        &outcome,
        "Dear Elara! left",
        &[(MENTION_ONE, ENTITY_ONE, 5, 10)],
        &[],
    );
}

#[test]
fn reordered_mentions_produce_the_same_canonical_result() {
    let ordered = annotated(
        "Elara met Elara",
        vec![
            mention(MENTION_ONE, ENTITY_ONE, 0, 5),
            mention(MENTION_TWO, ENTITY_TWO, 10, 15),
        ],
    );
    let reordered = annotated(
        "Elara met Elara",
        vec![
            mention(MENTION_TWO, ENTITY_TWO, 10, 15),
            mention(MENTION_ONE, ENTITY_ONE, 0, 5),
        ],
    );
    validate_mentions(&reordered).expect("valid");

    let first = apply_mention_edit(&ordered, &replace(2, 2, "")).expect("ordered");
    let second = apply_mention_edit(&reordered, &replace(2, 2, "")).expect("reordered");
    assert_eq!(first, second);
    assert_eq!(first.1, second.1);
    let outcome: Result<(AnnotatedBody, Vec<MentionId>)> = Ok(first);
    assert_outcome(
        &outcome,
        "Elara met Elara",
        &[
            (MENTION_ONE, ENTITY_ONE, 0, 5),
            (MENTION_TWO, ENTITY_TWO, 10, 15),
        ],
        &[],
    );
}
