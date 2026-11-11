# R1-B facts-first Reveal acceptance

Baseline verified after `git fetch origin`: main d2e2523e3b40c81aae98cf3f62255c2e12b60960,
merged R1-A #40. Issue #41 is the implementation authority. The local checkout
was fast-forwarded from 326a327; existing untracked installed UI dependencies
were preserved. No worktree was created. No schema, exchange, migration or
new dependency change. No real lore was used.

## Automated checks

- PASS: `cargo test --locked --all-targets` (359 tests across all root suites, including the new
  real-file entity_reveal suite).
- PASS: `cargo clippy --locked --all-targets -- -D warnings`, `cargo fmt --check`.
- PASS: UI `npm test` (118 tests), `npm run check`, `npm run desktop:check`,
  `npm run desktop:frontend:build`.
- PASS: Tauri `cargo test --locked` (20 bridge tests), `cargo check --locked`,
  `cargo fmt --check`, and `cargo clippy --locked --all-targets -- -D warnings`.
- PASS: `git diff --check`.

The store tests exercise real persisted outgoing and incoming fixture facts,
alternatives, duplicate labels/definition IDs, text, false, recorded Unknown,
intentional mystery scope, unnamed property, absent source, zero outgoing
claims, independently archived assertions/definitions, deterministic ordering,
invalid/missing IDs, Busy, malformed rows, unchanged database bytes and reopen.
Bridge tests cover token changes, null/tagged values, IDs, canon, complete source
wire shape, Busy and sanitized corrupt-read errors. Frontend tests use deferred
invoke responses and verify clean/dirty and unsaved-link behavior, closure,
same-entity reopen, selection/document/page/universe drift, history preservation,
source text safety, pagination, disambiguation and expanded-source repositioning.
Existing Link/Unlink, save guards and Undo/Redo tests continue to pass.

## Native Windows/WebView2 evidence

PASS: `node ui/scripts/webview2-reveal.mjs` launches the actual debug Tauri
executable, connects to its own WebView2 CDP target and uses the real IPC/store.
It imports the unchanged `tests/fixtures/reveal-facts.json` through `spike
import-new` at a fresh temporary SQLite path. It never opens a v2 database.
Vite serves the native app's development assets; this is not a browser-only
prototype or a mocked invoke. Screenshots are WebView2 viewport captures from
that process. See [machine results](evidence/reveal-native/result.json).

- Actual inner viewports: 1120 x 780 and 800 x 600, DPR 1. Prose column stayed
  640 CSS pixels in both. The compact popup fits and scrolls inside the viewport.
- Mara: genuine member of -> Ashfall Order, PROVISIONAL, folio 3; alternative
  Tidewardens ABIERTO remains visible. Recorded source expansion also fits.
- Ashfall: incoming references retain Mara/Corin as subjects and member of as
  the predicate. No invented inverse.
- Before/after text edits persist through Save and reopen, retaining original
  mention IDs. Reads leave doc identity, selection, mentions and dirty state
  unchanged; Escape restores editor focus. Long-scene scrollTop 3429 remains
  stable while inspecting and closing.

Evidence: [Mara 800](evidence/reveal-native/mara-800.png),
[Mara 1120](evidence/reveal-native/mara-1120.png),
[Ashfall 800](evidence/reveal-native/ashfall-800.png),
[Ashfall 1120](evidence/reveal-native/ashfall-1120.png),
[source 800](evidence/reveal-native/source-800.png),
[source 1120](evidence/reveal-native/source-1120.png),
[long scene](evidence/reveal-native/long-scene-800.png).

## Independent review and remaining human acceptance

GPT-6.1 Terra High independently inspected the implementation, tests and issue.
Its initial REQUEST CHANGES identified three P2s: same-name identity lost on
loading facts, saved status parsed from user text, and expanded source overflow.
All three were corrected and regression-tested. No P1 finding was reported.
The initial reviewer did not run tests or obtain native evidence. A second
independent GPT-6.1 Sol High review returned APPROVE with no remaining P1/P2,
verified all three corrections and independently passed 55 desktop tests,
three store reveal tests, one bridge reveal test and diff whitespace checks.
Neither reviewer repeated native acceptance. Valen/Ari review remains required.

UNTESTED by a human: physical keyboard/IME, complete Tab/Shift+Tab traversal,
subjective readability at both sizes, physical Undo/Redo, and scroll gestures.
CDP-dispatched keys and CodeMirror transactions do not prove those checks.

Valen should import the fixture at a fresh path with:
`cargo run --locked --bin spike -- import-new <fresh.sqlite> tests/fixtures/reveal-facts.json`.
Run `npm run desktop:dev` in ui, open that synthetic database, and:

1. At both sizes, type unsaved prose outside Mara, Ctrl+K on Mara, Tab to Show
   facts and press Enter. Read the provisional relation and source; expand it.
2. Escape, resume typing, Undo/Redo and verify selection, original mention,
   scroll and both edits. Save/reopen and verify the same linked occurrence.
3. Inspect Ashfall and read the subject-first incoming references. Use Tab,
   Shift+Tab, Next claims and Escape; focus must not trap or jump unexpectedly.
4. Repeat with a long page, nonzero scroll and the library closed at 800 wide.
   Confirm no reflow, clipping, unexpected save, or navigation.

This is a point-in-time read, not live canon synchronization. Large entity
claim sets are fully projected in memory, while presentation is bounded to
four rows per section. R2 and source navigation remain deferred. Stop at the
single PR; no merge is authorized.
