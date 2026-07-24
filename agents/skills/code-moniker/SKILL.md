---
name: code-moniker
description: Explore and diagnose any codebase through code-moniker's symbolic index instead of manual file reading. ALWAYS use this before grep/Glob/Read exploration when the question involves architecture, module structure, coupling or dependencies between parts, call graphs, callers/callees, impact of a change, code smells, refactor targets, or codebase health — manual exploration produces impressions, code-moniker produces counted facts (coupling edges with counts, ranked violations) in fewer calls. Typical requests, in any language: map the architecture, architecture or module overview, dependency graph, strongest or heaviest couplings, who calls or uses X, impact of changing X, where to refactor, is this codebase healthy, review this diff structurally. Zero configuration on any ts/rs/java/python/go/cs/sql project.
---

# code-moniker

code-moniker builds a symbolic index of a codebase: every definition gets a
stable moniker URI (`code+moniker://./lang:ts/dir:src/module:api/fn:save(x)`)
and every reference (calls, uses_type, extends, imports…) is a fact linking
two monikers. You navigate structure and relations instead of grepping text —
and you get counts, not impressions.

## Prefer the MCP surface

For agent exploration, the `code_moniker_*` MCP tools are the complete and
canonical interface. They add deterministic output budgets, compact rendering,
response-local aliases, pagination and safe follow-up calls around the typed
query engine. When the MCP surface is configured and available, do not repeat
the same exploration with `code-moniker query`, a daemon client, grep, or a
script: that duplicates facts and consumes context.

The MCP surface is optional. If it is not configured or unavailable in the
current session, continue with the local `code-moniker` binary instead of
blocking or reporting a parity defect. In particular, extractor work should
use:

```sh
/Users/alexandreboyer/.cargo/bin/code-moniker extract . --path <file> --shape callable --limit 80
```

Always anchor `extract` on the workspace root (`.`) and narrow with `--path`;
never anchor extraction directly on the file. The binary is also valid for
`stats`, `check`, and `diff` dogfood workflows. Use `code-moniker query` only
when an advanced structural question cannot be answered by those commands, and
read `references/query-dsl.md` before composing its syntax.

If the MCP is wired and responds but lacks a required read-only capability,
report a parity defect. Do not silently fall back to the daemon.

## Quick start on an unknown repo

0. If the MCP surface is unavailable, skip the MCP-only steps below. Start
   with `code-moniker stats <path>`, then use anchored `extract . --path
   <file>` calls to narrow the relevant symbols; use `check` for health and
   rule evidence.
1. Call `code_moniker_read uri:"workspace" expected_roots:["<current absolute
   workspace root>"] budget:"small"` for a bounded overview and a fail-closed
   workspace identity check. Stop immediately on `workspace_mismatch`. Stop if
   the overview answers the question.
2. Narrow with `code_moniker_symbols` (`path`, `lang`, `shape`, `name`, small
   `limit`). Never invent a URI.
3. Use `code_moniker_usages` or `code_moniker_graph` only for the selected
   canonical URI or file.
4. Before a structural edit, call `code_moniker_context focus:"<canonical>"`
   once. It combines impact, notes, applicable rules, local changes and checks.
5. Use `code_moniker_query` only for an advanced read-only verb not covered by
   an intent tool. Discover its current grammar with `query.describe`; a batch
   is limited to four queries at one workspace generation.

Then go by need:

- **Understand code, trace flows, find entry points** → `references/explore.md`
- **Health check, coupling, smells, refactor targets, dependency audit** → `references/diagnose.md`
- **Agent MCP contract, budgets and aliases** → `references/mcp.md`
- **Developer-only query grammar and dogfood** → `references/query-dsl.md`

## Rules that save you a failed call

- **Never guess a URI or a focus path.** Get URIs from
  `code_moniker_symbols` and pass them exactly; a guessed one returns
  `symbol_not_found` / `focus_not_found`.
- **Aliases are display-only.** `@1` exists only inside the response that
  declares it. Resolve it through that response's `aliases` block before a
  hand-built call; generated calls already preserve canonical URIs.
- **Keep the default small budget.** Set a narrow `limit`/`max_items`; request
  `medium` or `full`, code, wider scope or the next page only when the current
  question requires it. Stop once the evidence is sufficient.
- **Use `compact:true` by default.** `compact:false` is a diagnostic escape
  hatch for canonical typed detail, not a normal exploration mode.
- **Anchor extraction on the root**: `code-moniker extract . --path <file>`,
  never `extract <file>` — this applies only to extractor development.
- **Verify the workspace before facts.** The first workspace read must pass the
  current absolute root through `expected_roots`. A mismatch is a routing
  defect: do not continue with another server, the CLI, or guessed filters.
- Unresolved references are counted, never hidden. Treat the count as data
  (resolution coverage), not as an error.
