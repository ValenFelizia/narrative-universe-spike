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

The repository is currently a headless Rust/SQLite spike, not a usable writing
application. **I1 — persistence and relationships** is the only implemented
increment. I2 and I3 are planned and require separate approval.

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

## Constraints

- Do not store irreplaceable writing in this experimental format.
- Not implemented: editing UI, Tauri, MCP, JSON import/export, canon
  transitions, archive/restore commands, full history, and a fictional
  timeline engine.
- This spike does not commit the product to Tauri. The implementation
  language and future desktop shell are still being validated.
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
  workflow. JSON portability and safe restoration are later increments, not I1.

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
