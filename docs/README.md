# Documentation

This tree is organized by task. Start with the surface you want to use,
then drop into the reference pages only when you need exact grammar details.

## CLI

| Need | Page |
| ---- | ---- |
| Dump a symbol graph for a file or directory | [Extract](cli/extract.md) |
| Measure extraction coverage and scan time | [Stats](cli/stats.md) |
| List declared deps with their package moniker | [Manifest](cli/manifest.md) |
| Run architecture and naming rules | [Check](cli/check.md) |
| Write rule expressions | [Rule DSL](cli/check-dsl.md) |
| Use the VS Code workbench extension | [VS Code extension](vscode-extension.md) |
| Review local code smell warnings | [Code smell review](cli/code-smell-review.md) |
| Discover supported language tags, kinds, and shapes | [Discovery](cli/langs.md) |
| Install the agent skill, MCP, hooks, or connect `check` to CI | [Agent integration and hooks](cli/agent.md) |
| Request a bounded syntax tree through MCP or the TypeScript client | [On-demand syntax tree](cli/mcp-syntax-tree.md) |
| Run or query a resident workspace service | [Daemon](daemon.md) |
| Export daemon traces through OpenTelemetry | [Observability](observability.md) |

## Design

| Need | Page |
| ---- | ---- |
| Understand moniker URI grammar and matching | [Moniker URI](design/moniker-uri.md) |
| Understand MCP text/JSON selection, templates, and budgets | [Agent output boundary](design/agent-output-boundary.md) |

## Workspace

| Need | Page |
| ---- | ---- |
| Map non-standard production and test roots | [Source groups and source sets](source-groups.md) |

## Project

| Need | Page |
| ---- | ---- |
| Build, test, or add a language | [Contributing](../CONTRIBUTING.md) |
| Review benchmark numbers | [Performance](perf.md) |
