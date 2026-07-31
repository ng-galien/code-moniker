---
name: code-moniker
description: >-
  Explore and diagnose any codebase through code-moniker's symbolic index,
  using MCP tools when configured and otherwise the local binary. ALWAYS use
  this before grep/Glob/Read for architecture, module structure, coupling,
  dependencies, call graphs, callers/callees, change impact, code smells,
  refactor targets, codebase health, structural diff review, or interpretation
  of Code Moniker check, hook, MCP, and daemon output. First identify the exact
  execution surface; never infer daemon caching or incremental behavior from
  CLI or hook output. Typical requests include mapping architecture, finding
  heavy coupling, tracing calls, assessing a refactor, reviewing structural
  changes, or explaining why a check scanned zero files. Zero project
  configuration on ts/rs/java/python/go/cs/sql projects.
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
compact monikers, pagination and safe follow-up calls around the typed
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

## Establish output provenance before diagnosis

Treat CLI, hook, MCP, and daemon output as evidence from different execution
paths. Before naming a cache, invalidation, incremental, or stale-state bug,
record the producer, exact command or tool arguments, workspace and file scope,
rules source, and triggering event. If any of those are unknown, label the
diagnosis as a hypothesis and reproduce it; do not assign it to the daemon.

| Producer | Runtime contract | What an empty or changed result proves |
|---|---|---|
| Direct `code-moniker check` | Starts a one-shot filesystem check and loads the selected rules for that invocation. It does not query a running daemon. | Only that command, rules source, and scope. With `--file`, zero matching files is an intentional filtered result. |
| Generated agent hook | Reads the current write-tool payload, keeps existing touched paths, exits `0` before invoking `check` when none remain, otherwise runs direct `check <scope> --file ...`. | Silence or zero files can describe the tool payload; it does not prove cached rules, a workspace verdict, or daemon state. |
| `code_moniker_*` MCP tool | Queries the verified MCP workspace through its daemon-backed indexed surface. `code_moniker_rules action:"run"` loads the requested rules now and evaluates the source corpus pinned to the reported daemon generation. | Only the requested MCP scope at the reported workspace generation. Verify `expected_roots` before interpreting it. |
| `code-moniker query --daemon <ENDPOINT>` | Targets the exact daemon endpoint printed by `daemon list`; it never starts another daemon and never falls back to the filesystem. `rules.check` evaluates that daemon's pinned indexed corpus. | The response belongs to the selected daemon and its reported generation. |
| Daemon client, TUI, or extension | Uses an explicitly daemon-backed consumer and its indexed generation. | Daemon invalidation is a candidate only after this path and its generation are confirmed. |

Reproduce on the surface under suspicion. For a rules-file invalidation claim,
run the exact direct CLI command twice, changing only that rules file, and
capture both exit codes and reports. For a hook claim, replay the actual hook
payload or generated hook. Compare full checks only with full checks and
file-scoped checks only with file-scoped checks. A differently named
`--rules` file is standalone; only the canonical `.code-moniker.toml` root
discovers `code-moniker.fragment.toml` descendants.

When testing architecture heuristics on the indexed corpus,
`workspace.group.expr` can combine boolean logic, `count(member)`, and
descriptive aggregates over `(member, lines)`. Use a sample-size implication,
for example `count(member) >= 8 => gini(member, lines) <= 0.65`, and default
heuristics to warning severity. The currently indexed statistical projection
is only inclusive symbol `lines`; do not infer support for arbitrary
projections, entropy over linkage, history, or z-scores. A missing member line
range fails closed and is reported with available/total coverage. Boolean
composition is order-independent: a known false `AND` operand or known true
`OR` operand decides the result; otherwise unavailability propagates.

## Quick start on an unknown repo

### With MCP

1. Call `code_moniker_read uri:"workspace" expected_roots:["<current absolute
   workspace root>"] budget:"small"` for a bounded overview and a fail-closed
   workspace identity check. Stop immediately on `workspace_mismatch`. Stop if
   the overview answers the question.
2. Narrow with `code_moniker_symbols` (`path`, `lang`, `shape`, `name`, small
   `limit`). Never invent a moniker.
3. Use `code_moniker_usages` or `code_moniker_graph` only for the selected
   returned compact moniker or file.
4. Request `code_moniker_read uri:"<file-or-returned>" ast:true` only when the
   parser shape itself is required; keep the bounded named-node defaults.
5. Before a structural edit, call `code_moniker_context focus:"<returned>"`
   once. It combines impact, notes, applicable rules, local changes and checks.
6. Use `code_moniker_query` only for an advanced read-only verb not covered by
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
- **Agent MCP contract, budgets and compact monikers** → `references/mcp.md`
- **Developer-only query grammar and dogfood** → `references/query-dsl.md`

## Rules that save you a failed call

- **Never guess a moniker or a focus path.** Get compact monikers from
  `code_moniker_symbols` and pass them exactly; a guessed one returns
  `symbol_not_found` / `focus_not_found`.
- **Compact monikers are reusable.** The default `rs:...`, `java:...`, etc.
  form can be passed directly to symbol tools. Canonical URIs and symbol ids
  remain accepted; generated calls preserve the active compact or canonical
  mode.
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
