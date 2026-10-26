import assert from "node:assert/strict";
import { test } from "node:test";

import { ChangeSet } from "@codemirror/state";

import {
  applyReply,
  beginSave,
  isDirty,
  openSession,
  reconcileBusy,
  recordEdit,
  UnsupportedRevisionError,
  type SaveReply,
  type SaveState,
} from "../src/save-session.ts";

function insert(text: string, at: number, value: string): ChangeSet {
  return ChangeSet.of({ from: at, to: at, insert: value }, text.length);
}

function remove(text: string, from: number, to: number): ChangeSet {
  return ChangeSet.of({ from, to, insert: "" }, text.length);
}

function opened(doc = "D1", revision = 4, hasMentions = false): SaveState {
  return openSession({
    sessionId: "session-1",
    pageId: "22222222-2222-4222-8222-222222222222",
    doc,
    revision,
    hasMentions,
  });
}

function reread(
  state: SaveState,
  doc: string,
  revision: number,
): {
  sessionId: string;
  pageId: string;
  doc: string;
  revision: number;
} {
  return {
    sessionId: state.sessionId,
    pageId: state.pageId,
    doc,
    revision,
  };
}

function started(state: SaveState) {
  const began = beginSave(state);
  assert.equal(began.ok, true);
  if (!began.ok) {
    throw new Error("unreachable");
  }
  return began;
}

test("a valid acknowledgement survives typing during the save", () => {
  let state = opened("D1", 4);
  state = recordEdit(state, insert("D1", 2, "a"));
  const began = started(state);
  state = began.state;
  assert.equal(began.request.expectedRevision, 4);
  assert.equal(began.request.doc, "D1a");

  state = recordEdit(state, insert("D1a", 3, "b"));
  assert.equal(state.status, "saving");
  assert.equal(state.draftDoc, "D1ab");

  const reply: SaveReply = {
    status: "ok",
    requestId: began.request.requestId,
    sessionId: "session-1",
    pageId: "22222222-2222-4222-8222-222222222222",
    doc: "D1a",
    revision: 5,
  };
  const outcome = applyReply(state, reply);
  assert.equal(outcome.applied, true);
  state = outcome.state;

  assert.equal(state.baseDoc, "D1a");
  assert.equal(state.baseRevision, 5);
  assert.equal(state.draftDoc, "D1ab");
  assert.equal(state.status, "dirty");
  assert.equal(state.ledger.length, 1);
  assert.equal(isDirty(state), true);

  const second = started(state);
  assert.equal(second.request.expectedRevision, 5);
  assert.equal(second.request.doc, "D1ab");
  assert.equal(second.request.edits.length, 1);
});

test("a reply from another session or page is ignored", () => {
  let state = opened();
  state = recordEdit(state, insert("D1", 0, "x"));
  const began = started(state);
  state = began.state;

  const foreign: SaveReply = {
    status: "ok",
    requestId: began.request.requestId,
    sessionId: "session-2",
    pageId: began.request.pageId,
    doc: "D1x",
    revision: 99,
  };
  const ignored = applyReply(state, foreign);
  assert.equal(ignored.applied, false);
  assert.equal(ignored.ignored, "other_session");
  assert.equal(ignored.state.baseRevision, 4);
  assert.equal(ignored.state.status, "saving");

  const staleRequest: SaveReply = {
    status: "ok",
    requestId: began.request.requestId + 1,
    sessionId: began.request.sessionId,
    pageId: began.request.pageId,
    doc: "D1x",
    revision: 5,
  };
  const duplicate = applyReply(ignored.state, staleRequest);
  assert.equal(duplicate.ignored, "other_request");
  assert.equal(duplicate.state.baseRevision, 4);
});

test("a stale reply keeps the draft, base, and ledger", () => {
  let state = opened("D1", 4);
  state = recordEdit(state, insert("D1", 2, "a"));
  const began = started(state);
  state = began.state;
  state = recordEdit(state, insert("D1a", 3, "b"));

  const outcome = applyReply(state, {
    status: "stale",
    requestId: began.request.requestId,
    sessionId: state.sessionId,
    pageId: state.pageId,
    detail: "expected 4, found 5",
  });
  state = outcome.state;

  assert.equal(state.status, "stale");
  assert.equal(state.baseRevision, 4);
  assert.equal(state.baseDoc, "D1");
  assert.equal(state.draftDoc, "D1ab");
  assert.equal(state.ledger.length, 2);
  assert.equal(state.inFlight, null);
  assert.equal(isDirty(state), true);
});

