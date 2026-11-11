# Decisions

<!--
Record consequential decisions when useful. A decision may use stable DEC-NNN
identity, status and date, context, decision, rationale, and consequences;
add alternatives or supersession only when useful. Do not infer rationale or
accepted direction from implementation alone.
-->

## DEC-001 — GitHub is the work tracker

- Status: accepted
- Date: 2026-10-15
- Context: CSDD initialization. The maintainer directed init without
  `.csdd/todo.md` because the tracker is GitHub.
- Decision: This repository does not keep `.csdd/todo.md`. Operational work
  is tracked in GitHub Issues and pull requests on
  https://github.com/ValenFelizia/narrative-universe-spike.
- Rationale: Explicit maintainer instruction at initialization. Contribution
  guidance already requires an approved, scoped issue and a pull request.
- Consequences: `.csdd/` holds specifications, decisions, and handoffs only.
  Task state, ownership, and scope live in GitHub. Do not recreate
  `todo.md` unless this decision is superseded. The default four-document
  CSDD layout is intentionally not adopted in full.

## DEC-002 — Property labels, storage version 3 and exchange format 3

- Status: accepted
- Date: 2026-11-10
- Context: PA-I5/R0 (#38) selected a nullable, human-authored property
  label. Fable's architecture challenge (issue #38, comment `6776351471`)
  showed that bumping the schema constant silently redefines `spike migrate`
  and `spike verify`, that a supported two-build v2→v3 conversion cannot be
  tested from a single tree, and that a SQL check cannot express "meaningful
  trimmed content". Valen approved synthetic-only Conversion A in the R0
  closeout (comment `6777496032`). Issue #39 bounds the implementation to this
  contract and forbids Reveal work.
- Decision:
  1. `Definition.label` is `Option<DefinitionLabel>`. `DefinitionLabel` is the
     single validation owner for JSON decoding, record creation, stored-row
     reads and logical verification. A valid label is 1 to 120 Unicode scalar
     values with no leading or trailing Unicode whitespace and no control
     character. Invalid labels are rejected; they are never trimmed, case
     folded or normalized. `None` means unnamed and is never inferred. The
     type has no inverse-label concept, and duplicate labels are legal.
  2. Storage schema version 3 adds the nullable `definitions.label` column.
     `lib.rs` owns the current schema version; `store.rs` and `migration.rs`
     read it and keep only explicit legacy version constants. The SQL column
     check is a backstop for type and emptiness only.
  3. JSON exchange format version 3 is a distinct `EnvelopeV3` with its own
     `DefinitionDtoV3`. The version number is bound to the envelope type, not
     read from a free constant. The version 1 and version 2 envelopes and
     `DefinitionDto` stay decode-only, so `label` in a legacy document is
     invalid. A version 3 writer always emits `label`, including `null`.
  4. `spike migrate` is version 1 to the current version only, through the
     existing out-of-place, verified candidate and publish path. `verify`
     verifies the current version only. Both versions are 3.
  5. Conversion A: no in-place migration and no v2→v3 converter. A genuine
     version 2 database is refused without being opened for writing. `Store::open`
     distinguishes its exact frozen schema from foreign or forged v2 headers;
     `spike migrate` and `spike verify` reject a v2 header during read-only
     preflight without authenticating its schema, so an impostor can receive
     the same unsupported-version diagnostic. Neither operation writes it.
     Version 2 files are preserved test artifacts. Disposable synthetic
     universes are recreated from JSON fixtures with `import-new` at a new path.
  6. Version 1 and version 2 JSON remain decodable with `label: None`. JSON
     exchange is the compatibility boundary, and it is not a lossless
     database migration: export is capped at 10 MiB and page revision counters
     are not exchanged.
  7. Labels are write-once in this increment. There is no label update API, and
     importing a differing label over an existing definition is
     `ImportConflict`.
- Rationale: The label is display text attached to a stable definition
  identity, so it needs durable storage and a shape the domain can enforce.
  Both version numbers change because a version 2 build would otherwise read a
  labelled database and re-export it without its labels. Conversion A matches
  the only data the project may still destroy: fully reproducible synthetic
  universes. A converter that must be built from an older executable is not a
  supported conversion and cannot be regression-tested from one tree.
- Alternatives considered: an in-place `ALTER TABLE` upgrade (writes the
  source, and the backup/recovery gate has not passed); a side
  `definition_labels` table (still a storage bump, more joins); a required
  label (would require inventing labels for existing records); a label map held
  outside the database (not canon data); a single-build `spike convert` from a
  frozen v2 reader (deferred with the backup gate, before real lore).
- Consequences: A version 2 database must be recreated from JSON or kept
  untouched; the desktop app reports it as an unsupported database. A universe
  converted through JSON loses page revision counters and cannot exceed the
  10 MiB export cap. A later in-place converter, or a real lore data-safety
  gate, must revisit this decision rather than extend it silently.

## DEC-003 - R1-B persisted facts projection and capture lifetime

- Status: implementation decision within approved issue #41; awaiting PR review.
- Date: 2026-11-11
- Context: R1-A main baseline d2e2523 supplies validated Definition labels and
  the synthetic v3 reveal-facts fixture. R1-B authorizes persisted knowledge
  inspection, not an applicable-property enumeration or expanded R2 panel.
- Decision: `Store::entity_reveal` uses one deferred SQLite read transaction
  for identity and active outgoing/incoming claims. Reuse validated stored-row
  readers and applicability checks. Missing entities return NotFound; dangling
  or malformed records fail closed. The token-scoped bridge preserves typed
  values, assertion/definition IDs, property metadata, literal canon, source
  status, full provenance and scope. No schema/exchange changes.
- Decision: Next claims cycles deterministic groups of two within each
  section without ranking canon (reduced from four after PR #42 review: the
  peek must stay compact, capped at min(340px, 100vh - 220px) with internal
  scrolling). Empty sections collapse to one heading line. Null label, recorded Unknown, boolean false,
  no outgoing claims and absent provenance have distinct UI language. Sources
  are plain text, never invented navigation. Same-name identities receive the
  shortest distinguishing suffix (minimum four characters). Separate
  definitions sharing a property label, including several unnamed ones, get
  the same definition-ID suffix so they never read as alternatives of one
  property.
- Decision: an unsaved occurrence may read persisted entity knowledge. A new
  request has its own identity; closure or any captured editor/session drift
  invalidates it, including a same-entity reopen. Errors stay inside the peek
  and never replace save-state status. No automatic retry or write occurs.
- Consequences: one point-in-time projection, no live-update promise. Full
  definition ownership remains core data; the desktop DTO carries only display
  metadata needed here. No inferred inverse, canon resolution, authoring,
  applicability enumeration, or R2 context surface is added.
