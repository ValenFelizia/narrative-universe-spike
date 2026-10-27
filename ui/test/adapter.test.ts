import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { ChangeSet, Text } from "@codemirror/state";

import {
  AdapterError,
  assertDocumentLength,
  assertEditableText,
  assertU32Coordinate,
  composeTransactions,
  decideEditing,
  MAX_UTF16,
  nextDocumentLength,
  toSequentialEdits,
  toSequentialEditsFromTransactions,
  type PageEdit,
} from "../src/adapter.ts";

type Spec = [number, number, string];

type FixtureMention = {
  id: string;
  entity_id: string;
  start_utf16: number;
  end_utf16: number;
};

type FixtureCase = {
  id: string;
  base: string;
  mentions: FixtureMention[];
  transactions: Spec[][];
  expected_body: string;
  expected_mentions: FixtureMention[];
  expected_edits: Spec[];
  expected_changed: boolean;
  expected_revision: number;
};

const fixture = JSON.parse(
  readFileSync(
    new URL("../../tests/fixtures/bridge_cases.json", import.meta.url),
    "utf8",
  ),
) as { cases: FixtureCase[] };

function asText(text: string): Text {
  return Text.of(text.split("\n"));
}

function applyChanges(text: string, changes: ChangeSet): string {
  return changes.apply(asText(text)).toString();
}

function buildTransactions(base: string, specs: Spec[][]): ChangeSet[] {
  const transactions: ChangeSet[] = [];
  let text = base;
  for (const transaction of specs) {
    const changes = ChangeSet.of(
      transaction.map(([from, to, insert]) => ({ from, to, insert })),
      text.length,
    );
    transactions.push(changes);
    text = applyChanges(text, changes);
  }
  return transactions;
}

function asTuples(edits: readonly PageEdit[]): Spec[] {
  return edits.map((edit) => [
    edit.start_utf16,
    edit.end_utf16,
    edit.inserted_text,
  ]);
}

function applyEdits(text: string, edits: readonly PageEdit[]): string {
  let current = text;
  for (const edit of edits) {
    current =
      current.slice(0, edit.start_utf16) +
      edit.inserted_text +
      current.slice(edit.end_utf16);
  }
  return current;
}

function expectCode(run: () => void, code: string): void {
  assert.throws(run, (error: unknown) => {
    assert.ok(error instanceof AdapterError);
    assert.equal(error.code, code);
    return true;
  });
}

test("every fixture matches CodeMirror, literal edits, and a plain replay", () => {
  for (const item of fixture.cases) {
    const transactions = buildTransactions(item.base, item.transactions);
    const edits = toSequentialEditsFromTransactions(item.base, transactions);

    assert.deepEqual(asTuples(edits), item.expected_edits, item.id);

    const applied = transactions.reduce(
      (text, changes) => applyChanges(text, changes),
      item.base,
    );
    assert.equal(applied, item.expected_body, `${item.id} oracle`);
    assert.equal(applyEdits(item.base, edits), item.expected_body, item.id);
  }
});

test("a single transaction translates with the removed length, not toB", () => {
  const base = "abc";
  const changes = ChangeSet.of({ from: 1, to: 1, insert: "X" }, base.length);
  assert.deepEqual(asTuples(toSequentialEdits(changes, base)), [[1, 1, "X"]]);

  const deletion = ChangeSet.of({ from: 0, to: 1, insert: "" }, base.length);
  assert.deepEqual(asTuples(toSequentialEdits(deletion, base)), [[0, 1, ""]]);
});

test("the rejected toB translation is wrong for R1, R2, R3, and R4", () => {
  const cases = fixture.cases.filter((item) =>
    ["R1_pure_insert", "R2_pure_delete", "R3_unequal_replacement", "R4_insert_then_delete_shifted"].includes(
      item.id,
    ),
  );
  assert.equal(cases.length, 4);

  for (const item of cases) {
    const transactions = buildTransactions(item.base, item.transactions);
    const changes = composeTransactions(transactions, item.base.length);
    const naive: PageEdit[] = [];
    changes.iterChanges((fromA, toA, fromB, toB, inserted) => {
      naive.push({
        start_utf16: fromB,
        end_utf16: toB,
        inserted_text: inserted.toString(),
      });
    });
    assert.notDeepEqual(asTuples(naive), item.expected_edits, item.id);
    assert.notEqual(applyEdits(item.base, naive), item.expected_body, item.id);
  }
});

test("applying fromA in ascending order corrupts shifted text", () => {
  const base = "Elara left";
  const changes = ChangeSet.of(
    [
      { from: 0, to: 0, insert: "Dear " },
      { from: 5, to: 10, insert: "" },
    ],
    base.length,
  );
  const naive: PageEdit[] = [];
  changes.iterChanges((fromA, toA, fromB, toB, inserted) => {
    naive.push({
      start_utf16: fromA,
      end_utf16: toA,
      inserted_text: inserted.toString(),
    });
  });
  assert.notEqual(applyEdits(base, naive), "Dear Elara");
});

test("composition collapses the history that still removes a mention", () => {
  const item = fixture.cases.find(
    (candidate) => candidate.id === "R8_history_insert_then_delete",
  );
  assert.ok(item);

  const transactions = buildTransactions(item.base, item.transactions);
  const ledger = toSequentialEditsFromTransactions(item.base, transactions);
  assert.equal(ledger.length, 2);

  const net = composeTransactions(transactions, item.base.length);
  const netEdits = toSequentialEdits(net, item.base);
  assert.deepEqual(netEdits, []);
  assert.equal(applyChanges(item.base, net), item.base);
  assert.deepEqual(item.expected_mentions, []);
});