test("a mismatched acknowledgement never overwrites the base", () => {
  let state = opened("D1", 4);
  state = recordEdit(state, insert("D1", 2, "a"));
  const began = started(state);
  state = began.state;

  const outcome = applyReply(state, {
    status: "ok",
    requestId: began.request.requestId,
    sessionId: state.sessionId,
    pageId: state.pageId,
    doc: "something else",
    revision: 5,
  });
  assert.equal(outcome.applied, false);
  assert.equal(outcome.state.status, "error");
  assert.equal(outcome.state.baseRevision, 4);
  assert.equal(outcome.state.draftDoc, "D1a");
  assert.equal(outcome.state.ledger.length, 1);
});

test("a busy save that did not commit stays retryable with its ledger", () => {
  let state = opened("D1", 4);
  state = recordEdit(state, insert("D1", 2, "a"));
  const began = started(state);
  state = began.state;

  const busy = applyReply(state, {
    status: "busy",
    requestId: began.request.requestId,
    sessionId: state.sessionId,
    pageId: state.pageId,
    detail: "database is busy",
  });
  state = busy.state;
  assert.equal(state.status, "busy");
  assert.equal(state.draftDoc, "D1a");
  assert.equal(state.baseRevision, 4);

  state = reconcileBusy(state, reread(state, "D1", 4));
  assert.equal(state.status, "dirty");
  assert.equal(state.baseDoc, "D1");
  assert.equal(state.baseRevision, 4);
  assert.equal(state.draftDoc, "D1a");
  assert.equal(state.ledger.length, 1);

  const retry = started(state);
  assert.equal(retry.request.expectedRevision, 4);
  assert.deepEqual(retry.request.edits, [
    { start_utf16: 2, end_utf16: 2, inserted_text: "a" },
  ]);
});

test("another writer's revision cannot settle an unapplied R9 as clean", () => {
  // R9 on a linked page does not change the body. A different writer can
  // advance the revision while leaving that body alone, so matching text plus
  // the next revision is not evidence that this request committed.
  const base = "Elara";
  let state = opened(base, 1, true);
  state = recordEdit(
    state,
    ChangeSet.of({ from: 0, to: 5, insert: "Elara" }, base.length),
  );
  const began = started(state);
  state = began.state;
  state = applyReply(state, {
    status: "busy",
    requestId: began.request.requestId,
    sessionId: state.sessionId,
    pageId: state.pageId,
    detail: "database is busy",
  }).state;

  state = reconcileBusy(state, reread(state, base, 2));
  assert.equal(state.status, "error");
  assert.equal(state.baseRevision, 1);
  assert.equal(state.draftDoc, base);
  assert.equal(state.ledger.length, 1);
  assert.equal(isDirty(state), true);
});

test("typing after a busy reply preserves the ambiguity and the later edit", () => {
  let state = opened("D1", 4);
  state = recordEdit(state, insert("D1", 2, "a"));
  const began = started(state);
  state = began.state;
  state = applyReply(state, {
    status: "busy",
    requestId: began.request.requestId,
    sessionId: state.sessionId,
    pageId: state.pageId,
    detail: "database is busy",
  }).state;

  state = recordEdit(state, insert("D1a", 3, "b"));
  assert.equal(state.status, "busy");
  assert.equal(state.draftDoc, "D1ab");
  assert.equal(state.ledger.length, 2);
  assert.deepEqual(beginSave(state), { ok: false, reason: "busy_pending" });

  state = reconcileBusy(state, reread(state, "D1", 4));
  assert.equal(state.status, "dirty");
  assert.equal(state.baseDoc, "D1");
  assert.equal(state.baseRevision, 4);
  assert.equal(state.draftDoc, "D1ab");
  assert.equal(state.ledger.length, 2);

  const next = started(state);
  assert.equal(next.request.expectedRevision, 4);
  assert.equal(next.request.doc, "D1ab");
  // Net mode composes the retried ledger against the base again.
  assert.deepEqual(next.request.edits, [
    { start_utf16: 2, end_utf16: 2, inserted_text: "ab" },
  ]);
});

