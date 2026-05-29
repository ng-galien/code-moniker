# Repository Guidelines

## Project Structure & Module Organization

This is a Rust 2024 Cargo workspace with three crates:

- `crates/core`: pure Rust model, URI handling, declaration logic, and per-language extractors under `src/lang/`.
- `crates/cli`: the `code-moniker` binary, CLI parsing, formatting, rule checking, and integration tests in `crates/cli/tests/`.
- `crates/pg`: the `code-moniker-pg` pgrx PostgreSQL extension, SQL types, extractors, indexes, and extension metadata.

Supporting material lives in `docs/`, pgTAP tests in `pgtap/sql/`,
dogfood/regression tooling in `scripts/dogfood/`, and property-test
regressions in `proptest-regressions/`.

Use `bug/` for confirmed incorrect behavior with a minimal reproducer. Use
`evolutions/` for product or DX improvements that cannot yet be expressed
as executable code or check rules. When a finding, difficulty, or gotcha
looks repeatable, first try to consolidate it as a `code-moniker check`
DSL rule or sample rule before writing prose-only guidance.

## Build, Test, and Development Commands

- `cargo check --workspace --exclude code-moniker-pg --all-targets`: fast check for core and CLI.
- `cargo test --workspace --exclude code-moniker-pg`: run Rust unit and integration tests outside pgrx.
- `cargo fmt --all -- --check`: verify formatting.
- `cargo clippy --features pg17 --no-default-features --tests --no-deps -- -D warnings`: match CI linting, including pg-facing code.
- `cargo test --features pg17 --no-default-features --lib`: run CI-style library tests.
- `cargo pgrx install --manifest-path crates/pg/Cargo.toml --pg-config $HOME/.pgrx/17.9/pgrx-install/bin/pg_config`: install the extension locally.
- `./pgtap/run.sh`: run SQL extension tests after installing the extension.
- `cargo moniker-check`: run this project’s own agent guardrail rules.

## Validation Workflow

Prefer the narrowest validation that covers the files you changed. During
TDD, run focused tests first and only widen the gate when the behavior is
stable. Do not repeat the full workspace suite after every small edit.

Recent warm-cache timings on this repository are a useful order of
magnitude, not a contract: `fmt --check` ~0.4s, `cargo check` ~0.5s, all UI
tests ~1s, `cargo moniker-check` ~1s when release artifacts are hot but ~40s
after a release rebuild, `cargo test --features pg17 --no-default-features
--lib` ~2s, workspace tests ~5-6s, and `cargo install --path crates/cli`
~45s when it recompiles. Optimize for feedback latency: avoid cold
`cargo install` and repeated full workspace gates while iterating.
Run Cargo validations sequentially unless there is a concrete reason to do
otherwise: parallel `cargo check`/`cargo test`/`cargo clippy` commands contend
on the same build directory and make the feedback loop look slower than it is.
Treat `cargo moniker-check` as a targeted guardrail during iteration because
it can pay a cold release-build cost; prefer focused `code-moniker check
--file ...` calls when only a small touched surface needs architectural
validation.

Iteration loop examples:

- UI behavior: `cargo test -p code-moniker ui::tests::<test_name> --lib`,
  then `cargo test -p code-moniker ui::tests --lib` once the flow works.
- CLI behavior: `cargo test -p code-moniker --test cli_e2e <test_name>`.
- Extractor behavior: run the focused language unit test, then the relevant
  `cargo test -p code-moniker-core snapshot_<lang>` or conformance test.
- Agent guardrail rules: run `cargo moniker-check` only when touching UI/store
  boundaries, rules, imports, module organization, or before commit.

MCP behavior should be dogfooded against the Java multiproject fixture. Start
the TUI/MCP pair in tmux, then probe the streamable HTTP endpoint with compact
JSON-RPC calls:

```sh
tmux new-session -d -s cm-mcp-dogfood 'cargo run -p code-moniker -- ui crates/workspace/tests/fixtures/projects/java/multiprojet --mcp --mcp-port 33210'
tmux capture-pane -t cm-mcp-dogfood -p
curl -sS -X POST http://127.0.0.1:33210/mcp -H 'Content-Type: application/json' --data '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}'
curl -sS -X POST http://127.0.0.1:33210/mcp -H 'Content-Type: application/json' --data '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
curl -sS -X POST http://127.0.0.1:33210/mcp -H 'Content-Type: application/json' --data '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"code_moniker_read","arguments":{"uri":"workspace","depth":3,"limit":12}}}'
curl -sS -X POST http://127.0.0.1:33210/mcp -H 'Content-Type: application/json' --data '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"code_moniker_symbols","arguments":{"uri":"workspace","lang":"java","kind":"method","limit":5}}}'
curl -sS -X POST http://127.0.0.1:33210/mcp -H 'Content-Type: application/json' --data '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"code_moniker_symbols","arguments":{"uri":"workspace","action":"insights","lang":"java","limit":6}}}'
curl -sS -X POST http://127.0.0.1:33210/mcp -H 'Content-Type: application/json' --data '{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"code_moniker_rules","arguments":{"uri":"workspace","action":"list","lang":"java","limit":5}}}'
tmux kill-session -t cm-mcp-dogfood
```

