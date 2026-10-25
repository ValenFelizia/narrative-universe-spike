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

## PA-I2-B merged; PA-I2-C C2 pending delta review

PA-I2-B is merged into `main` as `714260d` (PR #15). The storage schema and
the JSON format are version 2.

Issue #16 authorizes PA-I2-C. C1 and C2 are implemented on
`feat/16-safe-v1-v2-migration`. `migrate_v1_to_v2` holds one pinned source
read transaction and its captured v1 snapshot through candidate build,
candidate verification, publication, and post-publication destination
verification. The candidate is a dot-prefixed private file in the destination
directory (`0600` on Unix, inherited ACL on Windows; the leading dot is not a
hidden attribute on Windows), is flushed and closed before verification, and
is published with exactly one atomic no-overwrite hard link; there is no
rename or copy fallback. The source is never written. The published
destination is fully re-verified while the source is still pinned. After the
source is released, the alias is fully verified once more, then the
destination's type, sidecars, header, and length are observed as the last check
before the unlink; only then is the alias removed. General `verify_v2` /
`spike verify` is read-only and accepts edited v2 databases with nonzero
revisions and valid occurrences while enforcing the store's cross-record
domain rules (assertion applicability, value-class agreement, known reference
targets); the migration verifier additionally requires exact snapshot
equality, zero revisions, zero occurrences, and exact table counts.

Ari's delta review requested two corrections, both now implemented with
adversarial regressions. General verification runs `validate_logical_snapshot`
on the typed v2 reading, so valid SQL with an inapplicable assertion or a
definition/value-class mismatch is rejected without mutation. Candidate
cleanup no longer treats content equality as proof of ownership: the file
identity reserved at candidate creation is compared immediately before an
unlink, and a mismatch or an unprovable identity is kept and reported. Identity
is Unix device/inode, or on Windows the volume serial number plus the full
128-bit `FILE_ID_INFO` file ID, because that API's 64-bit file index is not
unique on ReFS; a volume that cannot report the 128-bit identity fails closed.
Identity comparison and unlink are separate operations, so a same-user
replacement between them remains a documented race limit rather than a
universal guarantee.

A second Astra audit at `ccb6b72` found two more blockers, both reproduced on
Windows/NTFS and in the real CLI before the fix. Destination preflight compared
paths with ASCII folding, so `É.sqlite-journal` (and `-wal`, `-shm`) named the
sidecar of `é.sqlite` on default NTFS and published into the source directory;
Windows now compares with `CompareStringOrdinal` plus a Unicode uppercase
guard, and a comparison the platform cannot answer refuses the destination
before any candidate exists. General verification also echoed an invalid stored
`UniverseId` (and mention ids) from the database into the public error and
`verify` stderr; stored values are no longer interpolated into diagnostics
while rejection and read-only behavior are unchanged. The review pass found one
sibling path in stored definition ids (`list_definitions` / `lookup_definition`
formatted a not-yet-validated id); that is closed too, with API and CLI
regressions. macOS keeps its previous ASCII-only comparison and claims no
HFS+/APFS normalization equivalence.

Failures report a stage, a cause, and every residual path. Before publication,
cleanup removes the candidate only when it still verifies against the pinned
snapshot and still carries this run's identity; a changed, replaced, or
unprovable path is kept and reported. After publication, cleanup never removes
the destination or the alias, because either may be the only complete copy. A
process crash can leave a dot-prefixed candidate or a complete published
destination; no run resumes automatically, adopts, relinks, or deletes an
unknown file, and there is no `--force`. Windows power-loss durability of the
directory entry is not claimed. The debug-only `NARRATIVE_UNIVERSE_CRASH_AT`
hook covers `candidate-built`, `candidate-verified`, `published`,
`destination-verified`, `destination-rechecked`, `alias-verified`, and
`cleaned-up`.

Do not merge PA-I2-C and do not start another increment. Report residual paths
for review; do not delete a source, a changed candidate, or a published
destination by hand until the review says otherwise.
