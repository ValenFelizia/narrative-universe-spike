# Specifications

<!--
Record only evidence-backed durable truth. Code and tests may describe behavior
but do not independently establish durable intent. Do not invent requirements,
constraints, invariants, or stable interfaces. Sparse or empty sections are
valid. Add, remove, or rename domain-specific sections when appropriate.
-->

Sources for this initialization: root `README.md` and `CONTRIBUTING.md` on
`main` at `7f0d6d3`. Current behavior below is the documented I1 spike, not a
commitment that the product remains headless.

## Project Summary

Narrative Universe is an experimental, local-first foundation for writing and
organizing fictional worlds. It aims to combine freeform pages with structured
entities, user-defined properties, and assertions that can preserve competing
hypotheses without silently deciding what is canon.

The Rust/SQLite foundation now has a Tauri 2 desktop editorial workspace for
synthetic personal-alpha testing. Accepted Write and Connect contracts below
are grounded in #31's approved decisions and merged PR #34 at `dceda4d`.
This does not establish production backup or crash recovery acceptance.

## Requirements

I1, as documented:

- Pages have freeform text. The model includes entity types and entities with
  an optional type and primary page.
- Property definitions are owned by a type or by one entity.
- Assertions contain text, boolean values, entity references, or an explicit
  unknown value.
- Canon states are `ACORDADO`, `PROVISIONAL`, `ABIERTO`, and
  `MISTERIO_INTENCIONAL`.
- Records carry stable IDs, provenance fields, narrative scope notes, and
  archived flags.
- Storage is SQLite, with validation and bidirectional relationship queries
  backed by one stored assertion.
- An existing universe can be reopened from a different process.
- Prose belongs to pages; structured facts belong to assertions. Writing a
  page does not automatically create or canonize facts.
- Canon can be changed on one assertion without changing any other assertion.
- Archive and restore are repeatable. They do not delete values, provenance,
  or canon.
- A new assertion on an archived definition is rejected.

## Constraints

- Do not store irreplaceable writing in this experimental format.
- Tauri 2 and the manual-save editorial workspace are the synthetic desktop
  alpha implementation. MCP, a fictional timeline engine, and production
  recovery are outside the accepted Write/Connect scope.
- No license is selected. Public repository visibility is not a license grant.
  External contributors should discuss licensing before a substantial patch.
- The crate is `narrative-universe` `0.1.0`, edition 2021, `publish = false`,
  with Rust `1.96` / toolchain channel `1.96.0` and target
  `x86_64-pc-windows-msvc`. Follow `rust-toolchain.toml` and `Cargo.lock`.
- Windows/MSVC is the first tested environment. Other platforms are not
  verified. Bundled SQLite needs the MSVC C/C++ build tools.
- Start from an approved, scoped issue. Do not treat the next planned
  increment as automatically approved, and do not merge your own work unless
  maintainers explicitly authorize it.
- Documentation, code comments, contribution guidance, and pull-request
  descriptions are written in English. That does not set the language of a
  future user interface or of fictional source material.
- AI agents may plan, implement, or review only within an explicitly
  authorized scope, and must stop at the agreed gate.
- Do not publish private Narrative Bible excerpts, project files, credentials,
  or real-universe content. Public examples are synthetic or explicitly
  cleared. The checked-in fixture is synthetic, not War Archivist canon.
- There is no workspace-level multiwriter coordination or supported backup
  workflow.

## Invariants

- A missing assertion, explicit `Unknown`, and boolean `false` are different.
- A singular property can still contain multiple competing hypotheses.
- Canon status and archived status are independent.
- A relationship is stored once and can be queried from both directions.
- IDs and provenance must survive reads and, once implemented, export/import.
- Narrative source material is evidence, not permission to invent canon.
- `Store::create` reserves a new file path exclusively and does not overwrite
  an existing file.
- If initialization fails after that reservation, it leaves a residual file
  and reports its location instead of deleting another process's database.
