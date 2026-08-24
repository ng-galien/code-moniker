---
name: code-moniker
description: >-
  Use Code Moniker for targeted structural exploration when indexed facts add
  value: architecture, callers and callees, coupling, ownership, change impact,
  codebase-wide mechanisms, project-rule authoring or interpretation, or
  diagnosis of Code Moniker itself. Do not invoke it automatically for
  known-file edits, exact-string lookup, routine Git or test work, small local
  diffs, or every turn in a repository that happens to expose Code Moniker.
---

# Code Moniker

Code Moniker provides a symbolic index: stable symbol identities, definitions,
references and relationships. It is useful when the question is relational or
workspace-wide. It is not a mandatory wrapper around ordinary repository work.

## Decision boundary

Use Code Moniker when at least one of these is true:

- the code or repository area is unfamiliar and needs a structural map;
- the answer depends on callers, callees, coupling, hierarchy or change impact;
- a repository-wide mechanism owner and its consumers must be identified;
- project rules must be written, aligned, or read as architectural memory;
- the user explicitly asks for Code Moniker or for indexed symbolic evidence;
- Code Moniker, its daemon, MCP surface, hooks or indexed generation is itself
  under diagnosis.

Do not use it merely because it is installed. Prefer normal repository tools
for exact strings in known files, file inventories, Git state, formatting,
focused tests, direct error messages, straightforward local edits and small
diffs whose relevant contract is already visible. A review agent should invoke
Code Moniker only when the review question actually needs relationship or
workspace-wide ownership evidence.

Use the smallest number of calls that answers the question. Stop when the
evidence is sufficient.

## Rules as project memory

Project rules serve two distinct workflows: an author records a durable
invariant after learning or changing the code, while a later agent reads the
corpus to acquire project-specific seniority before modifying that area. In
both cases, read `references/rules.md` before acting on rule ids, taxonomy,
aliases, rationales, corpus diagnostics, or rule history.

Do not treat a rule as a prohibition without context. Its natural-language id,
semantic anchors, aliases, executable expression, rationale, origin and
optional Git history form one testimony about the project. Keep static corpus
classification separate from indexed evidence that a rule currently covers a
zone or symbol.

## Select one surface

1. If `code_moniker_*` MCP tools are available, use them for the selected
   exploration. Do not repeat the same exploration with the CLI or raw daemon
   requests.
2. If MCP is unavailable but the local binary exists, use its bounded
   `stats`, `extract`, `diff` or `check` commands.
3. Hooks are write-time policy only. They neither replace exploration nor
   prove anything about daemon state.
4. If neither MCP nor the binary is available, report that briefly and use the
   best normal repository inspection available.

## Workspace identity and freshness

Verify `expected_roots` once before the first workspace-wide MCP exploration,
after an MCP reconnect/restart, after roots change, or when a tool reports a
workspace mismatch. Do not repeat a workspace bootstrap on every turn or before
every targeted call in the same verified session.

If the relevant file or symbol scope is already known and the current MCP
connection is verified, start directly with a narrow `code_moniker_symbols`,
`code_moniker_usages` or `code_moniker_graph` call. `workspace/views` is for
unknown-repository or explicit architecture-view work, not a universal prelude.

Never infer caching, refresh or stale-state behavior from latency alone. Record
the producer, exact surface, workspace roots, generation or lifecycle state,
scope and triggering event before diagnosing invalidation.

## Bounded MCP workflow

- Unknown workspace: one `code_moniker_read` on `workspace` with
  `expected_roots`, `budget:"small"`, shallow depth and a tight limit.
- Known scope: use `code_moniker_symbols` with path/name/kind/shape filters.
- Relationship question: pass a returned moniker when available. The tools also
  accept an unambiguous bare name or `lang:path.kind:name` reference and return
  candidates instead of guessing when that natural reference is ambiguous.
- Structural edit with uncertain impact: use `code_moniker_context` once on the
  selected symbol or file. Skip it for local edits with known consumers.
- Project-defined architecture view: read `workspace/views` only when that view
  is relevant to the question. Follow a returned `workspace/views/<view.id>`
  call; do not build that URI from a fragment name or file path. See
  `references/fragments.md`.
- Advanced daemon query: use `code_moniker_query` only when no intent tool
  covers the required read-only capability, and discover the live grammar
  before composing a query.
- Rules: use `code_moniker_rules` only for a requested or applicable rule
  evaluation, not as a generic completion ritual. For rule-led discovery or
  authoring, follow `references/rules.md`.

Keep `compact:true`, a small budget and narrow limits by default. Request code,
larger budgets, paging or broader scope only when the current result proves it
is necessary.

## Local binary workflow

- `code-moniker stats <path>` for bounded language and concentration facts.
- `code-moniker extract . --path <file-or-glob> --shape callable --limit 80`
  for known files. Always anchor extraction on the workspace root `.`.
- `code-moniker diff [A..B] .` for a genuinely structural change review.
- `code-moniker check <scope> --profile <name> --max-violations <N>` only when
  the project or user selected that profile.

Do not translate an MCP sequence call-for-call into shell commands.

## Provenance and interpretation

CLI, hooks, MCP, daemon clients and extensions are different execution
surfaces. Attribute findings only to the surface actually exercised. Keep
indexed facts separate from architectural judgment, and report coverage or
truncation literally.

## Deeper references

Read only the reference needed for the current task:

- unfamiliar-code exploration: `references/explore.md`;
- architecture language and contextual views: `references/architecture.md`;
- rule authoring, rule-led discovery, aliases, corpus diagnostics and history:
  `references/rules.md`;
- fragment files, view URIs, and rule id namespaces: `references/fragments.md`;
- health, coupling and smell diagnosis: `references/diagnose.md`;
- detailed MCP contracts and budgets: `references/mcp.md`;
- developer-only query grammar: `references/query-dsl.md`.
