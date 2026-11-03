# Headless CodeMirror 6 → Rust edit bridge

PA-I3 checkpoint A1. This package contains no UI: it proves that a real
CodeMirror 6 `ChangeSet` can be translated into ordered
`MentionEdit::ReplaceText` edits that the existing Rust reducer accepts, and
that save acknowledgements stay correct while the user keeps typing.

## Dependencies

Everything is pinned to an exact version; there are no floating ranges.

| Package | Version | Why |
| --- | --- | --- |
| `@codemirror/state` | `6.7.6` | The contract is defined by real `ChangeSet`/`Text` objects. Reimplementing them would test a copy instead of CodeMirror. |
| `typescript` | `5.9.3` | Type check only (`--noEmit`). |
| `@types/node` | `24.19.2` | Types for `node:test`, `node:assert`, and `node:fs` in the test runner. |

The tests run with Node 24 native TypeScript stripping (`node --test`), so no
bundler, transform, or test framework is added. The TypeScript compiler is used
only for the strict type check.

`@codemirror/state` is a dependency, not a dev dependency, because the adapter
imports it at runtime.

## Commands

```sh
cd ui
npm install
npm run check   # tsc --noEmit
npm test        # node --test test/*.test.ts
```

The shared contract fixtures live in `tests/fixtures/`. `bridge_cases.json`
(R1 to R9 and the text cases, plus R10 to R28 for link, unlink, ordered text
and link, and Unicode boundaries) is read by the TypeScript tests,
`tests/bridge_contract.rs` (the real store and the pure reducer), and the
desktop bridge tests (the real agreement check). `grapheme_boundaries.json`
pins the extended grapheme cluster boundaries both sides must agree on. Edit
each fixture once; every side reads the same file.

`node scripts/webview2-parity.mjs` (after `cargo build --locked` in
`src-tauri`) starts the desktop shell with remote debugging, imports
`src/mentions.ts` inside the real WebView2, and replays both fixtures there.
It exits non-zero on any disagreement.

## Contract summary

- `start_utf16 = fromB`
- `end_utf16 = fromB + (toA - fromA)` — the original removed length, never `toB`
- `inserted_text = inserted.toString()`
- Edits are applied in the order CodeMirror yields them, each in the
  coordinates produced by the previous edit.
- Every document length is checked against the Rust `u32` UTF-16 limit,
  including the ChangeSet's result and each intermediate body, by numeric
  accounting before the changed body is allocated.
- Only LF text is editable. CR, CRLF, NUL, and unpaired surrogates are refused
  before a transaction reaches Rust.
- `src/mentions.ts` mirrors the Rust reducer: the same exclusive
  boundaries, the same drop rules, the same cluster check on survivors (with
  `Intl.Segmenter` against `unicode-segmentation`), canonical order, and
  sorted removed ids. A session refuses to open on stored mentions it cannot
  validate.
- The session ledger is ordered `text(ChangeSet)`, `link(Mention)`, and
  `unlink(id)` entries. A session opened on a linked page, or one that links
  or unlinks, sends that ledger in order; a mention-free session keeps the
  composed net diff. Text entries record the ids they removed, and
  `pendingDrops` names the ones the draft still lacks.
- A mention id stays bound to its first entity for the whole open document,
  across saves, unlinks, and history. A link that rebinds it is refused.
- `src/mention-history.ts` puts the mention set in a CodeMirror state field
  and uses `invertedEffects`. The inverse of every transaction on a linked
  page is a `restore` of its whole starting mention set, applied after the
  inverse text change. Undo therefore returns each id with its entity and
  range, even when one transaction replaced linked text and relinked the
  same id elsewhere. A restore expands into ordinary unlink and link
  operations, and the last restore in a transaction decides the set. The
  desktop must record each transaction into the session first and drop any
  transaction the session refuses.
- The desktop still opens linked pages read-only until PA-I4/C1-B; the
  contract above is not yet used by visual code.
- Every recorded transaction is checked against the live draft, so net mode
  cannot compose an invalid intermediate text out of existence before the
  adapter sees it.
- An acknowledgement is accepted only when `requestId`, `sessionId`, and
  `pageId` all match the in-flight request and the body equals the sent
  snapshot, the title, and the complete mention set. Its revision must be
  exactly `expectedRevision + 1`, except when the whole sent aggregate
  (body, title, and mentions) equals the base: then the store wrote nothing
  and the unchanged revision is accepted. A body that returns to the base
  while a mention disappears (R9) is not aggregate-neutral, so it cannot be
  acknowledged from an unchanged revision.
- Revisions must be safe integers between `0` and `Number.MAX_SAFE_INTEGER`.
  `openSession` refuses anything else. A save is refused with
  `revision_exhausted` when the base is already at `Number.MAX_SAFE_INTEGER`,
  so the client never commits a token it cannot represent.
- A `busy` reply means the write transaction did not commit, but a reread
  cannot prove that the request committed: another writer can advance the
  revision while leaving the body identical (the R9 shape). So busy
  reconciliation only proves non-commit, when the page still holds the base
  body, title, and mention set at the base revision. That makes the save retryable with its whole
  ledger intact. Any other reread is an error. A busy path is never reported
  clean. Typing keeps the ambiguity, and a new save is refused until a reread
  settles it.
