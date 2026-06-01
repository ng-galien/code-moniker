# CLI/MCP Single-Consumer Modules

Status: handled on 2026-06-01.

## Folded Modules

These modules were single-consumer implementation details and have been moved
under their consumers:

- `crates/cli/src/format.rs` -> `crates/cli/src/extract/format.rs`
- `crates/cli/src/perf.rs` -> `crates/cli/src/ui/perf.rs`

The MCP `lmnav` module had only one helper for server error rendering. It was
folded into `crates/cli/src/mcp/server.rs`.

## Deliberately Top-Level

These were reviewed and kept as top-level modules because the module boundary
is intentional:

- `ui` stays top-level. `ui_command` is only the CLI dispatch surface; `ui`
  owns the TUI shell, app state, reducers, runtime, rendering, and workspace
  read model.
- `views` stays top-level. Its fragment says the view DSL must stay independent
  from MCP and TUI renderers, even though MCP is the current consumer.
- Entrypoint modules such as `harness`, `langs`, `manifest`, `mcp_command`,
  `rules`, `shapes`, `stats`, and `ui_command` stay top-level command modules.
- MCP `context` and `tools` stay separate because they are shared across server,
  tool implementations, command wiring, and tests.
- MCP tool implementation modules (`read`, `rules`, `symbols`, `usages`) stay
  registered by `tools/mod.rs`; `scope` remains shared support for those tools.

No remaining item in this note is an open refactoring task.