test("a reread from another page or session never settles busy", () => {
  let state = opened("D1", 4);
  state = recordEdit(state, insert("D1", 2, "a"));
  const began = started(state);
  state = began.state;
  state = applyReply(state, {
    status: "busy",
    requestId: began.request.requestId,
    sessionId: state.sessionId,
    pageId: state.pageId,
    detail: "database is busy",
  }).state;

  const otherPage = reconcileBusy(state, {
    sessionId: state.sessionId,
    pageId: "33333333-3333-4333-8333-333333333333",
    doc: "D1a",
    revision: 5,
  });
  assert.equal(otherPage.status, "busy");
  assert.notEqual(otherPage.lastAttempt, null);

  const otherSession = reconcileBusy(state, {
    sessionId: "session-2",
    pageId: state.pageId,
    doc: "D1a",
    revision: 5,
  });
  assert.equal(otherSession.status, "busy");
  assert.notEqual(otherSession.lastAttempt, null);
});

test("an acknowledgement with an impossible revision is refused", () => {
  let state = opened("D1", 4);
  state = recordEdit(state, insert("D1", 2, "a"));
  const began = started(state);
  state = began.state;

  for (const revision of [3, 4, 6, -1, 1.5, Number.NaN]) {
    const outcome = applyReply(state, {
      status: "ok",
      requestId: began.request.requestId,
      sessionId: state.sessionId,
      pageId: state.pageId,
      doc: "D1a",
      revision,
    });
    assert.equal(outcome.applied, false, `revision ${revision}`);
    assert.equal(outcome.state.status, "error");
    assert.equal(outcome.state.baseDoc, "D1");
    assert.equal(outcome.state.baseRevision, 4);
    assert.equal(outcome.state.ledger.length, 1);
  }
});

test("a genuine no-op is accepted at the unchanged revision", () => {
  const base = "D1";
  let state = opened(base, 7, false);
  state = recordEdit(state, insert(base, 2, "x"));
  state = recordEdit(state, remove("D1x", 2, 3));
  const began = started(state);
  assert.deepEqual(began.request.edits, []);
  assert.equal(began.request.doc, base);

  const outcome = applyReply(began.state, {
    status: "ok",
    requestId: began.request.requestId,
    sessionId: state.sessionId,
    pageId: state.pageId,
    doc: base,
    revision: 7,
  });
  assert.equal(outcome.applied, true);
  assert.equal(outcome.state.status, "clean");
  assert.equal(outcome.state.baseDoc, base);
  assert.equal(outcome.state.baseRevision, 7);
  assert.equal(outcome.state.ledger.length, 0);
});

test("a changed body at the unchanged revision is refused", () => {
  let state = opened("D1", 4);
  state = recordEdit(state, insert("D1", 2, "a"));
  const began = started(state);

  const outcome = applyReply(began.state, {
    status: "ok",
    requestId: began.request.requestId,
    sessionId: state.sessionId,
    pageId: state.pageId,
    doc: "D1a",
    revision: 4,
  });
  assert.equal(outcome.applied, false);
  assert.equal(outcome.state.status, "error");
  assert.equal(outcome.state.baseRevision, 4);
  assert.equal(outcome.state.ledger.length, 1);
});

test("revisions outside the exactly representable range are refused", () => {
  for (const revision of [-1, 1.5, Number.NaN, 2 ** 53]) {
    assert.throws(
      () => opened("D1", revision),
      UnsupportedRevisionError,
      `revision ${revision}`,
    );
  }
  assert.equal(opened("D1", Number.MAX_SAFE_INTEGER).baseRevision, Number.MAX_SAFE_INTEGER);
});

test("the last representable revision accepts its save and then stops", () => {
  const last = Number.MAX_SAFE_INTEGER - 1;
  let state = opened("D1", last);
  state = recordEdit(state, insert("D1", 2, "a"));
  const began = started(state);
  assert.equal(began.request.expectedRevision, last);
  assert.equal(began.request.requestId, 1);

  const outcome = applyReply(began.state, {
    status: "ok",
    requestId: began.request.requestId,
    sessionId: state.sessionId,
    pageId: state.pageId,
    doc: "D1a",
    revision: Number.MAX_SAFE_INTEGER,
  });
  assert.equal(outcome.applied, true);
  assert.equal(outcome.state.baseRevision, Number.MAX_SAFE_INTEGER);
  assert.equal(outcome.state.status, "clean");

  state = recordEdit(outcome.state, insert("D1a", 3, "b"));
  assert.deepEqual(beginSave(state), {
    ok: false,
    reason: "revision_exhausted",
  });
});

