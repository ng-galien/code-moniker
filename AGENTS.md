# Repository Guidelines

## Project Structure & Module Organization

This is a Rust 2024 Cargo workspace with three crates:

- `crates/core`: pure Rust model, URI handling, declaration logic, and per-language extractors under `src/lang/`.
- `crates/cli`: the `code-moniker` binary, CLI parsing, formatting, rule checking, and integration tests in `crates/cli/tests/`.
- `crates/pg`: the `code-moniker-pg` pgrx PostgreSQL extension, SQL types, extractors, indexes, and extension metadata.

Supporting material lives in `docs/`, pgTAP tests in `pgtap/sql/`, dogfood/regression tooling in `scripts/dogfood/`, and property-test regressions in `proptest-regressions/`.

Use `bug/` for confirmed incorrect behavior with a minimal reproducer. Use `evolutions/` for product or DX improvements discovered while dogfooding, especially ESAC harness feedback that is not a strict correctness bug. When working from another repository such as ESAC, do not implement code-moniker changes opportunistically; only deposit bugs or evolutions here for the code-moniker agent to handle.

## Build, Test, and Development Commands

- `cargo check --workspace --exclude code-moniker-pg --all-targets`: fast check for core and CLI.
- `cargo test --workspace --exclude code-moniker-pg`: run Rust unit and integration tests outside pgrx.
- `cargo fmt --all -- --check`: verify formatting.
- `cargo clippy --features pg17 --no-default-features --tests --no-deps -- -D warnings`: match CI linting, including pg-facing code.
- `cargo test --features pg17 --no-default-features --lib`: run CI-style library tests.
- `cargo pgrx install --manifest-path crates/pg/Cargo.toml --pg-config $HOME/.pgrx/17.9/pgrx-install/bin/pg_config`: install the extension locally.
- `./pgtap/run.sh`: run SQL extension tests after installing the extension.
- `cargo arch-check`: run this project’s own architecture rules.

## Validation Workflow

Prefer the narrowest validation that covers the files you changed. During
TDD, run focused tests first and only widen the gate when the behavior is
stable. Do not repeat the full workspace suite after every small edit.

Recent warm-cache timings on this repository are a useful order of
magnitude, not a contract: `fmt --check` ~0.4s, `cargo check` ~0.5s, all UI
tests ~1s, `cargo arch-check` ~1s when release artifacts are hot but ~40s
after a release rebuild, `cargo test --features pg17 --no-default-features
--lib` ~2s, workspace tests ~5-6s, and `cargo install --path crates/cli`
~45s when it recompiles. Optimize for feedback latency: avoid cold
`cargo install` and repeated full workspace gates while iterating.

Iteration loop examples:

- UI behavior: `cargo test -p code-moniker ui::tests::<test_name> --lib`,
  then `cargo test -p code-moniker ui::tests --lib` once the flow works.
- CLI behavior: `cargo test -p code-moniker --test cli_e2e <test_name>`.
- Extractor behavior: run the focused language unit test, then the relevant
  `cargo test -p code-moniker-core snapshot_<lang>` or conformance test.
- Architecture rules: run `cargo arch-check` only when touching UI/store
  boundaries, rules, imports, module organization, or before commit.

Before code review, use a short gate:

- `cargo fmt --all -- --check`
- `cargo check --workspace --exclude code-moniker-pg --all-targets`
- the focused test group for the changed surface
- `cargo arch-check` when architectural boundaries are involved

For documentation-only changes, use `git diff --check`; do not run Rust
builds unless the docs include generated examples that need verification.

Before commit on non-release work, run the full gate once after review fixes:
`cargo test --workspace --exclude code-moniker-pg --quiet`, `cargo clippy
--features pg17 --no-default-features --tests --no-deps -- -D warnings`, and
`cargo test --features pg17 --no-default-features --lib`. Run `./pgtap/run.sh`
only when `crates/pg`, SQL types, or extension behavior changed. Install the
binary with `cargo install --path crates/cli` only when CLI/TUI behavior changed
or when the user will test the installed executable.

## CI & Release Workflow

GitHub CI lives in `.github/workflows/ci.yml`. It runs on `main` pushes
and pull requests, installs PostgreSQL 17 + pgTAP, then executes
`cargo fmt --all -- --check`, `cargo clippy --features pg17
--no-default-features --tests --no-deps -- -D warnings`, `cargo test
--features pg17 --no-default-features --lib`, installs the pgrx
extension, runs `./pgtap/run.sh`, and finishes with `cargo arch-check`.

The release workflow is `.github/workflows/release.yml` and only starts
when a `v*.*.*` tag is pushed. It verifies the tag matches the crates.io
package versions, then publishes `code-moniker-core` before
`code-moniker`. `code-moniker-pg` is versioned with the workspace for
extension metadata but is not auto-published (`publish = false`). Pushing
`main` is a CI action; pushing a version tag such as `v0.3.0` is the
release action. After a release, bump `main` to the next planned Cargo
version in `[workspace.package]`; do not use a `-snapshot` suffix.

## Coding Style & Naming Conventions

Use `rustfmt`; this repo sets `hard_tabs = true` in `rustfmt.toml`. Keep modules focused by responsibility. Language extractors follow `crates/core/src/lang/<lang>/` with files such as `mod.rs`, `kinds.rs`, `canonicalize.rs`, and `strategy.rs`. Public APIs shared with CLI or PG crates must be `pub`, not `pub(crate)`.

## UI Architecture Working Posture

Treat `code-moniker ui` as a contract-driven TUI shell for code supervision, not as an IDE clone or a pile of ratatui widgets. Before changing UI behavior, identify whether the change belongs to the shell, a feature, a screen, an effect, or the data store.

Keep global concerns in the shell: terminal loop, layout, routing, navigation registry, and effect application. Feature code should declare navigation, commands, routes, and screens; it should not directly own terminal state, mutate global navigation, or bypass shell effects.

Route user input through `ui::events` and typed messages, then through screen handling and `Effect` application. Do not add ad hoc key handling inside render code. Treat `ui::store` as the first data port: UI code should ask the store for data instead of reaching into `SessionIndex` or future index engines directly.

Keep terminal input and store notifications as distinct shell events. Filesystem or Git watcher events should enter through the shell event source and explicit store refresh methods, not as synthetic key events or render-side checks.

Use component markers as stable vocabulary for collaboration. They make feedback and bug reports unambiguous, but business behavior should not depend on rendered labels. Add focused tests for state transitions, route/effect behavior, and store boundaries; use `code-moniker extract`/`stats` plus `cargo arch-check` to keep new UI modules understandable.

## Testing Guidelines

Place Rust unit tests next to the code under `#[cfg(test)] mod tests`; use `crates/cli/tests/` for CLI behavior. SQL surface coverage belongs in numbered `pgtap/sql/*.sql` files and runs through `./pgtap/run.sh`. When changing extractors, include focused fixtures under `crates/core/tests/fixtures/` where useful and update proptest regressions intentionally.

## Commit & Pull Request Guidelines

Recent history uses Conventional Commit style: `fix(ts): ...`, `test(go): ...`, `docs(changelog): ...`, `refactor(ts): ...`. Keep the scope short and meaningful. PRs should describe the behavioral change, mention affected languages or crates, link issues when available, and include test evidence such as `cargo test ...`, `cargo clippy ...`, or `./pgtap/run.sh`. Include screenshots only for documentation or visual asset changes.

## Security & Configuration Tips

Do not commit generated `target/` output, local dogfood clones, credentials, or machine-specific pgrx paths. Configuration examples should use `.code-moniker.toml` and documented defaults.