- A second `seed` against an existing path fails without overwriting it.
- A new assertion cannot be created against an archived definition.
- Promoting one assertion does not change the id, value, provenance, scope,
  or canon of another assertion.
- Canon literals (`ACORDADO`, `PROVISIONAL`, `ABIERTO`,
  `MISTERIO_INTENCIONAL`), fixture text such as `al inicio`, fixed
  identifiers, and source quotations stay in their original form.

## Interfaces and Contracts

- CLI: `cargo run --locked --bin spike -- seed <path>` and
  `inspect <path>`. `seed` requires a path that does not exist.
- Separate `seed` and `inspect` processes print the same fixture report for
  the demonstration universe: one page, two types, two entities, one
  definition, and one assertion. The same assertion ID appears from its
  subject and from its target.
- Checked-in layout: `src/model.rs` (typed IDs, records, canon, values,
  property ownership), `src/store.rs` (synchronous domain checks,
  transactions, SQLite), `src/schema.sql` (tables, foreign keys,
  constraints), `src/bin/spike.rs` (`seed` / `inspect`), and `tests/`
  (real-file tests, process replay, validation, concurrency).
- Review checks: `cargo fmt --check`,
  `cargo clippy --locked --all-targets -- -D warnings`, and
  `cargo test --locked --all-targets`. Persistence changes need tests against
  real SQLite files, including close/reopen where relevant.

## Accepted Write and Connect contracts

Sources: #31 approval comment `6592317042`, merged PR #34, and authorized #33.

- Manual Save treats title, exact prose, and mentions as one revision-checked
  aggregate. Link and Unlink belong to the same draft and dirty-leave guard.
- The Rust reducer is canonical. The client predicts ordered edits, then the
  bridge checks expected body and mention set before writing. A mismatch
  writes nothing. Acknowledgements must match request, session, page, revision,
  body, title, and mentions before the UI may report Saved.
- The existing read-only entity index supplies stable IDs, labels, type labels,
  and primary-page titles. Linking is intentional and never creates entities,
  assertions, provenance, or canon. No schema change is required.
- D1 allows ordinary text edits to invalidate a link, with an immediate,
  persistent nonmodal notice until confirmed Save, restoration, or deliberate
  resolution. Deliberate Unlink is distinct from text-driven invalidation.
- D2 requires text, Link, and Unlink history. Undo restores the original mention
  ID, entity, and range. IDs cannot be rebound within the active document and
  history. No global lifetime claim is made after persisted unlink and reopen.
- D3 requires Ctrl+K and a discreet near-selection Link entity action using the
  same anchored picker. Escape returns focus and the captured selection.
  As refined in #35 acceptance comment `6666149588`, a caret inside a mention
  or one selection exactly matching it inspects that same link. The contextual
  action says Inspect link; partial, containing, and multiple selections refuse.
- D4 requires label, type, and primary-page title, plus a short ID suffix for
  otherwise exact ties. No candidate is selected automatically by matching text.
- A valid selection is one nonempty range, without line breaks or overlap,
  aligned to graphemes. Document, session, universe, and selection changes cancel
  a pending picker. Read-only and unresolved save states refuse new linking.
- Linked prose remains writable through the ordered mention-aware transaction
  path. CR, NUL, invalid scalars, and exhausted revisions retain their locks.
- D5 uses `tests/fixtures/connect-smoke.json` with `spike import-new`; the process
  replay seed remains unchanged. D6 retains structured drafts in memory on stale,
  Busy, uncertain, and mismatch outcomes. Copying text excludes links. Only a
  confirmed Discard may abandon the draft. Durable structured recovery and rebase
  are deferred for synthetic use only.
- #33's editor integration has automated UI regressions. Native evidence and
  remaining human acceptance are recorded in `docs/connect-acceptance.md`.
  Independent review and Valen/Ari's Windows acceptance are still merge gates.
