# MCP — the agent-shaped surface

When `code_moniker_*` tools are wired, prefer a project-owned stdio server with
an absolute root: `code-moniker mcp <absolute-root> --transport stdio`. HTTP remains available with
`code-moniker mcp <root> --transport http --port <p>` and endpoint `/mcp`.
The stdio supervisor keeps the client pipe stable across an atomic CLI reinstall
and refreshes the advertised tool list after its replacement worker initializes.
Use either transport as the complete agent surface:
do not shell out to the daemon or replay the same exploration through direct
queries. Responses
are compact text with `uri`, `completeness`, and a result body. A `next`
section appears only when the server has a useful pagination or navigation
follow-up; its generated calls are ready to execute.

`compact` defaults to `true` on agent-facing read tools. `budget:"small"`
also defaults to a deterministic 8,000-character ceiling; `medium` is 20,000
and `full` is 64,000. `max_chars` can override the level. A budget truncation
is explicit and preserves a small executable `next` block when possible.

With `compact:true`, every canonical symbol URI in descriptive data is rendered
in the existing compact moniker form, for example
`rs:crates/cli/src/mcp.tools.fn:run()`. Unlike the former response-local
aliases, this compact form can be passed directly to symbol tools. Canonical
URIs and `symbol:<file>:<def>` ids remain accepted. Generated calls use
reusable compact monikers in compact mode and can be copied verbatim.

Use `compact:false` when canonical URIs on every data occurrence and the fuller
set of guided follow-ups are worth the extra tokens. Generated pagination calls
preserve `compact:false`.

Compact symbol rows intentionally omit a duplicated `code_moniker_usages` call
for every result. Pass the row's compact moniker to `code_moniker_usages` when
needed.

## Tools by intent

| Intent | Tool | Notes |
|---|---|---|
| Orient / expand tree / read code or AST | `code_moniker_read` | `uri:"workspace"` for the summary + explorer; a returned compact moniker reads its source zone. Add `ast:true` to a relative or absolute file or returned moniker for a bounded on-demand syntax tree; absolute paths disambiguate duplicate multi-root paths. |
| Read project-defined contextual views | `code_moniker_read` | `uri:"workspace/views"` lists views; follow a returned `workspace/views/<id>` call for intent, boundaries, ownership, prohibitions, rules, gotchas and current indexed evidence. |
| List/filter symbols, workspace metrics | `code_moniker_symbols` | `action:"list"` with `path`/`lang`/`kind`/`shape`/`name` (name is a regex here); `action:"insights"` |
| Who uses it / what it uses | `code_moniker_usages` | `direction:"incoming"\|"outgoing"\|"both"`; `include_descendants:true` explicitly rolls member activity into an owner while excluding internal relations; compact mode groups references by symbolic context |
| Ego neighborhood before editing | `code_moniker_graph` | `focus` = returned moniker or workspace-relative path; filter with `direction`, `relation`, `min_count`, `include_internal`; coverage distinguishes total, matching and returned neighbors |
| One-call pre-change evidence | `code_moniker_context` | graph, coverage, notes, applicable rules, local changes and canonical suggested checks |
| Rules: inspect or run | `code_moniker_rules` | `action:"list"` (rationales) or `action:"run"` (optionally file-scoped). It shares rule-engine semantics with agent-hook checks, but MCP uses a daemon query while generated hooks launch the CLI directly. |
| Changes as symbol facts | `code_moniker_diff` | review surface |
| Text/structure search | `code_moniker_search` | when name filters aren't enough |
| Force re-index | `code_moniker_refresh` | after external file changes |
| Advanced read-only verb | `code_moniker_query` | use `query.describe`; one query or a batch of at most four at one generation |

## Working discipline

1. **Verify identity, then start scoped**: `code_moniker_read uri:"workspace"
   expected_roots:["<current absolute workspace root>"]` requires
   `expected_roots` and fails with `workspace_mismatch` unless the server is
   bound to exactly that root set.
   A successful read returns the canonical roots, language mix, concentration
   hints, and a first explorer level — plus `next` calls
   sized to the workspace. Deepen with `depth`/`path`/`lang` rather than
   asking for everything.
