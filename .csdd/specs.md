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

## PA-I5/R1-A property label and version contract

Sources: #38 R0 closeout comment `6777496032`, Fable's architecture challenge
`6776351471`, and authorized issue #39. Decisions: DEC-002. This contract is
data-only; it adds no Reveal read path, bridge fact command or authoring UI.

- `Definition.label` is `Option<DefinitionLabel>`. `DefinitionLabel` is the
  only place the label rule lives, so JSON decoding, creation, stored-row
  reads and logical verification cannot disagree. A label is 1 to 120 Unicode
  scalar values with no leading or trailing Unicode whitespace and no control
  character. The bytes are preserved exactly: no trimming, case folding or
  Unicode normalization. Invalid input is rejected, never repaired.
- `None` is "unnamed" and is never inferred from a type, entity, UUID or
  assertion. An empty string is invalid and is not another spelling of `None`.
  Labels are outward only; there is no inverse predicate. Duplicate labels are
  legal, including two definitions with the same owner and the same label, so
  a reader must disambiguate by stable ID, not by label.
- Two definitions may carry the same label. Nothing orders, ranks or merges
  two claims by label.
- Storage schema version is 3 and JSON format version is 3. A version 3
  document always writes `label`, including `null`. An absent or `null` label
  in a version 3 document reads as `None`.
- Version 1 and version 2 documents remain decodable and read as `label:None`.
  Their wire shapes are frozen: a legacy document that carries a `label` member
  is invalid, not a version 3 document in disguise.
- Version 2 databases are preserved, not upgraded. Opening one is refused
  without any write, with a refusal distinct from a foreign or forged version
  header. `spike migrate` converts version 1 to the current version only, and
  `spike verify` verifies the current version only. No command advertises a
  version 2 database conversion.
- JSON exchange is the supported compatibility boundary and is not a lossless
  database migration: export is capped at 10 MiB and page revision counters are
  not part of the format.
- Labels are write-once. There is no update API, and importing a differing
  label over an existing definition fails as `ImportConflict` with the whole
  batch rolled back.
- SQL enforces only that a stored label is text and nonempty. The Rust rule is
  authoritative, so a row written with checks disabled is still refused on
  read, export and verify, and is never echoed into a diagnostic.

## PA-I5/R1-B facts-first Reveal (issue #41)

Approved scope: inspect persisted active assertions and definitions from a
linked mention, including unsaved mentions, without changing the draft or
canon. The core read is one transaction; the bridge is universe-token-scoped.
Outgoing facts and incoming references retain their original orientation,
stable identities, literal canon and recorded provenance/scope. Missing entity
is unavailable, missing label is Unnamed property, false is No, stored Unknown
is Unknown (recorded), and no active outgoing claims is No recorded facts.
The UI makes no property-specific absence claim or automatic truth selection.
An intentional Show facts action opens a compact overlay; Link/Unlink and
Ctrl+K remain. Async responses require the original valid capture. No schema,
exchange, dependency, authoring, permanent panel or R2 change is authorized.
See DEC-003 and DESIGN.md for presentation and capture decisions; native
acceptance evidence is in docs/reveal-acceptance.md.

## PA-I5/R2-A Explore Context (issue #44)

Approved scope: follow-on deeper read-only knowledge view opened deliberately
from facts peek (`Explore context`) without modifying `entity_reveal`, Write,
page drafts, schema/exchange, or canon. Sources: R2-C0 decision memo on issue
#43, authorized issue #44, and `DEC-004`.

- Core read: `Store::entity_context(&EntityId)` runs inside one deferred
  SQLite read transaction (`ReadOnly` check via `Store::open`, `NotFound` for
  missing entities, fail-closed on dangling/malformed stored rows). It enumerates
  all applicable definitions for the entity (type-owned when `entity.type_id` is
  present, plus entity-owned), active outgoing claims grouped by definition ID
  (preserving zero-claim active definitions with `has_archived_claims`), active
  incoming references grouped by `(subject_id, definition_id)` keeping original
  `subject -> predicate -> target` orientation, and closed-by-default archived
  history grouped separately for outgoing and incoming while keeping
  `definition.archived` and `assertion.archived` independent.
- Hard bounds: `Store::entity_context` enforces `MAX_CONTEXT_GROUPS = 128`,
  `MAX_CONTEXT_UNIQUE_ASSERTIONS = 500`, and `MAX_CONTEXT_BYTES = 2 MiB` using
  an escaping-aware JSON wire size projection before finalizing large payloads,
  returning `Error::ContextTooLarge` (mapped to `BridgeError::ContextTooLarge`
  / `"context_too_large"`). `Store::create_assertion` remains strict on archived
  definitions; existing history-aware validation reads remain unchanged.
