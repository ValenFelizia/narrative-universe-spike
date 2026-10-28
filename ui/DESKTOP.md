# Read-only Windows desktop proof (PA-I3/A2-A)

The Tauri shell opens an existing database path that the user enters
explicitly, lists persisted pages, and displays the selected page's exact
title and body. It does not create a database or expose write commands. Use
only a disposable synthetic database.

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
**Open universe**. To check persistence, close the window, start it again, and
re-enter that same explicit path. Do not point this prototype at private or
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
opened or queried from the webview. The A1 adapter and save-session files
remain headless and are not wired to this shell.
`Store::open` uses a read-write SQLite connection even though this shell exposes
no write commands. Use only disposable synthetic data.