The TUI capture should first show the file tree, then symbol/linkage completion.
RPC responses must expose LMNAV-shaped text with `uri`, `completeness`,
`summary`/`explorer` or `results`, and `next` when paging applies. Also check at
least one scoped read (`path` + `lang`), one cursor follow-up, one
`action:"insights"` call, and one `code_moniker_read` call against a symbol URI
returned by `code_moniker_symbols` before accepting MCP changes. For rules
changes, also run `code_moniker_rules action:"list"` and a bounded
`code_moniker_rules action:"run"` call.

Before code review, use a short gate:

- `cargo fmt --all -- --check`
- `cargo check --workspace --exclude code-moniker-pg --all-targets`
- the focused test group for the changed surface
- `cargo moniker-check` when architectural boundaries are involved

For documentation-only changes, use `git diff --check`; do not run Rust
builds unless the docs include generated examples that need verification.

Before commit on non-release work, run the full gate once after review fixes:
`cargo test --workspace --exclude code-moniker-pg --quiet`, `cargo clippy
--features pg17 --no-default-features --tests --no-deps -- -D warnings`, and
`cargo test --features pg17 --no-default-features --lib`. Run `./pgtap/run.sh`
only when `crates/pg`, SQL types, or extension behavior changed. Install the
binary with `cargo install --path crates/cli` only when CLI/TUI behavior changed
or when the user will test the installed executable.

## Refactoring Workflow

Start refactoring work by invoking the code-smell review agent/skill
(`$code-moniker-smell-review`) or by running the project smell profile:

```sh
code-moniker check <target> \
  --profile smells \
  --max-violations 50 \
  --report
```

Adopt smell rules in `.code-moniker.toml` one by one. Keep a new rule at
`severity = "warn"` during adoption, measure its volume with bounded output,
and record whether the signal is usable or noisy. A low volume does not mean
"fix now"; it only means the finding set is small enough to inspect later. A
high volume means analyze scope, thresholds, and justified exclusions instead
of deleting the rule.

The `agent` profile is the active guardrail. Do not leave a promoted
smell rule excluded from that profile. While a rule is still in adoption, keep
it out of the hook/guardrail path to save tokens and run it explicitly with
`--profile smells`. When a later refactoring pass targets a module, fix or
consciously suppress the relevant warnings until that module is green. Once
the module is clean and thresholds are credible, promote the rule in
`.code-moniker.toml` to `severity = "error"` and remove any profile exclusion
that would prevent the guardrail from seeing it.

Refactor in functional units. After a unit is complete, run the narrowest
meaningful validation plus the relevant smell check, then commit that unit
with a Conventional Commit message. Do not batch unrelated smell fixes into
one commit simply because they were discovered by the same review pass.

## Normal Workflow

During ordinary implementation, treat every surprising finding, recurring
difficulty, harness gotcha, or review comment as a candidate for executable
knowledge. Before adding prose to `AGENTS.md`, `CLAUDE.md`, or docs, ask
whether it can become:

- a project rule in `.code-moniker.toml` or a `code-moniker.fragment.toml`;
- a reusable sample in `docs/cli/check-samples/`;
- a missing DSL/operator evolution in `evolutions/`;
- a focused test that preserves the discovered behavior.

Prefer a DSL rule when the condition is structural and locally observable.
Use documentation only for judgment calls, process intent, or behavior that
the current DSL cannot express.

When running `code-moniker check` on a broad scope, always pass
`--max-violations <N>` before retrieving output in the agent context. For
full JSON analysis, redirect the output to a temporary file and inspect only
targeted summaries with tools such as `jq`; do not paste or capture the full
violation stream into the conversation.

When you believe development is complete but before committing, inspect the
impacted files symbolically with `code-moniker extract`/`code-moniker stats`
or an equivalent symbol outline. Verify that the ubiquitous language is
consistent with the module's domain vocabulary and that new or touched
functions have explicit, behavior-bearing names. Then launch an independent
review agent on the diff or touched module and address its actionable
findings before staging the commit.

## CI & Release Workflow

GitHub CI lives in `.github/workflows/ci.yml`. It runs on `main` pushes
and pull requests, installs PostgreSQL 17 + pgTAP, then executes
`cargo fmt --all -- --check`, `cargo clippy --features pg17
--no-default-features --tests --no-deps -- -D warnings`, `cargo test
--features pg17 --no-default-features --lib`, installs the pgrx
extension, runs `./pgtap/run.sh`, and finishes with `cargo moniker-check`.

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

