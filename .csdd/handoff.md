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

## PA-I2-B merged; PA-I2-C C1 checkpoint pending review

PA-I2-B is merged into `main` as `714260d` (PR #15). The storage schema and
the JSON format are version 2; a version 1 database still fails closed with a
migration outcome until PA-I2-C publishes the conversion.

Issue #16 authorizes PA-I2-C. C1 is complete on
`feat/16-safe-v1-v2-migration`: a crate-private `src/migration.rs` recognizes
both historical version 1 schemas, holds one consistent read transaction
across extraction, candidate construction, and verification, builds a private
candidate through `Store::import_new`, and verifies it through a read-only
connection. Every migrated page is checked for revision 0 and zero
occurrences. Publication, recovery, hard links, and the migration CLI remain
C2 and are not implemented. Do not merge C1, do not start C2, and do not
delete a source or candidate on failure; residuals stay in place for review.