- Bridge: `entity_context` command (`#[tauri::command(async)]`) is scoped by
  `universe_token`, maps `UniverseNotOpen`, `UniverseChanged`, `EntityMissing`,
  `ContextTooLarge`, `Busy`, and sanitized `ReadFailed`, and emits camelCase
  DTOs with complete identity, definition metadata, literal canon states, full
  provenance, `sourceStatusRaw`, `scopeNote`, and independent archive flags.
- UI & layout: `Explore context` transitions from compact facts peek while
  capturing a `FactsSnapshot` (`Back to facts` restores pagination, expanded
  provenance, and scrollTop without refetching `entity_reveal`). The shared
  context renderer places the view in the right margin (`width = min(360,
  freeRight)`) when `proseRect.width > 0` and `window.innerWidth - proseRect.right
  - 24 - 16 >= 300`, with zero prose reflow/recentering, and falls back to an
  anchored overlay (preferring below caret, flipping above when near bottom)
  otherwise. `Escape` restores editor focus (`preventScroll: true`), selection,
  and scroll while valid; outside `pointerdown` closes without stealing the
  clicked control's focus. No incompatibility flags, occurrence lists,
  clickable provenance links, authoring, AI, or permanent right panel are added.

## PA-I6/KA1 Codex Foundations (issue #49)

Approved scope: first native Codex workspace over an open synthetic universe
allowing worldbuilders to browse, search, create custom types and entities
(typed or untyped), and inspect persisted entity knowledge independently of
document saves. Sources: authorized issue #49, approved direction document
`Codex — Product Direction & KA1 Foundations`, and `DEC-005`.

- Core reads and label contract: `validate_codex_label` shares
  `validate_label_text` (`1..=120` Unicode scalar values, no leading or trailing
  Unicode whitespace, no control characters, exact byte preservation without
  folding or normalization) with `DefinitionLabel` while keeping persisted
  labels un-normalized globally. `Store::type_index` reads all custom types
  (including zero-entity types) ordered by SQLite `COLLATE BINARY` `(label, id)`;
  `Store::codex_entity_index` reads all entities with optional `type_id`,
  `type_label`, and `primary_page_title`; `Store::codex_index` executes both in
  one deferred SQLite read transaction with fail-closed UUID, text-type, and
  foreign-key validation. Existing `Store::entity_index` remains untouched for
  Connect compatibility.
- Bridge contract: token-scoped Tauri commands `codex_index`,
  `create_entity_type`, and `create_entity` delegate exclusively to
  `Store::codex_index`, `Store::create_entity_type`, `Store::create_entity`
  (forcing `page_id: None`), and `Store::entity_type`, mapping
  `ValidationReason::UnknownType` to `BridgeError::TypeMissing`. Post-commit
  type-label resolution is best-effort so a committed entity never reports a
  false failure. Schema version remains 3 and JSON exchange format remains 3.
- Workspace & draft isolation: `.context-strip` offers `Write` and `Codex`
  switching for any open universe, including universes with 0 pages, and keeps
  `#context-name` pinned to `Codex` (without stealing focus) if `open_universe`
  or `selectPage` runs while Codex is active. Codex lists entities
  deterministically with shortest-suffix ID disambiguation for duplicate
  entity and type labels, filters by custom type ID (never collapsing duplicate
  type labels) and `Untyped`, renders an editorial entry layout via
  `renderCodexEntrySections` (tucking full UUIDs, definition schema metadata,
  and raw canon codes behind `Identifier` / `Property details` / assertion
  disclosures while leaving Write's `renderEntityContextSections` unchanged),
  and offers modal `New type` and `New entity` flows (`Untyped` default, no
  auto-assigned type/page, no autocreation on search, single-flight submit).
  Definite non-commit rejections (`busy`, `invalid_input`, `duplicate_id`,
  `type_missing`, stale universe) keep user input intact inside the modal;
  ambiguous/unclassified outcomes close the modal, retain pending operation
  identity (`token`, `kind`, `id`, `label`), and lock new creations until
  `Refresh Codex` verifies against `codex_index` (or universe switch resets
  state), without auto-retrying creation. Write retains its live `EditorView`
  draft, title draft, selection, scroll offsets, undo history, and `#guard`
  close/universe-switch protection across Codex trips; `Ctrl+S` in Codex does
  not save hidden Write drafts, and newly created Codex entities are
  immediately available to Write's Connect (`Ctrl+K`) workflow without
  reopening the universe.
- Non-goals preserved: no KA2–KA5 features, entity/type rename/delete/retype,
  notes editor, assertion/property authoring, inline create-and-link, canon or
  archive edits, graph/timeline/backlinks, AI/MCP, or real-data safety claims.
