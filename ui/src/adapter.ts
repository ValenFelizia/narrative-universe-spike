import { ChangeSet, Text } from "@codemirror/state";

/// The largest offset the Rust reducer can hold. `Mention` and
/// `MentionEdit` carry `u32` UTF-16 positions.
export const MAX_UTF16 = 0xffff_ffff;

/// One Rust `MentionEdit::ReplaceText` in the coordinates of the body that
/// the previous edit produced.
export type PageEdit = {
  start_utf16: number;
  end_utf16: number;
  inserted_text: string;
};

export type AdapterErrorCode =
  | "carriage_return"
  | "nul"
  | "unpaired_surrogate"
  | "split_scalar"
  | "coordinate_overflow"
  | "length_mismatch"
  | "inconsistent_changeset";

export class AdapterError extends Error {
  readonly code: AdapterErrorCode;

  constructor(code: AdapterErrorCode, message: string) {
    super(message);
    this.name = "AdapterError";
    this.code = code;
  }
}

/// PA-I3 accepts only LF text that is already valid Unicode. Anything else
/// must be refused before an editor normalizes it.
export function assertEditableText(text: string): void {
  if (text.includes("\r")) {
    throw new AdapterError(
      "carriage_return",
      "carriage returns are not editable in PA-I3",
    );
  }
  if (text.includes("\u0000")) {
    throw new AdapterError("nul", "NUL is not editable in PA-I3");
  }
  for (let index = 0; index < text.length; index += 1) {
    const unit = text.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = text.charCodeAt(index + 1);
      if (Number.isNaN(next) || next < 0xdc00 || next > 0xdfff) {
        throw new AdapterError(
          "unpaired_surrogate",
          `unpaired high surrogate at ${index}`,
        );
      }
      index += 1;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      throw new AdapterError(
        "unpaired_surrogate",
        `unpaired low surrogate at ${index}`,
      );
    }
  }
}

export function assertU32Coordinate(value: number, label: string): void {
  if (!Number.isSafeInteger(value) || value < 0 || value > MAX_UTF16) {
    throw new AdapterError(
      "coordinate_overflow",
      `${label} ${value} does not fit a u32 UTF-16 offset`,
    );
  }
}

function isScalarBoundary(text: string, position: number): boolean {
  if (position <= 0 || position >= text.length) {
    return true;
  }
  const before = text.charCodeAt(position - 1);
  const at = text.charCodeAt(position);
  const pair =
    before >= 0xd800 && before <= 0xdbff && at >= 0xdc00 && at <= 0xdfff;
  return !pair;
}

/// Translates one CodeMirror transaction into ordered Rust edits.
///
/// `[fromB, toB)` is the inserted range in the new document. The removed
/// range starts at `fromB` in the body Rust currently holds and is
/// `toA - fromA` units long. It is not `toB`.
export function toSequentialEdits(
  changes: ChangeSet,
  baseText: string,
): PageEdit[] {
  assertEditableText(baseText);
  assertU32Coordinate(baseText.length, "base length");
  if (changes.length !== baseText.length) {
    throw new AdapterError(
      "length_mismatch",
      `changes describe ${changes.length} units but the body has ${baseText.length}`,
    );
  }

  const edits: PageEdit[] = [];
  let current = baseText;

  changes.iterChanges((fromA, toA, fromB, toB, inserted) => {
    const insertedText = inserted.toString();
    if (toB - fromB !== insertedText.length) {
      throw new AdapterError(
        "inconsistent_changeset",
        `inserted range ${fromB}..${toB} does not match ${insertedText.length} units`,
      );
    }
    assertEditableText(insertedText);
    const removed = toA - fromA;
    const end = fromB + removed;
    assertU32Coordinate(fromB, "edit start");
    assertU32Coordinate(end, "edit end");
    if (!isScalarBoundary(current, fromB)) {
      throw new AdapterError(
        "split_scalar",
        `edit starts inside a surrogate pair at ${fromB}`,
      );
    }
    if (!isScalarBoundary(current, end)) {
      throw new AdapterError(
        "split_scalar",
        `edit ends inside a surrogate pair at ${end}`,
      );
    }
    edits.push({
      start_utf16: fromB,
      end_utf16: end,
      inserted_text: insertedText,
    });
    current =
      current.slice(0, fromB) + insertedText + current.slice(end);
  });

  const oracle = changes.apply(Text.of(baseText.split("\n"))).toString();
  if (current !== oracle) {
    throw new AdapterError(
      "inconsistent_changeset",
      "sequential edits do not reproduce the CodeMirror document",
    );
  }
  return edits;
}

/// Applies real CodeMirror transactions in order. Each transaction starts
/// from the text the previous one produced, so the ordered ledger is
/// preserved instead of being compressed into one net diff.
export function toSequentialEditsFromTransactions(
  baseText: string,
  transactions: readonly ChangeSet[],
): PageEdit[] {
  const edits: PageEdit[] = [];
  let text = baseText;
  for (const transaction of transactions) {
    edits.push(...toSequentialEdits(transaction, text));
    text = transaction.apply(Text.of(text.split("\n"))).toString();
  }
  return edits;
}

export function composeTransactions(
  transactions: readonly ChangeSet[],
  baseLength: number,
): ChangeSet {
  let composed = ChangeSet.empty(baseLength);
  for (const transaction of transactions) {
    composed = composed.compose(transaction);
  }
  return composed;
}

export type EditDecision =
  | { editable: true }
  | {
      editable: false;
      reason:
        | "carriage_return"
        | "nul"
        | "unpaired_surrogate"
        | "linked";
    };

/// PA-I3 policy. A page with any stored occurrence opens read-only, because
/// the reducer is history-sensitive and PA-I4 owns link affordances.
export function decideEditing(
  text: string,
  mentionCount: number,
): EditDecision {
  try {
    assertEditableText(text);
  } catch (error) {
    if (error instanceof AdapterError) {
      const code = error.code;
      if (
        code === "carriage_return" ||
        code === "nul" ||
        code === "unpaired_surrogate"
      ) {
        return { editable: false, reason: code };
      }
    }
    throw error;
  }
  if (mentionCount > 0) {
    return { editable: false, reason: "linked" };
  }
  return { editable: true };
}
