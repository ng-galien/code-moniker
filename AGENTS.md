# Repository Guidelines

## Structure

- Workspace: Rust 2024.
- `crates/core`: model, URI, language extractors.
- `crates/workspace`: workspace scan, graph, linkage, changes, glob.
- `crates/check`: rules engine — DSL, rule config/profiles, evaluation, suppression, scan pipeline. Produces structured reports.
- `crates/cli`: `code-moniker`, CLI, check rendering/dispatch, formatting, TUI/MCP.
- `crates/cli/tests/`: CLI integration tests.
- `docs/`: user and developer docs.
- `scripts/dogfood/`: dogfood/regression tooling.
- `proptest-regressions/`: property-test regressions.
- `bug/`: confirmed bugs with minimal reproducers.
- `evolutions/`: product/DX ideas not yet executable.
- Prefer executable knowledge: `.code-moniker.toml`, fragments, samples, tests.

## Commands

- Fast core/CLI check: `cargo check --workspace --all-targets`
- Core/CLI tests: `cargo test --workspace`
- Format gate: `cargo fmt --all -- --check`
- CI clippy: `cargo clippy --workspace --tests --no-deps -- -D warnings`
- Agent guardrail: `cargo moniker-check`

## Build Latency

- Default profile: `dev`.
- `dev`: `debug = false`, `debug-assertions = false`, `overflow-checks = false`, `panic = "abort"`, `incremental = true`, `codegen-units = 256`.
- Debug profile: `--profile dev-debug`.
- Cargo jobs: `.cargo/config.toml` `jobs = 10`.
- macOS linker: `/opt/homebrew/opt/lld/bin/ld64.lld`.
- Cache wrapper: disabled by default.
- Release speed profile: `release-lto`.
- Keep one Cargo command active.
- Keep feature/profile/target flags stable per session.
- Reuse warm command shapes.
- Avoid broad gates in tight loops.

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

When using `code-moniker extract` for project-local inspection, anchor on the
workspace root (`.`) and filter with `--path <file>`. Do not run
`code-moniker extract <file>` for this repo, because it changes the anchor
moniker and can produce symbol paths that differ from project/index checks.

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
- CLI/TUI install gate: `cargo install --path crates/cli --features tui,mcp --no-default-features` and check the exit code; `-q` piped through `tail` hides failures.
- VS Code extension gate (after touching `vscode-extension/`): `npm test` + `npm run compile` + `npm run test:integration`; UI claims require a webview ack or a browser-harness screenshot (`docs/vscode-testing.md`).

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

## TUI Verification

```sh
cargo install --path crates/cli --features tui,mcp --no-default-features
tmux kill-session -t cm-tui-debug
tmux new-session -d -s cm-tui-debug 'code-moniker ui <source-root>'
tmux resize-window -t cm-tui-debug -x 160 -y 45
tmux capture-pane -t cm-tui-debug -p
```

- Navigate with keys:
  - `tmux send-keys -t cm-tui-debug Enter`
  - `tmux send-keys -t cm-tui-debug Down Down Enter`
  - `tmux send-keys -t cm-tui-debug s R i s k P o l i c y`
  - `tmux send-keys -t cm-tui-debug Escape`
- Capture after each navigation step:
  - `tmux capture-pane -t cm-tui-debug -p`
- Verify visible contract:
  - header: `code-moniker [ui.header]`
  - navigator: `[ui.navigator]`
  - active panel: `[ui.panel.<name>]`
  - workspace counts: `files`, `defs`, `refs`
  - selected row marker: `>`
  - expanded row marker: `▾`
  - collapsed row marker: `▸`
- Verify file tree before symbols:
  - load catalog-only acceptance test when relevant.
  - expected: files present, `defs 0`, `refs 0`.
- Verify symbols after index:
  - expected: navigator title has nonzero `defs`.
  - expand to a file with symbols.
  - expected: visible symbol rows include kind, visibility, path/name.
- Verify module-only Rust files:
  - `code-moniker extract <source-root> --path <mod-file> --format json --max-symbols 20`
  - expected TUI row: `0 defs <N> reexports`.
- Verify search:
  - `tmux send-keys -t cm-tui-debug s <query>`
  - expected: mode `search`, navigator title `filtered`, selected symbol panel `outline`.
- Verify views:
  - `tmux send-keys -t cm-tui-debug v`
  - expected: panel `[ui.panel.views]`, tree markers `[vN]`.
  - `tmux send-keys -t cm-tui-debug v`
  - expected: panel returns to `overview`.
- Verify truncation:
  - always resize to `160x45` before final capture.
  - if text is still truncated, navigate to put the target row near center and recapture.
- Kill debug TUI after verification:
  - `tmux kill-session -t cm-tui-debug`

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
- Reusable example: executable scenario in `samples/` (`docs/check-scenarios.md`).
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
- CI gates: fmt, clippy, lib tests, `cargo moniker-check`.
- Release workflow: `.github/workflows/release.yml`.
- Release trigger: `v*.*.*` tag.
- Publish order: `code-moniker-core`, `code-moniker-workspace`, `code-moniker-check`, then `code-moniker`.
- After release: bump `[workspace.package]` version on `main`.
- Version suffix: no `-snapshot`.

## Style

- Formatting: `rustfmt`, `hard_tabs = true`.
- Extractor path: `crates/core/src/lang/<lang>/`.
- Extractor files: `mod.rs`, `kinds.rs`, `canonicalize.rs`, `strategy.rs`.
- Shared public APIs: `pub`.
- Module focus: one responsibility.
- Tests: CLI in `crates/cli/tests/`; extractor fixtures in `crates/core/tests/fixtures/`.
- Commits: Conventional Commit, short scope.
- PR evidence: command outputs relevant to changed surface.
- Screenshots: docs or visual assets only.

## Security

- Never commit `target/`.
- Never commit local dogfood clones.
- Never commit credentials.
- Config examples: `.code-moniker.toml` and documented defaults.