test("a same-text replacement is still a save, not a clean document", () => {
  const item = fixture.cases.find(
    (candidate) => candidate.id === "R9_same_text_replacement",
  );
  assert.ok(item);

  const transactions = buildTransactions(item.base, item.transactions);
  const net = composeTransactions(transactions, item.base.length);
  assert.equal(applyChanges(item.base, net), item.base);
  assert.equal(net.empty, false);
  const netText = applyEdits(item.base, toSequentialEdits(net, item.base));
  assert.equal(netText, item.expected_body);
  assert.deepEqual(item.expected_mentions, []);
});

test("CR, NUL, and unpaired surrogates are refused", () => {
  expectCode(() => assertEditableText("a\rb"), "carriage_return");
  expectCode(() => assertEditableText("a\r\nb"), "carriage_return");
  expectCode(() => assertEditableText("a\u0000b"), "nul");
  expectCode(() => assertEditableText("a\uD800b"), "unpaired_surrogate");
  expectCode(() => assertEditableText("a\uDC00b"), "unpaired_surrogate");
  expectCode(() => assertEditableText("\uD800\uD800"), "unpaired_surrogate");
  assertEditableText("emoji 📚 and combining e\u0301\nnext");
});

test("an edit boundary inside a surrogate pair is refused", () => {
  const base = "A📚B";
  assert.equal(base.length, 4);
  const changes = ChangeSet.of({ from: 2, to: 2, insert: "x" }, base.length);
  expectCode(() => toSequentialEdits(changes, base), "split_scalar");
});

test("inserted text is checked before it reaches Rust", () => {
  const base = "abc";

  // CodeMirror itself normalizes CR and CRLF to LF when a change is created,
  // so no editor transaction can smuggle one through. A body that already
  // contains a CR is still refused, and NUL or lone surrogates are refused
  // wherever they appear.
  for (const inserted of ["\r", "\r\n", "a\rb"]) {
    const changes = ChangeSet.of({ from: 1, to: 1, insert: inserted }, 3);
    assert.deepEqual(asTuples(toSequentialEdits(changes, base)), [
      [1, 1, inserted.replace(/\r\n?/g, "\n")],
    ]);
  }
  expectCode(
    () => toSequentialEdits(ChangeSet.empty(3), "a\rb"),
    "carriage_return",
  );

  const surrogate = ChangeSet.of({ from: 1, to: 1, insert: "\uD800" }, 3);
  expectCode(() => toSequentialEdits(surrogate, base), "unpaired_surrogate");

  const nul = ChangeSet.of({ from: 1, to: 1, insert: "\u0000" }, 3);
  expectCode(() => toSequentialEdits(nul, base), "nul");
});

test("length mismatch and unsafe coordinates are refused", () => {
  const changes = ChangeSet.of({ from: 0, to: 0, insert: "x" }, 2);
  expectCode(() => toSequentialEdits(changes, "abc"), "length_mismatch");
  expectCode(() => assertU32Coordinate(2 ** 32, "offset"), "coordinate_overflow");
  expectCode(() => assertU32Coordinate(-1, "offset"), "coordinate_overflow");
  expectCode(() => assertU32Coordinate(1.5, "offset"), "coordinate_overflow");
  expectCode(
    () => assertU32Coordinate(Number.MAX_SAFE_INTEGER + 2, "offset"),
    "coordinate_overflow",
  );
});

test("document and intermediate lengths are guarded numerically", () => {
  assert.equal(nextDocumentLength(5, 2, 3), 6);
  assert.equal(nextDocumentLength(MAX_UTF16, 0, 0), MAX_UTF16);
  assert.equal(nextDocumentLength(MAX_UTF16, 1, 1), MAX_UTF16);
  assert.equal(nextDocumentLength(0, 0, MAX_UTF16), MAX_UTF16);
  assertDocumentLength(MAX_UTF16, "result length");

  expectCode(() => assertDocumentLength(MAX_UTF16 + 1, "result length"), "coordinate_overflow");
  expectCode(() => nextDocumentLength(MAX_UTF16, 0, 1), "coordinate_overflow");
  expectCode(() => nextDocumentLength(MAX_UTF16 - 1, 0, 2), "coordinate_overflow");
  expectCode(() => nextDocumentLength(3, 4, 0), "inconsistent_changeset");
  expectCode(() => nextDocumentLength(3, -1, 0), "inconsistent_changeset");
  expectCode(() => nextDocumentLength(3, 0, 1.5), "inconsistent_changeset");
});

test("a ChangeSet whose result exceeds u32 is refused before allocation", () => {
  const huge = ChangeSet.of(
    { from: MAX_UTF16, to: MAX_UTF16, insert: "x" },
    MAX_UTF16,
  );
  assert.equal(huge.newLength, MAX_UTF16 + 1);
  expectCode(() => toSequentialEdits(huge, "abc"), "coordinate_overflow");
});

test("an empty document accepts an insertion", () => {
  const changes = ChangeSet.of({ from: 0, to: 0, insert: "x" }, 0);
  assert.deepEqual(asTuples(toSequentialEdits(changes, "")), [[0, 0, "x"]]);
});

test("PA-I3 opens linked and unsupported documents read-only", () => {
  assert.deepEqual(decideEditing("plain prose", 0), { editable: true });
  assert.deepEqual(decideEditing("linked prose", 1), {
    editable: false,
    reason: "linked",
  });
  assert.deepEqual(decideEditing("a\r\nb", 0), {
    editable: false,
    reason: "carriage_return",
  });
  assert.deepEqual(decideEditing("a\u0000b", 0), {
    editable: false,
    reason: "nul",
  });
  assert.deepEqual(decideEditing("a\uD800b", 0), {
    editable: false,
    reason: "unpaired_surrogate",
  });
});
