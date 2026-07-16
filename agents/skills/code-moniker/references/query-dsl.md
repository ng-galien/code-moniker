# Query DSL — developer and dogfood reference

`code-moniker query '<verb> field:value …'` talks to the workspace daemon.
This is not the normal agent exploration path when MCP is available; use
`code_moniker_query` so output budgets, aliases and safety stay enforced.
Discover what a daemon supports with `code-moniker daemon status <root>`
(the `queries:` line) — a long-running daemon may predate newer verbs.

## Verbs

| Verb | Key fields | Returns |
|---|---|---|
| `query.describe` | `verb:` | live query capabilities, fields, defaults and projections |
| `workspace.status` | — | phase, counts, staleness |
| `identity.children` | `prefix:""` | one level of the identity tree (kind, name, def counts, URIs for defs) |
| `identity.graph` | `prefix:""` | that level as a graph: nodes, rolled-up edges (kinds + counts), ports_in/out, unresolved count |
| `symbol.search` | `name:`, `shape:`, `path:`, `limit:` | matching symbols with exact URIs |
| `symbol.detail` | `uri:`, `context_lines:` | one symbol + its source zone |
| `symbol.usages` | `uri:`, `limit:` | incoming references with kinds and locations |
| `symbol.graph` | `focus:`, `direction:`, `relation:`, `min_count:` | filtered ego view: members, internal edges, callers `<`, callees `>` |
| `symbol.insights` | `limit:` | languages, kinds, concentration |
| `tree.children` | `path:` | file-tree navigation |
| `rules.list` / `rules.check` | `profile:` | compiled rules / run a check |
| `rules.applicable` | `focus:`, `profile:` | applicable, ignored and potential rules with reasons |
| `change.review` | — | git changes as symbol facts |
| `change.context` | `focus:`, `profile:`, `max_items:` | bounded graph, notes, applicable rules, changes and suggested checks |
| `resolution.audit` | `prefix:` | quantified unresolved-reference causes and zones |
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
- `project field ...` requests only supported result fields. Use
  `query.describe verb:"<verb>"` to discover `project:` fields instead of
  guessing them.

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
the VS Code extension apply their own consistency policies; the advanced MCP
tool accepts the same inline `consistency:` field.

## Chaining pattern for engine development

The shell example below is for daemon/CLI testing only. Agent workflows use
MCP intent tools or a bounded `code_moniker_query` batch and never parse output
with grep.

```sh
uri=$(code-moniker query 'symbol.search name:"PaymentService" shape:type limit:1' \
      | grep -o 'code+moniker://[^ ]*' | head -1)
code-moniker query "symbol.usages uri:\"$uri\" limit:20"
code-moniker query "symbol.graph focus:\"$uri\""
```
