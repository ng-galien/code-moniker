---
name: code-moniker
description: Explore and diagnose any codebase through code-moniker's symbolic index instead of manual file reading. ALWAYS use this before grep/Glob/Read exploration when the question involves architecture, module structure, coupling or dependencies between parts, call graphs, callers/callees, impact of a change, code smells, refactor targets, or project health — manual exploration produces impressions, code-moniker produces counted facts (coupling edges with counts, ranked violations) in fewer calls. Triggers on requests like "map/cartographie the architecture", "strongest couplings", "who calls X", "is this module healthy", "review this diff structurally". Zero configuration on any ts/rs/java/python/go/cs/sql project.
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
- `query` defaults to `--consistency stale-ok` and waits (bounded) for a
  loading daemon: first calls do not fail on state. Force freshness with
  `--consistency refresh-if-stale` when you just changed files.
- **Filter syntax**: `shape:callable`, or OR-combine with `shape:callable,type`
  / `shape:[callable,type]`. Unknown fields are parse errors with a
  suggestion — read them, they name the valid fields.
- **Always bound output**: `limit:` on queries, `--max-violations` on checks.
- **Anchor extraction on the root**: `code-moniker extract . --path <file>`,
  never `extract <file>` — anchoring on the file changes every moniker.
- Unresolved references are counted, never hidden. Treat the count as data
  (resolution coverage), not as an error.