Route user input through `ui::events` and typed `Msg` values, then through `AppState::reduce_ui_msg`. The reducer decides by updating state or emitting an app-level `Effect::RunCommand(AppCommand)`; `App` interprets those commands for workspace reads, navigation, clipboard, checks, and async work. Every runtime dispatch from `App` must drain and apply its returned effects through the shared dispatch helper; never call `app_store.dispatch(...)` and ignore the `Transition`. When a reducer must return information to its caller, use a typed reduction outcome instead of a captured local or an `App` flag. `ui::contracts` stays app-neutral: screens are render contracts, not input controllers, and effects belong to `ui::app`. Do not add ad hoc key handling inside render code. Treat `workspace::WorkspaceStore` as the moniker data boundary: UI code should consume workspace read models (`SymbolSummary`, `SymbolDetail`, reference groups, change summaries) instead of reaching into `SessionIndex`, `CodeGraph`, raw `DefRecord`/`RefRecord`, or Git diff internals. Shared UI state should move through the reusable reactive-store pattern (`dispatch` action, reducer transition, selector read) so refreshes can reconcile state instead of resetting navigation or panels.

Model durable shell state in `ui/app` before rendering it, but do not mirror workspace data there. `AppState` owns shell mode, navigation, check state, and async work epochs; `WorkspaceStore` owns moniker data and exposes selectors/read models. The work catalog names asynchronous candidates (`ProjectLoad`, `FileCatalog`, `GraphIndex`, `SearchIndex`, `GitOverlay`, `ImpactIndex`, `PanelData`, `CoverageIndex`) so lazy loading can schedule work explicitly without inventing fake `FileId`/`SymbolId` state. Add new cross-panel state there only when the UI must remember it independently of the workspace snapshot.

Keep terminal input, store notifications, and background task completions as distinct shell events. Filesystem or Git watcher events should enter through the shell event source and explicit store refresh methods, not as synthetic key events or render-side checks. Long-running UI work should be expressed as typed effects and task specs; use the UI runtime/Rayon bridge rather than blocking reducers or render functions.

Keep panel view-model construction with the feature that owns the user journey. Shared panel modules should define pure VM/rendering primitives; feature modules decide which panel VM is appropriate for their routes and modes. Use component markers as stable vocabulary for collaboration. They make feedback and bug reports unambiguous, but business behavior should not depend on rendered labels. Add focused tests for state transitions, route/effect behavior, and store boundaries; use `code-moniker extract`/`stats` plus `cargo moniker-check` to keep new UI modules understandable.

## Module Boundary Rules & Tests

For any module that belongs to an identifiable problem class, architecture
rules should first define the module boundary: what the module may consume,
what it may expose, what it owns, and what must stay outside. Tests should
exercise that boundary through the module's durable contract, not through the
current implementation structure.

Behavioral coverage should use black-box fixtures, corpus tests, snapshots,
or integration-style tests over stable public outputs: returned values,
diagnostics, persisted effects, emitted records, or other observable contract
results. Snapshot payloads must expose only the stable public model; do not
snapshot mutable state, wiring, helper types, cursors, transition objects, or
other accidental implementation details.

Robustness coverage should be separate from behavioral coverage and expressed
as property or fuzz tests over invariants: does not panic, terminates, rejects
without corrupting state, preserves a round-trip, stays within a bound, or
maintains a declared consistency property.

Tests that lock the current internal structure are disallowed when they
compete with the module's boundary contract. Internal tests are acceptable
only when they target a named sub-component whose local contract is itself
stable and intentionally exposed inside the module design. Treat that
sub-component as a smaller boundary with its own problem class, contract, and
rules.

Adopt boundary rules the same way as other guardrails: start at
`severity = "warn"`, inspect signal and noise, migrate code or tests toward
the contract, then promote to `severity = "error"` once the module is green
and the invariant is credible. Prefer executable `code-moniker check` rules
for observable boundaries, and use prose only for design judgment that the DSL
cannot yet express.

## Testing Guidelines

Choose the test shape from the module boundary. Use black-box fixtures,
corpus/snapshot tests, integration tests, or property tests when behavior is
observable through a durable contract. Place Rust unit tests next to the code
under `#[cfg(test)] mod tests` only for named sub-components with stable local
contracts.

Use `crates/cli/tests/` for CLI behavior. SQL surface coverage belongs in
numbered `pgtap/sql/*.sql` files and runs through `./pgtap/run.sh`. When
changing extractors, include focused fixtures under
`crates/core/tests/fixtures/` where useful and update proptest regressions
intentionally.

## Commit & Pull Request Guidelines

Recent history uses Conventional Commit style: `fix(ts): ...`, `test(go): ...`, `docs(changelog): ...`, `refactor(ts): ...`. Keep the scope short and meaningful. PRs should describe the behavioral change, mention affected languages or crates, link issues when available, and include test evidence such as `cargo test ...`, `cargo clippy ...`, or `./pgtap/run.sh`. Include screenshots only for documentation or visual asset changes.

## Security & Configuration Tips

Do not commit generated `target/` output, local dogfood clones, credentials, or machine-specific pgrx paths. Configuration examples should use `.code-moniker.toml` and documented defaults.
