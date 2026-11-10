# I1 synthetic fixture

This document describes the dataset created by `spike seed` and exercised by `tests/process_replay.rs`.

**All records are synthetic.** They are not statements of War Archivist canon. The narrative design review required an explicit editorial mapping and a precise source before recording the Narrator's house membership as `ACORDADO`. That mapping has not been approved for this fixture. Its one assertion therefore uses `PROVISIONAL`, the labels explicitly say the records are synthetic, and `source_status_raw` is absent.

The page is freeform and is not the primary page of either entity. The definition belongs to the Character (`Personaje`) type, takes an entity reference, and has singular cardinality. The reference from `Narrador sintético` to `Casa sintética` is stored as **one assertion**; looking it up from either end returns the same ID, with no inverse row.

The scope note is the literal text `al inicio`. It tests preservation of narrative scope text and does not establish any fact in the Narrative Bible.

## Fixed identifiers

| Record | ID |
| --- | --- |
| Universe | `11111111-1111-4111-8111-111111111111` |
| Page | `22222222-2222-4222-8222-222222222222` |
| Character type (`Personaje`) | `33333333-3333-4333-8333-333333333333` |
| House type (`Casa`) | `44444444-4444-4444-8444-444444444444` |
| Synthetic Narrator entity | `55555555-5555-4555-8555-555555555555` |
| Synthetic House entity | `66666666-6666-4666-8666-666666666666` |
| Synthetic membership definition | `77777777-7777-4777-8777-777777777777` |
| Assertion | `88888888-8888-4888-8888-888888888888` |

## Literal test content

The page title is `Página libre`.

The page body is `Texto con acento áéíóú ñ y emoji 📚`.

Entity labels are `Narrador sintético` and `Casa sintética`. The assertion is `PROVISIONAL`, and its `scope_note` is `al inicio`.

Expected record counts: **1 page, 2 types, 2 entities, 1 definition, 1 assertion**.

Keep these Spanish literals and the fixed identifiers unchanged unless the fixture contract and its tests are deliberately updated together.

## Version 3 interchange fixture

`reveal-facts.json` is a synthetic `format_version: 3` universe used by `tests/exchange_v3.rs`. It is deliberately independent of `spike seed` and of `connect-smoke.json`, which is left unchanged. The fixture exercises the label contract and the documented version 3 wire shape:

- **1 page**, `Night at the ford`, with two mentions.
- **3 types**: `Person`, `Faction`, `Place`.
- **6 entities** across a faction, two people, and three places.
- **8 definitions**, including a bool value class, a `null` label, and two definitions named `member of` with different IDs. Duplicate labels are legal because labels are not identifiers; the stable definition ID decides ties.
- **12 assertions**, covering text, boolean `false`, entity reference, and `Unknown` values, archived and unarchived claims, and varied provenance members.

Mention and provenance members are always written, so the fixture spells out `null` for an absent provenance member rather than omitting it. `tests/exchange_v3.rs` compares the decoded and re-encoded document to the fixture as JSON values, not as bytes; the pretty-printed form exists only so the fixture stays reviewable.

All records are synthetic. The names are placeholders for exercising spread, not War Archivist canon.

## Preserved version 2 schema

The frozen version 2 DDL now lives at `src/migration/legacy_v2.sql`, next to the version 1 DDL, because the store uses it at runtime. A file is recognised as a genuine version 2 universe only when its `sqlite_master` matches those statements exactly; matching table names alone is not enough. Tests execute the same DDL and then set `PRAGMA user_version = 2` to build a genuine version 2 database, which proves that such a file is preserved and refused rather than silently converted (DEC-002).

Version 2 differs from version 3 only in `definitions`, which has no `label` column. Neither version has `metadata.format_version`; that column belongs to version 1.

Do not edit the frozen DDL to follow the current schema. If the schema changes again, add a new frozen file instead.
