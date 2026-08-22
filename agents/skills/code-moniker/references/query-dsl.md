# Query DSL — developer and dogfood reference

`code-moniker query '<verb> field:value …'` talks to the workspace daemon.
This is not the normal agent exploration path when MCP is available; use
`code_moniker_query` so output budgets, compact monikers and safety stay enforced.
Discover what a daemon supports with `code-moniker daemon status <root>`
(the `queries:` line) — a long-running daemon may predate newer verbs.

Use `code-moniker daemon list` followed by
`code-moniker query --daemon <ENDPOINT> '<verb> ...'` to target an already
running daemon exactly. This direct selector never starts a replacement daemon
and never falls back to the filesystem. The same endpoint is accepted by
`daemon status --daemon` and `daemon stop --daemon`. For `rules.check`, the
rules file is loaded for the request and the sources/linkage come from the
selected daemon's reported index generation.

## Verbs

| Verb | Key fields | Returns |
|---|---|---|
| `query.describe` | `verb:` | live query capabilities, fields, defaults and projections |
| `workspace.status` | — | phase, counts, staleness |
| `identity.children` | `prefix:""`, `limit:` | one bounded level of the identity tree (kind, name, def counts, URIs for defs) |
| `identity.graph` | `prefix:""`, `path:`, `min_count:`, `limit:`, `cursor:` | path-selected level as a paginated graph: nodes, rolled-up edges, ports, coverage and unresolved count |
| `view.read` | `uri:"workspace/views"` or a returned `workspace/views/<view.id>` | view list or one view. The leaf is `[[views]] id`, never `fragment =`. See `fragments.md`. |
| `symbol.search` | `name:`, `shape:`, `path:`, `limit:` | matching symbols with exact URIs |
| `symbol.detail` | `uri:`, `context_lines:` | one symbol + its source zone |
| `syntax.tree` | `focus:`, `max_depth:`, `max_nodes:`, `named_only:`, `include_text:` | bounded on-demand Tree-sitter tree for a file or symbol |
| `symbol.usages` | `uri:`, `include_descendants:`, `limit:` | exact usages by default; optional owner roll-up across navigable descendants, with internal relations excluded |
| `symbol.graph` | `focus:`, `direction:`, `relation:`, `min_count:`, `limit:` | bounded ego view: members, internal edges, callers `<`, callees `>` |
| `graph.path` | `from:`, `to:`, `relation:`, `expect:`, `max_depth:`, `max_symbols:`, `max_edges:`, `min_coverage:` | bounded shortest witness with tri-state confidence |
| `graph.corridor` | `from:`, `to:`, `relation:`, one or more of `path:`/`lang:`/`kind:`/`shape:`/`srcset:`, traversal budgets | one stateless bounded connectivity subgraph; `result_complete` and `search_complete` are separate |
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
  Canonical URIs, compact monikers returned by MCP, symbol ids, unique bare
  names, and unambiguous `lang:path.kind:name` references are accepted by
  symbol-targeting verbs. Ambiguous natural references fail with concrete
  canonical candidates instead of choosing one silently.
- Numbers bare: `limit:10`.
- `syntax.tree` defaults to named nodes, depth 6 and 100 nodes. Set
  `named_only:false` only when punctuation or anonymous grammar nodes matter;
  `include_text:true max_text_chars:80` attaches normalized text to leaves.
- Multi-value fields OR-combine. Natural forms are equivalent:
  `shape:callable,type`, `shape:"callable,type"`, `shape:[callable,type]`,
  `shape:["callable","type"]`, or repeated `shape:` fields. Quoting a whole
  list does not turn its comma into a literal glob. Inside a bracketed list,
  quote one item to preserve a literal comma, for example
  `path:["generated,a.ts"]`. Spaces after list commas are accepted. An
  unclosed `[` remains a parse error.
- Result budgets accept `limit:` and `max_items:` as aliases. `query.describe`
  exposes the canonical field, its request default and its accepted range;
  `symbol.graph` and `identity.children` accept `1..500`. Supplying both aliases
  is rejected because two competing budgets are genuinely ambiguous.
- `max_symbols`, `max_edges`, `max_depth`, and `min_coverage` are request
  controls, not hidden product constants. Read their current defaults and
  protocol ceilings with `query.describe verb:"graph.path"` or
  `query.describe verb:"graph.corridor"`. A budget error first gives the exact
  field/value to raise, then a scope-narrowing alternative.
- Path and corridor accept a type or namespace owner naturally: the endpoint
  becomes a Roaring set containing that owner and its navigable members. The
  result reports both endpoint-set cardinalities. Use a member endpoint when
  the owner scope is broader than the intended question.
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
