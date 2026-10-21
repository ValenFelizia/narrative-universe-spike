//! Pure mention identity and single-edit semantics.
//!
//! A mention points at one occurrence of an entity inside a body of exact
//! text. The occurrence keeps its own stable identity, so the same visible
//! words can carry different mentions and different entities.
//!
//! This module is a pure domain reducer. It stores nothing, opens no
//! database, and never mutates its input. The offsets are UTF-16 code unit
//! positions over the exact body, which is the coordinate system a browser
//! editor already speaks.
//!
//! The rules are deliberately narrow. Mention ranges are nonempty and must
//! not overlap, though adjacent mentions are allowed. Mention endpoints must
//! sit on extended grapheme cluster boundaries so a link never points into
//! the middle of a character sequence. Text edits may fall inside a cluster
//! so a writer can really edit combining marks.
//!
//! Two limits are deliberate and provisional. The reducer compares content,
//! so it cannot detect a change that moves the text away and back. And when
//! an edit makes a surviving link endpoint fall inside a cluster, the prose
//! is preserved and the link is dropped instead of being moved elsewhere.

use std::collections::HashSet;

use unicode_segmentation::UnicodeSegmentation;

use crate::error::{Error, Result, ValidationReason};
use crate::model::{EntityId, MentionId};

/// One occurrence of an entity inside a body of text.
///
/// The occurrence carries no copy of the entity name, no canonical status,
/// no page, no quote, and no revision. Only the identity of the occurrence
/// and the entity it points at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mention {
    pub id: MentionId,
    pub entity_id: EntityId,
    pub start_utf16: u32,
    pub end_utf16: u32,
}

/// A body of exact text with the mentions that point into it.
///
/// The text is kept exactly as given. Nothing is trimmed, normalized, or
/// otherwise rewritten.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotatedBody {
    pub body: String,
    pub mentions: Vec<Mention>,
}

/// One operation on an annotated body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MentionEdit {
    /// Replaces the half-open UTF-16 range `[start_utf16, end_utf16)` with
    /// `inserted_text`. An empty range is an insertion.
    ReplaceText {
        start_utf16: u32,
        end_utf16: u32,
        inserted_text: String,
    },
    /// Adds one occurrence. The prose does not change.
    Link { mention: Mention },
    /// Removes the occurrence with this id. The prose does not change.
    Unlink { mention_id: MentionId },
}

/// Checks that the mentions of a body are consistent with its exact text.
///
/// A valid body has nonempty ranges inside the text, endpoints on extended
/// grapheme cluster boundaries, no overlapping ranges, and no repeated
/// mention id. Mentions may arrive in any order.
pub fn validate_mentions(body: &AnnotatedBody) -> Result<()> {
    let length = utf16_len(&body.body)?;
    let mut seen: HashSet<&str> = HashSet::new();
    for mention in &body.mentions {
        if !seen.insert(mention.id.as_str()) {
            return Err(Error::validation(
                ValidationReason::DuplicateMentionId,
                "mention_id",
                Some(mention.id.as_str()),
            ));
        }
        validate_mention_range(&body.body, length, mention)?;
    }
    let mut ordered: Vec<&Mention> = body.mentions.iter().collect();
    ordered.sort_by_key(|mention| (mention.start_utf16, mention.end_utf16, mention.id.as_str()));
    for pair in ordered.windows(2) {
        let (first, second) = (pair[0], pair[1]);
        if second.start_utf16 < first.end_utf16 {
            return Err(Error::validation(
                ValidationReason::OverlappingMentions,
                "mentions",
                Some(second.id.as_str()),
            ));
        }
    }
    Ok(())
}

/// Applies one operation to a body and returns the new body with the ids of
/// the occurrences that the operation removed.
///
/// The input is never changed, on success and on failure. The returned
/// mentions are canonical, sorted by start, then end, then id. The removed
/// ids are sorted in ascending order.
pub fn apply_mention_edit(
    body: &AnnotatedBody,
    edit: &MentionEdit,
) -> Result<(AnnotatedBody, Vec<MentionId>)> {
    validate_mentions(body)?;
    match edit {
        MentionEdit::ReplaceText {
            start_utf16,
            end_utf16,
            inserted_text,
        } => replace_text(body, *start_utf16, *end_utf16, inserted_text),
        MentionEdit::Link { mention } => link(body, mention),
        MentionEdit::Unlink { mention_id } => unlink(body, mention_id),
    }
}

