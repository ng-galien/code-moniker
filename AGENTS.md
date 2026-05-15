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

## Testing Guidelines

Place Rust unit tests next to the code under `#[cfg(test)] mod tests`; use `crates/cli/tests/` for CLI behavior. SQL surface coverage belongs in numbered `pgtap/sql/*.sql` files and runs through `./pgtap/run.sh`. When changing extractors, include focused fixtures under `crates/core/tests/fixtures/` where useful and update proptest regressions intentionally.

## Commit & Pull Request Guidelines

Recent history uses Conventional Commit style: `fix(ts): ...`, `test(go): ...`, `docs(changelog): ...`, `refactor(ts): ...`. Keep the scope short and meaningful. PRs should describe the behavioral change, mention affected languages or crates, link issues when available, and include test evidence such as `cargo test ...`, `cargo clippy ...`, or `./pgtap/run.sh`. Include screenshots only for documentation or visual asset changes.

## Security & Configuration Tips

Do not commit generated `target/` output, local dogfood clones, credentials, or machine-specific pgrx paths. Configuration examples should use `.code-moniker.toml` and documented defaults.
