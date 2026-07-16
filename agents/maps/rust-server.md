# Map — Rust Workspace (engine, daemon, CLI/TUI/MCP)

Operational map for agents working on the Rust side. The bootstrap
(`AGENTS.md`) holds the everyday commands and gates; this map holds the
detail you only need when acting on a specific surface.

## Cartography

- `crates/core`: model, URI/moniker, language extractors (`src/lang/<lang>/`).
- `crates/workspace`: scan, graph, linkage, changes, snapshot views, glob.
- `crates/check`: rules engine — DSL, profiles, evaluation, suppression, reports.
- `crates/query`: query/verb layer — `Query`/`QueryResult` DTOs, text parser, formatters, JSON schema source.
- `crates/daemon`: workspace daemon — handlers, incremental refresh, registry (`$TMPDIR/code-moniker-daemons/*.json`).
- `crates/daemon-client`: client for CLI/extension-side daemon access.
- `crates/cli`: `code-moniker` binary — check rendering, formatting, TUI (`src/ui/`), MCP (`src/mcp/`).
- Schema flow: `crates/query` → `docs/schema/daemon.schema.json` → `vscode-extension/src/daemon/generated.ts` (`npm run generate:daemon-types`).

## Build Latency

- Default profile `dev`: `debug = false`, `debug-assertions = false`, `overflow-checks = false`, `panic = "abort"`, `incremental = true`, `codegen-units = 256`.
- Debug profile: `--profile dev-debug`. Release speed: `release-lto`.
- Cargo jobs: `.cargo/config.toml` `jobs = 10`; macOS uses the system linker so CI and local builds share a portable configuration.
- Keep one Cargo command active; keep feature/profile/target flags stable per session; reuse warm command shapes; avoid broad gates in tight loops.

## Extract Anchoring

Anchor project-local inspection on the workspace root and filter:
`code-moniker extract . --path <file>`. Never `code-moniker extract <file>`
here — it changes the anchor moniker and produces symbol paths that differ
from project/index checks.

## MCP Probes

After rebuilding or restarting `cm-mcp`, validate the surface:

- TUI capture: file tree, then symbol/linkage completion.
- MCP text: `uri`, `completeness`, `summary`/`explorer` or `results`; partial
  pages expose an optional `next` cursor call.
- Compact contract: default responses may declare response-local `@N` moniker
  aliases in descriptive data; generated calls must retain canonical URIs.
  Verify `compact:false` returns canonical verbose output and pagination keeps
  that mode.
- Budget contract: every non-refresh tool defaults to `budget:"small"`; an
  explicit `max_chars` is a hard ceiling and reports `truncated_by:max_chars`.
- Parity probes: `code_moniker_query` with `query.describe`, a two-query batch
  sharing one alias table, and `code_moniker_context` with facts, coverage and
  canonical suggested checks.
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

- Navigate with keys, capture after each step:
  - `tmux send-keys -t cm-tui-debug Enter`
  - `tmux send-keys -t cm-tui-debug Down Down Enter`
  - `tmux send-keys -t cm-tui-debug s R i s k P o l i c y`
  - `tmux send-keys -t cm-tui-debug Escape`
- Visible contract: header `code-moniker [ui.header]`, navigator `[ui.navigator]`,
  active panel `[ui.panel.<name>]`, workspace counts `files`/`defs`/`refs`,
  row markers `>` (selected), `▾` (expanded), `▸` (collapsed).
- File tree before symbols: catalog-only acceptance load shows files with `defs 0`, `refs 0`.
- Symbols after index: navigator title has nonzero `defs`; expanded file rows show kind, visibility, path/name.
- Module-only Rust files: `code-moniker extract <source-root> --path <mod-file> --format json --max-symbols 20`; expected TUI row `0 defs <N> reexports`.
- Search: `tmux send-keys -t cm-tui-debug s <query>` → mode `search`, navigator `filtered`, panel `outline`.
- Views: `v` → panel `[ui.panel.views]`, tree markers `[vN]`; `v` again → back to `overview`.
- Truncation: always resize to `160x45` before the final capture; if still truncated, center the target row and recapture.
- Kill the debug TUI afterwards: `tmux kill-session -t cm-tui-debug`.

## TUI Architecture

- Shell owns: terminal loop, layout, routing, navigation registry, effects.
- Features own: navigation entries, commands, routes, screens, panel VMs.
- Input path: `ui::events` → `Msg` → `AppState::reduce_ui_msg`; reducer output is a typed outcome.
- Runtime path: `Effect::RunCommand(AppCommand)` → `App` → shared dispatch helper.
- Contracts: render contracts only. Workspace boundary: `workspace::WorkspaceStore`.
- UI data: workspace read models only. Shell state: `AppState`.
- Async catalog: `ProjectLoad`, `FileCatalog`, `GraphIndex`, `SearchIndex`, `GitOverlay`, `ImpactIndex`, `PanelData`, `CoverageIndex`.
- Long work: typed effects and task specs. Component markers: collaboration vocabulary only.
- Tests: state transitions, route/effect behavior, store boundaries.

## Boundaries & Tests

- Define a boundary as: consumes, exposes, owns, excludes. Test through the durable contract.
- Behavioral tests: black-box fixtures, corpus, snapshots, integration tests.
- Snapshot payloads: stable public model only. Robustness: properties/fuzz invariants.
- Internal tests: named stable sub-component only.
- Boundary rules: start `warn`, inspect, migrate, promote `error`.

## Daemon Debugging

- Registry: `$TMPDIR/code-moniker-daemons/*.json` (endpoint, pid, workspace roots).
- Probe over WebSocket JSON-RPC with the extension's exact wire shape — see the daemon-probing recipe in `agents/maps/vscode-extension.md`.
- Check handshake capabilities, not the version string: a long-running daemon can predate a query verb while reporting the same version.
- Every open project registers its own daemon; a stale one in another workspace reproduces "works here, fails there".
