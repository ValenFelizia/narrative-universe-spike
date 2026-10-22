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

## PA-I2-A merged, PA-I2-B checkpoint B1 pending review

PA-I2-A is the reviewed baseline. It added the pure mention contract in
`src/mentions.rs` with `MentionId`, `Mention`, `AnnotatedBody`, `MentionEdit`,
`validate_mentions`, and `apply_mention_edit`. That module is pure and stays
unchanged.

Issue #14 is the authorized PA-I2-B increment. The branch
`feat/14-page-mention-persistence` holds the complete candidate: checkpoint B2
adds the JSON format 2 exchange, so an export carries the occurrences and a
version 1 JSON document still imports as a universe without occurrences. The
storage schema is version 2 and the JSON format is version 2, and they are
separate concepts. Do not merge it, do not add a migration, and do not start
PA-I2-C. A version 1 database still fails closed with a migration outcome,
and that migration belongs to PA-I2-C. Do not copy this unreviewed behavior
into `.csdd/specs.md` or `.csdd/decisions.md` before the gate passes.
