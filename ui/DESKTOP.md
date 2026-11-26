# Windows desktop editor proof (PA-I3/A2-B1)

The Tauri shell opens an existing database path that the user enters
explicitly, lists persisted pages, and displays the selected page's exact
title and body. Editable page bodies can be saved manually with revision checks.
The shell does not create databases or edit titles. Use only a disposable synthetic database.

## Prerequisites

- Rust 1.96.0 with the `x86_64-pc-windows-msvc` target from
  `rust-toolchain.toml`.
- Visual Studio C++ build tools and a Developer PowerShell environment with
  `cl.exe` available.
- Node.js 24 and npm. The existing A1 tests use Node's native TypeScript support.
- Microsoft Edge WebView2 Runtime.

## Run

From a Developer PowerShell at the repository root:

```powershell
cargo run --locked --bin spike -- seed .\synthetic-universe.sqlite
cd ui
npm ci
npm run desktop:dev
```

In the app, enter the full path to the file just seeded (for example,
`C:\path\to\narrative-universe-spike\synthetic-universe.sqlite`) and choose
**Open universe**. Select a page, edit its body, and use **Save** or **Ctrl+S**.
To check persistence, close the window, start it again, re-enter that same path,
and select the page. A dialog protects unsaved changes during navigation,
opening another universe, and normal window closure. Pages with links,
unsupported text, or unsupported revisions stay read-only. If a save cannot
be confirmed or conflicts with another writer, copy the visible draft before
leaving. Unsaved drafts are not crash recovery and do not survive process or
computer termination. Do not point this prototype at private or
irreplaceable writing. To create another fixture, choose a new path that does
not exist and run `cargo run --locked --bin spike -- seed <path>`; `seed`
refuses to overwrite an existing file.

## Checks

From `ui/`:

```powershell
npm run desktop:check
npm run desktop:frontend:build
npm run desktop:build
```

The native app stores the open `Store` in Rust process state. SQLite is never
opened or queried from the webview. The editor uses the A1 adapter and
save-session contracts and writes through `Store::save_page_edits`.

## Codex workspace (PA-I6/KA1)

After opening any synthetic universe (including a universe with zero pages), use
the **Write / Codex** switcher in the top strip to open **Codex**:

- **Browse and inspect:** Search by label, type, primary document, or entity
  ID, filter by any custom type (including types with zero entities) or
  `Untyped`, and select an entity to inspect its type, optional read-only
  primary document metadata, persisted knowledge (`Facts`, `Referenced by`,
  closed `Archived`) via `Store::entity_context`, and progressive disclosures
  (`Identifier`, `Property details`, assertion provenance/IDs) for technical
  metadata.
- **Independent creation:** Choose **New type** or **New entity** (`Untyped` by
  default). Confirmed Codex creations commit directly to SQLite through
  `Store::create_entity_type` and `Store::create_entity`, independent of
  document **Save** or `Ctrl+S`. If a creation outcome cannot be confirmed,
  further creation stays locked until **Refresh Codex** verifies whether the
  row committed.
- **Safe round-trip with Write:** Switching to Codex and back preserves unsaved
  Write title and prose drafts, selection, undo history, and scroll position.
  `Ctrl+S` in Codex never saves a hidden Write draft, the window close guard
  continues protecting dirty drafts while Codex is open, and newly created Codex
  entities appear immediately in Write's Connect (`Ctrl+K`) picker without
  reopening the universe.

## Knowledge authoring in Codex (PA-I7/KA2-A)

Inside any loaded Codex entry:

- **New property (`Facts` header):** Create an entity-owned property (default)
  or a type-owned property (`All <Type> entities`, unavailable for `Untyped`
  entities) with a required nonblank label (1–120 Unicode scalars, no
  surrounding whitespace or control characters; invalid labels are rejected
  rather than trimmed), value kind (`Text`, `Yes / No`, or `Link to another
  entity`), and narrative cardinality (`Usually one` or `Multiple`). With
  **Record a first fact now** checked (default), property and first assertion
  commit atomically in one SQLite `IMMEDIATE` transaction; unchecking creates an
  intentionally empty property.
- **Add value (per active outgoing property):** Record additional text, explicit
  `Yes`/`No`, exact-ID entity reference (including self-reference), or explicit
  `Unknown` assertions with a visible **Provisional** canon default (all four
  canon states available) and optional collapsed **Source & scope** metadata.
  Competing claims on singular properties are preserved without replacing
  earlier claims.
- **Reconciliation & bounds:** Uncertain writes lock KA1 and KA2 creation
  controls until **Verify write** probes SQLite by primary key in one snapshot
  (independent of bounded `entity_context` limits) and fences the earlier
  operation ID so delayed writes cannot commit after confirmed absence. Partial
  or mismatched records keep session writes locked as an integrity anomaly
  without auto-repair or deletion. Newly authored facts appear immediately in
  Write's Reveal (`Show facts` / `Explore context`) on the next deliberate read.

For native verification beyond unit checks, build `spike` and the desktop
binary (`cargo build --locked --bin spike` and `cargo build --locked
--manifest-path ui/src-tauri/Cargo.toml`) and run `node
ui/scripts/webview2-authoring.mjs`, `webview2-codex.mjs`, `webview2-parity.mjs`,
`webview2-connect.mjs`, `webview2-reveal.mjs`, and `webview2-context.mjs`
sequentially from the repo root.
