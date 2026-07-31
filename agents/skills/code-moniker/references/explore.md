# Explore — understand a codebase through MCP

Use only the `code_moniker_*` MCP tools for agent exploration. They preserve
the typed query model while enforcing compact output, deterministic budgets,
compact monikers and canonical follow-up calls.

## First contact

Call `code_moniker_read uri:"workspace" expected_roots:["<current absolute
workspace root>"] budget:"small"`. `expected_roots` is mandatory for this
workspace read; omitting it fails with `workspace_identity_required`, and an
incorrect identity fails with `workspace_mismatch`. A successful call returns
the language mix, definition/reference counts, concentration hints and a
bounded first explorer level. Stop there if it answers the question; otherwise
follow only the narrow `next` call relevant to the requested scope.

## Load project context when it changes interpretation

For architecture, audit, refactor, or project-convention questions, call
`code_moniker_read uri:"workspace/views"` after the verified workspace read.
Follow only a relevant returned view call. A project-defined view is a
contextual lens over the current index: its intent, summary, boundaries,
ownership, prohibitions, rules and gotchas orient exploration while its
selectors resolve to current evidence.

Treat declared intent as project context and resolved symbols/rules as indexed
facts. Missing or ambiguous evidence is coverage information, not proof that a
boundary or responsibility is absent. If the project defines no relevant view,
continue from the user's scope and the general index without inventing one.

## Drill structure

Use `code_moniker_read` with `path`, `lang`, `depth` and a small `limit` for
filesystem-oriented navigation. For a purely symbolic hierarchy or a rolled-up
scope graph, use the advanced MCP entry without leaving MCP:

```text
code_moniker_query query:'identity.children prefix:"lang:ts/dir:apps" limit:20'
code_moniker_query query:'identity.graph prefix:"lang:ts/dir:apps" path:"apps/**" min_count:2 limit:20'
```

Discover the live fields first with
`code_moniker_query query:'query.describe verb:"identity.graph"'` when the
running server may differ from this reference.

## Find a symbol

Use `code_moniker_symbols action:"list"` with the narrowest available `path`,
`lang`, `shape`, `kind` and `name`, plus a small `limit`. Every result carries a
compact moniker that can be passed directly to other symbol tools. Never guess
one; canonical URIs and symbol ids remain accepted when already available.

## Inspect dependencies

Use `code_moniker_graph focus:"<returned moniker or file>"` for the ego
view. `direction`, `relation`, `min_count` and `include_internal` keep only the
edges needed by the question. The result separates callers, callees, internal
edges and unresolved coverage.

Use `code_moniker_usages uri:"<returned moniker>" direction:"incoming|outgoing|both"`
when individual consumers or producers matter. Its compact default groups
repeated references by symbolic context and includes only bounded,
representative source evidence; use `evidence:"none"` for a map without code or
`technical:"include"` when imports and annotations matter. Keep the first page
unless the question explicitly requires more.
For an owner whose behavior is exposed through members, add
`include_descendants:true`; label the result as owner roll-up, not exact symbol
usage, and preserve it in pagination calls.

## Prepare a modification

After selecting a target, call `code_moniker_context focus:"<returned moniker>"`
once. It combines bounded source context, graph facts, notes, applicable rules,
worktree changes, coverage and canonical suggested checks. Do not re-fetch the
same sections separately unless coverage shows that the omitted facts matter.

## Read code only when necessary

`code_moniker_read uri:"<returned compact moniker>" context_lines:2` reads the
target zone. Source and wider context are opt-in because they dominate token
cost. Structural questions should stay on symbols, usages and graphs.

When the exact parser shape is material, request it explicitly:
`code_moniker_read uri:"src/service.ts" ast:true max_depth:6 max_nodes:100`.
A returned symbol moniker can replace the file path to focus the tree on that
declaration. In a multi-root workspace, use an absolute path or returned
moniker when the same relative path exists in more than one root. The default
is a named-node AST projection; punctuation
(`named_only:false`) and bounded leaf text (`include_text:true`) are opt-in.

## Failure modes

- `symbol_not_found` or `focus_not_found`: search again; the moniker/path was
  guessed, stale or outside the workspace.
- `workspace_loading`: retry the same bounded MCP call. Curated read tools
  refresh stale snapshots automatically; use `code_moniker_refresh` only when
  an explicit re-index is required.
- `completeness: partial`: page only if the omitted rows can change the answer.
- Missing read-only verb: confirm with `query.describe` and report an MCP
  parity defect; do not switch to a daemon or shell query.
