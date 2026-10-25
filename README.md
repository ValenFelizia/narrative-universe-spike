# Narrative Universe

An experimental, local-first foundation for writing and organizing fictional worlds.

Narrative Universe aims to combine freeform pages with structured entities, user-defined properties, and assertions that can preserve competing hypotheses without silently deciding what is canon. **This repository is currently a headless Rust/SQLite spike, not a usable writing application.**

## Current scope

**I1 — Persistence and relationships** and **I2 — Canon alternatives and reversible archival** are the reviewed baseline. **I3 — Versioned JSON exchange** is implemented on this branch and is waiting for G3 review. **PA-I2-A — pure mention editing semantics** is implemented on this branch and is waiting for review. **PA-I2-B — page occurrences with a local revision and the JSON format 2 exchange** is merged into the reviewed baseline as PR #15 (`714260d`). An occurrence is stored with its page, the JSON exchange carries it, and a version 1 JSON document still imports as a universe without occurrences. **PA-I2-C — safe version 1 to version 2 migration** is implemented on this branch and is waiting for review. C2 publishes a verified v2 candidate with one atomic no-overwrite hard link, keeps the source read transaction pinned through post-publication verification, and adds `spike migrate` and `spike verify`. The source is never modified.

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

A version 1 database is migrated to a **new** destination path; neither command overwrites an existing file:

```sh
cargo run --locked --bin spike -- migrate ./legacy-universe.sqlite ./migrated-universe.sqlite
cargo run --locked --bin spike -- verify ./migrated-universe.sqlite
```

`migrate` prints the universe id, counts, destination, and any residual path on success; on failure it prints the stage, both paths, the cause, and every residual path. `verify` is a read-only general check: it accepts an edited v2 database with nonzero page revisions and valid occurrences, and it enforces the same cross-record domain rules as the store, including assertion applicability, definition value-class agreement, and reference targets. It does not upgrade or create anything.

The fixture is synthetic, not War Archivist canon. See [fixture notes](tests/fixtures/README.md). The Spanish text inside the fixture and the canon values are intentional test data, **not documentation left untranslated**.

## Code map

| Path | Purpose |
| --- | --- |
| `src/model.rs` | Typed IDs, record types, canon, values, and property ownership |
| `src/store.rs` | Synchronous domain checks, transactions, SQLite reads and writes |
| `src/exchange.rs` | Versioned JSON parse, encode, and the 10 MiB input cap |
| `src/mentions.rs` | Pure mention identity and single-edit semantics |
| `src/migration.rs` | Version 1 snapshot reading, v2 verification, and no-overwrite publication |
| `src/schema.sql` | Persistent tables, foreign keys, and constraints |
| `src/bin/spike.rs` | Minimal `seed` / `inspect` / exchange / `migrate` / `verify` demonstration |
| `tests/` | Real-file tests, process replay, validation, and concurrency regressions |

Design principle: **prose belongs to pages; structured facts belong to assertions**. Writing a page does not automatically create or canonize facts.

## Data safety and limitations

This is an experimental format intended for disposable test universes. Do **not** store irreplaceable writing in it yet.

- `Store::create` reserves a new file path exclusively. It does not overwrite an existing file.
- If initialization fails after that reservation, it leaves a **residual file** and reports its location instead of risking deletion of another process's database.
- A successful JSON export is compact UTF-8 and must fit the same 10 MiB cap as import. An oversized universe fails export before the destination file is created.
- A failed export write keeps the reserved file and reports `residual file` plus the path. It does not delete that path.
- An import refuses a document that repeats a JSON key, because a repeated key would otherwise hide one of its values.
- An import refuses a positional JSON array where the format defines a record, so a page, an occurrence or a nested record is never read by position.
- An import into a new file checks the occurrence geometry of the document before it reserves the destination, so a malformed document leaves no file behind.
- An export refuses a page whose stored revision is not a checked nonnegative integer, so a corrupt row cannot leave the database as part of a healthy universe.
- An import refuses to reconcile a page whose stored revision is not a checked nonnegative integer, and rolls the whole batch back.
- Creating a page refuses an id whose occurrence rows still exist without their page row, which only a deletion outside SQLite with foreign keys disabled can produce. Those rows are never adopted, relinked or deleted.
- A migration refuses an existing destination, a symlink, and any destination sidecar. Publication is one atomic no-overwrite hard link inside the destination directory; there is no copy or rename fallback, and an existing file is never replaced.
- The version 1 source is opened read-only and its read transaction stays pinned through candidate construction, candidate verification, publication, and post-publication destination verification. Boundary rechecks compare the regular-file type, sidecar absence, SQLite header, and length; a same-size out-of-band edit that preserves those fields is a documented path-race limit, not a byte-for-byte guarantee.
- A migration candidate is a dot-prefixed private file in the destination directory (mode `0600` on Unix; on Windows it inherits the directory's ACL, and the leading dot is not a hidden attribute there). It is flushed and closed before verification and is never written through the alias again.
- The published destination is fully re-verified while the source is still pinned. After the source is released, the alias is fully verified once more, then the destination's type, sidecars, header, and length are observed as the last check before the unlink. The alias is removed only when it both matched the migrated database immediately before that observation and still carried the file identity reserved by this run; a changed, replaced (even byte-identical), or unremovable alias is kept and reported. Identity is the Unix device and inode, or on Windows the volume serial number and the full 128-bit file ID rather than the 64-bit index that is not unique on ReFS; a volume that cannot report that identity yields no proof, so the path is kept. Identity is compared immediately before the unlink; the comparison and the unlink are separate operations, so a same-user replacement between them remains a documented race limit.
- Before publication, a failed run removes the candidate only when it still verifies against the pinned snapshot **and** its file identity still matches this run's reservation; otherwise the path is kept and reported. After publication, cleanup never deletes the destination or the alias, because either may be the only complete copy; every surviving path is reported for inspection.
- General verification applies the store's cross-record domain rules to the typed snapshot: an assertion must be applicable to its subject, its value must fit the definition's value class, and every reference target must exist. A database can pass SQLite integrity and foreign-key checks while breaking those rules, so both checks are required.
- A process crash can leave such a candidate or a complete published destination. A later run never resumes automatically, adopts, rewrites, or deletes an unknown file; report it for manual inspection instead. Windows directory-entry durability after power loss is not claimed.
- Publication needs a filesystem with hard-link support. An unsupported filesystem fails closed instead of falling back to a copy.
- SQLite transactions protect individual writes, but there is no workspace-level multiwriter coordination or supported backup workflow yet.
- JSON portability and safe restoration are part of a later increment, not I1.

## Project workflow

Development proceeds in small, reviewable increments with explicit acceptance gates. Read [CONTRIBUTING.md](CONTRIBUTING.md) for development commands, documentation conventions, pull-request expectations, and agent safeguards.

The implementation language and future desktop shell are still being validated; this spike does not commit the product to Tauri.

**License:** not selected. Public repository visibility does not by itself grant a license to reuse or redistribute this code.
