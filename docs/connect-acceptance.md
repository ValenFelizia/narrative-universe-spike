# Connect acceptance handoff

Issue #33, based on approved D1-D6 in #31 and merged #34 at `dceda4d`.
Synthetic data only. Valen/Ari acceptance remains required before merge.

## Implementation

`SaveState` and `mentionField` own the aggregate. A discriminated picker state
captures the editor, document, universe token, session, selection and mentions.
The alternative was putting picker state in another CodeMirror field. The
controller keeps asynchronous index loading and DOM focus separate from the
canonical mention history without duplicating the mention model.

Transactions are recorded before the view updates. A refused transaction changes
neither draft nor view. Stored links are editable with the existing text and
revision locks. Decorations distinguish saved and draft links. Both Ctrl+K and
the selection action open the anchored existing-entity picker. Enter requires an
explicit arrow-key choice; pointer selection uses entity IDs. Exact display ties
show a subdued suffix, extended when needed to distinguish IDs.

Text invalidation uses `pendingDrops`. Undo restores the original identity.
A regression found that deliberate Unlink after Undo revived a historical drop
warning. Ordered Link/Unlink entries now resolve the earlier notice. Failed
saves retain the structured draft and never report Saved. Copying text explicitly
excludes links. The seed and schema remain unchanged.

## Automated evidence

- PASS: `npm ci`, `npm run check`, `npm run desktop:check`, `npm test`
  (108 tests after review), and `npm run desktop:frontend:build` in `ui`.
- PASS: pinned Rust 1.96.0 `cargo test --locked --all-targets -j 1` at root
  (338 tests including the new import/reopen fixture test).
- PASS: pinned Rust 1.96.0 `cargo test --locked -j 1` in `ui/src-tauri`
  (19 tests), and formatting checks in both crates.
- Initial concurrent Rust builds failed with resource/compiler errors, including
  `0xc000012d`, metadata errors and `STATUS_STACK_BUFFER_OVERRUN`. Single-job
  pinned retries passed. Supplemental 1.99.1 root and Tauri tests also passed.
- PASS: pinned Rust 1.96.0 `cargo clippy --locked --all-targets -j 1 -- -D warnings`
  in both crates, and `cargo build --locked -j 1` in `ui/src-tauri`.
  Final native captures use this pinned build. Earlier exploratory native runs
  used the supplemental 1.99.1 build.

UI regressions cover selection refusals, explicit candidate choice, exact ties,
search choice reset, Escape focus/selection, canceled asynchronous lookup, document
and selection drift, boundary and interior edits, invalid text rejection, original
mention identity through Undo/Redo, persistent warnings, deliberate Unlink,
link-only guards, Save then proceed, Discard, mismatch and unresolved saves.
Existing reducer/property tests cover joined history and Unicode transaction parity.

## Actual native evidence

The repeatable `ui/scripts/webview2-connect.mjs` imports the fixture via the real
`spike import-new` CLI into a newly reserved temporary database, launches the real
Tauri executable with WebView2 debugging, and operates its actual DOM and editor.
It does not substitute a browser mock or a fake bridge. Native shortcut checks
use synthetic DOM keyboard events through the application's bindings; they do
not prove physical keyboard routing or OS close events.

Windows reported `10.0.26300.0`. WebView2 reported Edg/156. Viewport was 1120x780
CSS pixels with devicePixelRatio 1. This is not a claim of other display scales.

PASS in this native run:

- Visible contextual action and anchored picker with all three Valdoria entities.
- Place/House disambiguation and suffixes for the two exact Place/Atlas ties.
- Link, manual Save, immediate text-drop notice, Ctrl+Z same ID/entity/range,
  and Ctrl+Y repeating the drop.
- Process termination and relaunch against the same synthetic file preserve the
  committed mention ID. Explicit reopen also retains it.
- Ctrl+K inspect, deliberate Unlink without an accidental-drop warning, and Save.
- Long-scene popup leaves prose width unchanged. Escape returns editor focus
  and the captured selection.
- A text-driven drop in the long scene leaves its notice inside the visible
  viewport. The refreshed long-scene screenshot shows that notice.
- `node scripts/webview2-parity.mjs` matches all 22 boundary and 37 reducer cases
  in real WebView2 with zero failures.

Screenshots and the machine-readable result are in
[`evidence/connect-native`](evidence/connect-native). Picker and inspection PNGs
were visually inspected. The relaunch probe terminates the process after the
aggregate was saved; it does not exercise the guarded window-close UX.

## Remaining human Windows acceptance

