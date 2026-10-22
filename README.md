# Narrative Universe

An experimental, local-first foundation for writing and organizing fictional worlds.

Narrative Universe aims to combine freeform pages with structured entities, user-defined properties, and assertions that can preserve competing hypotheses without silently deciding what is canon. **This repository is currently a headless Rust/SQLite spike, not a usable writing application.**

## Current scope

**I1 — Persistence and relationships** and **I2 — Canon alternatives and reversible archival** are the reviewed baseline. **I3 — Versioned JSON exchange** is implemented on this branch and is waiting for G3 review. **PA-I2-A — pure mention editing semantics** is implemented on this branch and is waiting for review. **PA-I2-B — page occurrences with a local revision and the JSON format 2 exchange** is implemented on this branch and is waiting for review. An occurrence is stored with its page, the JSON exchange carries it, and a version 1 JSON document still imports as a universe without occurrences.

The spike covers:

- Pages with freeform text, entity types, and entities with optional type and primary page.
- Property definitions owned by a type or one entity.
- Assertions containing text, boolean values, entity references, or an explicit unknown value.
- The four canon states: `ACORDADO`, `PROVISIONAL`, `ABIERTO`, and `MISTERIO_INTENCIONAL`.
- Stable IDs, provenance fields, narrative scope notes, and archived flags.
- SQLite storage, validation, and bidirectional relationship queries backed by **one stored assertion**.
- Reopening an existing universe from a different process.
- An explicit canon change for one assertion. Other assertions keep their ids, values, provenance, scope, and canon.
- Idempotent archive and restore for definitions and assertions. Archival does not delete values or provenance, and it does not change canon.
- Rejection of a new assertion on an archived definition.
- A read-only query for possible incompatibilities: two active `ACORDADO` assertions with different known values on the same subject and the same active singular definition.
- Versioned JSON export, import into the same universe, and import into a new file. A conflicting record rolls back the whole batch. Import can restore an assertion whose definition is already archived. Creating a new assertion on an archived definition still fails.

**Not implemented:** editing UI, Tauri, MCP, full history, a fictional timeline engine. I3 is not accepted until G3 review, and the mention contract is not accepted until its own review. A green test suite does not authorize merge.

## Getting started

The checked-in toolchain is Rust 1.96.0. On Windows, the bundled SQLite build also needs the MSVC C/C++ build tools; use a Visual Studio Developer PowerShell if `cl.exe` is not on your `PATH`.

From the repository root:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

To try the persistence demonstration, choose a **new path that does not exist**:

```sh
cargo run --locked --bin spike -- seed ./demo-universe.sqlite
cargo run --locked --bin spike -- inspect ./demo-universe.sqlite
cargo run --locked --bin spike -- export ./demo-universe.sqlite ./demo-universe.json
cargo run --locked --bin spike -- import-new ./demo-copy.sqlite ./demo-universe.json
```

The two commands run in separate processes and should print the same fixture report: one page, two types, two entities, one definition, and one assertion. The same assertion ID appears when navigating from its subject and from its target. A second `seed` against an existing path fails without overwriting it.

The fixture is synthetic, not War Archivist canon. See [fixture notes](tests/fixtures/README.md). The Spanish text inside the fixture and the canon values are intentional test data, **not documentation left untranslated**.

## Code map

| Path | Purpose |
| --- | --- |
| `src/model.rs` | Typed IDs, record types, canon, values, and property ownership |
| `src/store.rs` | Synchronous domain checks, transactions, SQLite reads and writes |
| `src/exchange.rs` | Versioned JSON parse, encode, and the 10 MiB input cap |
| `src/mentions.rs` | Pure mention identity and single-edit semantics |
| `src/schema.sql` | Persistent tables, foreign keys, and constraints |
| `src/bin/spike.rs` | Minimal `seed` / `inspect` demonstration |
| `tests/` | Real-file tests, process replay, validation, and concurrency regressions |

Design principle: **prose belongs to pages; structured facts belong to assertions**. Writing a page does not automatically create or canonize facts.

## Data safety and limitations

This is an experimental format intended for disposable test universes. Do **not** store irreplaceable writing in it yet.

- `Store::create` reserves a new file path exclusively. It does not overwrite an existing file.
- If initialization fails after that reservation, it leaves a **residual file** and reports its location instead of risking deletion of another process's database.
- A successful JSON export is compact UTF-8 and must fit the same 10 MiB cap as import. An oversized universe fails export before the destination file is created.
- A failed export write keeps the reserved file and reports `residual file` plus the path. It does not delete that path.
- SQLite transactions protect individual writes, but there is no workspace-level multiwriter coordination or supported backup workflow yet.
- JSON portability and safe restoration are part of a later increment, not I1.

## Project workflow

Development proceeds in small, reviewable increments with explicit acceptance gates. Read [CONTRIBUTING.md](CONTRIBUTING.md) for development commands, documentation conventions, pull-request expectations, and agent safeguards.

The implementation language and future desktop shell are still being validated; this spike does not commit the product to Tauri.

**License:** not selected. Public repository visibility does not by itself grant a license to reuse or redistribute this code.
