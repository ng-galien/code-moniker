# MCP — the agent-shaped surface

When `code_moniker_*` tools are wired, prefer a project-owned stdio server with
an absolute root: `code-moniker mcp <absolute-root> --transport stdio`. HTTP remains available with
`code-moniker mcp <root> --transport http --port <p>` and endpoint `/mcp`.
Use either transport as the complete agent surface:
do not shell out to the daemon or replay the same exploration through direct
queries. Responses
are compact text with `uri`, `completeness`, and a result body. A `next`
section appears only when the server has a useful pagination or navigation
follow-up; its generated calls are ready to execute.

`compact` defaults to `true` on agent-facing read tools. `budget:"small"`
also defaults to a deterministic 8,000-character ceiling; `medium` is 20,000
and `full` is 64,000. `max_chars` can override the level. A budget truncation
is explicit and preserves a small canonical `next` block when possible.

Repeated canonical monikers in descriptive data can then be declared once in
an `aliases` section and referenced as `@1`, `@2`, etc.

Alias rules are strict:

- an alias belongs only to the response that declares it;
- the server stores no alias table between calls or responses;
- tools reject aliases as arguments;
- generated tool calls keep canonical URIs and can be copied verbatim;
- when building a call from an aliased data field, resolve `@N` through the
  `aliases` section in that same response.

Use `compact:false` when canonical URIs on every data occurrence and the fuller
set of guided follow-ups are worth the extra tokens. Generated pagination calls
preserve `compact:false`.

Compact symbol rows intentionally omit a duplicated `code_moniker_usages` call
for every result. Pass the row's canonical URI to `code_moniker_usages` when
needed; if a data field is an alias, resolve it from that response first.

## Tools by intent

| Intent | Tool | Notes |
|---|---|---|
| Orient / expand tree / read a symbol's code | `code_moniker_read` | `uri:"workspace"` for the summary + explorer; a symbol URI reads its source zone (`context_lines`) |
| List/filter symbols, workspace metrics | `code_moniker_symbols` | `action:"list"` with `path`/`lang`/`kind`/`shape`/`name` (name is a regex here); `action:"insights"` |
| Who uses it / what it uses | `code_moniker_usages` | `direction:"incoming"\|"outgoing"\|"both"`; compact mode groups references by symbolic context, summarizes technical noise, and samples bounded source evidence |
| Ego neighborhood before editing | `code_moniker_graph` | `focus` = URI or workspace-relative path; filter with `direction`, `relation`, `min_count`, `include_internal` |
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
2. **URIs only from tool output.** `code_moniker_symbols` result rows include
   either the canonical URI or a response-local alias declared above the
   result. Copy generated calls as-is: they deliberately retain canonical
   URIs. Compact symbol rows may have no pre-built usages call, so pass their
   URI to `code_moniker_usages`; resolve an alias first if necessary. A
   hand-built URI fails with `symbol_not_found` on the first signature nuance.
3. **Respect paging**: `completeness: partial (usages 0-5 of 14, next cursor
   5)` tells you exactly what you have; when more rows exist, the optional
   `next` section carries the cursor call. Usage pages may exceed `limit` to
   keep one symbolic context group intact; generated cursors always start at a
   group boundary.
4. **Bound everything**: keep `budget:"small"`, a narrow `limit` or
   `max_items`, and `compact:true`. Truncation is reported, never silent.
5. **Stop progressively**: do not page, broaden scope, request source code or
   switch to `medium`/`full` unless the current evidence is insufficient for
   the question. Never fetch a second rendering of facts you already have.
6. **Prepare edits once**: after selecting a target, prefer one
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
budget and alias contract. Use `query.describe` or
`query.describe verb:"identity.graph"` instead of recalling syntax from
memory. `queries:[...]` executes two to four independent operations at one
workspace generation and shares a single response-local alias table. Mutating
queries such as notes are rejected; use their intent tool.

Projections keep expensive collections narrow, for example:

```text
code_moniker_query query:'symbol.search name:"parse_query" limit:5 project name file line_range uri'
```

The URI remains canonical navigation data even when another occurrence is
rendered as an alias. `compact:false` returns typed JSON and is intentionally
more expensive.

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
