import { ChangeSet, Text } from "@codemirror/state";

import { AdapterError, MAX_UTF16, toSequentialEdits, type PageEdit } from "./adapter.ts";

/// One occurrence of an entity inside a body, in UTF-16 code units. The same
/// shape crosses the bridge.
export type Mention = {
  id: string;
  entityId: string;
  startUtf16: number;
  endUtf16: number;
};

/// One ordered save operation, in the coordinates of the body that the
/// previous operation produced. Mirrors Rust `MentionEdit` and is the wire
/// shape of `save_page`.
export type MentionOp =
  | { kind: "replace"; startUtf16: number; endUtf16: number; insertedText: string }
  | { kind: "link"; mentionId: string; entityId: string; startUtf16: number; endUtf16: number }
  | { kind: "unlink"; mentionId: string };

export type AnnotatedBody = {
  body: string;
  mentions: readonly Mention[];
};

export type MentionErrorReason =
  | "invalid_text_range"
  | "split_scalar"
  | "coordinate_overflow"
  | "invalid_mention_range"
  | "split_grapheme"
  | "overlapping_mentions"
  | "duplicate_mention_id"
  | "mention_id_conflict"
  | "segmenter_unavailable";

export class MentionError extends Error {
  readonly reason: MentionErrorReason;
  readonly mentionId: string | null;

  constructor(reason: MentionErrorReason, mentionId: string | null = null) {
    super(mentionId ? `${reason}: ${mentionId}` : reason);
    this.name = "MentionError";
    this.reason = reason;
    this.mentionId = mentionId;
  }
}

let segmenter: Intl.Segmenter | null = null;

function graphemes(): Intl.Segmenter {
  if (segmenter) return segmenter;
  if (typeof Intl === "undefined" || typeof Intl.Segmenter !== "function") {
    throw new MentionError("segmenter_unavailable");
  }
  segmenter = new Intl.Segmenter("en", { granularity: "grapheme" });
  return segmenter;
}

/// The positions the mention rules need, built once per body like Rust
/// `TextIndex`. Cluster starts come from `Intl.Segmenter`; the Rust side uses
/// `unicode-segmentation`. The shared fixtures and the bridge agreement check
/// are what keep the two equal.
class TextIndex {
  readonly length: number;
  private readonly text: string;
  private readonly clusters: Set<number>;

  constructor(text: string) {
    this.text = text;
    this.length = text.length;
    this.clusters = new Set();
    for (const segment of graphemes().segment(text)) this.clusters.add(segment.index);
  }

  isScalarBoundary(position: number): boolean {
    if (position === this.length || position === 0) return true;
    if (position < 0 || position > this.length) return false;
    const before = this.text.charCodeAt(position - 1);
    const at = this.text.charCodeAt(position);
    return !(before >= 0xd800 && before <= 0xdbff && at >= 0xdc00 && at <= 0xdfff);
  }

  isClusterBoundary(position: number): boolean {
    return position === this.length || this.clusters.has(position);
  }
}

export function compareMentions(left: Mention, right: Mention): number {
  return (
    left.startUtf16 - right.startUtf16 ||
    left.endUtf16 - right.endUtf16 ||
    (left.id < right.id ? -1 : left.id > right.id ? 1 : 0)
  );
}

export function canonicalMentions(mentions: readonly Mention[]): Mention[] {
  return [...mentions].sort(compareMentions);
}

export function sameMentions(left: readonly Mention[], right: readonly Mention[]): boolean {
  if (left.length !== right.length) return false;
  const a = canonicalMentions(left);
  const b = canonicalMentions(right);
  return a.every((held, index) => {
    const other = b[index]!;
    return (
      held.id === other.id &&
      held.entityId === other.entityId &&
      held.startUtf16 === other.startUtf16 &&
      held.endUtf16 === other.endUtf16
    );
  });
}

function validateRange(index: TextIndex, mention: Mention): void {
  if (
    !Number.isSafeInteger(mention.startUtf16) ||
    !Number.isSafeInteger(mention.endUtf16) ||
    mention.startUtf16 < 0 ||
    mention.startUtf16 >= mention.endUtf16 ||
    mention.endUtf16 > index.length
  ) {
    throw new MentionError("invalid_mention_range", mention.id);
  }
  if (!index.isScalarBoundary(mention.startUtf16) || !index.isScalarBoundary(mention.endUtf16)) {
    throw new MentionError("split_scalar", mention.id);
  }
  if (!index.isClusterBoundary(mention.startUtf16) || !index.isClusterBoundary(mention.endUtf16)) {
    throw new MentionError("split_grapheme", mention.id);
  }
}

