import assert from "node:assert/strict";
import { test } from "node:test";

import {
  applyMentionOp,
  applyTransaction,
  MentionError,
  validateMentions,
  type AnnotatedBody,
  type Mention,
  type MentionOp,
} from "../src/mentions.ts";
import {
  applyReply,
  beginSave,
  openSession,
  pendingDrops,
  recordEdit,
  recordLink,
  recordUnlink,
  type SaveState,
} from "../src/save-session.ts";
import { asMention, changesFor, expectedOps, fixture, read, steps, type FixtureCase } from "./fixture.ts";

const corpus = read("grapheme_boundaries.json") as {
  cases: { id: string; text: string; boundaries: number[] }[];
};

test("grapheme cluster boundaries match the shared corpus", () => {
  for (const item of corpus.cases) {
    const found = [0];
    for (let end = 1; end <= item.text.length; end += 1) {
      try {
        validateMentions({
          body: item.text,
          mentions: [{ id: "m", entityId: "e", startUtf16: 0, endUtf16: end }],
        });
        found.push(end);
      } catch (error) {
        assert.ok(error instanceof MentionError, item.id);
        assert.ok(["split_scalar", "split_grapheme"].includes(error.reason), `${item.id} ${error.reason}`);
      }
    }
    assert.deepEqual(found, item.boundaries, item.id);
  }
});

test("the mirrored reducer reproduces every shared case and its dropped ids", () => {
  for (const item of fixture.cases) {
    let body: AnnotatedBody = { body: item.base, mentions: item.mentions.map(asMention) };
    const dropped = new Set<string>();
    for (const op of expectedOps(item)) {
      const next = applyMentionOp(body, op);
      if (op.kind === "replace") for (const id of next.removed) dropped.add(id);
      body = next;
    }
    assert.equal(body.body, item.expected_body, `${item.id} body`);
    assert.deepEqual(body.mentions, item.expected_mentions.map(asMention), `${item.id} mentions`);
    if (item.expected_dropped) {
      const live = new Set(body.mentions.map((held) => held.id));
      assert.deepEqual([...dropped].filter((id) => !live.has(id)).sort(), item.expected_dropped, `${item.id} dropped`);
    }
  }
});

function replaySession(item: FixtureCase): SaveState {
  let state = openSession({
    sessionId: "session",
    pageId: "22222222-2222-4222-8222-222222222222",
    doc: item.base,
    title: "Bridge",
    revision: item.mentions.length > 0 ? 1 : 0,
    mentions: item.mentions.map(asMention),
  });
  for (const step of steps(item)) {
    if ("text" in step) state = recordEdit(state, changesFor(state.draftDoc, step.text));
    else if ("link" in step) state = recordLink(state, asMention(step.link));
    else state = recordUnlink(state, step.unlink);
  }
  return state;
}

test("the save session sends each shared case as the Rust reducer expects it", () => {
  for (const item of fixture.cases) {
    const state = replaySession(item);
    assert.equal(state.draftDoc, item.expected_body, `${item.id} draft`);
    assert.deepEqual(state.draftMentions, item.expected_mentions.map(asMention), `${item.id} draft mentions`);
    if (item.expected_dropped) assert.deepEqual(pendingDrops(state), item.expected_dropped, `${item.id} drops`);
    const began = beginSave(state);
    if (steps(item).length === 0) {
      assert.deepEqual(began, { ok: false, reason: "no_changes" }, item.id);
      continue;
    }
    assert.equal(began.ok, true, item.id);
    if (!began.ok) continue;
    assert.deepEqual(began.request.ops, expectedOps(item), `${item.id} ops`);
    assert.deepEqual(began.request.mentions, item.expected_mentions.map(asMention), `${item.id} expected mentions`);
    assert.equal(began.request.doc, item.expected_body, `${item.id} doc`);
  }
});

