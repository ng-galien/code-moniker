# Agent output boundary

Status: accepted

## Decision

Every public MCP tool has one output pipeline and selects exactly one response
representation. JSON-RPC is only the transport envelope; it does not make the
tool payload structured automatically.

- `format=text` is the default. Agent output is compact and uses the `small`
  volume profile by default, then the shared MiniJinja template renders the
  projected DTO as Markdown in MCP `content`.
- `format=json` ignores `compact` and `budget`, selects the `full` projection
  automatically, and returns the typed projection in MCP `structuredContent`.
  The client never needs to send `budget=full`.

`content` and `structuredContent` are mutually exclusive for successful and
error responses. A response never publishes both representations.

An MCP tool explicitly declares either:

- `OutputContract::Agent`: the registry injects `format=text`, `compact=true`
  and `budget=small` into its public input schema and parses them before tool
  execution. Text uses the selected volume profile before rendering. JSON
  ignores the presentation budget and uses the complete projection; the same
  typed template context becomes `structuredContent` without rendering.
- `OutputContract::Plain`: the tool returns a small operational response that
  does not need Markdown template rendering. This is currently reserved for
  workspace refresh, which still supplies equivalent text and typed JSON
  candidates to the shared selector.

The project taxonomy names three presentation components:

- `presentation`: the shared MiniJinja environment, filters, and template
  contract;
- `presentation@mcp`: lazy rendering at the MCP output boundary;
- `presentation@cli`: CLI documents rendered through the same engine.

Scoped presentation components are atomic. They identify one presentation
contract in its delivery context; they are not implicit aliases for every MCP
or CLI module.

Templates render a complete DTO. Explicit MiniJinja filters compact typed URI
fields and the finite prose fields that declare moniker candidates. No stage
counts characters, slices rendered text, or tries to recover a continuation
block from the rendered document.

## Invariants

1. Every MCP tool publishes `format=text|json` from the central contract; no
   individual tool owns or redeclares that option.
2. Text output is the default and is the only representation affected by
   `compact` and `budget`.
3. JSON output ignores the supplied budget, uses the full projection by
   default, emits only `structuredContent`, and never renders a template.
4. Compact text output is opt-out, never opt-in, for every agent-facing MCP
   tool.
5. Canonical monikers remain accepted as input and available with
   `compact=false`.
6. Generated follow-up calls preserve the requested rendering mode: compact
   monikers by default, canonical arguments with `compact=false`. Both remain
   unambiguous and executable.
7. A text volume profile changes the projection before rendering; it never rewrites
   rendered text.
8. Schema publication and argument parsing have one owner: `OutputContract`;
   each tool owns the semantic projection of its result volume.
9. Adding a new `McpTool` requires an explicit output contract at compile time.
10. Errors honor the selected representation just like successful results.
11. Diagnostic payloads whose size follows source complexity, such as syntax
   trees, are opt-in and carry explicit structural limits in the typed query
   contract. Depth and node volume are client-selected; the daemon rejects a
   zero node budget and enforces the shared per-leaf text ceiling.

### On-demand syntax trees

`syntax.tree` applies this decision beyond final string truncation. It requests
a `ParsedDocument` from the common language SDK only when asked, never retains
a Tree-sitter tree in the workspace snapshot, and bounds depth, node count,
grammar-node detail, and optional leaf text before rendering. Semantic
extraction consumes the same parsed document contract, including any
embedded-language trees; the daemon cannot grow a parallel language parser.
The MCP intent surface is `code_moniker_read` with `ast=true`; named nodes,
depth 6, no leaf text, and a profile-bound node cap are the defaults. The cap is
20 for `small`, 80 for `medium`, and 500 for `full`; explicit larger values are
reduced before query execution.

The same surface accepts only `source` plus `language` for direct input and
routes that request to `syntax.parse`; `ast=true` remains necessary only for
indexed URI reads. This operation runs before workspace snapshot loading,
never indexes or persists its input, and uses the same `ParsedDocument` and
bounded renderer as indexed `syntax.tree`. Direct source has an independent
1 MiB input ceiling.

For syntax reads, the JSON candidate deliberately overrides the presentation
DTO with the public `SyntaxTreeResult`. Text still renders the syntax Markdown
template. JSON therefore exposes the complete typed tree directly and cannot
drift when Markdown wording or layout changes.

The root rule set proves the shared language pipeline plus three fail-closed
output paths:

- `LangExtractor::extract` parses once and delegates to `extract_parsed`;
- every language SDK extraction pipeline consumes `ParsedDocument` and does
  not parse the source again;
- the daemon response consumes the SDK `ParsedDocument`;
- the stateless response consumes `SyntaxParseQuery` and calls the shared
  language SDK parser;
- the query contract marks `syntax.parse` as snapshot-free, the MCP runtime
  skips preload, and the daemon dispatches it before live-index work;
- the daemon response consumes the typed `SyntaxTreeQuery`;
- the response enters the daemon structural-limit validator;
- `ReadTool::input_schema` delegates to the bounded AST schema renderer.

Field-level rules additionally require every volume control in the typed query,
the daemon's node and leaf-text validation, and the complete opt-in contract in
the MCP schema. Moving or deleting any selected boundary makes the path rule
fail because all endpoints use `require_non_empty = true`.

## Executable enforcement

The root `.code-moniker.toml` encodes this decision as error-severity rules:

- the contract must publish and parse format, compact, and volume-profile
  options before tool execution;
- the selector must route text through Markdown rendering, route JSON through
  structured content only, and force JSON to the full projection;
- templated agent output is rendered only through `OutputContract::finalize`;
- output finalization must not slice rendered text;
- every concrete tool contract under the MCP tool surface must declare
  `OutputContract::Agent`, with refresh as the sole explicit exception;
- per-tool schemas must not redeclare `format`, `compact`, or `budget`;
- global response-string compaction is forbidden; typed URI and prose fields
  use explicit template filters.

The generic `workspace.path` expectation `all_paths_via` expresses the
mandatory-boundary property. It proves connectivity, removes the selected
boundary symbols, and fails with the surviving bypass path when one exists.
Incomplete bounded traversal is reported as inconclusive rather than accepted.
These guardrails set `require_non_empty = true`, so renaming or deleting every
symbol selected at an endpoint or boundary is itself an error rather than a
vacuous inconclusive result.

The executable catalog scenario
[`samples/catalog/workspace-path.cm.md`](../../samples/catalog/workspace-path.cm.md)
contains both a protected path and a direct bypass.

## Consequences

The public MCP surface remains token-efficient in text mode and lossless in
JSON mode as tools are added or renamed.
The contract rule uses an open selector over the MCP tool surface rather than
enumerating the current tool modules, so adding or renaming a tool cannot make
it disappear from enforcement. Mixed compact/canonical output and ignored
volume profiles become structural violations during local checks and CI
instead of review-time conventions. Projectors own information volume;
templates own text presentation; the registry owns the shared public options
and exclusive representation selection. Machine clients consume typed content
instead of parsing Markdown whose layout is free to evolve.
