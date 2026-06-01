# CLI/MCP Single-Consumer Modules

## CLI Top-Level Modules

Modules from `crates/cli/src/lib.rs` consumed by exactly one other top-level module:

- `format` -> `extract`
- `perf` -> `ui`
- `ui` -> `ui_command`
- `views` -> `mcp`

Entrypoint modules with no internal top-level consumer outside `lib.rs` dispatch:

- `harness`
- `langs`
- `manifest`
- `mcp_command`
- `rules`
- `shapes`
- `stats`
- `ui_command`

Shared top-level modules:

- `args`
- `check`
- `color`
- `extract`
- `glob`
- `language_kinds`
- `mcp`
- `moniker_render`
- `page`
- `session`
- `tree`
- `workspace_index`

## MCP Top-Level Modules

Modules from `crates/cli/src/mcp/mod.rs` consumed by exactly one MCP top-level module:

- `lmnav` -> `server`

MCP modules with localized but multi-module use:

- `context` -> `server`, `tools`, `mcp_command`, tests
- `tools` -> `server`, tests

MCP runtime surface:

- `server::router` -> `mcp_command`

## MCP Tools Submodules

Tool implementation modules are registered by `tools/mod.rs`:

- `read`
- `rules`
- `symbols`
- `usages`

Shared MCP tool support:

- `scope` -> `read`, `rules`, `symbols`, `usages`, tests