fn replace_text(
    body: &AnnotatedBody,
    start_utf16: u32,
    end_utf16: u32,
    inserted_text: &str,
) -> Result<(AnnotatedBody, Vec<MentionId>)> {
    let length = utf16_len(&body.body)?;
    if start_utf16 > end_utf16 || end_utf16 > length {
        return Err(Error::validation(
            ValidationReason::InvalidTextRange,
            "end_utf16",
            None,
        ));
    }
    let start_byte = utf16_to_byte(&body.body, start_utf16, ValidationReason::InvalidTextRange)?;
    let end_byte = utf16_to_byte(&body.body, end_utf16, ValidationReason::InvalidTextRange)?;
    let inserted_length = utf16_len(inserted_text)?;
    if start_utf16 == end_utf16 && inserted_length == 0 {
        return Ok((canonical(body), Vec::new()));
    }

    let mut text = String::with_capacity(body.body.len() + inserted_text.len());
    text.push_str(&body.body[..start_byte]);
    text.push_str(inserted_text);
    text.push_str(&body.body[end_byte..]);

    let delta = i64::from(inserted_length) - i64::from(end_utf16 - start_utf16);
    let mut removed: Vec<MentionId> = Vec::new();
    let mut survivors: Vec<Mention> = Vec::new();
    for mention in &body.mentions {
        let start = mention.start_utf16;
        let end = mention.end_utf16;
        let overlaps = start_utf16 < end_utf16 && start_utf16 < end && start < end_utf16;
        let inside = start_utf16 == end_utf16
            && inserted_length > 0
            && start < start_utf16
            && start_utf16 < end;
        if overlaps || inside {
            removed.push(mention.id.clone());
            continue;
        }
        let (new_start, new_end) = if end_utf16 <= start {
            (checked_shift(start, delta)?, checked_shift(end, delta)?)
        } else {
            (start, end)
        };
        survivors.push(Mention {
            id: mention.id.clone(),
            entity_id: mention.entity_id.clone(),
            start_utf16: new_start,
            end_utf16: new_end,
        });
    }

    let kept = keep_whole_clusters(&text, survivors, &mut removed);
    removed.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    Ok((
        AnnotatedBody {
            body: text,
            mentions: canonical_order(kept),
        },
        removed,
    ))
}

fn link(body: &AnnotatedBody, mention: &Mention) -> Result<(AnnotatedBody, Vec<MentionId>)> {
    let length = utf16_len(&body.body)?;
    validate_mention_range(&body.body, length, mention)?;
    if let Some(existing) = body.mentions.iter().find(|held| held.id == mention.id) {
        if existing.entity_id == mention.entity_id
            && existing.start_utf16 == mention.start_utf16
            && existing.end_utf16 == mention.end_utf16
        {
            return Ok((canonical(body), Vec::new()));
        }
        return Err(Error::validation(
            ValidationReason::MentionIdConflict,
            "mention_id",
            Some(mention.id.as_str()),
        ));
    }
    if let Some(held) = body
        .mentions
        .iter()
        .find(|held| mention.start_utf16 < held.end_utf16 && held.start_utf16 < mention.end_utf16)
    {
        return Err(Error::validation(
            ValidationReason::OverlappingMentions,
            "mentions",
            Some(held.id.as_str()),
        ));
    }
    let mut mentions = body.mentions.clone();
    mentions.push(mention.clone());
    Ok((
        AnnotatedBody {
            body: body.body.clone(),
            mentions: canonical_order(mentions),
        },
        Vec::new(),
    ))
}

fn unlink(body: &AnnotatedBody, mention_id: &MentionId) -> Result<(AnnotatedBody, Vec<MentionId>)> {
    let mut mentions: Vec<Mention> = Vec::new();
    let mut removed: Vec<MentionId> = Vec::new();
    for mention in &body.mentions {
        if &mention.id == mention_id {
            removed.push(mention.id.clone());
        } else {
            mentions.push(mention.clone());
        }
    }
    Ok((
        AnnotatedBody {
            body: body.body.clone(),
            mentions: canonical_order(mentions),
        },
        removed,
    ))
}

fn validate_mention_range(body: &str, length: u32, mention: &Mention) -> Result<()> {
    if mention.start_utf16 >= mention.end_utf16 || mention.end_utf16 > length {
        return Err(Error::validation(
            ValidationReason::InvalidMentionRange,
            "end_utf16",
            Some(mention.id.as_str()),
        ));
    }
    let start = utf16_to_byte(
        body,
        mention.start_utf16,
        ValidationReason::InvalidMentionRange,
    )?;
    let end = utf16_to_byte(
        body,
        mention.end_utf16,
        ValidationReason::InvalidMentionRange,
    )?;
    let boundaries = grapheme_boundaries(body);
    if !boundaries.contains(&start) || !boundaries.contains(&end) {
        return Err(Error::validation(
            ValidationReason::SplitGrapheme,
            "start_utf16",
            Some(mention.id.as_str()),
        ));
    }
    Ok(())
}

