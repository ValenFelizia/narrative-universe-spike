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

The shared contract fixture lives at `tests/fixtures/bridge_cases.json` and is
read by both the TypeScript tests and `tests/bridge_contract.rs`. Keep the two
sides in sync by editing the fixture once.

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
- Pages that already hold mentions are read-only in PA-I3.
- Mention-bearing pages send the ordered transaction ledger, never a composed
  net diff, because composition erases history that the reducer needs.
- Every recorded transaction is checked against the live draft, so net mode
  cannot compose an invalid intermediate text out of existence before the
  adapter sees it.
- An acknowledgement is accepted only when `requestId`, `sessionId`, and
  `pageId` all match the in-flight request and the body equals the sent
  snapshot. Its revision must be exactly `expectedRevision + 1`, except for a
  mention-free net request whose text did not change: that no-op may return
  the unchanged revision and is provably aggregate-neutral. Ledger-mode
  no-ops are not accepted from an unchanged revision, because the body can
  stay identical while a mention disappears; PA-I3 linked pages are read-only
  anyway, and A2 can add an explicit `changed` acknowledgement.
- Revisions must be safe integers between `0` and `Number.MAX_SAFE_INTEGER`.
  `openSession` refuses anything else. A save is refused with
  `revision_exhausted` when the base is already at `Number.MAX_SAFE_INTEGER`,
  so the client never commits a token it cannot represent.
- A `busy` reply means the write transaction did not commit, but a reread
  cannot prove that the request committed: another writer can advance the
  revision while leaving the body identical (the R9 shape). So busy
  reconciliation only proves non-commit, when the page still holds the base
  body at the base revision. That makes the save retryable with its whole
  ledger intact. Any other reread is an error. A busy path is never reported
  clean. Typing keeps the ambiguity, and a new save is refused until a reread
  settles it.
