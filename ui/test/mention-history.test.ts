import assert from "node:assert/strict";
import { test } from "node:test";

import { EditorState, Transaction, type TransactionSpec } from "@codemirror/state";
import { history, redo, redoDepth, undo, undoDepth } from "@codemirror/commands";

import {
  linkSpec,
  mentionField,
  mentionHistory,
  recordTransaction,
  unlinkSpec,
} from "../src/mention-history.ts";
import { canonicalMentions, type Mention } from "../src/mentions.ts";
import { beginSave, openSession, pendingDrops, type SaveState } from "../src/save-session.ts";
import { asMention, changesFor, fixture, steps } from "./fixture.ts";

const PLACE = "11111111-1111-4111-8111-111111111111";
const HOUSE = "33333333-3333-4333-8333-333333333333";
const SCENE = "Elara crossed into Valdoria.";
const VALDORIA: Mention = { id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", entityId: PLACE, startUtf16: 19, endUtf16: 27 };

/// An editor wired the way the desktop must wire it: every transaction goes
/// through the save session first, and a refused one never reaches the state.
class Editor {
  state: EditorState;
  session: SaveState;
  time = 0;

  constructor(doc: string, mentions: Mention[]) {
    this.state = EditorState.create({ doc, extensions: [history(), mentionHistory(mentions)] });
    this.session = openSession({ sessionId: "s", pageId: "p", doc, title: "T", revision: 1, mentions });
  }

  apply(tr: Transaction): void {
    this.session = recordTransaction(this.session, tr);
    this.state = tr.state;
    assert.equal(this.state.doc.toString(), this.session.draftDoc, "editor and session text");
    assert.deepEqual(this.mentions(), this.session.draftMentions, "editor and session mentions");
  }

  dispatch(spec: TransactionSpec, join = false): void {
    this.time += join ? 10 : 10_000;
    this.apply(this.state.update({ ...spec, annotations: [...[spec.annotations ?? []].flat(), Transaction.time.of(this.time)] }));
  }

  undo(): boolean {
    return undo({ state: this.state, dispatch: (tr) => this.apply(tr) });
  }

  redo(): boolean {
    return redo({ state: this.state, dispatch: (tr) => this.apply(tr) });
  }

  mentions(): readonly Mention[] {
    return this.state.field(mentionField);
  }

  text(): string {
    return this.state.doc.toString();
  }
}

test("undo of a text deletion restores the removed link with its id, entity, and range", () => {
  const editor = new Editor(SCENE, [VALDORIA]);
  editor.dispatch({ changes: { from: 22, to: 23 } });
  assert.deepEqual(editor.mentions(), []);
  assert.deepEqual(pendingDrops(editor.session), [VALDORIA.id]);

  assert.equal(editor.undo(), true);
  assert.equal(editor.text(), SCENE);
  assert.deepEqual(editor.mentions(), [VALDORIA]);
  assert.deepEqual(pendingDrops(editor.session), []);

  assert.equal(editor.redo(), true);
  assert.equal(editor.text(), "Elara crossed into Valoria.");
  assert.deepEqual(editor.mentions(), []);

  assert.equal(editor.undo(), true);
  assert.deepEqual(editor.mentions(), [VALDORIA]);
  const began = beginSave(editor.session);
  assert.equal(began.ok, true);
  if (!began.ok) return;
  assert.deepEqual(began.request.mentions, [VALDORIA]);
  assert.deepEqual(began.request.ops.filter((op) => op.kind === "link"), [
    { kind: "link", mentionId: VALDORIA.id, entityId: PLACE, startUtf16: 19, endUtf16: 27 },
    { kind: "link", mentionId: VALDORIA.id, entityId: PLACE, startUtf16: 19, endUtf16: 27 },
  ]);
});

test("undo and redo of a cluster-glued edit at the end restore the same link", () => {
  const editor = new Editor(SCENE, [VALDORIA]);
  editor.dispatch({ changes: { from: 27, insert: "́" } });
  assert.deepEqual(editor.mentions(), []);
  editor.undo();
  assert.deepEqual(editor.mentions(), [VALDORIA]);
  editor.redo();
  assert.deepEqual(editor.mentions(), []);
});

test("link and unlink are their own history events", () => {
  const editor = new Editor(SCENE, []);
  editor.dispatch({ changes: { from: 0, insert: "Old " } });
  editor.dispatch(linkSpec({ ...VALDORIA, startUtf16: 23, endUtf16: 31 }), true);
  editor.dispatch({ changes: { from: 31, insert: "n" } }, true);
  assert.deepEqual(editor.mentions(), [{ ...VALDORIA, startUtf16: 23, endUtf16: 31 }]);

  editor.undo();
  assert.equal(editor.text(), "Old " + SCENE);
  assert.deepEqual(editor.mentions(), [{ ...VALDORIA, startUtf16: 23, endUtf16: 31 }]);
  editor.undo();
  assert.deepEqual(editor.mentions(), []);
  assert.equal(editor.text(), "Old " + SCENE);
  editor.redo();
  assert.deepEqual(editor.mentions(), [{ ...VALDORIA, startUtf16: 23, endUtf16: 31 }]);

  editor.dispatch(unlinkSpec(VALDORIA.id));
  assert.deepEqual(editor.mentions(), []);
  editor.undo();
  assert.deepEqual(editor.mentions(), [{ ...VALDORIA, startUtf16: 23, endUtf16: 31 }]);
});

test("a joined typing group restores the link at its original range", () => {
  const editor = new Editor(SCENE, [VALDORIA]);
  editor.dispatch({ changes: { from: 19, insert: "the " } });
  editor.dispatch({ changes: { from: 23, insert: "big " } }, true);
  assert.deepEqual(editor.mentions(), [{ ...VALDORIA, startUtf16: 27, endUtf16: 35 }]);
  editor.dispatch({ changes: { from: 29, to: 30 } }, true);
  assert.deepEqual(editor.mentions(), []);
  assert.equal(undoDepth(editor.state), 2);
  editor.undo();
  assert.equal(editor.text(), "Elara crossed into the big Valdoria.");
  assert.deepEqual(editor.mentions(), [{ ...VALDORIA, startUtf16: 27, endUtf16: 35 }]);
  editor.undo();
  assert.equal(editor.text(), SCENE);
  assert.deepEqual(editor.mentions(), [VALDORIA]);
});

test("plain text undo keeps working without mentions", () => {
  const editor = new Editor("abc", []);
  editor.dispatch({ changes: { from: 3, insert: "d" } });
  editor.undo();
  assert.equal(editor.text(), "abc");
  assert.deepEqual(editor.mentions(), []);
  editor.redo();
  assert.equal(editor.text(), "abcd");
});

test("undo restores a link that a save already committed as removed", () => {
  const editor = new Editor(SCENE, [VALDORIA]);
  editor.dispatch({ changes: { from: 22, to: 23 } });
  const began = beginSave(editor.session);
  assert.equal(began.ok, true);
  if (!began.ok) return;
  editor.session = began.state;
  editor.undo();
  assert.deepEqual(editor.mentions(), [VALDORIA]);
  assert.equal(editor.session.ledger.at(-1)?.kind, "link");
});

test("history never hands an id to another entity", () => {
  const editor = new Editor(SCENE, [VALDORIA]);
  assert.throws(() => editor.dispatch(linkSpec({ ...VALDORIA, entityId: HOUSE, startUtf16: 0, endUtf16: 5 })));
  assert.deepEqual(editor.mentions(), [VALDORIA]);
  assert.equal(undoDepth(editor.state), 0);
});

test("every shared fixture case undoes to its base and redoes to its result", () => {
  for (const item of fixture.cases) {
    const initial = item.mentions.map(asMention);
    const editor = new Editor(item.base, initial);
    for (const step of steps(item)) {
      if ("text" in step) editor.dispatch({ changes: changesFor(editor.text(), step.text) });
      else if ("link" in step) editor.dispatch(linkSpec(asMention(step.link)));
      else editor.dispatch(unlinkSpec(step.unlink));
    }
    const expected = item.expected_mentions.map(asMention);
    assert.equal(editor.text(), item.expected_body, item.id);
    assert.deepEqual(editor.mentions(), expected, item.id);
    while (editor.undo());
    assert.equal(editor.text(), item.base, `${item.id} undone`);
    assert.deepEqual(editor.mentions(), canonicalMentions(initial), `${item.id} undone mentions`);
    while (editor.redo());
    assert.equal(editor.text(), item.expected_body, `${item.id} redone`);
    assert.deepEqual(editor.mentions(), expected, `${item.id} redone mentions`);
  }
});

const PIECES = ["a", "b", " ", "\n", "́", "‍", "\u{1F469}", "\u{1F4BB}", "\u{1F3FD}", "\u{1F1E6}", "\u{1F1F7}", "क", "्", "ष", "Valdoria"];

function random(seed: number): () => number {
  return () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function scalarStarts(text: string): number[] {
  const starts: number[] = [];
  for (let i = 0; i <= text.length; i += 1) {
    const unit = text.charCodeAt(i);
    if (!(unit >= 0xdc00 && unit <= 0xdfff)) starts.push(i);
  }
  return starts;
}

function clusterStarts(text: string): number[] {
  const starts = [...new Intl.Segmenter("en", { granularity: "grapheme" }).segment(text)].map((part) => part.index);
  starts.push(text.length);
  return starts;
}

test("random Unicode editing with joined groups, links, undo, and redo keeps identity", () => {
  const entities = [PLACE, HOUSE];
  let restored = 0;
  for (let seed = 1; seed <= 300; seed += 1) {
    const next = random(seed);
    const pick = <T>(items: readonly T[]): T => items[Math.floor(next() * items.length)]!;
    const base = "Elara \u{1F469}‍\u{1F4BB} met क् ष in Valdoria \u{1F1E6}\u{1F1F7}.";
    const initial: Mention[] = [
      { id: "00000000-0000-4000-8000-000000000001", entityId: PLACE, startUtf16: 0, endUtf16: 5 },
      { id: "00000000-0000-4000-8000-000000000002", entityId: HOUSE, startUtf16: 6, endUtf16: 11 },
    ];
    const editor = new Editor(base, initial);
    const entityOf = new Map(initial.map((held) => [held.id, held.entityId]));
    let fresh = 2;
    for (let step = 0; step < 40; step += 1) {
      const roll = next();
      const text = editor.text();
      const scalars = scalarStarts(text);
      if (roll < 0.35) {
        editor.dispatch({ changes: { from: pick(scalars), insert: pick(PIECES) } }, next() < 0.6);
      } else if (roll < 0.55 && text.length > 0) {
        const a = pick(scalars), b = pick(scalars);
        editor.dispatch({ changes: { from: Math.min(a, b), to: Math.max(a, b), insert: next() < 0.3 ? pick(PIECES) : "" } }, next() < 0.6);
      } else if (roll < 0.62) {
        const a = pick(scalars), b = pick(scalars);
        if (a !== b) editor.dispatch({ changes: [{ from: Math.min(a, b), insert: pick(PIECES) }, { from: Math.max(a, b), insert: pick(PIECES) }] }, next() < 0.5);
      } else if (roll < 0.72) {
        const clusters = clusterStarts(text);
        const a = pick(clusters), b = pick(clusters);
        const start = Math.min(a, b), end = Math.max(a, b);
        const free = start < end && !editor.mentions().some((held) => start < held.endUtf16 && held.startUtf16 < end);
        if (free) {
          fresh += 1;
          const id = `00000000-0000-4000-8000-${String(fresh).padStart(12, "0")}`;
          const entityId = pick(entities);
          entityOf.set(id, entityId);
          editor.dispatch(linkSpec({ id, entityId, startUtf16: start, endUtf16: end }));
        }
      } else if (roll < 0.77 && editor.mentions().length > 0) {
        editor.dispatch(unlinkSpec(pick(editor.mentions()).id));
      } else if (roll < 0.9) {
        const before = new Set(editor.mentions().map((held) => held.id));
        editor.undo();
        restored += editor.mentions().filter((held) => !before.has(held.id)).length;
      } else {
        editor.redo();
      }
      for (const held of editor.mentions()) assert.equal(held.entityId, entityOf.get(held.id), `seed ${seed} entity`);
    }
    const reached = { text: editor.text(), mentions: editor.mentions() };
    const depth = redoDepth(editor.state);
    while (editor.undo());
    assert.equal(editor.text(), base, `seed ${seed} undone text`);
    assert.deepEqual(editor.mentions(), canonicalMentions(initial), `seed ${seed} undone mentions`);
    while (editor.redo());
    if (depth === 0) {
      assert.equal(editor.text(), reached.text, `seed ${seed} redone text`);
      assert.deepEqual(editor.mentions(), reached.mentions, `seed ${seed} redone mentions`);
    }
    const began = beginSave(editor.session);
    if (began.ok) assert.deepEqual(began.request.mentions, editor.mentions());
  }
  assert.ok(restored > 50, `restorations exercised: ${restored}`);
});
