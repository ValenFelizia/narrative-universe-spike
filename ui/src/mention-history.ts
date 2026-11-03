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
// end belongs before. When the range cannot survive the mapping the effect is
// discarded rather than restored somewhere else.
function mapMention(mention: Mention, mapping: ChangeDesc): Mention | undefined {
  const startUtf16 = mapping.mapPos(mention.startUtf16, 1);
  const endUtf16 = mapping.mapPos(mention.endUtf16, -1);
  if (startUtf16 >= endUtf16) return undefined;
  return { ...mention, startUtf16, endUtf16 };
}

export const linkMention = StateEffect.define<Mention>({ map: mapMention });
export const unlinkMention = StateEffect.define<string>();

export function mentionEffects(tr: Transaction): MentionEffect[] {
  const effects: MentionEffect[] = [];
  for (const effect of tr.effects) {
    if (effect.is(linkMention)) effects.push({ kind: "link", mention: effect.value });
    else if (effect.is(unlinkMention)) effects.push({ kind: "unlink", mentionId: effect.value });
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

// Undo of a link is an unlink and the reverse. Undo of text that removed a
// link restores that link with its id, entity, and original range, which is
// valid again because the same undo restores the original text. The new set
// is recomputed because `tr.state` is not available while history runs.
const invertMentions = invertedEffects.of((tr) => {
  const before = tr.startState.field(mentionField, false);
  if (!before) return [];
  const effects = mentionEffects(tr);
  if (before.length === 0 && effects.length === 0) return [];
  const after = applyTransaction(tr.startState.doc.toString(), before, tr.changes, effects).mentions;
  const beforeIds = new Set(before.map((held) => held.id));
  const afterIds = new Set(after.map((held) => held.id));
  return [
    ...after.filter((held) => !beforeIds.has(held.id)).map((held) => unlinkMention.of(held.id)),
    ...before.filter((held) => !afterIds.has(held.id)).map((held) => linkMention.of(held)),
  ];
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
