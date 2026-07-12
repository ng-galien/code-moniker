# Repository Guidelines

## Structure

- Workspace: Rust 2024.
- `crates/core`: model, URI, language extractors.
- `crates/workspace`: workspace scan, graph, linkage, changes, glob.
- `crates/check`: rules engine — DSL, profiles, evaluation, suppression, reports.
- `crates/query`: query verbs, DTOs, text parser, JSON schema source.
- `crates/daemon` + `crates/daemon-client`: workspace daemon and its client.
- `crates/cli`: `code-moniker` binary — CLI, check rendering, TUI/MCP.
- `vscode-extension/`: VS Code extension (React webviews, daemon-backed trees).
- `docs/`: reference documentation for humans.
- `agents/`: canonical agent layer — see Agent Layer below.
- `samples/`, `scripts/dogfood/`, `proptest-regressions/`, `bug/`, `evolutions/`: executable knowledge, dogfood tooling, regressions, reproducers, ideas.
- Prefer executable knowledge: `.code-moniker.toml`, fragments, samples, tests.

## Agent Layer

- `agents/` is the single source of truth: `maps/` (operational maps), `skills/`, `hooks/`.
- `.claude/` and `.codex/` are projections: tool-specific wiring plus symlinks into `agents/`. Never edit through the symlinks' duplicates — there are none to edit.
- `docs/` is human reference; operational agent knowledge belongs in `agents/maps/`.
- Maps: `agents/maps/rust-server.md` (build latency, TUI verification/architecture, boundaries, MCP probes, daemon debugging), `agents/maps/vscode-extension.md` (cartography, ship routine, UI verification harnesses).

## Commands

- Fast core/CLI check: `cargo check --workspace --all-targets`
- Core/CLI tests: `cargo test --workspace`
- Format gate: `cargo fmt --all -- --check`
- CI clippy: `cargo clippy --workspace --tests --no-deps -- -D warnings`
- Agent guardrail: `cargo moniker-check`
- Build latency knobs (profiles, jobs, linker): `agents/maps/rust-server.md`. Keep one Cargo command active; keep feature/profile flags stable per session.

## Iteration

- Inspect symbols: `/Users/alexandreboyer/.cargo/bin/code-moniker extract . --path <file> --shape callable --limit 80`
- Inspect stats: `/Users/alexandreboyer/.cargo/bin/code-moniker stats <file>`
- Format touched Rust: `rustfmt --edition 2024 --config-path rustfmt.toml <files>`
- Check rules: `/Users/alexandreboyer/.cargo/bin/code-moniker check . --profile <name> --max-violations <N>`
- Symbolic diff: `/Users/alexandreboyer/.cargo/bin/code-moniker diff [A..B] .` (facts: moves/renames/bodies/refs; `docs/cli/diff.md`)
- Show rules: `/Users/alexandreboyer/.cargo/bin/code-moniker rules show . --profile <name>`
- DSL tests: `cargo test -p code-moniker check::expr --lib`
- UI tests: `cargo test -p code-moniker ui::tests::<test_name> --lib`
- CLI test: `cargo test -p code-moniker --test cli_e2e <test_name>`
- Extractor tests: focused language test, then relevant `cargo test -p code-moniker-core snapshot_<lang>`.
- Docs-only validation: `git diff --check`
- Always anchor `extract` on `.` with `--path <file>`, never on the file directly (anchor moniker drift — see `agents/maps/rust-server.md`).

## Review Gates

- Short gate:
  - `rustfmt --edition 2024 --config-path rustfmt.toml --check <touched-rust-files>`
  - focused test group
  - `/Users/alexandreboyer/.cargo/bin/code-moniker check . --profile agent --max-violations 50`
- Full non-release gate:
  - `cargo test --workspace --quiet`
  - `cargo clippy --workspace --tests --no-deps -- -D warnings`
  - `cargo test -p code-moniker --features mcp --no-default-features --lib`
  - `cargo test -p code-moniker --features tui,mcp --no-default-features --lib`
- MCP surface gate (after touching `crates/cli/src/mcp/`):
  - `cargo clippy -p code-moniker --features mcp --no-default-features --lib --no-deps -- -D warnings`
- TUI surface gate (after touching `crates/cli/src/ui/` or `crates/workspace/src/snapshot/`):
  - `cargo clippy -p code-moniker --features tui,mcp --no-default-features --lib --no-deps -- -D warnings`
  - visible-contract verification: `agents/maps/rust-server.md`.
- CLI/TUI install gate: `cargo install --path crates/cli --features tui,mcp --no-default-features` and check the exit code; `-q` piped through `tail` hides failures.
- VS Code extension gate (after touching `vscode-extension/`): `npm test` + `npm run compile` + `npm run test:integration`; UI claims require a webview ack or a browser-harness screenshot (`agents/maps/vscode-extension.md`).

## MCP Dogfood

- Session tmux: `cm-mcp`. Keep MCP up while working on this repo; treat it as the navigation compass.
- Structural questions (who calls X, module map, impact of a change, hierarchy of a type) go through code-moniker FIRST — load the `code-moniker` skill before the first such query of a session, never type query syntax from memory. Grep is for exact strings in files you already know.
- Rebuild and restart `cm-mcp` after MCP/TUI/CLI changes or crashes.
- Normal use: MCP client tools only (`code_moniker_read`, `code_moniker_symbols`, `code_moniker_usages`, `code_moniker_rules`). No JSON-RPC/curl for code exploration.
- Post-restart probes: `agents/maps/rust-server.md`.

```sh
tmux new-session -d -s cm-mcp 'cargo run -p code-moniker --features mcp --no-default-features -- mcp . --port 3210'
tmux capture-pane -t cm-mcp -p
```

## Refactoring

- Start with `$code-moniker-smell-review` or `code-moniker check <target> --profile smells --max-violations 50 --report`.
- Adopt smell rules one by one: new rules `warn`, promote to `error` after signal review and module cleanup, keep promoted rules visible to `agent`.
- Refactor by functional unit; validate each unit narrowly; commit each unit Conventional-style.

## Rules Knowledge

- Structural finding: prefer DSL rule.
- Reusable example: executable scenario in `samples/` (`docs/check-scenarios.md`).
- Missing DSL/operator: `evolutions/`.
- Behavior preservation: focused test.
- Broad check output: always pass `--max-violations <N>`.
- JSON analysis: redirect to temp file; inspect with `jq`.
- Pre-commit symbolic review: `code-moniker extract`/`stats`; independent review agent before staging.

## CI & Release

- CI workflow: `.github/workflows/ci.yml` — fmt, clippy, lib tests, `cargo moniker-check`.
- Release: `v*.*.*` tag → `.github/workflows/release.yml`; publish order `code-moniker-core`, `code-moniker-workspace`, `code-moniker-check`, `code-moniker`.
- After release: bump `[workspace.package]` version on `main`; no `-snapshot` suffix.

## Style

- Formatting: `rustfmt`, `hard_tabs = true`.
- Extractor path: `crates/core/src/lang/<lang>/` (`mod.rs`, `kinds.rs`, `canonicalize.rs`, `strategy.rs`).
- Shared public APIs: `pub`. Module focus: one responsibility.
- Tests: CLI in `crates/cli/tests/`; extractor fixtures in `crates/core/tests/fixtures/`.
- Commits: Conventional Commit, short scope. PR evidence: command outputs relevant to changed surface.
- Screenshots: docs or visual assets only.

## Security

- Never commit `target/`, local dogfood clones, or credentials.
- Config examples: `.code-moniker.toml` and documented defaults.
