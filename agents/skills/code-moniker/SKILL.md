---
name: code-moniker
description: >-
  Use Code Moniker as a developer workflow: enter an unfamiliar project through
  its rule taxonomy and general index, focus structural exploration, then
  maintain the executable architectural memory affected by a change. Also use
  it for callers and callees, coupling, ownership, change impact, project-rule
  work, or diagnosis of Code Moniker itself. Do not invoke it automatically for
  routine Git, tests, exact-string lookup, or small known-file edits.
---

# Code Moniker

Code Moniker combines project-authored architectural testimony with a symbolic
index of stable identities, definitions, references and relationships. Use the
testimony to learn how the project names and constrains itself, then use the
index to test and deepen that understanding. It is not a mandatory wrapper
around ordinary repository work.

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

## Developer journey: two modes

When Code Moniker applies, follow this order. Complete the orientation once per
project context; do not repeat it before every focused call unless the rule
corpus changed.

### Mode 1 — Discover and develop

In the arriving-developer posture, build the project map before choosing a
symbol. In the focused-developer posture, narrow that map and use indexed facts
to implement the task.

#### 1. Enter through the project vocabulary

Start with the static rule map before symbolic exploration:

```sh
code-moniker rules show .
```

Read the declared patterns, components, scoped components, pattern-by-component
matrix, fragment origins and conformance summary as the project's initial
architectural map. Rule ids are compact statements in the project's ubiquitous
language. This first pass is deliberately unfiltered: it reveals what concepts
the project considers stable before the current task narrows attention.

Do not equate classification with architectural truth and do not optimize
advisory counters. If the taxonomy model itself is unfamiliar, use
`code-moniker rules learn taxonomy` for explanation after seeing the real
project map. If no project taxonomy or rule corpus exists, record that absence
and continue from the general index rather than inventing vocabulary.

The static taxonomy map is a permitted orientation companion to MCP because the
current MCP rules list does not expose the same corpus matrix and diagnostics.
If the local binary is unavailable, inspect `[rules.taxonomy]` in the canonical
project rules file and use `code_moniker_rules action:"list"` for the active
testimonies.

#### 2. Establish the general indexed map

After learning the vocabulary, inspect the workspace summary: language mix,
definition and reference scale, concentration hints and the first bounded
explorer level. With MCP, verify `expected_roots` and read `workspace`; in
binary-only mode, use `code-moniker stats .`. Relate this general index to the
taxonomy without assuming that directory names and components are identical.

#### 3. Focus the development task

Filter the corpus by the component, pattern or exact rule relevant to the task,
then request details:

```sh
code-moniker rules show . --component <name> --details
code-moniker rules show . --pattern <name> --details
```

Follow the chain from component to rules, aliases, expressions and rationales,
then use those aliases and terms to select narrow symbols, usages, graphs,
views or source reads. Call `code_moniker_context` once before a structural edit
when ownership, consumers, applicable rules or change impact remain uncertain.

### Mode 2 — Maintain architectural memory after the change

After implementing and testing, reassess the rules and taxonomy touched by the
new design. Update an existing rule when its executable boundary or rationale
changed; add one when the work established a durable non-obvious invariant;
remove one when the protected decision no longer exists. Keep unchanged rules
when the code still honors their testimony.

Any taxonomy change is an explicit project-language decision. Do not add
components, patterns or aliases merely to silence diagnostics. Re-run the
focused static corpus view and the executable check whose scope can actually
exercise the affected rule. Follow `references/rules.md` for this maintenance
posture.

## Rules as project memory

Project rules are both the entry map for an arriving developer and the durable
memory maintained by a developer who changes the architecture. Read
`references/rules.md` before acting on rule ids, taxonomy, aliases, rationales,
corpus diagnostics or rule history.

Do not treat a rule as a prohibition without context. Its natural-language id,
semantic anchors, aliases, executable expression, rationale, origin and
optional Git history form one testimony about the project. Keep static corpus
classification separate from indexed evidence that a rule currently covers a
zone or symbol.

## Select one symbolic surface

The preliminary static taxonomy view above does not replace the symbolic
surface. After it, select one surface for indexed exploration:

1. If `code_moniker_*` MCP tools are available, use them for the selected
   exploration. Do not repeat the same symbolic exploration with the CLI or raw
   daemon requests.
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

After the one-time taxonomy and general-index orientation, a known file or
symbol scope may start directly with a narrow `code_moniker_symbols`,
`code_moniker_usages` or `code_moniker_graph` call. `workspace/views` is for a
relevant project-defined lens, not a universal prelude.

Never infer caching, refresh or stale-state behavior from latency alone. Record
the producer, exact surface, workspace roots, generation or lifecycle state,
scope and triggering event before diagnosing invalidation.

## Bounded MCP workflow

- General index after taxonomy orientation: one `code_moniker_read` on `workspace` with
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
- Rules: after the static taxonomy map, use `code_moniker_rules action:"list"`
  for active testimonies and rationales or `action:"run"` for applicable
  indexed evaluation. Follow `references/rules.md`.

Keep `compact:true`, a small budget and narrow limits by default. Request code,
larger budgets, paging or broader scope only when the current result proves it
is necessary.

## Local binary workflow

- `code-moniker rules show .` for the initial project vocabulary and corpus map;
  add a component or pattern filter and `--details` only after that orientation.
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
