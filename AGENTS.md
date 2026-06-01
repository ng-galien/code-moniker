# Repository Guidelines

## Structure

- Workspace: Rust 2024.
- `crates/core`: model, URI, declarations, language extractors.
- `crates/workspace`: workspace scan, graph, linkage, changes.
- `crates/cli`: `code-moniker`, CLI, rules, formatting, TUI/MCP.
- `crates/pg`: `code-moniker-pg` pgrx extension.
- `crates/cli/tests/`: CLI integration tests.
- `docs/`: user and developer docs.
- `pgtap/sql/`: SQL extension tests.
- `scripts/dogfood/`: dogfood/regression tooling.
- `proptest-regressions/`: property-test regressions.
- `bug/`: confirmed bugs with minimal reproducers.
- `evolutions/`: product/DX ideas not yet executable.
- Prefer executable knowledge: `.code-moniker.toml`, fragments, samples, tests.

## Commands

- Fast core/CLI check: `cargo check --workspace --exclude code-moniker-pg --all-targets`
- Core/CLI tests: `cargo test --workspace --exclude code-moniker-pg`
- Format gate: `cargo fmt --all -- --check`
- CI clippy: `cargo clippy --features pg17 --no-default-features --tests --no-deps -- -D warnings`
- CI lib tests: `cargo test --features pg17 --no-default-features --lib`
- Install pg: `cargo pgrx install --manifest-path crates/pg/Cargo.toml --pg-config $HOME/.pgrx/17.9/pgrx-install/bin/pg_config`
- pgTAP: `./pgtap/run.sh`
- Agent guardrail: `cargo moniker-check`

## Build Latency

- Default profile: `dev`.
- `dev`: `debug = false`, `debug-assertions = false`, `overflow-checks = false`, `panic = "abort"`, `incremental = true`, `codegen-units = 256`.
- Debug profile: `--profile dev-debug`.
- Cargo jobs: `.cargo/config.toml` `jobs = 10`.
- macOS linker: `/opt/homebrew/opt/lld/bin/ld64.lld`.
- pgrx flags: `-undefined dynamic_lookup`.
- Cache wrapper: disabled by default.
- Release speed profile: `release-lto`.
- Keep one Cargo command active.
- Keep feature/profile/target flags stable per session.
- Reuse warm command shapes.
- Avoid broad gates in tight loops.

## Iteration

- Inspect symbols: `/Users/alexandreboyer/.cargo/bin/code-moniker extract <file> --shape callable --limit 80`
- Inspect stats: `/Users/alexandreboyer/.cargo/bin/code-moniker stats <file>`
- Format touched Rust: `rustfmt --edition 2024 --config-path rustfmt.toml <files>`
- Check rules: `/Users/alexandreboyer/.cargo/bin/code-moniker check . --profile <name> --max-violations <N>`
- Show rules: `/Users/alexandreboyer/.cargo/bin/code-moniker rules show . --profile <name>`
- DSL tests: `cargo test -p code-moniker check::expr --lib`
- UI tests: `cargo test -p code-moniker ui::tests::<test_name> --lib`
- CLI test: `cargo test -p code-moniker --test cli_e2e <test_name>`
- Extractor tests: focused language test, then relevant `cargo test -p code-moniker-core snapshot_<lang>`.
- Docs-only validation: `git diff --check`

## Review Gates

- Short gate:
  - `rustfmt --edition 2024 --config-path rustfmt.toml --check <touched-rust-files>`
  - focused test group
  - `/Users/alexandreboyer/.cargo/bin/code-moniker check . --profile agent --max-violations 50`
- Full non-release gate:
  - `cargo test --workspace --exclude code-moniker-pg --quiet`
  - `cargo clippy --features pg17 --no-default-features --tests --no-deps -- -D warnings`
  - `cargo test --features pg17 --no-default-features --lib`
- pg gate: `cargo pgrx install ...`, then `./pgtap/run.sh`.
- CLI/TUI install gate: `cargo install --path crates/cli`.

## MCP Dogfood

- Session tmux: `cm-mcp`.
- Keep MCP up while working on this repo.
- Use MCP to explore and analyze code.
- Treat MCP as the project navigation compass.
- Restart MCP after crashes or relevant rebuilds.
- Keep MCP updated after code changes.
- Rebuild and restart `cm-mcp` after MCP/TUI/CLI changes.
- Normal use: MCP client tools only.
- Main tools: `code_moniker_read`, `code_moniker_symbols`, `code_moniker_usages`, `code_moniker_rules`.
- No JSON-RPC/curl for code exploration.