2. **Load views when project intent matters**: for architecture, audit,
   refactor, or project-convention questions, read `workspace/views` and only
   the relevant returned view before interpreting the graph. A view declares
   context and resolves evidence against the index; it does not replace graph,
   rule, change, resolution, or coverage facts.
3. **Monikers only from tool output.** `code_moniker_symbols` result rows
   include reusable compact monikers. Copy generated calls as-is: they
   preserve the active compact or canonical mode. Compact symbol rows may have
   no pre-built usages call, so pass their moniker to `code_moniker_usages`.
   A hand-built moniker fails with `symbol_not_found` on the first signature
   nuance.
4. **Respect paging**: `completeness: partial (usages 0-5 of 14, next cursor
   5)` tells you exactly what you have; when more rows exist, the optional
   `next` section carries the cursor call. Usage pages may exceed `limit` to
   keep one symbolic context group intact; generated cursors always start at a
   group boundary.
   `identity.graph` is also generation-aware and paginated across deterministic
   node, edge, incoming-port and outgoing-port rows. Preserve `prefix`, `path`,
   `min_count`, `limit` and the returned cursor on every page.
5. **Bound everything**: keep `budget:"small"`, a narrow `limit` or
   `max_items`, and `compact:true`. For AST reads, keep the defaults
   `max_depth:6`, `max_nodes:100`, `named_only:true`; leaf text and punctuation
   are explicit opt-ins. Truncation is reported, never silent.
6. **Stop progressively**: do not page, broaden scope, request source code or
   switch to `medium`/`full` unless the current evidence is insufficient for
   the question. Never fetch a second rendering of facts you already have.
7. **Prepare edits once**: after selecting a target, prefer one
   `code_moniker_context` call over separate graph, notes, rules and diff calls.

For usages, the default `evidence:"representative"` keeps the map exhaustive
while attaching code only to a small, direction-balanced selection of semantic
groups. Use `evidence:"none"` for pure cartography. Imports, annotations and
non-primary type relations are counted but omitted from the group list by
default; use `technical:"include"` when those relations are material. Bound
source with `max_evidence` and `context_lines` instead of increasing the whole
response budget.

## Advanced queries without leaving MCP

`code_moniker_query` runs the same typed read-only DSL while retaining the MCP
budget and compact-moniker contract. Use `query.describe` or
`query.describe verb:"identity.graph"` instead of recalling syntax from
memory. `queries:[...]` executes two to four independent operations at one
workspace generation. Mutating queries such as notes are rejected; use their
intent tool. A paginated single query emits an executable `next` call with the
original expression and a generation-aware `cursor`; replay that call instead
of reconstructing `prefix`, `path`, `min_count` or `limit`.

Projections keep expensive collections narrow, for example:

```text
code_moniker_query query:'symbol.search name:"parse_query" limit:5 project name file line_range uri'
```

The same typed protocol exposes
`syntax.tree focus:"src/service.ts" max_depth:6 max_nodes:100`. Prefer the
intent form `code_moniker_read uri:"src/service.ts" ast:true`; use the generic
query only when testing the daemon contract.

The default response renders projected URIs as reusable compact monikers.
`compact:false` returns canonical typed JSON and is intentionally more
expensive.

## Failure modes

- Filesystem-root refusal: the MCP host resolved a relative project path to
  `/` (or another platform root). Fix the MCP configuration to pass the
  canonical absolute project path; never bypass this guard.
- `restart required` / connection-closed errors: the MCP server lost its
  daemon (killed or restarted underneath it). Restart the MCP server process,
  then retry.
- `workspace_mismatch`: the client reached a server for another project. Stop;
  fix the project MCP configuration instead of querying, refreshing, or using
  the CLI against that server.
- `workspace_identity_required`: retry the initial workspace read with the
  current absolute workspace roots; the server deliberately refuses an
  unverified workspace summary.
- Tool errors carry `problem` / `where` / `fix_hint` — read them; they are
  usually literal.
