# PA-I6/KA1 Codex Foundations acceptance

Baseline verified after `git fetch origin`: main `6456920` (merged PA-I5/R2-A PR
#46). Authorized issue #49 and the approved product direction memo
`Codex — Product Direction & KA1 Foundations` (`DEC-005`) govern this
increment. No schema, index, migration, JSON exchange format, or dependency
change (`SCHEMA_VERSION` and JSON format remain `3`). `tests/fixtures/reveal-facts.json`
is untouched; no real lore was used.

## Automated checks

- PASS: `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`,
  and `cargo test --locked --all-targets` (`371` tests across all root suites,
  including `3` real-file tests in `tests/codex.rs` covering `Store::codex_index`,
  `Store::type_index`, `Store::codex_entity_index`, `validate_codex_label`,
  empty/zero-entity types, duplicate type/entity labels, optional primary page
  titles, reopen persistence, and fail-closed corruption handling).
- PASS: Tauri `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`,
  and `cargo test --locked --all-targets` (`25` bridge tests, including
  `codex_bridge_reads_creates_persists_and_enforces_validation_and_lock`).
- PASS: UI `npm test` (`130` tests across `adapter`, `save-session`, `mentions`,
  `mention-history`, `desktop`, and `codex`), `npm run check`,
  `npm run desktop:check`, and `npm run desktop:frontend:build`.
- PASS: `node scripts/verify.mjs --diff-base origin/main` (including WebView2
  harness checks across 6 scripts and `git diff --check`).

Bridge and frontend tests additionally verify:

- Opening any synthetic universe (including `0`-page empty databases) enables
  switching between `Write` and `Codex` without creating a document.
- Search filters by entity label, type label, primary document title, or entity
  ID; custom type filters keep duplicate type labels distinct by `typeId`, show
  zero-entity types, and include `Untyped`.
- `New type` and `New entity` dialogs default new entities to `Untyped`, trim
  and validate labels (`1..=120` scalars, single line, no control chars) before
  IPC, block double-submission, keep user input intact on definite non-commit
  rejections (`busy`, `invalid_input`, `duplicate_id`, `type_missing`, stale
  universe), and close + surface `Refresh Codex` without auto-retrying creation
  on ambiguous failures.
- `Write <-> Codex` preserves dirty Write title, body, selection, undo history,
  scroll offset, and `#guard` close/universe-switch protection; `Ctrl+S` in
  Codex never saves hidden Write drafts; `#library-toggle` is disabled in Codex;
  and Codex-created entities appear immediately in Write's Connect (`Ctrl+K`)
  picker.

## Native Windows/WebView2 evidence

PASS: All five native verification scripts ran sequentially against the rebuilt
debug `spike` (`target/debug/spike.exe`) and desktop Tauri binary
(`ui/src-tauri/target/debug/narrative-universe-desktop.exe`) over fresh
temporary v3 SQLite databases:

- PASS: `node ui/scripts/webview2-codex.mjs` ([machine results](evidence/codex-native/result.json))
- PASS: `node ui/scripts/webview2-parity.mjs` (`22` boundary cases, `37` reducer cases, `0` failures)
- PASS: `node ui/scripts/webview2-connect.mjs`
- PASS: `node ui/scripts/webview2-reveal.mjs`
- PASS: `node ui/scripts/webview2-context.mjs`

Captured KA1 WebView2 checks (`docs/evidence/codex-native/`):

- **Zero-page empty universe (`1120 × 780`, DPR 1):** Opens an empty v3 synthetic
  database (`0` pages, `0` types, `0` entities), switches cleanly into Codex,
  rejects blank type creation in-place, creates custom type `House (0)`, creates
  `Untyped` (`Ashfall Bell`) and duplicate-label (`Mara Vey · …c498`,
  `Mara Vey · …12e7`) entities without creating any pages, and returns to Write
  with `0` pages untouched.
- **Knowledge inspection & editorial fidelity (`1600 × 900`, DPR 1):** Dirties
  Write's title/body and scrolls to `220`, switches to Codex, verifies `Ctrl+S`
  keeps Write dirty, inspects `Mara Vey` via `Store::entity_context` (including
  disambiguated `member of · …0031` / `…0038`, zero-claim `oath sworn` ->
  `Not recorded`, provenance expansion, and closed-by-default `Archived history`).
- **Custom type & entity creation (`1600 × 900` & `800 × 600`):** Creates type
  `Chronicle (0)` and entity `Sable Lantern`, inspects type filter narrowing at
  `800 × 600` stacked layout with zero horizontal overflow, and inspects `Ashfall
  Order` search and subject-first incoming references at `1120 × 780`.
- **Write <-> Codex round-trip, Connect, Save & Relaunch:** Returning to Write
  restores exact dirty title, prose, `scrollTop = 220`, Undo/Redo (`Ctrl+Z` /
  `Ctrl+Y`), and dirty save state; Connect (`Ctrl+K` / contextual action) offers
  newly created `Sable Lantern (Chronicle)` immediately; saving and fully
  relaunching the native desktop binary preserves exact type/entity UUIDs,
  saved prose, and the new mention link.

Evidence: [1120 empty universe](evidence/codex-native/codex-1120-empty.png),
[1600 Mara Vey context](evidence/codex-native/codex-1600-mara.png),
[1600 New entity dialog](evidence/codex-native/codex-1600-entity-dialog.png),
[1120 Ashfall search](evidence/codex-native/codex-1120-ashfall.png),
[800 stacked filter](evidence/codex-native/codex-800-stacked.png).

## Independent review

Gemini 4 Argon (`thinking_level: high`) independently reviewed the full diff
against `origin/main`, `.csdd/specs.md`, `.csdd/decisions.md` (`DEC-005`),
`DESIGN.md`, `ui/DESKTOP.md`, and issue #49, and returned **APPROVE** with zero
P1, P2, or P3 findings. It also independently ran
`node scripts/verify.mjs --diff-base origin/main` (`371` root tests, `25` Tauri
tests, `130` UI tests, TypeScript/Vite checks, WebView2 harness check, and
`git diff --check`). Valen and Ari retain final product and merge authority.

## Remaining human acceptance (Windows)

Note on test scope: `webview2-codex.mjs` uses CDP-dispatched DOM/keyboard events
and programmatic window resizing, so it does **not** prove physical keyboard/IME
input, display scaling above 100%, or human visual judgment.

UNTESTED by a human:

- Physical keyboard and IME input across Codex search, `New type`, `New entity`
  label input and `<select>`, `Tab` / `Shift+Tab` traversal, and `Escape`
  dialog dismissal.
- Subjective readability, contrast, focus rings, and spacing at real Windows
  display scaling (`100%` and `125%+` DPI) across `1600×900`, `1120×780`, and
  `<= 620 px` stacked layout.
- Physical pointer navigation, modal backdrop/Cancel dismissal, and Write
  `#guard` dialog triggers while viewing Codex before closing or switching
  universes.

Checklist for Valen (using fresh synthetic fixtures via `spike import-new` or
`spike create` — never a real database):

1. Open a synthetic `0`-page database, switch to **Codex**, create a custom
   type, create both an `Untyped` entity and a typed entity, restart the app,
   and confirm both persist with no invented documents.
2. Import `tests/fixtures/reveal-facts.json` to a fresh path, open it, dirty a
   document title/prose and scroll down, switch to **Codex**, press `Ctrl+S`
   (verify nothing saves), create a new entity, switch back to **Write**, confirm
   exact draft/scroll/Undo survive, link the new entity via `Ctrl+K`, and save.
3. Inspect `Mara Vey` and `Ashfall Order` in Codex across desktop (`~1600×900`),
   medium (`~1120×780`), and narrow (`<= 620 px`) widths to verify legibility,
   filtering by `Untyped` and duplicate types, and zero horizontal clipping.
4. Confirm attempting to switch universes or close the window while a Write
   draft is dirty and **Codex** is active still triggers the unsaved-changes
   guard.
