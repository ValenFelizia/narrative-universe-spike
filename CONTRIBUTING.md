# Contributing to Narrative Universe

Thanks for taking an interest in Narrative Universe. The repository currently exists to validate a small Rust/SQLite worldbuilding domain, **not** to implement the full desktop product.

## Before making changes

- Start from an approved, scoped issue or discuss the proposed change before coding.
- Work on a focused branch and submit a pull request. Do not treat the next planned increment as automatically approved.
- Keep unrelated refactors, new features, and dependency upgrades out of the same PR.
- Do not merge your own work or begin the next increment unless the project maintainers have explicitly authorized it.

The current architecture is provisional: one Rust crate, explicit SQL through `rusqlite`, synchronous operations, and no UI, MCP server, ORM, or generic repository framework.

## Conventions

Keep these rules small and consistent across human and AI contributors:

| Area | Convention |
| --- | --- |
| **Commits** | [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/): `feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`, or `ci:`. Use `type(scope): short imperative summary` when a scope helps, e.g. `feat(store): support assertion archival`. Breaking changes require an explicit explanation in the commit body. |
| **Branches** | `feat/`, `fix/`, `docs/`, `test/`, `refactor/`, or `chore/`, followed by a GitHub issue number and a short kebab-case description: `feat/3-canon-lifecycle`, `fix/17-file-creation`. For small maintenance work without an issue, a short descriptive name is enough. |
| **Formatting** | **Rust:** use the pinned `rustfmt` via `cargo fmt` before committing; verify with `cargo fmt --check`. No Prettier or Bun is needed for the current Rust-only crate. |
| **Lint and tests** | Run `cargo clippy --locked --all-targets -- -D warnings` and `cargo test --locked --all-targets`, along with the formatting check. Tests must cover changed behavior and relevant regressions. |
| **Code style** | Follow idiomatic Rust (currently edition 2021), prefer typed IDs/enums, explicit `Result`-based error handling, and small straightforward functions. Avoid `unwrap`/`expect` on recoverable production paths; they are appropriate in tests. Avoid `unsafe`, new dependencies, and generic abstraction layers without a clear, reviewed need. |
| **Issues and PRs** | Work from an authorized GitHub issue, keep each PR focused, and link it with `Refs #3` or `Closes #3` **only when the PR actually completes the issue**. Never push implementation directly to `main`; stop at the agreed human review gate before merging. |
| **UI conventions** | **Not defined yet.** When a TypeScript/Tauri interface is approved, add separate TypeScript, formatting, and design-system rules then. Do not import an unrelated dashboard design guide into this headless spike. |

Keep commit titles and branch names in English. Prefer meaningful tests over enforcing naming or architectural patterns mechanically. A documentation-only PR does not need a Rust test run unless it changes executable examples or other behavior; report what was actually verified.

## Development environment and checks

Follow the pinned `rust-toolchain.toml` and `Cargo.lock`. Windows/MSVC is the first tested environment; bundled SQLite requires the Visual Studio C/C++ build tools. Other platforms have not been verified yet.

Run before requesting review:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

If a required command cannot run, state exactly why; do not report it as passed. When changing persistence, include tests against real SQLite files, including a close/reopen where relevant. Avoid mocks that hide database behavior.

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
