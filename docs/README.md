# Documentation

The CLI bundles this documentation and the daemon schema. Read them offline,
without a checkout, network connection, or running daemon:

```sh
code-moniker docs
code-moniker docs cli/query.md
code-moniker docs cli/mcp.md
code-moniker docs schema/daemon.schema.json
```

`docs` lists bundled paths relative to this directory. `--json` returns the
inventory as an array of `{path, title}`, or a selected page as `{path, body}`.
The page body is the original file content from the binary's build; relative
links retain their repository meaning. Follow other bundled pages by their
listed paths; links to source code, samples, or external sites require those
resources separately. Unknown paths return exit code 2 and the index command.

This tree is organized as one path from discovery to exact reference. Start
with the product surface you want to use, follow its task guide, then open the
grammar or generated contract only when you need exact details. The ownership
and anti-duplication rules are recorded in the
[documentation system](design/documentation-system.md).

## Start by surface

| Surface | Discover and learn | Use | Reference |
| --- | --- | --- | --- |
| Project rules and architecture | `code-moniker rules learn` | [Check](cli/check.md) | [Rule DSL](cli/check-dsl.md) |
| Published workspace index | `code-moniker query 'query.describe'` | [Indexed Query DSL](cli/query.md) | [Daemon schema](schema/daemon.schema.json) |
| Agent exploration | MCP `tools/list` and the installed skill | [MCP tools](cli/mcp.md) | [Agent output boundary](design/agent-output-boundary.md) |
| Language vocabulary | `code-moniker langs`, `code-moniker shapes`, `rules learn languages` | [Extract](cli/extract.md) | [Languages and shapes](cli/langs.md) |
| Resident service | `code-moniker daemon list`, `daemon status` | [Workspace Daemon](daemon.md) | [Daemon schema](schema/daemon.schema.json) |
| TypeScript integration | Package exports and generated types | [`@code-moniker/client`](../packages/client/README.md) | [Generated client types](../packages/client/src/generated.ts) |
| VS Code | Activity bar and command palette | [VS Code extension](vscode-extension.md) | [Extension package README](../vscode-extension/README.md) |

## CLI command reference

| Command | Guide |
| --- | --- |
| `docs` | Offline documentation inventory and page reader (above) |
| `extract` | [Extract a symbol graph](cli/extract.md) |
| `stats` | [Extraction metrics](cli/stats.md) |
| `check` | [Run project rules](cli/check.md) |
| `rules` | [Rule lifecycle and learning](cli/check.md), [exact DSL](cli/check-dsl.md) |
| `diff` | [Symbol-level changes](cli/diff.md) |
| `manifest` | [Declared dependencies](cli/manifest.md) |
| `langs`, `shapes` | [Language and shape discovery](cli/langs.md) |
| `daemon` | [Workspace Daemon](daemon.md) |
| `query` | [Indexed Query DSL](cli/query.md) |
| `mcp` | [MCP agent tools](cli/mcp.md) |
| `agent` | [Agent integration, hooks, and CI](cli/agent.md) |

## Focused workflows

- [On-demand syntax tree](cli/mcp-syntax-tree.md)
- [Code smell review](cli/code-smell-review.md)
- [Executable `.cm.md` scenarios](check-scenarios.md)
- [Source groups and source sets](source-groups.md)
- [OpenTelemetry observability](observability.md)
- [Performance and reproduction](perf.md)
- [Release and distribution](release.md)

## Contracts and design

| Need | Page |
| ---- | ---- |
| Understand moniker URI grammar and matching | [Moniker URI](design/moniker-uri.md) |
| Understand MCP text/JSON selection, templates, and budgets | [Agent output boundary](design/agent-output-boundary.md) |
| Understand documentation ownership and navigation | [Documentation system](design/documentation-system.md) |
| Understand the Git runtime boundary | [Git runtime dependency](design/git-runtime-dependency.md) |

## Project

| Need | Page |
| ---- | ---- |
| Build, test, or add a language | [Contributing](../CONTRIBUTING.md) |
