import { readFileSync } from "node:fs";

import { ChangeSet } from "@codemirror/state";

import type { Mention } from "../src/mentions.ts";

export type Spec = [number, number, string];
export type FixtureMention = { id: string; entity_id: string; start_utf16: number; end_utf16: number };
export type FixtureStep = { text: Spec[] } | { link: FixtureMention } | { unlink: string };
export type FixtureOp = { replace: Spec } | { link: FixtureMention } | { unlink: string };
export type FixtureCase = {
  id: string;
  base: string;
  mentions: FixtureMention[];
  transactions?: Spec[][];
  steps?: FixtureStep[];
  expected_body: string;
  expected_mentions: FixtureMention[];
  expected_edits?: Spec[];
  expected_ops?: FixtureOp[];
  expected_dropped?: string[];
  expected_changed: boolean;
  expected_revision: number;
};

export const read = (name: string) =>
  JSON.parse(readFileSync(new URL(`../../tests/fixtures/${name}`, import.meta.url), "utf8"));
export const fixture = read("bridge_cases.json") as { cases: FixtureCase[] };

export const asMention = (held: FixtureMention): Mention => ({
  id: held.id,
  entityId: held.entity_id,
  startUtf16: held.start_utf16,
  endUtf16: held.end_utf16,
});

export function steps(item: FixtureCase): FixtureStep[] {
  return item.steps ?? (item.transactions ?? []).map((text) => ({ text }));
}

export function changesFor(doc: string, specs: Spec[]): ChangeSet {
  return ChangeSet.of(specs.map(([from, to, insert]) => ({ from, to, insert })), doc.length);
}
