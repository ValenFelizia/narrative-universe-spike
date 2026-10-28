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
