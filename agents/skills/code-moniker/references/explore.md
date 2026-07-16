# Explore — understand a codebase through MCP

Use only the `code_moniker_*` MCP tools for agent exploration. They preserve
the typed query model while enforcing compact output, deterministic budgets,
response-local aliases and canonical follow-up calls.

## First contact

Call `code_moniker_read uri:"workspace" budget:"small"`. It returns the
language mix, definition/reference counts, concentration hints and a bounded
first explorer level. Stop there if it answers the question; otherwise follow
only the narrow `next` call relevant to the requested scope.

## Drill structure

Use `code_moniker_read` with `path`, `lang`, `depth` and a small `limit` for
filesystem-oriented navigation. For a purely symbolic hierarchy or a rolled-up
scope graph, use the advanced MCP entry without leaving MCP:

```text
code_moniker_query query:'identity.children prefix:"lang:ts/dir:apps" limit:20'
code_moniker_query query:'identity.graph prefix:"lang:ts/dir:apps" limit:20'
```

Discover the live fields first with
`code_moniker_query query:'query.describe verb:"identity.graph"'` when the
running server may differ from this reference.

## Find a symbol

Use `code_moniker_symbols action:"list"` with the narrowest available `path`,
`lang`, `shape`, `kind` and `name`, plus a small `limit`. Every result carries a
canonical URI or a response-local alias declared above it. Never guess an URI;
resolve an alias through that response before constructing a call.

## Inspect dependencies

Use `code_moniker_graph focus:"<canonical URI or returned file>"` for the ego
view. `direction`, `relation`, `min_count` and `include_internal` keep only the
edges needed by the question. The result separates callers, callees, internal
edges and unresolved coverage.

Use `code_moniker_usages uri:"<canonical URI>" direction:"incoming|outgoing|both"`
when individual consumers or producers matter. Keep the first page unless the
question explicitly requires more.

## Prepare a modification

After selecting a target, call `code_moniker_context focus:"<canonical URI>"`
once. It combines bounded source context, graph facts, notes, applicable rules,
worktree changes, coverage and canonical suggested checks. Do not re-fetch the
same sections separately unless coverage shows that the omitted facts matter.

## Read code only when necessary

`code_moniker_read uri:"<canonical symbol URI>" context_lines:2` reads the
target zone. Source and wider context are opt-in because they dominate token
cost. Structural questions should stay on symbols, usages and graphs.

## Failure modes

- `symbol_not_found` or `focus_not_found`: search again; the URI/path was
  guessed, stale or outside the workspace.
- `workspace_loading` or `workspace_stale`: retry the same bounded MCP call or
  use `code_moniker_refresh` after an external change.
- `completeness: partial`: page only if the omitted rows can change the answer.
- Missing read-only verb: confirm with `query.describe` and report an MCP
  parity defect; do not switch to a daemon or shell query.
