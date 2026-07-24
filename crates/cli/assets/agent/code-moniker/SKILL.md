---
name: code-moniker
description: Explore and diagnose any codebase through code-moniker's symbolic index, using the best installed surface (MCP tools when configured, otherwise the local binary). ALWAYS use this before grep/Glob/Read exploration when the question involves architecture, module structure, coupling or dependencies between parts, call graphs, callers/callees, impact of a change, code smells, refactor targets, or codebase health — manual exploration produces impressions, code-moniker produces counted facts (coupling edges with counts, ranked violations) in fewer calls. Typical requests, in any language: map the architecture, architecture or module overview, dependency graph, strongest or heaviest couplings, who calls or uses X, impact of changing X, where to refactor, is this codebase healthy, review this diff structurally. Zero project configuration on any ts/rs/java/python/go/cs/sql project.
---

# code-moniker

code-moniker builds a symbolic index of a codebase: every definition gets a
stable moniker URI (`code+moniker://./lang:ts/dir:src/module:api/fn:save(x)`)
and every reference (calls, uses_type, extends, imports…) is a fact linking
two monikers. You navigate structure and relations instead of grepping text —
and you get counts, not impressions.

## Select the installed surface

Code Moniker can be installed in several useful modes. Detect the capabilities
available in the current session before choosing a workflow:

1. **MCP tools available** — use the `code_moniker_*` tools as the complete
   exploration surface.
2. **No MCP tools, local binary available** — use the binary workflow below.
   This is a supported installation mode, not a degraded MCP session.
3. **Hooks configured** — treat their feedback as the project's deterministic
   write-time policy. Hooks do not replace exploration and do not imply that
   MCP is installed.
4. **Neither MCP nor binary available** — report that no Code Moniker read
   surface is installed. Do not wait for MCP and do not claim that repository
   hooks provide navigation.

For agent exploration, the `code_moniker_*` MCP tools are the complete and
canonical interface. They add deterministic output budgets, compact rendering,
response-local aliases, pagination and safe follow-up calls around the typed
query engine. When the MCP surface is configured and available, do not repeat
the same exploration with `code-moniker query`, a daemon client, grep, or a
script: that duplicates facts and consumes context.

The MCP surface is optional. If it is not configured in the current
installation or unavailable in the current session, continue immediately with
the local `code-moniker` binary instead of blocking or reporting a parity
defect. Resolve `code-moniker` from `PATH`; a checkout may use the explicit
Cargo install path. In particular, extractor work should use:

```sh
code-moniker extract . --path <file> --shape callable --limit 80
```

Always anchor `extract` on the workspace root (`.`) and narrow with `--path`;
never anchor extraction directly on the file. The binary is also valid for
`stats`, `check`, and `diff` dogfood workflows. Use `code-moniker query` only
when an advanced structural question cannot be answered by those commands, and
read `references/query-dsl.md` before composing its syntax.

If the MCP is wired, responds, and lacks a required read-only capability,
report a parity defect. That is different from an installation without MCP:
the latter must use the binary normally. Do not silently fall back to the
daemon.

## Quick start on an unknown repo

### With MCP

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

### With the local binary

1. Start with `code-moniker stats <path>` for language, definition, reference,
   resolution and concentration facts.
2. Narrow known files with
   `code-moniker extract . --path <file-or-glob> --shape callable --limit 80`.
   Keep `.` as the anchor.
3. Use `code-moniker check <scope> --profile <name> --max-violations <N>` only
   when the project or user explicitly selected that profile. Without an
   explicit profile, run `code-moniker check <scope> --max-violations <N>`.
4. Use `code-moniker diff [A..B] .` for symbolic change review.
5. Use `code-moniker query` only for an advanced structural question that
   `stats`, `extract`, `check`, and `diff` cannot answer, and read
   `references/query-dsl.md` before composing its syntax.

Do not translate the MCP workflow call-for-call into shell commands. Use the
bounded CLI primitives that answer the question and stop when evidence is
sufficient.

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