```sh
tmux new-session -d -s cm-mcp 'cargo run -p code-moniker --features mcp --no-default-features -- mcp . --port 3210'
tmux capture-pane -t cm-mcp -p
```

- TUI capture: file tree, then symbol/linkage completion.
- MCP text: `uri`, `completeness`, `summary`/`explorer` or `results`.
- Paging: `next` when applicable.
- Required probes: scoped read, cursor follow-up, `action:"insights"`, symbol URI read.
- Rules probes: `action:"list"`, bounded `action:"run"`.

## Refactoring

- Start with `$code-moniker-smell-review` or:

```sh
code-moniker check <target> \
  --profile smells \
  --max-violations 50 \
  --report
```

- Adopt smell rules one by one.
- New smell rule severity: `warn`.
- Promote to `error` after signal review and module cleanup.
- Keep promoted rules visible to `agent`.
- Refactor by functional unit.
- Validate each unit narrowly.
- Commit each unit with Conventional Commit style.

## Rules Knowledge

- Structural finding: prefer DSL rule.
- Reusable example: `docs/cli/check-samples/`.
- Missing DSL/operator: `evolutions/`.
- Behavior preservation: focused test.
- Broad check output: always pass `--max-violations <N>`.
- JSON analysis: redirect to temp file; inspect with `jq`.
- Pre-commit symbolic review: `code-moniker extract`/`stats`.
- Diff review: independent review agent before staging.

## UI Architecture

- Shell owns: terminal loop, layout, routing, navigation registry, effects.
- Features own: navigation entries, commands, routes, screens, panel VMs.
- Input path: `ui::events` -> `Msg` -> `AppState::reduce_ui_msg`.
- Runtime path: `Effect::RunCommand(AppCommand)` -> `App` -> shared dispatch helper.
- Reducer output: typed outcome.
- Contracts: render contracts only.
- Workspace boundary: `workspace::WorkspaceStore`.
- UI data: workspace read models only.
- Shell state: `AppState`.
- Workspace data: `WorkspaceStore`.
- Async catalog: `ProjectLoad`, `FileCatalog`, `GraphIndex`, `SearchIndex`, `GitOverlay`, `ImpactIndex`, `PanelData`, `CoverageIndex`.
- Events: terminal input, store notifications, background completions.
- Long work: typed effects and task specs.
- Component markers: collaboration vocabulary only.
- Tests: state transitions, route/effect behavior, store boundaries.

## Boundaries & Tests

- Define boundary: consumes, exposes, owns, excludes.
- Test through durable contract.
- Behavioral tests: black-box fixtures, corpus, snapshots, integration tests.
- Snapshot payloads: stable public model only.
- Robustness tests: properties/fuzz invariants.
- Internal tests: named stable sub-component only.
- Boundary rules: start `warn`, inspect, migrate, promote `error`.

## CI & Release

- CI workflow: `.github/workflows/ci.yml`.
- CI gates: fmt, clippy, pg17 lib tests, pgrx install, pgTAP, `cargo moniker-check`.
- Release workflow: `.github/workflows/release.yml`.
- Release trigger: `v*.*.*` tag.
- Publish order: `code-moniker-core`, then `code-moniker`.
- `code-moniker-pg`: `publish = false`.
- After release: bump `[workspace.package]` version on `main`.
- Version suffix: no `-snapshot`.

## Style

- Formatting: `rustfmt`, `hard_tabs = true`.
- Extractor path: `crates/core/src/lang/<lang>/`.
- Extractor files: `mod.rs`, `kinds.rs`, `canonicalize.rs`, `strategy.rs`.
- Shared public APIs: `pub`.
- Module focus: one responsibility.
- Tests: CLI in `crates/cli/tests/`; SQL in `pgtap/sql/*.sql`; extractor fixtures in `crates/core/tests/fixtures/`.
- Commits: Conventional Commit, short scope.
- PR evidence: command outputs relevant to changed surface.
- Screenshots: docs or visual assets only.

## Security

- Never commit `target/`.
- Never commit local dogfood clones.
- Never commit credentials.
- Never commit machine-specific pgrx paths.
- Config examples: `.code-moniker.toml` and documented defaults.
