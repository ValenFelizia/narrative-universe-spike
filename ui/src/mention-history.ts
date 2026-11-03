import {
  StateEffect,
  StateField,
  type ChangeDesc,
  type Extension,
  type Transaction,
  type TransactionSpec,
} from "@codemirror/state";
import { invertedEffects, isolateHistory } from "@codemirror/commands";

import {
  applyTransaction,
  canonicalMentions,
  type Mention,
  type MentionEffect,
} from "./mentions.ts";
import { recordEdit, type SaveState } from "./save-session.ts";

// History stores an inverse effect in the start coordinates of its event and
// maps it back through the inverse of earlier changes when events join. A
// start that sat at the end of a removed insertion belongs after it, and an
// end belongs before. When the range cannot survive the mapping the mention
// is discarded rather than restored somewhere else.
function mapMention(mention: Mention, mapping: ChangeDesc): Mention | undefined {
  const startUtf16 = mapping.mapPos(mention.startUtf16, 1);
  const endUtf16 = mapping.mapPos(mention.endUtf16, -1);
  if (startUtf16 >= endUtf16) return undefined;
  return { ...mention, startUtf16, endUtf16 };
}

export const linkMention = StateEffect.define<Mention>({ map: mapMention });
export const unlinkMention = StateEffect.define<string>();
const restoreMentions = StateEffect.define<readonly Mention[]>({
  map: (mentions, mapping) => mentions.flatMap((held) => mapMention(held, mapping) ?? []),
});

export function mentionEffects(tr: Transaction): MentionEffect[] {
  const effects: MentionEffect[] = [];
  for (const effect of tr.effects) {
    if (effect.is(linkMention)) effects.push({ kind: "link", mention: effect.value });
    else if (effect.is(unlinkMention)) effects.push({ kind: "unlink", mentionId: effect.value });
    else if (effect.is(restoreMentions)) effects.push({ kind: "restore", mentions: effect.value });
  }
  return effects;
}

/// The draft mention set inside the editor state. It follows the same
/// prediction as the save session, so a transaction the session refuses
/// must never be applied to the view.
export const mentionField = StateField.define<readonly Mention[]>({
  create: () => [],
  update(value, tr) {
    const effects = mentionEffects(tr);
    if (value.length === 0 && effects.length === 0) return value;
    return applyTransaction(tr.startState.doc.toString(), value, tr.changes, effects).mentions;
  },
});

// The inverse of a transaction is its whole starting mention set, applied
// after the inverse text change. Comparing ids alone is not enough: a
// transaction can drop a link with its text and link the same id again at a
// new range, and the inverse text change then removes the new range while the
// id sets still look equal. When history joins events, the inverse of the
// earliest one runs last and is exact in the coordinates undo returns to, so
// the restored set never depends on how the later inverses were mapped.
const invertMentions = invertedEffects.of((tr) => {
  const before = tr.startState.field(mentionField, false);
  if (!before) return [];
  const effects = mentionEffects(tr);
  // A transaction with no text change and no mention effect cannot change
  // the mention set. Returning an effect for it, such as a cursor move, would
  // make history record it as its own undo step.
  if (effects.length === 0 && (tr.changes.empty || before.length === 0)) return [];
  return [restoreMentions.of(before)];
});

export function mentionHistory(initial: readonly Mention[]): Extension {
  return [mentionField.init(() => canonicalMentions(initial)), invertMentions];
}

/// A deliberate link is its own history event, never joined with typing.
export function linkSpec(mention: Mention): TransactionSpec {
  return { effects: linkMention.of(mention), annotations: isolateHistory.of("full") };
}

export function unlinkSpec(mentionId: string): TransactionSpec {
  return { effects: unlinkMention.of(mentionId), annotations: isolateHistory.of("full") };
}

/// Records an editor transaction, undo and redo included, into the save
/// session. Call it before the view applies the transaction; when it throws,
/// drop the transaction.
export function recordTransaction(state: SaveState, tr: Transaction): SaveState {
  return recordEdit(state, tr.changes, mentionEffects(tr));
}
