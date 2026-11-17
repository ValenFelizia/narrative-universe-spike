# Agent Instructions (`AGENTS.md`)

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`.csdd/decisions.md`](.csdd/decisions.md) before editing code.

## Core invariants

- **Work within explicit scope:** Start from an authorized issue or directive, branch from `main`, keep unrelated refactoring/dependencies out of the PR, and stop at the human review gate before merging.
- **SQLite mode:** The store uses `PRAGMA journal_mode = DELETE` (not WAL) and synchronous `rusqlite` calls. Snapshot overlap tests must use deterministic two-connection interleaving or `#[cfg(test)]` rendezvous hooks rather than timing/barrier loops.
- **Fail closed on stored corruption:** Do not replace column-validating lookups (`id`, `typeof(label) = 'text'`, `type_id`, `page_id`) with unvalidated `SELECT 1` / `SELECT COUNT(*)` checks. Never echo untrusted stored field text in error messages.
- **Native WebView2 scripts:** Use shared helpers in [`ui/scripts/webview2-harness.mjs`](ui/scripts/webview2-harness.mjs) (`EnumWindows`, `waitPageReady`, `cdpCommand`). Never use `Process.MainWindowHandle` (flaky on Windows 11) or copy-paste CDP/Vite launcher boilerplate.

## Rule-enforcement table

| Rule | Enforced by |
| --- | --- |
| Pinned Rust toolchain (`1.96.0`) and lockfiles (`Cargo.lock`, `ui/src-tauri/Cargo.lock`, `ui/package-lock.json`) | `scripts/verify.mjs` (`checkPinnedRustToolchain`, `--locked` flags) + `.github/workflows/ci.yml` (`npm ci`) |
| Root and `ui/src-tauri` formatting, `-D warnings` clippy, and `--all-targets` tests pass | `scripts/verify.mjs` + `.github/workflows/ci.yml` |
| UI unit tests, TypeScript checks (`check`, `desktop:check`), and Vite build pass | `scripts/verify.mjs` (`npm test`, `npm run check`, `npm run desktop:check`, `npm run desktop:frontend:build`) + `.github/workflows/ci.yml` |
| PR diff has no conflict markers, trailing whitespace, or EOF anomalies | `scripts/verify.mjs` (`git diff --check` + `--diff-base`) + `.github/workflows/ci.yml` |
| `ui/scripts/webview2-*.mjs` never use `MainWindowHandle` or duplicate CDP/Vite/`EnumWindows` harness | `scripts/verify.mjs` (`checkWebView2Scripts`) + `ui/scripts/webview2-harness.mjs` |
| `entity_context` outgoing/incoming SQL reads stay index-backed without `TEMP B-TREE` sorts | `tests/entity_context.rs` (`query_plans_and_scale_measurement_report`) |
| `entity_context` caps (128 groups, 500 assertions, 2 MiB wire JSON) hold under escaping and mixed variants without reading unbounded unprojected owner labels | `tests/entity_context.rs` + `ui/src-tauri/src/lib.rs` (`context_wire_bound_*`, `context_wire_size_helpers_*`, `core_conservative_has_archived_claims_allowance_*`) + `src/store.rs` (`require_bounded_definition_owner`) |
| `DELETE`-mode atomic snapshot isolation (`entity_context` / `import_snapshot`) releases locks without tearing or unbounded loops | `tests/entity_context.rs` (`synchronized_concurrent_writer_observes_atomic_snapshot_and_releases_read_lock`) + `src/store.rs` (`context_reader_overlaps_atomic_import_commit_with_zero_tearing_and_releases_lock`) |
| Native WebView2 bridge, DPI/viewport fit, persistence across relaunch, and UI layout hold end-to-end | Local `node ui/scripts/webview2-parity.mjs`, `webview2-connect.mjs`, `webview2-reveal.mjs`, `webview2-context.mjs` (plus human Windows acceptance for physical keyboard/IME and DPI scaling) |
| Stored IDs/values fail closed without echoing raw stored strings | `src/store.rs` (`stored_id_errors_do_not_echo_the_value`) + `tests/process_migration.rs` |
| Scope discipline, no speculative abstractions, and preservation of domain/validation semantics | Independent code review + human PR merge gate (`CONTRIBUTING.md`) |