function validateWith(index: TextIndex, mentions: readonly Mention[]): void {
  const seen = new Set<string>();
  for (const mention of mentions) {
    if (seen.has(mention.id)) throw new MentionError("duplicate_mention_id", mention.id);
    seen.add(mention.id);
    validateRange(index, mention);
  }
  const ordered = canonicalMentions(mentions);
  for (let i = 1; i < ordered.length; i += 1) {
    if (ordered[i]!.startUtf16 < ordered[i - 1]!.endUtf16) {
      throw new MentionError("overlapping_mentions", ordered[i]!.id);
    }
  }
}

/// Mirrors Rust `validate_mentions`.
export function validateMentions(body: AnnotatedBody): void {
  validateWith(new TextIndex(body.body), body.mentions);
}

export type MentionResult = AnnotatedBody & { removed: string[] };

/// Mirrors Rust `apply_mention_edit`: the same ranges, the same exclusive
/// boundaries, the same cluster check on survivors, canonical output order,
/// and removed ids in ascending order. The input is never changed.
export function applyMentionOp(body: AnnotatedBody, op: MentionOp): MentionResult {
  const index = new TextIndex(body.body);
  validateWith(index, body.mentions);
  switch (op.kind) {
    case "replace":
      return replaceText(body, index, op.startUtf16, op.endUtf16, op.insertedText);
    case "link":
      return link(body, index, {
        id: op.mentionId,
        entityId: op.entityId,
        startUtf16: op.startUtf16,
        endUtf16: op.endUtf16,
      });
    case "unlink": {
      const kept = body.mentions.filter((held) => held.id !== op.mentionId);
      const removed = kept.length === body.mentions.length ? [] : [op.mentionId];
      return { body: body.body, mentions: canonicalMentions(kept), removed };
    }
  }
}

function replaceText(
  body: AnnotatedBody,
  index: TextIndex,
  start: number,
  end: number,
  inserted: string,
): MentionResult {
  if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end) || start < 0 || start > end || end > index.length) {
    throw new MentionError("invalid_text_range");
  }
  if (!index.isScalarBoundary(start) || !index.isScalarBoundary(end)) {
    throw new MentionError("split_scalar");
  }
  if (start === end && inserted.length === 0) {
    return { body: body.body, mentions: canonicalMentions(body.mentions), removed: [] };
  }
  if (index.length - (end - start) + inserted.length > MAX_UTF16) {
    throw new MentionError("coordinate_overflow");
  }
  const text = body.body.slice(0, start) + inserted + body.body.slice(end);
  const delta = inserted.length - (end - start);
  const removed: string[] = [];
  const survivors: Mention[] = [];
  for (const mention of body.mentions) {
    const overlaps = start < end && start < mention.endUtf16 && mention.startUtf16 < end;
    const inside =
      start === end && inserted.length > 0 && mention.startUtf16 < start && start < mention.endUtf16;
    if (overlaps || inside) {
      removed.push(mention.id);
      continue;
    }
    survivors.push(
      end <= mention.startUtf16
        ? { ...mention, startUtf16: mention.startUtf16 + delta, endUtf16: mention.endUtf16 + delta }
        : { ...mention },
    );
  }
  const result = new TextIndex(text);
  const kept = survivors.filter((mention) => {
    const whole =
      result.isScalarBoundary(mention.startUtf16) &&
      result.isScalarBoundary(mention.endUtf16) &&
      result.isClusterBoundary(mention.startUtf16) &&
      result.isClusterBoundary(mention.endUtf16);
    if (!whole) removed.push(mention.id);
    return whole;
  });
  removed.sort();
  return { body: text, mentions: canonicalMentions(kept), removed };
}

function link(body: AnnotatedBody, index: TextIndex, mention: Mention): MentionResult {
  validateRange(index, mention);
  const existing = body.mentions.find((held) => held.id === mention.id);
  if (existing) {
    if (
      existing.entityId === mention.entityId &&
      existing.startUtf16 === mention.startUtf16 &&
      existing.endUtf16 === mention.endUtf16
    ) {
      return { body: body.body, mentions: canonicalMentions(body.mentions), removed: [] };
    }
    throw new MentionError("mention_id_conflict", mention.id);
  }
  const held = body.mentions.find(
    (other) => mention.startUtf16 < other.endUtf16 && other.startUtf16 < mention.endUtf16,
  );
  if (held) throw new MentionError("overlapping_mentions", held.id);
  return { body: body.body, mentions: canonicalMentions([...body.mentions, mention]), removed: [] };
}

