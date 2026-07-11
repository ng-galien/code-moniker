---
name: code-moniker
description: Explore and understand any codebase symbolically, and diagnose architecture, dependency or refactoring problems, using code-moniker (CLI, workspace daemon queries, MCP tools). Use when mapping an unfamiliar repository, tracing callers/callees of a symbol, measuring coupling between modules, finding code smells and refactor hotspots, reviewing git changes as symbol-level facts, or auditing declared dependencies. Works on any project in its supported languages (ts, rs, java, python, go, cs, sql) with zero configuration.
---

# code-moniker

code-moniker builds a symbolic index of a codebase: every definition gets a
stable moniker URI (`code+moniker://./lang:ts/dir:src/module:api/fn:save(x)`)
and every reference (calls, uses_type, extends, imports…) is a fact linking
two monikers. You navigate structure and relations instead of grepping text —
and you get counts, not impressions.

## Pick a surface

| You have | Use | Notes |
|---|---|---|
| Just the binary | `code-moniker <cmd>` | `stats`, `extract`, `check`, `diff`, `manifest`, `ui` — no daemon needed |
| A running daemon | `code-moniker query '<verb> …'` | richest navigation: identity tree, scope graphs, ego graphs, usages |
| MCP tools wired | `code_moniker_read/symbols/usages/graph/rules/diff` | same data, agent-shaped output with ready-made follow-up calls |

Start the daemon once per workspace (background it):
`code-moniker daemon start --live-refresh auto <root>`. Check health anytime
with `code-moniker daemon status <root>` (shows staleness, counts, and the
query verbs this daemon supports).

## Quick start on an unknown repo

```sh
code-moniker stats .                                   # census: files/defs/refs per language, in ms
code-moniker query 'identity.children prefix:""'       # symbolic roots (lang:*, srcset:*)
code-moniker query 'identity.graph prefix:"lang:ts"'   # module map: coupling edges with counts
code-moniker query 'symbol.search name:"<Name>" limit:10'
code-moniker query 'symbol.graph focus:"<URI from search>"'  # callers < / callees > / members
```

Then go by need:

- **Understand code, trace flows, find entry points** → `references/explore.md`
- **Health check, coupling, smells, refactor targets, dependency audit** → `references/diagnose.md`
- **Full query verb grammar and syntax traps** → `references/query-dsl.md`
- **Working through the MCP tools** → `references/mcp.md`

## Rules that save you a failed call

- **Never guess a URI or a focus path.** Get URIs from `symbol.search` /
  `code_moniker_symbols` and paste them exactly; a guessed one returns
  `symbol_not_found` / `focus_not_found`.
- **`workspace is stale`** on `query` → the daemon needs a refresh cycle:
  `code-moniker daemon stop <root>` then restart with `--live-refresh auto`.
- **Filter syntax**: `shape:callable`, or OR-combine with `shape:callable,type`
  / `shape:[callable,type]`. Unknown fields are parse errors with a
  suggestion — read them, they name the valid fields.
- **Always bound output**: `limit:` on queries, `--max-violations` on checks.
- **Anchor extraction on the root**: `code-moniker extract . --path <file>`,
  never `extract <file>` — anchoring on the file changes every moniker.
- Unresolved references are counted, never hidden. Treat the count as data
  (resolution coverage), not as an error.
