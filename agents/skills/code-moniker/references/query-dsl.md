# Query DSL — verbs, fields, syntax traps

`code-moniker query '<verb> field:value …'` talks to the workspace daemon.
Discover what a daemon supports with `code-moniker daemon status <root>`
(the `queries:` line) — a long-running daemon may predate newer verbs.

## Verbs

| Verb | Key fields | Returns |
|---|---|---|
| `workspace.status` | — | phase, counts, staleness |
| `identity.children` | `prefix:""` | one level of the identity tree (kind, name, def counts, URIs for defs) |
| `identity.graph` | `prefix:""` | that level as a graph: nodes, rolled-up edges (kinds + counts), ports_in/out, unresolved count |
| `symbol.search` | `name:`, `shape:`, `path:`, `limit:` | matching symbols with exact URIs |
| `symbol.detail` | `uri:`, `context_lines:` | one symbol + its source zone |
| `symbol.usages` | `uri:`, `limit:` | incoming references with kinds and locations |
| `symbol.graph` | `focus:"<URI or rel path>"` | ego view: members, internal edges, callers `<`, callees `>` |
| `symbol.insights` | `limit:` | languages, kinds, concentration |
| `tree.children` | `path:` | file-tree navigation |
| `rules.list` / `rules.check` | `profile:` | compiled rules / run a check |
| `change.review` | — | git changes as symbol facts |
| `notes` | — | project notes surface |

## Field syntax

- Strings quoted: `prefix:"lang:ts/dir:src"`, `name:"ChangeService"`.
- Numbers bare: `limit:10`.
- Multi-value fields OR-combine: `shape:callable,type`, `shape:[callable,type]`
  (bracket list sugar on `lang`/`kind`/`shape`/`severity`), or repeat the field.
  No spaces inside an unquoted list; an unclosed `[` is a parse error.
- `symbol.search` filters AND-combine: `name:"change" shape:callable path:"src/**"`.
- Fields are validated per verb: an unknown field (e.g. `text:`) is a parse
  error with a suggestion (`did you mean \`name\`?`) or the valid-field list.

## Identity prefixes

Segments are `kind:name` joined by `/`:
`srcset:main/lang:java/package:com/package:acme/module:Billing/class:Billing`.
Rust flavor: `lang:rs/dir:crates/module:lib/fn:parse(input:&str)`. An empty
prefix lists the roots. Full `code+moniker://` URIs are accepted anywhere a
prefix is and get normalized.

## Consistency and staleness

A stale workspace answers
`workspace is stale; request consistency refresh-if-stale or stale-ok`.
Fix: add `consistency:refresh-if-stale` (or `consistency:stale-ok`) inline to
the query, or restart the daemon with `--live-refresh auto`. The MCP tools and
the VS Code extension pass `stale_ok` themselves.

## Chaining pattern

Search hands you URIs; everything else consumes them:

```sh
uri=$(code-moniker query 'symbol.search name:"PaymentService" shape:type limit:1' \
      | grep -o 'code+moniker://[^ ]*' | head -1)
code-moniker query "symbol.usages uri:\"$uri\" limit:20"
code-moniker query "symbol.graph focus:\"$uri\""
```
