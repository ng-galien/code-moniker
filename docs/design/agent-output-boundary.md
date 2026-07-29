# Agent output boundary

Status: accepted

## Decision

Every public MCP response intended for an agent is compact and bounded by
default. The contract is owned by one registry boundary, not by individual
renderers.

An MCP tool explicitly declares either:

- `OutputContract::Agent`: the registry injects `compact=true`, `budget=small`,
  and the optional `max_chars` override into its public input schema; validates
  those arguments before tool execution; replaces registered canonical
  monikers with compact monikers; then applies the hard output budget.
- `OutputContract::Plain`: the tool returns a small operational response that
  does not need agent-output transformation. This is currently reserved for
  workspace refresh.

Tools return canonical text plus the finite set of monikers present in that
text. They may still use `compact` to choose a minimal or verbose information
shape, but they never perform URI compaction or output truncation themselves.

## Invariants

1. Compact output is opt-out, never opt-in, for every agent-facing MCP tool.
2. Canonical monikers remain accepted as input and available with
   `compact=false`.
3. Generated follow-up calls keep canonical arguments so they remain
   unambiguous and executable.
4. Moniker compaction runs before the hard character budget.
5. Schema publication, argument validation, compaction, and budgeting have one
   owner: `OutputContract`.
6. Adding a new `McpTool` requires an explicit output contract at compile time.

## Executable enforcement

The root `.code-moniker.toml` encodes this decision as error-severity rules:

- every graph path from the `ToolRegistry` success and error output entry
  points to compaction or budgeting must pass through
  `OutputContract::finalize`;
- the finalizer must apply compaction before budgeting;
- the contract must publish and validate compact and budget options;
- every concrete tool contract under the MCP tool surface must declare
  `OutputContract::Agent`, with refresh as the sole explicit exception;
- per-tool schemas must not redeclare `compact`, `budget`, or `max_chars`;
- production code must not call the moniker compactor or output budgeter
  outside the finalizer.

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
it disappear from enforcement. Mixed compact/canonical output and missing
response budgets become structural violations during local checks and CI
instead of review-time conventions. Renderers remain responsible for
information shape; the output boundary remains responsible for transport
shape.
