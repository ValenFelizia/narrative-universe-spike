# PA-I7/KA2-A Knowledge Authoring acceptance

Baseline verified after `git fetch origin`: `main` at `41b8778f854dc7f5a30d9079aef82387ae00b4b5`
(merged PA-I6/KA1 PR #50). Authorized issue #52, approved C0 memo on #51, and
`DEC-006` govern this increment. No schema, index, migration, JSON exchange
format, or dependency change (`SCHEMA_VERSION` and `FORMAT_VERSION` remain `3`).
Only synthetic universes and fixtures were used.

## Automated checks

- PASS: `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`,
  and `cargo test --locked --all-targets` (`373` root tests, including `5`
  real-file tests in `tests/codex.rs` covering atomic
  `Store::create_definition_with_assertion` commit/reopen and rollback on every
  second-step failure, empty properties, typed/untyped ownership, future
  entities of exact type, all value classes plus `Unknown`, competing singular
  claims, self-reference, incoming relations stored once, and
  `Store::probe_knowledge_records` beyond `MAX_CONTEXT_GROUPS`).
- PASS: Tauri `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`,
  and `cargo test --locked --all-targets` (`26` bridge tests, including
  `ka2_knowledge_bridge_validates_commits_atomically_fences_delayed_ops_and_reconciles`
  verifying `deny_unknown_fields`, strict property-label rejection without
  silent trimming, exact UTF-8 byte boundary tests including multi-byte Unicode
  at `16,384` / `1,024` / `4,096` bytes, verbatim byte preservation, atomic
  rollback, and session-scoped `cancelled_ops` fencing proving delayed original
  writes return `OperationCancelled` after `reconcile_codex_write`).
- PASS: UI `npm test` (`134` tests across `adapter`, `save-session`, `mentions`,
  `mention-history`, `desktop`, and `codex`), `npm run check`,
  `npm run desktop:check`, and `npm run desktop:frontend:build`.
- PASS: `node scripts/verify.mjs --diff-base origin/main` (including WebView2
  shared-harness checks across `7` scripts and `git diff --check`).

## Native Windows/WebView2 evidence

PASS: All six native verification scripts ran sequentially against the rebuilt
debug `spike` (`target/debug/spike.exe`) and desktop Tauri binary
(`ui/src-tauri/target/debug/narrative-universe-desktop.exe`) over fresh
temporary v3 SQLite databases:

- PASS: `node ui/scripts/webview2-authoring.mjs` ([machine results](evidence/authoring-native/result.json))
- PASS: `node ui/scripts/webview2-codex.mjs`
- PASS: `node ui/scripts/webview2-parity.mjs` (`22` boundary cases, `37` reducer cases, `0` failures)
- PASS: `node ui/scripts/webview2-connect.mjs`
- PASS: `node ui/scripts/webview2-reveal.mjs`
- PASS: `node ui/scripts/webview2-context.mjs`

Captured KA2-A WebView2 evidence (`docs/evidence/authoring-native/`):

- **Synthetic zero-knowledge start -> atomic property & fact authoring (`1600 × 900`, DPR 1):**
  Dirties Write title/prose, switches to Codex (`Ctrl+S` leaves Write dirty),
  creates type `Order`, typed entities `Kestrel` and `Sable`, and untyped
  `Ash Spire`; rejects `"  Motto  "` without silently trimming; atomically
  creates type-owned `Motto` (`Provisional` + provenance/scope); creates
  intentionally empty boolean property `Sworn`; records competing `No`
  (`ACORDADO`) and `Unknown (recorded)` (`MISTERIO_INTENCIONAL`) claims;
  creates reference property `Allied with` targeting `Ash Spire` and
  self-referencing `Kestrel`; verifies shared `Motto` appears on sibling
  `Sable` and incoming `Kestrel → Ash Spire` appears on `Ash Spire` (with no
  authoring controls on incoming groups or type ownership on untyped entities).
- **Competing-claim modal & responsive narrow layouts (`1120 × 780`, `800 × 600`, `600 × 720`):**
  Verifies compact `Add fact` modal with competing-claim note, unpreselected
  `Yes`/`No` radios, `Provisional` default, focus restoration on Cancel, and
  zero horizontal overflow across all tested viewports.
- **Write Reveal parity & process relaunch (`1120 × 780`):**
  Returns to Write with dirty draft intact, links `Kestrel` via Connect, saves,
  opens `Show facts` (Write Reveal) showing the newly authored claims, and
  relaunches the native desktop binary to confirm exact persisted definition
  and assertion IDs.

Evidence:
[1600 New property dialog](evidence/authoring-native/authoring-1600-new-property-dialog.png),
[1600 Kestrel authored facts](evidence/authoring-native/authoring-1600-kestrel-facts.png),
[1120 Add value dialog](evidence/authoring-native/authoring-1120-add-value-dialog.png),
[800 narrow layout](evidence/authoring-native/authoring-800-narrow.png),
[600 stacked layout](evidence/authoring-native/authoring-600-stacked.png),
[1120 Write Reveal parity](evidence/authoring-native/authoring-1120-write-reveal-parity.png).

## Independent technical review

Claude Sonnet 5.5 (`effort: high`, via Claude Code) independently reviewed
`git diff origin/main` across core, bridge, frontend, tests, and docs against
issue #52 and returned **PASS** with zero P1 or P2 findings and four P3 notes:

- Moved `refreshSelectedEntityAfterCommit` outside the write `try`/`catch`
  blocks in `ui/desktop/codex.ts` so a post-commit DOM/refresh exception can
  never trigger the unconfirmed-write catch path.
- Added a clarifying comment on `ActiveUniverse::cancelled_ops` in
  `ui/src-tauri/src/lib.rs` documenting that the fence is keyed by
  `operation_id` only and scoped to the active session.
- Remaining P3 notes retained for awareness: stored corruption detected during
  probe maps to `ReadFailed` (fails safe by keeping the write lock active as
  unconfirmed).
- Follow-up review pass (addressing Ari's PR #53 review findings on `56e5941`):
  - Derived persistent uncertainty/integrity notices from `integrityAnomaly`,
    `pendingKnowledgeWrite.notice`, and `pendingCreate.notice` so warnings
    persist across entity navigation, search/filter changes, and Write <-> Codex
    reactivation while writes are locked.
  - Disambiguated duplicate and unnamed property labels in `Add fact` via
    shortest distinguishing ID suffix (`formatDefinitionDisplay`), matching the
    Codex context renderer.
  - Guarded native window close (`requestClose` in `ui/desktop/main.ts`) to
    block ordinary closure during in-flight Codex writes/verifications and
    display an explicit `#codex-close-guard` confirmation before leaving
    unresolved Codex write/anomaly states (preserving Write's dirty-draft guard).
  - Switched the reference picker to native button `aria-pressed` semantics
    inside `role="group"` with in-place selection updates so selecting a target
    never tears down DOM or drops keyboard focus.
  - Made modal action buttons sticky inside scrolling `.codex-dialog-wide`
    dialogs (with bottom padding inside the sticky bar so scrolled fields do
    not peek below it) so `Cancel` and `Save`/`Create` remain accessible when
    `Source & scope (optional)` expands.
  - Focused independent review of the follow-up changes by **Claude Sonnet 5.5**
    (`effort: high`, via Claude Code) returned **PASS** with zero P1/P2
    findings; added `onclose` settlement on `#codex-close-guard`, cached
    disambiguated `factTargetLabel` in `openFactDialog`, and eliminated bottom
    peek beneath the sticky dialog actions.

## Trade-offs, limitations & remaining human acceptance (Windows)

- **Session-scoped fencing only:** `cancelled_ops` and `pendingKnowledgeWrite`
  live in memory for the current active universe token only. They do **not**
  provide durable crash recovery across process termination; backup/recovery
  remains mandatory before real-lore dogfooding.
- **CDP vs physical QA:** `webview2-authoring.mjs` uses CDP-dispatched DOM
  events and programmatic resizing. It does **not** prove physical keyboard/IME
  input, screen-reader/arrow-key ergonomics on the reference picker, display
  scaling above 100%, or human visual judgment.

Checklist for Valen (using fresh synthetic databases only — never real lore):

1. Open a synthetic universe with zero preloaded knowledge (e.g., via `spike import-new`),
   switch to **Codex**, create a custom type and two entities of that type plus
   one `Untyped` entity.
2. On the first typed entity, click **New property**, verify `"  Motto  "` is
   rejected rather than silently trimmed, then create a type-owned text
   property with a first fact (`Provisional` default) and optional **Source &
   scope**. Confirm the shared property appears on the sibling entity as
   `Not recorded`.
3. Create an intentionally empty `Yes / No` property (unchecking **Record a
   first fact now**), then use **Add value** to record `No` (`Agreed`) and a
   competing `Unknown (recorded)` (`Intentional mystery`). Verify the singular
   competing-claim note appears and neither `Yes` nor `No` is preselected.
4. Create an entity-reference property, test searching duplicate entity names
   and selecting both another entity and `(this entity)`, then verify the
   incoming reference appears on the target entity without authoring buttons.
5. Return to **Write** (confirming any dirty draft/title/Undo survived), link
   the entity via `Ctrl+K`, open **Show facts** (Write Reveal) to verify
   parity, close and relaunch the app, and confirm exact persisted facts and
   comfortable legibility at 100% / 125%+ Windows DPI across wide and narrow
   windows.
