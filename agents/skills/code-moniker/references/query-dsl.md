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
- `shape:callable` — one bare word. **`shape:[callable]` parses but matches
  nothing** (silent empty result); repeat the field to OR values.
- `symbol.search` filters AND-combine: `name:"change" shape:callable path:"src/**"`.
- `name:` is substring; there is no `text:` field (an unknown field is
  ignored and the query returns everything — if a filter seems ignored,
  the field name is wrong).

## Identity prefixes

Segments are `kind:name` joined by `/`:
`srcset:main/lang:java/package:com/package:acme/module:Billing/class:Billing`.
Rust flavor: `lang:rs/dir:crates/module:lib/fn:parse(input:&str)`. An empty
prefix lists the roots. Full `code+moniker://` URIs are accepted anywhere a
prefix is and get normalized.

## Consistency and staleness

The CLI `query` has no consistency flag; a stale workspace answers
`workspace is stale; request consistency refresh-if-stale or stale-ok`.
Fix: restart the daemon with `--live-refresh auto`, or accept the hint and
use a client that passes consistency (the MCP tools and the VS Code
extension use `stale_ok`).

## Chaining pattern

Search hands you URIs; everything else consumes them:

```sh
uri=$(code-moniker query 'symbol.search name:"PaymentService" shape:type limit:1' \
      | grep -o 'code+moniker://[^ ]*' | head -1)
code-moniker query "symbol.usages uri:\"$uri\" limit:20"
code-moniker query "symbol.graph focus:\"$uri\""
```
