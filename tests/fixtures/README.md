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
