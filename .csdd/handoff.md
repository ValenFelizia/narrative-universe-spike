# Handoff

<!--
Create an entry only when both a real boundary and concrete resumption risk
exist. Include only non-obvious state needed to resume safely. Keep one
replaceable current snapshot per task or workstream; remove or replace it when
the risk is consumed. Do not duplicate the GitHub tracker or accumulate history.
-->

## I3 pending G3

Issue #4 is the authorized I3 increment. Do not merge it and do not start
Tauri, UI, MCP, or another increment. I1 and I2 are the reviewed baseline.
Do not copy the unreviewed JSON contract into `.csdd/specs.md` or
`.csdd/decisions.md`. G3 review asked for three boundary fixes before merge:
a successful export must fit the 10 MiB import cap, CLI reads are bounded to
that cap plus one byte, and a failed export keeps its residual file instead
of deleting the path. Those fixes are on `feat/4-json-exchange` and still
need re-review. Schema version 1 is unchanged.

## PA-I2-A pending review

Issue #12 is the authorized PA-I2-A increment. The branch
`feat/12-mention-edits` adds the pure mention contract in `src/mentions.rs`,
with `MentionId`, `Mention`, `AnnotatedBody`, `MentionEdit`,
`validate_mentions`, and `apply_mention_edit`, plus tests in
`tests/mentions.rs`. Nothing is persisted. No mention, page, entity, or canon
is written anywhere, and `src/store.rs`, the schema, and the JSON exchange are
untouched. Do not merge it, do not add persistence, and do not start PA-I2-B.
Do not copy this unreviewed behavior into `.csdd/specs.md` or
`.csdd/decisions.md` before the gate passes.
