# Agent output boundary

Status: accepted

## Decision

Every public MCP response intended for an agent is compact and uses a small
result-volume profile by default. The contract is parsed by one registry
boundary and applied while each tool projects its response, before rendering.

An MCP tool explicitly declares either:

- `OutputContract::Agent`: the registry injects `compact=true` and
  `budget=small` into its public input schema, parses both before tool
  execution, and passes typed output options to the tool. The tool maps the
  volume profile to result counts, traversal depth, witnesses, and optional
  detail before constructing its presentation DTO.
- `OutputContract::Plain`: the tool returns a small operational response that
  does not need agent-output transformation. This is currently reserved for
  workspace refresh.

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

1. Compact output is opt-out, never opt-in, for every agent-facing MCP tool.
2. Canonical monikers remain accepted as input and available with
   `compact=false`.
3. Generated follow-up calls preserve the requested rendering mode: compact
   monikers by default, canonical arguments with `compact=false`. Both remain
   unambiguous and executable.
4. A volume profile changes the projection before rendering; it never rewrites
   rendered text.
5. Schema publication and argument parsing have one owner: `OutputContract`;
   each tool owns the semantic projection of its result volume.
6. Adding a new `McpTool` requires an explicit output contract at compile time.
7. Diagnostic payloads whose size follows source complexity, such as syntax
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

- the contract must publish and parse compact and volume-profile options before
  tool execution;
- templated agent output is rendered only through `OutputContract::finalize`;
- output finalization must not slice rendered text;
- every concrete tool contract under the MCP tool surface must declare
  `OutputContract::Agent`, with refresh as the sole explicit exception;
- per-tool schemas must not redeclare `compact` or `budget`;
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

The public MCP surface remains token-efficient as tools are added or renamed.
The contract rule uses an open selector over the MCP tool surface rather than
enumerating the current tool modules, so adding or renaming a tool cannot make
it disappear from enforcement. Mixed compact/canonical output and ignored
volume profiles become structural violations during local checks and CI
instead of review-time conventions. Projectors own information volume;
templates own presentation; the registry owns the shared public options.