UNTESTED: physical Ctrl+K and keyboard-only operation, OS X and Alt+F4 link-only
close guard, IME composition, screen-reader announcements, varied display scale,
long-scene scrolled selection at screen edges, and subjective visual acceptance.
Native stale/Busy/uncertain multi-process interaction is not manually accepted;
the automated controller, session and real SQLite bridge suites test those paths.
No production backup, crash recovery, mobile or cross-platform claim is made.

1. Import `tests/fixtures/connect-smoke.json` with
   `cargo run --locked --bin spike -- import-new <new.sqlite> tests/fixtures/connect-smoke.json`.
2. Run the desktop and open that synthetic file. On the short and long scenes,
   select Valdoria, use each entry point, filter, choose House or one exact Place
   tie, and cancel with Escape. Check selection, focus, wrap and scroll retention.
3. Save, close normally, relaunch, inspect the same target, Unlink and keep typing.
4. Type before, at start, at end, inside and across the occurrence. Check the
   persistent notice and original ID restoration through Undo/Redo. Save/reopen.
5. Make only a link change. Exercise X and Alt+F4 with Continue, Save then proceed,
   and explicit Discard. Confirm the structured draft survives unresolved saves.
6. Use another instance for a stale revision. Confirm no false Saved or reload
   replaces the draft. Text copied to the clipboard must not imply link recovery.

Stop on silent link loss, identity replacement, stale text being linked, false
Saved or content loss. Do not relax a contract to complete acceptance.

## Independent review and corrections

GPT 6.1 Terra High independently reviewed `03c3f99` against `dceda4d`, the
approved decisions, and #33. Its verdict was REQUEST CHANGES for the drop
notice's promise that one Undo would restore a link even after later edits.
The button now says **Undo last change**, and the notice explains that restoring
links requires undoing recent edits. A new regression drops two links in separate
history events, makes another edit, and checks each successive Undo and exact
restored identities. All 108 UI tests pass.

The review also requested an accurate coordinate-fallback comment, now corrected,
and identified long-scene notice visibility as untested. The coordinator reran
the real WebView2 probe with that assertion and inspected the new screenshot.
The reviewer found no independently demonstrated hard stop-condition violation.
The coordinator also repeated both Rust test suites, UI checks, and frontend build.
The reviewer did not rerun tests or perform human native acceptance.

## Greptile P2 follow-up

The picker prepares captions, locale-normalized search text and exact-tie ID
suffixes once for each loaded entity index. Filtering, rendering and inspection
reuse those prepared rows. Caption groups and suffix counts replace repeated
full-index duplicate searches; entity order and stable IDs remain unchanged.

PASS: `npm run check`, `npm run desktop:check`, `npm test` (110 tests),
`npm run desktop:frontend:build`, and `git diff --check` for this follow-up.
Focused regressions cover exact captions, shortest distinguishing suffixes,
optional type/page labels, case normalization and index order. An instrumented
index verifies that repeated search input and Arrow navigation do not reread
source entity fields, while reopening prepares a newly fetched index. Existing
keyboard, pointer, Escape, stale-capture and history tests still pass.

Rust suites and native acceptance were not repeated for this picker-only change.
The earlier native evidence and the remaining human acceptance limits above are
unchanged.

## D3 exact-selection refinement

Valen/Ari's #35 acceptance finding `6666149588` approves inspection when one
selection exactly matches an existing mention, as well as a caret inside it.
One shared selection classifier now routes both Ctrl+K and the contextual action.
The action says **Inspect link** for either inspection case. Partial overlaps,
containing selections and multiple ranges still refuse without changing data.

PASS: `npm run check`, `npm run desktop:check`, `npm test` (113 tests),
`npm run desktop:frontend:build`, and `git diff --check`. Focused regressions cover
saved and draft exact selection, reversed selection, both entry points, caret
inspection, Escape focus/range, stale selection cancellation, refusal cases,
and unchanged mention identity and Undo/Redo. The unresolved-Busy fixture now
uses a changed reread revision so it actually exercises an ambiguous outcome.

PASS: extended `node scripts/webview2-connect.mjs` against the real desktop with
a disposable synthetic database and a separate temporary WebView2 profile.
After Save and process relaunch, an exact selection inspects the persisted link
through both entry points, keeps the same ID/entity/range and clean aggregate,
and returns focus and selection on Escape. Partial overlap refuses; selection
drift cancels. Earlier native save/history/Unlink assertions also passed.
See [`exact-selection-inspect.png`](evidence/connect-native/exact-selection-inspect.png)
and the refreshed `result.json`.

The first native attempt could not find a debugging target; using a separate
profile allowed the probe to run without closing the active acceptance app.
The native probe still uses injected keyboard events at 1120x780, DPR 1, Edg/156.
Physical Ctrl+K retesting and the remaining human acceptance steps above stay
pending. Rust/bridge code is unchanged and those suites were not repeated.
