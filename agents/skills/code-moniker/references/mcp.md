# MCP — the agent-shaped surface

When `code_moniker_*` tools are wired (server: `code-moniker mcp <root>
--port <p>`, HTTP endpoint `/mcp`), prefer them over shelling out: responses
are compact text with `uri`, `completeness`, a result body, and a `next`
section containing **ready-made follow-up calls** — copy those instead of
building parameters by hand.

## Tools by intent

| Intent | Tool | Notes |
|---|---|---|
| Orient / expand tree / read a symbol's code | `code_moniker_read` | `uri:"workspace"` for the summary + explorer; a symbol URI reads its source zone (`context_lines`) |
| List/filter symbols, workspace metrics | `code_moniker_symbols` | `action:"list"` with `path`/`lang`/`kind`/`shape`/`name` (name is a regex here); `action:"insights"` |
| Who uses it / what it uses | `code_moniker_usages` | `direction:"incoming"\|"outgoing"\|"both"`; summaries include kinds, dominant prefix, `shared_helper_signal` |
| Ego neighborhood before editing | `code_moniker_graph` | `focus` = URI or workspace-relative path; callers/callees/members with counts |
| Rules: inspect or run | `code_moniker_rules` | `action:"list"` (rationales) or `action:"run"` (optionally file-scoped — the same check agent hooks run) |
| Changes as symbol facts | `code_moniker_diff` | review surface |
| Text/structure search | `code_moniker_search` | when name filters aren't enough |
| Force re-index | `code_moniker_refresh` | after external file changes |

## Working discipline

1. **Start scoped**: `code_moniker_read uri:"workspace"` returns language
   mix, concentration hints, and a first explorer level — plus `next` calls
   sized to the workspace. Deepen with `depth`/`path`/`lang` rather than
   asking for everything.
2. **URIs only from tool output.** `code_moniker_symbols` result rows include
   the exact URI *and* a pre-built `usages:` call for each symbol. A
   hand-built URI fails with `symbol_not_found` on the first signature
   nuance.
3. **Respect paging**: `completeness: partial (usages 0-5 of 14, next cursor
   5)` tells you exactly what you have; the `next` section carries the cursor
   call.
4. **Bound everything**: `limit`, `max_items` — truncation is always
   reported, never silent.

## Failure modes

- `restart required` / connection-closed errors: the MCP server lost its
  daemon (killed or restarted underneath it). Restart the MCP server process,
  then retry.
- Tool errors carry `problem` / `where` / `fix_hint` — read them; they are
  usually literal.
