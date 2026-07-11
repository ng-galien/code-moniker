# Diagnose — architecture, coupling, smells, refactor targets

Recipes for judging a codebase's health with counts instead of impressions.
All commands were run for real on multi-language projects.

## Contents

1. Coupling map (who leans on whom, how hard)
2. Boundary crossings and layering
3. Smell scan and refactor hotspots
4. Bootstrap rules on a virgin project
5. Architecture rules (dependency direction as executable law)
6. Dependency audit (declared vs used)
7. Review changes as symbol facts
8. Reading the health signals

## 1. Coupling map

`identity.graph` projects one level of the identity tree as a graph: nodes =
that level's children, edges = every resolved reference rolled up to the pair
of children it connects, with kinds and counts.

```sh
code-moniker query 'identity.graph prefix:"lang:ts/dir:apps/dir:trust/dir:src"'
```

```text
scope: …/dir:src   nodes: 4  edges: 2  unresolved refs: 6359
- dir:client  -> dir:shared x179 [implements,uses_type]
- dir:plugins -> dir:shared x84  [uses_type]
```

Read it top-down: start at `prefix:""` or `lang:*`, descend into the heavy
nodes. What to look for:

- **Heavy edges** (x100+) = load-bearing dependency; changes to the target
  ripple. Fine when it points at a `shared`/`core`; alarming between peers.
- **Bidirectional pairs** (A→B and B→A) = entanglement; a candidate seam to
  cut.
- **A node with edges to everything** = hub. Deliberate (kernel) or accreted
  (god module)? Check its def count and smells.
- **Missing edges** you expected = the layers are honestly independent, or
  the traffic goes through something outside this scope (see ports, below).

## 2. Boundary crossings

The same `identity.graph` output lists `ports_in` / `ports_out`: references
crossing the scope's boundary, aggregated at the scope's own depth
(`> lang:ts/dir:packages/… x87 [extends,uses_type]`). Use them to answer
"what does this subtree need from the outside, and who reaches into it" —
the two lists a refactor must keep stable.

## 3. Smell scan and refactor hotspots

Requires a rules file (section 4 if the project has none). If the project
defines its own packs, respect its profile conventions:

```sh
code-moniker check . --profile smells --default-rules off --report --max-violations 50
```

```text
37 violation(s) across 25 file(s) (226 scanned, 225 ms)
Failed rules:
- ts.shape.callable.smell-long-callable: 24        # ranked = your priority list
- ts.shape.callable.smell-long-parameter-list: 7
Rule report:
- …smell-long-callable: evaluated=2423, matches=2399, violations=24
```

Triage order: (1) files hit by several *different* rules at once — those are
the sick zones; (2) the top rule's worst instances (a 288-line function in a
2400-callable codebase is a real hotspot, not noise). The `--report` block
tells you how discriminating each rule is (violations vs evaluated).

Rules files are directory-scoped: run from the directory that owns the
`.code-moniker.toml` (a monorepo package may hold its own).

## 4. Bootstrap on a virgin project

```sh
code-moniker rules init .        # creates .code-moniker.toml with detected aliases
code-moniker check . --max-violations 30   # embedded default rules
code-moniker rules learn         # the full check DSL as copyable snippets
```

`rules init` writes a starter with `default_rules = true` and a commented
dependency-rule example. For a curated smell pack methodology (severity
`warn` first, promote after signal review), see the companion
`code-moniker-smell-review` skill if present.

## 5. Architecture rules as executable law

Dependency direction is checkable. The generated template shows the shape:

```toml
[[refs.where]]
id = "domain-no-infra"
expr = "source ~ '**/dir:domain/**' => NOT target ~ '**/dir:infrastructure/**'"
severity = "warn"
```

Workflow: express the intended layering as `refs.where` rules at `warn`, run
`check --report`, inspect the violations (they are either bugs in the code or
bugs in the stated architecture), migrate, then promote to `error` so the
boundary stays enforced — in CI and in agent hooks. `code-moniker rules show .
--profile <name>` prints the effective compiled set.

## 6. Dependency audit

```sh
code-moniker manifest package.json      # or Cargo.toml, or a directory for all manifests
```

One line per declared dependency (name, version, kind) with a moniker per
package. Cross-check against reality: search usages of a suspicious package's
symbols; a declared-but-never-referenced dependency is removable weight.

## 7. Review changes as symbol facts

```sh
code-moniker diff .          # HEAD..worktree
code-moniker diff A..B .
```

Reports moves, renames, body changes and retargeted references per symbol —
not lines. Use it to review your own (or another agent's) work before
staging: "what did this change *structurally*" is a different question from
"what lines moved". Non-code files are listed as `not analyzable` — that is
honesty, not failure.

## 8. Reading the health signals

- **Unresolved refs** (printed by every graph verb): the share of references
  the index could not bind. High and stable across scopes = imports of
  external packages (normal). Concentrated in one subtree = generated code,
  unusual dynamism, or an extraction gap — do not over-interpret coupling
  counts there.
- **Concentration** (`stats`, insights): a few files holding a large share of
  defs/refs = where reviews and refactors pay off first.
- **shared_helper_signal** (usages diagnostics via MCP): `localized_not_shared`
  means the symbol's consumers cluster in one prefix — safe to reshape;
  a genuinely shared helper deserves contract-level care.
- **Zero-usage exported symbols**: entry points or dead exports. Decide with
  the ego graph, not alone.
