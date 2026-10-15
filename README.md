# Narrative Universe

An experimental, local-first foundation for writing and organizing fictional worlds.

Narrative Universe aims to combine freeform pages with structured entities, user-defined properties, and assertions that can preserve competing hypotheses without silently deciding what is canon. **This repository is currently a headless Rust/SQLite spike, not a usable writing application.**

## Current scope

**I1 — Persistence and relationships** is the only implemented increment:

- Pages with freeform text, entity types, and entities with optional type and primary page.
- Property definitions owned by a type or one entity.
- Assertions containing text, boolean values, entity references, or an explicit unknown value.
- The four canon states: `ACORDADO`, `PROVISIONAL`, `ABIERTO`, and `MISTERIO_INTENCIONAL`.
- Stable IDs, provenance fields, narrative scope notes, and archived flags.
- SQLite storage, validation, and bidirectional relationship queries backed by **one stored assertion**.
- Reopening an existing universe from a different process.

**Not implemented:** editing UI, Tauri, MCP, JSON import/export, canon transitions, archive/restore commands, full history, or a fictional timeline engine. I2 and I3 are planned but require separate approval.

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
```

The two commands run in separate processes and should print the same fixture report: one page, two types, two entities, one definition, and one assertion. The same assertion ID appears when navigating from its subject and from its target. A second `seed` against an existing path fails without overwriting it.

The fixture is synthetic, not War Archivist canon. See [fixture notes](tests/fixtures/README.md). The Spanish text inside the fixture and the canon values are intentional test data, **not documentation left untranslated**.

## Code map

| Path | Purpose |
| --- | --- |
| `src/model.rs` | Typed IDs, record types, canon, values, and property ownership |
| `src/store.rs` | Synchronous domain checks, transactions, SQLite reads and writes |
| `src/schema.sql` | Persistent tables, foreign keys, and constraints |
| `src/bin/spike.rs` | Minimal `seed` / `inspect` demonstration |
| `tests/` | Real-file tests, process replay, validation, and concurrency regressions |

Design principle: **prose belongs to pages; structured facts belong to assertions**. Writing a page does not automatically create or canonize facts.

## Data safety and limitations

This is an experimental format intended for disposable test universes. Do **not** store irreplaceable writing in it yet.

- `Store::create` reserves a new file path exclusively. It does not overwrite an existing file.
- If initialization fails after that reservation, it leaves a **residual file** and reports its location instead of risking deletion of another process's database.
- SQLite transactions protect individual writes, but there is no workspace-level multiwriter coordination or supported backup workflow yet.
- JSON portability and safe restoration are part of a later increment, not I1.

## Project workflow

Development proceeds in small, reviewable increments with explicit acceptance gates. Read [CONTRIBUTING.md](CONTRIBUTING.md) for development commands, documentation conventions, pull-request expectations, and agent safeguards.

The implementation language and future desktop shell are still being validated; this spike does not commit the product to Tauri.

**License:** not selected. Public repository visibility does not by itself grant a license to reuse or redistribute this code.
