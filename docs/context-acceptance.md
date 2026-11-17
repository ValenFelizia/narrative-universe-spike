# PA-I5/R2-A Explore Context acceptance

Baseline verified after `git fetch origin`: main `deefede` (merged PA-I5/R1-B PR
#42). Authorized issue #44 and the R2-C0 decision memo on issue #43 govern this
increment. No schema, index, migration, JSON exchange format, or dependency
change. `tests/fixtures/reveal-facts.json` is untouched; no real lore was used.

## Automated checks

- PASS: `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`,
  and `cargo test --locked --all-targets` (365 tests across all root suites,
  including 6 real-file tests in `tests/entity_context.rs`).
- PASS: Tauri `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`,
  and `cargo test --locked` (21 bridge tests).
- PASS: UI `npm test` (126 tests), `npm run check`, `npm run desktop:check`, and
  `npm run desktop:frontend:build`.
- PASS: `git diff --check`.

Root `tests/entity_context.rs` verifies:

- Real SQLite fixture reads for Mara Vey (active grouped definitions, zero-claim
  applicable `oath sworn`, archived entity-owned `archived sigil`, duplicate
  `member of` definition suffixes) and Ashfall Order (subject-first incoming
  groups).
- Synchronized concurrent writer/reader snapshot consistency (one deferred read
  transaction never observes a partial write and releases the read lock cleanly).
- Independent `definition.archived` and `assertion.archived` combinations,
  zero-claim archived definitions, and self-references appearing in both
  outgoing and incoming groups.
- Hard bounds (`MAX_CONTEXT_GROUPS = 128`, `MAX_CONTEXT_UNIQUE_ASSERTIONS = 500`,
  and `MAX_CONTEXT_BYTES = 2 MiB` with escaping-aware byte projection returning
  `Error::ContextTooLarge`).
- Fail-closed behavior on `Busy` and malformed active or archived rows without
  mutating database bytes.

Measured SQLite query plans and scale latency (`query_plans_and_scale_measurement_report`):

- `defs`: `["SCAN definitions USING INDEX sqlite_autoindex_definitions_1"]`
- `out`: `["SEARCH assertions USING INDEX idx_assertions_subject_definition (subject_id=?)", "USE TEMP B-TREE FOR LAST TERM OF ORDER BY"]`
- `in`: `["SEARCH assertions USING INDEX idx_assertions_target (target_entity_id=?)", "USE TEMP B-TREE FOR LAST TERM OF ORDER BY"]`
- `2040 defs + 300 claims elapsed: 4.9365ms` (without adding schema indexes)

Bridge tests (`ui/src-tauri/src/lib.rs`) cover universe-token validation,
camelCase DTOs, `ContextTooLarge` (`"context_too_large"`) under worst-case JSON
quote escaping, independent archive flags, zero-claim archived definitions, `Busy`,
and sanitized `ReadFailed` errors.

Frontend tests (`ui/test/desktop.test.ts`) verify grouped rendering, zero-claim
differentiation (`Not recorded` vs `Not recorded · Archived history available`),
incoming orientation, closed-by-default archived history, `Back to facts` state
restoration without refetching `entity_reveal`, error handling inside the dialog
without altering save status, late-response cancellation across close/back/drift,
outside `pointerdown` closing without stealing the clicked control's focus,
`Escape` focus/selection/scroll/draft preservation, and live transitions between
V2 edge placement and V1 anchored fallback (including near-bottom flipping).

## Native Windows/WebView2 evidence

PASS: `node ui/scripts/webview2-context.mjs` launches the actual debug Tauri
executable (`narrative-universe-desktop.exe`), connects to its WebView2 CDP
target, and invokes the real Rust/SQLite backend against fresh temporary v3
databases (`reveal-fixture.sqlite` imported from
`tests/fixtures/reveal-facts.json` via `spike import-new`, plus a temporary
synthetic v3 archive/self-reference database). See
[machine results](evidence/context-native/result.json).

- **1600 × 900 wide viewport (DPR 1, library open & closed):** Measures `freeRight`
  and selects V2 edge layout (`width: 332px`, `left: 1252px`, `top: 92px`,
  `bottom: 844px`, `height: 752px`). Prose column stays fixed at `640px` wide
  and `left: 587.5px` (`docLeftBefore === docLeftAfter`) with library both open
  and closed—zero prose reflow or recentering, and clear separation (`>= 24px`)
  from the writing column.
- **Live resize transition (1600 × 900 ↔ 1120 × 780 ↔ 800 × 600):** Resizing the
  native Tauri window while Context is open smoothly transitions between V2 edge
  and V1 anchored layout (`width: 440px`), fitting cleanly inside the reading
  area at `1120 × 780` (library closed and open) and `800 × 600`.
- **800 × 600 long-scene near-bottom caret:** With `scrollTop = 3504` and caret
  at `top: 526.08px`, V1 anchored fallback flips above the caret (`top: 84px`,
  `bottom: 520.08px`), keeping the active prose line visible and preserving
  scrollTop, selection, unsaved link indicator, and dirty draft.
- **Synthetic archive & self-reference check (1600 × 900):** Confirms `Alden ·
  sworn rival → Alden` in both outgoing and incoming groups, `oath bound` as
  `Not recorded · Archived history available`, unarchived claim `Warden of Glass`
  under an archived definition, and zero-claim archived `unused retired slot`.

Evidence: [1600 library open](evidence/context-native/context-1600-library-open.png),
[1600 library closed](evidence/context-native/context-1600-library-closed.png),
[1120 library open](evidence/context-native/context-1120-library-open.png),
[Ashfall 1120](evidence/context-native/ashfall-1120.png),
[800 anchored](evidence/context-native/context-800.png),
[800 long near-bottom](evidence/context-native/long-near-bottom-800.png),
[synthetic archive & self-reference](evidence/context-native/synthetic-archive-selfref-1600.png).

## Independent review

An independent Codex (`gpt-6.1-terra`, `high` reasoning effort) review
delegate was started against this branch and failed immediately with an
account usage-limit error (`You’ve hit your usage limit`). Per project rules, no
silent provider switch or self-certified review substitution was performed;
human PR review by Valen and Ari (or an authorized re-run once quota resets)
remains required before merge.

## Remaining human acceptance (Windows)

Note on test scope: `webview2-context.mjs` uses CDP-dispatched keyboard/DOM
events and programmatic window resizing, so it does **not** prove physical
keyboard/IME traversal, display scaling above 100%, or human readability.

UNTESTED by a human:

- Physical keyboard and IME traversal (`Tab` / `Shift+Tab` / `Enter` / `Escape`)
  across `Back to facts`, `Close · Esc`, provenance `<details>`, and closed/open
  `Archived history`.
- Subjective legibility, contrast, and spacing at real display scaling (100% and
  125%+ DPI) across 1600×900, 1120×780, and 800×600.
- Physical mouse pointer click outside the context overlay (for example onto the
  document title input or another prose line) confirming it closes cleanly
  without stealing focus from the clicked control.

Valen should import the fixture at a fresh path with:
`cargo run --locked --bin spike -- import-new <fresh.sqlite> tests/fixtures/reveal-facts.json`
Run `npm run desktop:dev` in `ui/`, open that synthetic database, and:

1. At ~1600×900, inspect `Mara Vey` -> `Show facts` (advance to page 2, expand
   one source) -> `Explore context`. Confirm the context panel docks in the
   right margin without shifting the prose column, both with the library open
   and closed. Click `Back to facts` and verify pagination and expanded source
   return immediately.
2. Resize the window down to ~1120×780 and ~800×600 while Context is open;
   confirm smooth transition to anchored placement and verify near-bottom carets
   flip above the active line.
3. Expand `Archived history`, verify stored archive notes and zero-claim
   definitions (`oath sworn` -> `Not recorded`), press `Escape` to return focus
   and scroll to the editor, and test a physical click onto the title input.