test("ledger mode does not accept a no-op from an unchanged revision", () => {
  const base = "Elara";
  let state = opened(base, 3, true);
  // Insert and delete outside the mention: the body returns to the base, but
  // only the reducer knows whether the mention survived.
  state = recordEdit(state, insert(base, 5, "x"));
  state = recordEdit(state, remove("Elarax", 5, 6));
  const began = started(state);
  assert.equal(began.request.doc, base);

  const outcome = applyReply(began.state, {
    status: "ok",
    requestId: began.request.requestId,
    sessionId: state.sessionId,
    pageId: state.pageId,
    doc: base,
    revision: 3,
  });
  assert.equal(outcome.applied, false);
  assert.equal(outcome.state.status, "error");
  assert.equal(outcome.state.baseRevision, 3);
  assert.equal(outcome.state.ledger.length, 2);
});

test("a reply that names another page is ignored", () => {
  let state = opened("D1", 4);
  state = recordEdit(state, insert("D1", 2, "a"));
  const began = started(state);
  state = began.state;

  const outcome = applyReply(state, {
    status: "ok",
    requestId: began.request.requestId,
    sessionId: state.sessionId,
    pageId: "33333333-3333-4333-8333-333333333333",
    doc: "D1a",
    revision: 5,
  });
  assert.equal(outcome.applied, false);
  assert.equal(outcome.ignored, "other_session");
  assert.equal(outcome.state.status, "saving");
  assert.equal(outcome.state.baseRevision, 4);
  assert.equal(outcome.state.ledger.length, 1);
});

test("net mode refuses an invalid draft before composition can hide it", () => {
  const state = opened("abc", 0, false);

  assert.throws(
    () => recordEdit(state, ChangeSet.of({ from: 1, to: 1, insert: "\u0000" }, 3)),
    /NUL/,
  );
  assert.throws(
    () =>
      recordEdit(
        opened("abc", 0, false),
        ChangeSet.of({ from: 1, to: 1, insert: "\uD800" }, 3),
      ),
    /unpaired/,
  );
  // A transaction that replaces only the low surrogate leaves a well-formed
  // draft but starts inside a pair; recording must not accept it just because
  // a later transaction could cancel it.
  assert.throws(
    () =>
      recordEdit(
        opened("A📚B", 0, false),
        ChangeSet.of({ from: 2, to: 3, insert: "📠" }, 4),
      ),
    /surrogate pair/,
  );
  assert.equal(state.ledger.length, 0);
});

test("an unreconciled busy outcome preserves the draft", () => {
  let state = opened("D1", 4);
  state = recordEdit(state, insert("D1", 2, "a"));
  const began = started(state);
  state = began.state;
  state = applyReply(state, {
    status: "busy",
    requestId: began.request.requestId,
    sessionId: state.sessionId,
    pageId: state.pageId,
    detail: "database is busy",
  }).state;

  state = reconcileBusy(state, reread(state, "external winner", 7));
  assert.equal(state.status, "error");
  assert.equal(state.baseRevision, 4);
  assert.equal(state.draftDoc, "D1a");
  assert.equal(state.ledger.length, 1);
});

test("save is single-flight and refuses an empty ledger", () => {
  const clean = opened();
  assert.deepEqual(beginSave(clean), { ok: false, reason: "no_changes" });

  let state = recordEdit(clean, insert("D1", 2, "a"));
  const first = started(state);
  state = first.state;
  assert.deepEqual(beginSave(state), { ok: false, reason: "in_flight" });
});

test("net mode composes while ledger mode keeps the ordered history", () => {
  const base = "Elara";
  const first = insert(base, 2, "x");
  const second = remove("Elxara", 2, 3);

  let prose = opened(base, 0, false);
  prose = recordEdit(prose, first);
  prose = recordEdit(prose, second);
  const net = started(prose);
  assert.deepEqual(net.request.edits, []);
  assert.equal(net.request.doc, base);

  let linked = opened(base, 0, true);
  linked = recordEdit(linked, first);
  linked = recordEdit(linked, second);
  const ledger = started(linked);
  assert.deepEqual(ledger.request.edits, [
    { start_utf16: 2, end_utf16: 2, inserted_text: "x" },
    { start_utf16: 2, end_utf16: 3, inserted_text: "" },
  ]);
});