/// Drops the survivors whose endpoints no longer sit on a cluster boundary of
/// the complete result, and records their ids as removed.
fn keep_whole_clusters(
    text: &str,
    survivors: Vec<Mention>,
    removed: &mut Vec<MentionId>,
) -> Vec<Mention> {
    let boundaries = grapheme_boundaries(text);
    let mut kept = Vec::new();
    for mention in survivors {
        let start = utf16_to_byte(text, mention.start_utf16, ValidationReason::SplitScalar);
        let end = utf16_to_byte(text, mention.end_utf16, ValidationReason::SplitScalar);
        let whole = match (start, end) {
            (Ok(start), Ok(end)) => boundaries.contains(&start) && boundaries.contains(&end),
            _ => false,
        };
        if whole {
            kept.push(mention);
        } else {
            removed.push(mention.id.clone());
        }
    }
    kept
}

fn canonical(body: &AnnotatedBody) -> AnnotatedBody {
    AnnotatedBody {
        body: body.body.clone(),
        mentions: canonical_order(body.mentions.clone()),
    }
}

fn canonical_order(mut mentions: Vec<Mention>) -> Vec<Mention> {
    mentions.sort_by(|left, right| {
        left.start_utf16
            .cmp(&right.start_utf16)
            .then(left.end_utf16.cmp(&right.end_utf16))
            .then(left.id.as_str().cmp(right.id.as_str()))
    });
    mentions
}

fn grapheme_boundaries(text: &str) -> HashSet<usize> {
    let mut boundaries: HashSet<usize> = text
        .grapheme_indices(true)
        .map(|(start, _)| start)
        .collect();
    boundaries.insert(text.len());
    boundaries
}

fn utf16_len(text: &str) -> Result<u32> {
    let length: u64 = text.chars().map(|ch| ch.len_utf16() as u64).sum();
    u32::try_from(length)
        .map_err(|_| Error::validation(ValidationReason::CoordinateOverflow, "body", None))
}

/// Converts a UTF-16 position into a byte offset of the exact text.
///
/// A position that falls inside a surrogate pair has no byte offset and is
/// rejected. `out_of_bounds` names the reason to report when the position is
/// outside the text.
fn utf16_to_byte(text: &str, position: u32, out_of_bounds: ValidationReason) -> Result<usize> {
    let target = u64::from(position);
    let mut units: u64 = 0;
    for (byte, ch) in text.char_indices() {
        if units == target {
            return Ok(byte);
        }
        let width = ch.len_utf16() as u64;
        if units + width > target {
            return Err(Error::validation(
                ValidationReason::SplitScalar,
                "utf16",
                None,
            ));
        }
        units += width;
    }
    if units == target {
        return Ok(text.len());
    }
    Err(Error::validation(out_of_bounds, "utf16", None))
}

fn checked_shift(offset: u32, delta: i64) -> Result<u32> {
    let shifted = i64::from(offset) + delta;
    u32::try_from(shifted)
        .map_err(|_| Error::validation(ValidationReason::CoordinateOverflow, "start_utf16", None))
}

#[cfg(test)]
mod tests {
    use super::{checked_shift, utf16_len};
    use crate::{Error, ValidationReason};

    #[test]
    fn checked_shift_reports_overflow_near_the_u32_limit() {
        assert_eq!(
            checked_shift(u32::MAX, -1).expect("shift down"),
            u32::MAX - 1
        );
        let err = checked_shift(u32::MAX, 1).expect_err("overflow");
        assert!(matches!(
            err,
            Error::Validation {
                reason: ValidationReason::CoordinateOverflow,
                ..
            }
        ));
        let err = checked_shift(0, -1).expect_err("underflow");
        assert!(matches!(
            err,
            Error::Validation {
                reason: ValidationReason::CoordinateOverflow,
                ..
            }
        ));
    }

    #[test]
    fn utf16_length_counts_code_units() {
        assert_eq!(utf16_len("Elara").expect("length"), 5);
        assert_eq!(utf16_len("A\u{1F4DA}").expect("length"), 3);
    }
}
