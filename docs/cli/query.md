# `code-moniker query` — indexed Query DSL

`code-moniker query` sends a request to a workspace daemon. Indexed verbs read
one published workspace generation rather than inspecting the filesystem
directly; `syntax.parse` is the explicit stateless exception. The text following
the command is a small request language for the daemon's typed query protocol;
it is separate from the TOML [rule DSL](check-dsl.md).

Use the CLI when you need to diagnose or integrate directly with an indexed
daemon. Agents with Code Moniker MCP should normally use the intent tool that
matches the question (`code_moniker_symbols`, `code_moniker_usages`,
`code_moniker_graph`, and so on). `code_moniker_query` exposes the same Query
DSL for advanced read-only operations that have no intent tool.

## Discover the live language

The daemon capability registry is the executable source of truth. Do not copy a
remembered list of fields into an integration:

```sh
# All verbs, grouped with their fields, defaults, constraints and examples
code-moniker query 'query.describe'

# One verb, including its required and repeatable fields
code-moniker query 'query.describe verb:"symbol.search"'
```

The MCP equivalent is:

```text
code_moniker_query query:'query.describe verb:"symbol.search"'
```

`query.describe` is generated from the same capability registry used to
validate requests. It reports whether an operation is read-only, its preferred
MCP tool, pagination, projections, field types, defaults, accepted values,
constraints, and a valid example. The `--json` form additionally exposes the
accepted positional count as structured data.

## Target an index

Without `--daemon`, the CLI selects or starts the daemon whose identity matches
the requested workspace roots, project, cache and live-refresh policy:

```sh
code-moniker query --root . 'workspace.status'
code-moniker query --root . 'symbol.search name:"PaymentService" limit:10'
```

To query one already-running index without starting a replacement or falling
back to the filesystem, select its endpoint explicitly:

```sh
code-moniker daemon list
code-moniker query --daemon 127.0.0.1:3210 'workspace.status'
code-moniker query --daemon 127.0.0.1:3210 \
  'symbol.search name:"PaymentService" shape:type limit:10'
```

The endpoint comes from `daemon list`. An explicit `--daemon` conflicts with
workspace-identity options because it selects a process, not a project lookup
hint. See [Workspace Daemon](../daemon.md#commands) for daemon ownership,
lifecycle and discovery.

## Grammar

The common request shape is:

```text
<query>       ::= <verb> <argument>* <section>*
<argument>    ::= <position> | <field> ":" <value>
<value>       ::= <bare> | "<quoted>" | "[" <list-item> ("," <list-item>)* "]"
<section>     ::= NEWLINE ("filter" | "page") <argument>*
                | NEWLINE "project" <result-field>+
                | NEWLINE "consistency" <consistency>
                | NEWLINE "direction" <direction>
<consistency> ::= "current" | "refresh-if-stale" | "stale-ok"
```

The first token is always a verb such as `symbol.search`, `symbol.usages`,
`graph.path`, or `rules.check`. Most requests can stay on one line:

```text
symbol.search name:"Payment Service" shape:type lang:ts limit:20
```

Some verbs accept one positional value as an alternative to their natural
named field. These two requests are equivalent:

```text
symbol.detail "code+moniker://workspace/lang:ts/module:billing/class:Invoice"
symbol.detail uri:"code+moniker://workspace/lang:ts/module:billing/class:Invoice"
```

`query.describe verb:"<verb>" --json` reports whether a positional value is
accepted; both text and JSON identify required fields. Supplying conflicting
positional and named values is an error.

### Values and lists

- Quote values containing whitespace. Quoting paths and monikers consistently
  also avoids shell interpretation.
- Numbers and booleans are bare: `limit:10`, `include_code:true`.
- Repeatable fields accept repeated values, comma-separated values, or a
  bracketed list. Values inside one field are alternatives; different filter
  fields combine to narrow the query.

```text
symbol.search shape:callable shape:type path:["src/**", "tests/**"] limit:20
```

Inside a bracketed list, quote an item to preserve a literal comma, for example
`path:["src/generated,a.ts"]`. An unknown field, a conflicting repeated scalar,
or an incomplete list is rejected before the query runs.

### Sections and projections

Long requests may put ordinary fields below `filter` or `page`. Projectable
verbs also accept a `project` section containing only the desired result fields:

```text
symbol.search name:"Service" shape:callable
filter path:"src/**" lang:[ts,tsx]
page limit:20
project name uri file line_range
```

Use `query.describe` to discover whether a verb supports projection and the
accepted result fields. Projection narrows the rendered result; it does not
change which indexed symbols match.

## Pagination and consistency

Paginated verbs accept `limit:` and a generation-aware `cursor:`. Reuse the
cursor returned by the previous response rather than constructing one. A
cursor belongs to the immutable generation that produced it, so paging never
silently crosses an index refresh.

The Query DSL supports three freshness policies:

- `current` fails if the selected workspace generation is stale;
- `refresh-if-stale` refreshes before answering;
- `stale-ok` answers immediately from the published generation.

Set the policy in the request with `consistency:stale-ok`, on a separate
`consistency stale-ok` line, or through the CLI `--consistency` option. An
explicit field in the query text wins over the CLI option. The CLI defaults to
`stale-ok`; the typed protocol default reported by `query.describe` is
`current`.

## Main query families

Run `query.describe` for the current complete inventory. The stable families
are:

| Family | Purpose | Typical verbs |
| --- | --- | --- |
| Discovery and workspace | Learn the protocol and inspect index state | `query.describe`, `workspace.status` |
| Navigation and symbols | Browse files, symbols, detail and usages | `tree.children`, `symbol.search`, `symbol.detail`, `symbol.usages` |
| Syntax | Parse source or inspect a bounded tree | `syntax.parse`, `syntax.tree` |
| Graph and identity | Inspect neighborhoods, paths, corridors and identity levels | `symbol.graph`, `graph.path`, `graph.corridor`, `identity.children`, `identity.graph` |
| Rules and changes | Evaluate project testimony against one generation | `rules.list`, `rules.check`, `rules.applicable`, `change.review`, `change.context` |
| Diagnostics and metrics | Explain resolution and measure coupling | `resolution.audit`, `metrics.coupling` |

For the syntax-tree request and response contract, see
[On-demand syntax tree](mcp-syntax-tree.md). The `syntax.tree` query is an
indexed read; the `ast` domain in project rules is instead documented by the
[rule DSL](check-dsl.md#quantifiers) and the executable `rules learn ast`
recipe.

## Authoritative artifacts

- Query capability registry, parser, DTOs and text formatter:
  `crates/query/src/lib.rs`
- CLI daemon selection and consistency override:
  `crates/cli/src/query/mod.rs`
- Generic MCP Query DSL tool and bounded agent output:
  `crates/cli/src/mcp/tools/query.rs`
- Generated wire schema: `docs/schema/daemon.schema.json`
