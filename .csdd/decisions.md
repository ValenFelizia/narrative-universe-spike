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

## DEC-004 — PA-I5/R2-A Explore Context read path, bounds, and responsive edge/anchored presentation

- Status: accepted within authorized issue #44 (R2-C0 decision memo on #43);
  awaiting PR review and Windows human acceptance.
- Date: 2026-11-17
- Context: R1-B (`DEC-003`) kept compact facts strictly bounded to active
  assertions and a small anchored overlay. Issue #44 authorizes the next step
  from R2-C0 (#43): a deliberate deeper read showing applicable zero-claim
  definitions, grouped active outgoing/incoming claims, and archived history
  without interrupting Write or altering schema/JSON contracts.
- Decision:
  1. Separate core read (`Store::entity_context`) rather than expanding
     `Store::entity_reveal`: keeps `entity_reveal` compact and unchanged while
     executing `entity_context` in one deferred SQLite read transaction. Reuses
     validated bounded row readers (`read_bounded_entity`, `read_definition_from_row`,
     and bounded assertion readers) with history-aware applicability checks
     (archived definitions remain readable in archived context while
     `Store::create_assertion` still rejects new assertions against archived
     definitions) and bounded owner existence checks (`typeof(label)` without
     allocating unprojected owner labels). Collected assertions (at most 500) are
     sorted in memory so SQLite never runs `TEMP B-TREE` payload sorts.
  2. Fail-closed bounds with exact wire-byte accounting: enforce
     `MAX_CONTEXT_GROUPS = 128`, `MAX_CONTEXT_UNIQUE_ASSERTIONS = 500`, and
     `MAX_CONTEXT_BYTES = 2 MiB` (`2_097_152` serialized camelCase DTO bytes)
     before copying oversized payloads, accounting for worst-case JSON string
     escaping in core and re-verifying the exact serialized DTO length at the
     Tauri bridge (`Error::ContextTooLarge` -> `BridgeError::ContextTooLarge`).
  3. Shared frontend context renderer with measured V2-edge / V1-anchored
     layout: render on demand inside `.entity-picker.context` rather than as a
     persistent right-hand split pane. Measure `proseRect` against
     `window.innerWidth`; use the right edge (`300..360 px`) only when `>= 300 px`
     of clear margin remains beside the writing column (zero prose reflow or
     recentering), otherwise fall back to anchored positioning (flipping above
     near-bottom carets so the active line remains visible).
  4. Zero-refetch `Back to facts`: preserve a `FactsSnapshot` (reveal payload,
     error text, section pagination offsets, expanded provenance set, and popup
     scrollTop) across `Explore context -> Back to facts` so writers can move
     between peek and context without extra IPC calls or lost state.
- Rationale: Keeping `entity_reveal` untouched protects R1-B's minimal path and
  latency; separating `entity_context` lets bounds and archive grouping evolve
  cleanly without schema bumps (`SCHEMA_VERSION` stays `3`). Measuring
  available viewport margin dynamically preserves `DESIGN.md`'s stationary prose
  column at 1600x900 (whether library is open or closed) while guaranteeing clean
  usability at 1120x780 and 800x600.
- Consequences: Context reads stay point-in-time and bounded; entities exceeding
  group, assertion, or 2 MiB byte limits advise the writer to use compact facts.
  Incompatibility warnings, backlinks/occurrence lists, clickable provenance
  navigation, and authoring remain out of scope for R2-A.

## DEC-005 — PA-I6/KA1 Codex workspace, identity-preserving reads, and Write draft retention

- Status: accepted within authorized issue #49 (`Codex — Product Direction & KA1
  Foundations`); awaiting independent review, CI, and Valen/Ari physical
  Windows acceptance.
- Date: 2026-11-18
- Context: Issue #49 authorizes PA-I6/KA1 Codex Foundations so worldbuilders can
  browse, search, create custom types and entities, and inspect existing
  knowledge independently of document saves without introducing schema bumps,
  inline authoring, or Write regressions.
- Decision:
  1. Separate read-only Codex index (`Store::codex_index`, `Store::type_index`,
     `Store::codex_entity_index`) and shared nonblank input validator
     (`validate_codex_label` / `validate_label_text`, `MAX_CODEX_LABEL_SCALARS
     = 120`): preserves `Store::entity_index` untouched for Connect while adding
     validated type identities (including 0-entity types) and optional
     `type_id`/`type_label`/`primary_page_title` metadata in one deferred
     read transaction with fail-closed row validation. Frontend input trims
     leading/trailing whitespace before submission, checks nonblank/control-char
     /120-scalar bounds prior to IPC, and never globally normalizes or
     deduplicates persisted labels.
  2. Token-scoped Codex bridge (`codex_index`, `create_entity_type`,
     `create_entity`) backed directly by `Store::create_entity_type` and
     `Store::create_entity` (`page_id: None`, explicit `TypeMissing` error
     mapping, best-effort post-commit `type_label` lookup): prevents duplicated
     SQL write paths in Tauri and ensures an already-committed entity never
     reports a spurious failure on post-commit metadata lookup.
  3. Non-destructive Write <-> Codex switching via view visibility and explicit
     scroll capture: keeps the live CodeMirror `EditorView`, `SaveState`,
     draft title, selection, undo stack, and `#guard` close/universe-switch
     protection intact when entering Codex, disables `#library-toggle` and
     Write's `Ctrl+S` handler while in Codex, and triggers a fresh
     `codex_index` load when entering Codex rather than mutating Write state.
  4. Explicit modal creation with definite vs uncertain failure handling:
     definite pre-commit rejections (`busy`, `invalid_input`, `duplicate_id`,
     `type_missing`, stale universe) keep user input inside the open modal for
     correction/retry, whereas ambiguous/unclassified failures close the modal
     and surface `Refresh Codex` without auto-retrying creation so duplicate
     UUID creations cannot occur silently after an uncertain commit.
- Rationale: Preserving Write in memory avoids forcing a premature
  save/discard prompt merely to look up or add a Codex entity mid-sentence,
  while keeping Codex writes strictly separate from Write's aggregate save
  ledger and Undo history. Keeping `entity_index` untouched avoids breaking
  accepted Connect contracts while letting Write's Connect picker see new Codex
  entities on its very next read.
- Consequences: `SCHEMA_VERSION` and JSON exchange format remain `3`. Duplicate
  labels remain legal and are disambiguated by shortest ID suffix for both
  entities and types. Editing/deleting types or entities, entity notes,
  property/assertion authoring, and inline create-and-link remain explicitly
  deferred to future authorized increments.