export function replaceOp(edit: PageEdit): MentionOp {
  return {
    kind: "replace",
    startUtf16: edit.start_utf16,
    endUtf16: edit.end_utf16,
    insertedText: edit.inserted_text,
  };
}

export function linkOp(mention: Mention): MentionOp {
  return {
    kind: "link",
    mentionId: mention.id,
    entityId: mention.entityId,
    startUtf16: mention.startUtf16,
    endUtf16: mention.endUtf16,
  };
}

/// A link operation carried by an editor transaction, applied after the
/// transaction's text changes.
export type MentionEffect =
  | { kind: "link"; mention: Mention }
  | { kind: "unlink"; mentionId: string }
  /// Makes the mention set exactly this one. Each held mention that is not in
  /// it with the same id, entity, and range is unlinked, then each missing one
  /// is linked. History uses it to restore a whole state, and it expands into
  /// the ordinary unlink and link operations the bridge replays.
  | { kind: "restore"; mentions: readonly Mention[] };

export type AppliedEffect = Exclude<MentionEffect, { kind: "restore" }>;

/// A restore decides the whole set, so effects before the last one in a
/// transaction are overwritten and never applied. This matters when undo
/// covers a joined history group: the restores of later events arrive first,
/// mapped back through earlier changes where a range can land inside a
/// cluster, and the earliest event's exact restore comes last.
export function effectiveEffects(effects: readonly MentionEffect[]): readonly MentionEffect[] {
  let last = effects.length - 1;
  while (last >= 0 && effects[last]!.kind !== "restore") last -= 1;
  return last < 0 ? effects : effects.slice(last);
}

export type TransactionResult = {
  doc: string;
  mentions: readonly Mention[];
  /// Ids the text changes removed, in ascending order.
  removed: string[];
  /// The effects that changed the mention set, in order. A link that is
  /// already held and an unlink of an absent id change nothing.
  applied: AppliedEffect[];
};

/// Predicts one editor transaction the way the bridge will replay it: the
/// sequential edits of its text changes, then its effects in order. The save
/// session and the editor mention field both call this, so they cannot
/// disagree about what a transaction did.
export function applyTransaction(
  doc: string,
  mentions: readonly Mention[],
  changes: ChangeSet,
  effects: readonly MentionEffect[],
): TransactionResult {
  let body: AnnotatedBody = { body: doc, mentions };
  const removed = new Set<string>();
  if (!changes.empty) {
    const edits = toSequentialEdits(changes, doc);
    const after = changes.apply(Text.of(doc.split("\n"))).toString();
    if (mentions.length > 0) {
      for (const edit of edits) {
        const next = applyMentionOp(body, replaceOp(edit));
        for (const id of next.removed) removed.add(id);
        body = next;
      }
      if (body.body !== after) {
        throw new AdapterError("inconsistent_changeset", "mention prediction does not reproduce the document");
      }
    } else {
      body = { body: after, mentions };
    }
  }
  const applied: AppliedEffect[] = [];
  const holds = (set: readonly Mention[], mention: Mention) => set.some((other) => sameMentions([other], [mention]));
  for (const effect of effectiveEffects(effects)) {
    if (effect.kind === "restore") {
      const target = canonicalMentions(effect.mentions);
      for (const held of canonicalMentions(body.mentions)) {
        if (holds(target, held)) continue;
        body = applyMentionOp(body, { kind: "unlink", mentionId: held.id });
        applied.push({ kind: "unlink", mentionId: held.id });
      }
      for (const mention of target) {
        if (holds(body.mentions, mention)) continue;
        body = applyMentionOp(body, linkOp(mention));
        applied.push({ kind: "link", mention });
      }
    } else if (effect.kind === "link") {
      const next = applyMentionOp(body, linkOp(effect.mention));
      if (!sameMentions(next.mentions, body.mentions)) applied.push(effect);
      body = next;
    } else if (body.mentions.some((held) => held.id === effect.mentionId)) {
      body = applyMentionOp(body, { kind: "unlink", mentionId: effect.mentionId });
      applied.push(effect);
    }
  }
  return { doc: body.body, mentions: body.mentions, removed: [...removed].sort(), applied };
}