const SCENE = "Elara crossed into Valdoria.";
const PLACE = "11111111-1111-4111-8111-111111111111";
const HOUSE = "33333333-3333-4333-8333-333333333333";
const VALDORIA: Mention = { id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", entityId: PLACE, startUtf16: 19, endUtf16: 27 };

function linked(mentions: Mention[] = [VALDORIA]): SaveState {
  return openSession({ sessionId: "s", pageId: "p", doc: SCENE, title: "T", revision: 1, mentions });
}

function ack(state: SaveState, revision: number) {
  const began = beginSave(state);
  assert.equal(began.ok, true);
  if (!began.ok) throw new Error("unreachable");
  return {
    began,
    reply: {
      status: "ok" as const,
      requestId: began.request.requestId,
      sessionId: began.request.sessionId,
      pageId: began.request.pageId,
      doc: began.request.doc,
      title: began.request.title,
      mentions: began.request.mentions,
      revision,
    },
  };
}

test("a linked page opens into a mention-aware ledger session", () => {
  const state = linked();
  assert.equal(state.mode, "ledger");
  assert.deepEqual(state.baseMentions, [VALDORIA]);
  assert.deepEqual(state.draftMentions, [VALDORIA]);
  assert.equal(state.status, "clean");
});

test("a stored link the client cannot validate refuses to open a session", () => {
  assert.throws(
    () => linked([{ ...VALDORIA, startUtf16: 20 }, VALDORIA]),
    (error: unknown) => error instanceof MentionError,
  );
  assert.throws(
    () => openSession({ sessionId: "s", pageId: "p", doc: "é", title: "T", revision: 1, mentions: [{ ...VALDORIA, startUtf16: 0, endUtf16: 1 }] }),
    (error: unknown) => error instanceof MentionError && error.reason === "split_grapheme",
  );
});

test("a mention-only change is dirty and saves as an ordered link", () => {
  let state = linked([]);
  assert.equal(state.mode, "net");
  state = recordLink(state, VALDORIA);
  assert.equal(state.status, "dirty");
  assert.equal(state.mode, "ledger");
  const { began, reply } = ack(state, 1);
  assert.deepEqual(began.request.ops, [
    { kind: "link", mentionId: VALDORIA.id, entityId: PLACE, startUtf16: 19, endUtf16: 27 },
  ]);
  assert.equal(applyReply(began.state, { ...reply, revision: 2 }).state.status, "clean");
});

test("an acknowledgement covers exactly the sent snapshot while later work stays dirty", () => {
  let state = linked();
  state = recordEdit(state, changesFor(SCENE, [[0, 0, "Old "]]));
  const { began, reply } = ack(state, 2);
  state = began.state;
  state = recordUnlink(state, VALDORIA.id);
  state = recordEdit(state, changesFor(state.draftDoc, [[0, 4, ""]]));
  const outcome = applyReply(state, reply);
  assert.equal(outcome.applied, true);
  state = outcome.state;
  assert.equal(state.baseRevision, 2);
  assert.deepEqual(state.baseMentions, [{ ...VALDORIA, startUtf16: 23, endUtf16: 31 }]);
  assert.deepEqual(state.draftMentions, []);
  assert.equal(state.status, "dirty");
  assert.equal(state.ledger.length, 2);
  const next = beginSave(state);
  assert.equal(next.ok, true);
  if (!next.ok) return;
  assert.deepEqual(next.request.ops, [
    { kind: "unlink", mentionId: VALDORIA.id },
    { kind: "replace", startUtf16: 0, endUtf16: 4, insertedText: "" },
  ]);
});

test("an acknowledgement whose mentions differ in any field is not success", () => {
  const state = recordEdit(linked(), changesFor(SCENE, [[0, 0, "Old "]]));
  const { began, reply } = ack(state, 2);
  const shifted = { ...VALDORIA, startUtf16: 23, endUtf16: 31 };
  for (const mentions of [[], [{ ...shifted, entityId: HOUSE }], [{ ...shifted, id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb" }], [{ ...shifted, endUtf16: 30 }], [shifted, shifted]]) {
    const outcome = applyReply(began.state, { ...reply, mentions });
    assert.equal(outcome.applied, false);
    assert.equal(outcome.state.status, "error");
    assert.deepEqual(outcome.state.baseMentions, [VALDORIA]);
    assert.deepEqual(outcome.state.draftMentions, [shifted]);
    assert.equal(outcome.state.ledger.length, 1);
  }
  const missing: Record<string, unknown> = { ...reply };
  delete missing.mentions;
  assert.equal(applyReply(began.state, missing as unknown as typeof reply).state.status, "error");
});

test("stale, busy, and uncertain replies retain the whole structured draft", () => {
  let state = linked();
  state = recordEdit(state, changesFor(SCENE, [[22, 23, ""]]));
  state = recordLink(state, { id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", entityId: HOUSE, startUtf16: 0, endUtf16: 5 });
  const { began } = ack(state, 2);
  for (const status of ["stale", "busy", "error"] as const) {
    const outcome = applyReply(began.state, { status, requestId: began.request.requestId, sessionId: "s", pageId: "p", detail: status });
    assert.equal(outcome.state.status, status);
    assert.equal(outcome.state.draftDoc, state.draftDoc);
    assert.deepEqual(outcome.state.draftMentions, state.draftMentions);
    assert.deepEqual(outcome.state.baseMentions, [VALDORIA]);
    assert.deepEqual(outcome.state.ledger, state.ledger);
    assert.deepEqual(pendingDrops(outcome.state), [VALDORIA.id]);
  }
});

test("busy reconciliation requires the base mention set as well as the text", async () => {
  const { reconcileBusy } = await import("../src/save-session.ts");
  let state = recordLink(linked([]), VALDORIA);
  const { began } = ack(state, 1);
  state = applyReply(began.state, { status: "busy", requestId: began.request.requestId, sessionId: "s", pageId: "p", detail: "busy" }).state;
  const stored = { sessionId: "s", pageId: "p", doc: SCENE, title: "T", revision: 1 };
  assert.equal(reconcileBusy(state, { ...stored, mentions: [VALDORIA] }).status, "error");
  assert.equal(reconcileBusy(state, { ...stored, revision: 2, mentions: [] }).status, "error");
  const settled = reconcileBusy(state, { ...stored, mentions: [] });
  assert.equal(settled.status, "dirty");
  assert.deepEqual(settled.draftMentions, [VALDORIA]);
  assert.equal(settled.ledger.length, 1);
});

test("an id is never repurposed for another entity while the document is open", () => {
  let state = linked();
  assert.throws(() => recordLink(state, { ...VALDORIA, entityId: HOUSE }), (error: unknown) => error instanceof MentionError && error.reason === "mention_id_conflict");
  state = recordEdit(state, changesFor(SCENE, [[22, 23, ""]]));
  assert.deepEqual(state.draftMentions, []);
  assert.throws(() => recordLink(state, { ...VALDORIA, entityId: HOUSE, startUtf16: 0, endUtf16: 5 }), (error: unknown) => error instanceof MentionError && error.reason === "mention_id_conflict");
  const { began, reply } = ack(state, 2);
  state = applyReply(began.state, reply).state;
  assert.equal(state.status, "clean");
  assert.throws(() => recordLink(state, { ...VALDORIA, entityId: HOUSE, startUtf16: 0, endUtf16: 5 }), (error: unknown) => error instanceof MentionError && error.reason === "mention_id_conflict");
  const relinked = recordLink(state, { ...VALDORIA, startUtf16: 0, endUtf16: 5 });
  assert.deepEqual(relinked.draftMentions, [{ ...VALDORIA, startUtf16: 0, endUtf16: 5 }]);
});

test("invalid links are refused without touching the draft", () => {
  const state = linked();
  const cases: [Mention, string][] = [
    [{ ...VALDORIA, id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", startUtf16: 20, endUtf16: 22 }, "overlapping_mentions"],
    [{ ...VALDORIA, id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", startUtf16: 3, endUtf16: 3 }, "invalid_mention_range"],
    [{ ...VALDORIA, id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", startUtf16: 0, endUtf16: 99 }, "invalid_mention_range"],
  ];
  for (const [mention, reason] of cases) {
    assert.throws(() => recordLink(state, mention), (error: unknown) => error instanceof MentionError && error.reason === reason, reason);
  }
  assert.equal(recordUnlink(state, "cccccccc-cccc-4ccc-8ccc-cccccccccccc"), state);
  assert.equal(recordLink(state, VALDORIA), state);
});

test("a restore makes the set exact through unlink and link, and the last restore decides", () => {
  const entity = "11111111-1111-4111-8111-111111111111";
  const id = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
  const other = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
  const doc = "Elara crossed the river.";
  const renamed: Mention = { id, entityId: entity, startUtf16: 0, endUtf16: 8 };
  const original: Mention = { id, entityId: entity, startUtf16: 0, endUtf16: 5 };
  const river: Mention = { id: other, entityId: entity, startUtf16: 18, endUtf16: 23 };
  const result = applyTransaction("Valdoria crossed the river.", [renamed], changesFor("Valdoria crossed the river.", [[0, 8, "Elara"]]), [
    { kind: "restore", mentions: [{ ...original, endUtf16: 3 }] },
    { kind: "restore", mentions: [original, river] },
  ]);
  assert.equal(result.doc, doc);
  assert.deepEqual(result.mentions, [original, river]);
  assert.deepEqual(result.removed, [id]);
  assert.deepEqual(result.applied, [
    { kind: "link", mention: original },
    { kind: "link", mention: river },
  ]);

  const moved = applyTransaction(doc, [original], changesFor(doc, []), [{ kind: "restore", mentions: [{ ...original, startUtf16: 6, endUtf16: 13 }] }]);
  assert.deepEqual(moved.applied, [
    { kind: "unlink", mentionId: id },
    { kind: "link", mention: { ...original, startUtf16: 6, endUtf16: 13 } },
  ]);
});
