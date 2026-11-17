# Contributing to Narrative Universe

Thanks for taking an interest in Narrative Universe. The repository currently exists to validate a small Rust/SQLite worldbuilding domain, **not** to implement the full desktop product.

## Before making changes

- Start from an approved, scoped issue or discuss the proposed change before coding.
- Work on a focused branch and submit a pull request. Do not treat the next planned increment as automatically approved.
- Keep unrelated refactors, new features, and dependency upgrades out of the same PR.
- Do not merge your own work or begin the next increment unless the project maintainers have explicitly authorized it.

The current architecture consists of a synchronous Rust/SQLite core crate (`narrative-universe`, explicit SQL through `rusqlite`, `PRAGMA journal_mode = DELETE`, and no ORM or generic repository framework) plus a local Tauri v2 / TypeScript / CodeMirror 6 desktop shell (`ui/` and `ui/src-tauri/`).

## Conventions

Keep these rules small and consistent across human and AI contributors:

| Area | Convention |
| --- | --- |
| **Commits** | [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/): `feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`, or `ci:`. Use `type(scope): short imperative summary` when a scope helps, e.g. `feat(store): support assertion archival`. Breaking changes require an explicit explanation in the commit body. |
| **Branches** | `feat/`, `fix/`, `docs/`, `test/`, `refactor/`, or `chore/`, followed by a GitHub issue number and a short kebab-case description: `feat/3-canon-lifecycle`, `fix/17-file-creation`. For small maintenance work without an issue, a short descriptive name is enough. |
| **Formatting** | **Rust:** use the pinned `rustfmt` via `cargo fmt` in both the root crate and `ui/src-tauri`; verify with `cargo fmt --check`. **Whitespace/EOF:** keep `git diff --check` clean across the PR diff. |
| **Lint and tests** | Run `node scripts/verify.mjs` (same command run in `.github/workflows/ci.yml`): checks the pinned toolchain, `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, and `cargo test --locked --all-targets` in both `.` and `ui/src-tauri`, `npm test`, `npm run check`, `npm run desktop:check`, `npm run desktop:frontend:build` in `ui/`, WebView2 harness guardrails, and `git diff --check`. |
| **Code style** | Follow idiomatic Rust (currently edition 2021), prefer typed IDs/enums, explicit `Result`-based error handling, and small straightforward functions. Avoid `unwrap`/`expect` on recoverable production paths; they are appropriate in tests. Avoid `unsafe`, new dependencies, and generic abstraction layers without a clear, reviewed need. |
| **Issues and PRs** | Work from an authorized GitHub issue, keep each PR focused, and link it with `Refs #3` or `Closes #3` **only when the PR actually completes the issue**. Never push implementation directly to `main`; stop at the agreed human review gate before merging. |
| **UI conventions** | Follow `DESIGN.md` and `ui/DESKTOP.md`. Desktop native verification scripts (`ui/scripts/webview2-*.mjs`) must use `ui/scripts/webview2-harness.mjs` (`EnumWindows`, `waitPageReady`) instead of `Process.MainWindowHandle` or copied CDP plumbing. |

Keep commit titles and branch names in English. Prefer meaningful tests over enforcing naming or architectural patterns mechanically. A documentation-only PR does not need a full local build rerun unless it changes executable examples or behavior; report what was actually verified. See `AGENTS.md` for the rule-enforcement table.

## Development environment and checks

Follow the pinned `rust-toolchain.toml` (`1.96.0`), `Cargo.lock`, `ui/src-tauri/Cargo.lock`, and `ui/package-lock.json` (`npm ci` in `ui/`). Windows/MSVC is the first tested environment; bundled SQLite requires the Visual Studio C/C++ build tools.

Run the shared local/CI verification suite before requesting review:

```sh
node scripts/verify.mjs
```

For changes touching `ui/` or `ui/src-tauri/` desktop behavior, also build both debug binaries (`cargo build --locked --bin spike` and `cargo build --locked --manifest-path ui/src-tauri/Cargo.toml`) and run the local native WebView2 verification scripts sequentially:

```sh
node ui/scripts/webview2-parity.mjs
node ui/scripts/webview2-connect.mjs
node ui/scripts/webview2-reveal.mjs
node ui/scripts/webview2-context.mjs
```

If a required command cannot run, state exactly why; do not report it as passed. When changing persistence, include tests against real SQLite files, including close/reopen where relevant. Because the store uses `PRAGMA journal_mode = DELETE` (not WAL), write concurrency tests using deterministic two-connection interleaving or `#[cfg(test)]` rendezvous hooks (`TEST_HOOK`) rather than timing/barrier-only loops. Avoid mocks that hide database behavior.

## Domain and data rules

Preserve the meaning of the domain, not just the schema:

- A missing assertion, explicit `Unknown`, and boolean `false` are different.
- A singular property can still contain multiple competing hypotheses.
- Canon status and archived status are independent.
- A relationship is stored once and can be queried from both directions.
- IDs and provenance must survive reads and, once implemented, export/import.
- Narrative source material is evidence, **not** permission to invent canon. Use explicitly labeled synthetic fixtures when sources do not establish a fact.

A bug that risks silently overwriting or deleting data takes priority over feature work. Changes to serialization, schemas, file lifecycle, or concurrency deserve explicit regression tests and review.

## Documentation language

Write **documentation, code comments, contribution guidance, and PR descriptions in English**.

This does **not** change the language of the future user interface or of fictional source material. Keep serialized canon values (`ACORDADO`, `PROVISIONAL`, `ABIERTO`, `MISTERIO_INTENCIONAL`), test fixture text such as `al inicio`, fixed identifiers, and source quotations in their original form. Do not translate contract literals under the guise of translating documentation.

Keep the root README approachable and up to date; put detailed fixture-specific information in `tests/fixtures/README.md`. Document actual capabilities and measured results, not future features as if they already exist.

## Pull-request checklist

Describe:

1. **Why:** issue, bug, or approved increment and intended behavior.
2. **Scope:** changed files, notable choices, and what remains out of scope.
3. **Verification:** commands actually run, test results, and reproducible evidence where relevant.
4. **Risks:** possible data loss, migrations, external side effects, or known limitations.
5. **Review gate:** what decision is needed before merging or continuing.

Reviewers should inspect the actual diff and data integrity, not rely only on an agent's completion report. Avoid adding layers simply to satisfy stylistic preferences.

## AI-assisted development

AI agents may plan, implement, or review **within an explicitly authorized scope**. They must stop at the agreed gate, report assumptions, and surface unexpected architectural decisions instead of silently expanding the task. Prefer an independent review for changes to persistent data.

Do not automatically publish private Narrative Bible excerpts, project files, credentials, or real-universe content. Public examples should be synthetic or explicitly cleared for publication.

## Licensing

There is **no license selected yet**. Public visibility is not a license grant. If you are an external contributor, please discuss licensing and contribution terms with the maintainer before submitting a substantial patch.
