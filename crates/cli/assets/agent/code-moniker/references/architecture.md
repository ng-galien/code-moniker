# Architecture review — interpret the symbolic index

Use this workflow for architecture audits, boundary analysis, refactor
reasoning, and design recommendations. Code Moniker supplies facts and
coverage; architecture remains an evidence-backed interpretation.

## Contents

- Language contract
- Project context and dynamic roles
- Evidence workflow
- Interpretation and optional lenses
- Candidates and output contract

## Language contract

Keep three registers distinct:

| Register | Use | Vocabulary |
|---|---|---|
| Code Moniker facts | Describe what the indexed generation establishes | workspace, generation, scope, symbol, moniker, definition, reference, relation, graph, caller, callee, rule, change, resolution, coverage |
| Software architecture | Interpret those facts in general terms | responsibility, boundary, contract, exposed surface, dependency, coupling, cohesion, encapsulation, ownership, invariant, testability, change propagation, risk |
| Optional lens | Test one hypothesis without imposing a doctrine | deep/shallow module, seam, adapter, leverage, locality, deletion test |

Never rename a Code Moniker output to fit an architecture theory. Never present
an interpretation or heuristic as an indexed fact.

Disambiguate overloaded terms:

- A **scope** is an indexed selection or identity subtree. Call it a **module**
  only when the project or indexed identity actually defines one; otherwise use
  **architectural unit**.
- An `interface` symbol is a language construct. An architectural **contract**
  or **exposed surface** also includes invariants, ordering, errors,
  configuration and operational constraints.
- A view **boundary** describes project intent around responsibilities. Graph
  boundary crossings measure references entering or leaving a scope.
- `ports_in` and `ports_out` are aggregated graph crossings. Do not call them
  Ports and Adapters ports unless the project actually uses that architecture.
- A **seam** is a point where behavior can vary or be substituted. It may sit
  on a boundary, but the terms are not synonyms.
- **Depth** is a qualitative relation between exposed surface and encapsulated
  behavior, not a native Code Moniker metric.

## Load project context and choose a role

After the fail-closed workspace read, list project-defined views:

```text
code_moniker_read uri:"workspace/views"
```

Follow only a relevant returned view call. Its intent, summary, ownership,
prohibitions, rules and gotchas are declared project context. Resolved symbol
and rule evidence belongs to the current index generation. Missing or ambiguous
evidence is a coverage limit, not proof that the declared architecture is
absent. If no relevant view exists, continue from the user's scope without
inventing one.

In binary-only mode, read `query-dsl.md` before the equivalent advanced query:

```text
code-moniker query -r <root> "view.read workspace/views"
```

Choose a role dynamically as a query strategy, not as a persona:

| Role | Primary question | Evidence route |
|---|---|---|
| Cartographer | What exists and how does it connect? | views, identity tree, graph, usages |
| Architect | Are responsibilities and dependency directions coherent? | views, graph crossings, rules, usages |
| Auditor | Where are health, resolution or policy risks concentrated? | rules, resolution audit, graph, coverage |
| Change reviewer | What changed and what may propagate? | diff, context, callers, applicable rules |
| Refactor designer | Which candidate concentrates responsibility safely? | all relevant facts, then optional design lenses |

Keep the role implicit in normal prose unless naming it helps the user
understand the chosen evidence route.

## Build evidence before recommendations

1. Scope from the user's module, subsystem, pain point or change. If none is
   given, use views and concentration hints before widening.
2. Read the relevant project view before interpreting generic graph topology.
3. Use `identity.graph` for one symbolic level and `code_moniker_graph` for one
   selected unit. Apply `path` before identity aggregation when production,
   tests, generated code or examples share logical identities. Use `min_count`
   and cursor pagination instead of requesting an unbounded map.
4. Use `code_moniker_usages` when caller distribution or dependency spread can
   change the conclusion. Exact usages and `include_descendants:true` owner
   roll-up answer different questions; do not add or compare their relation
   counts as if they were the same metric.
5. Use rules, resolution audit, notes, context and diff only when they answer
   the selected role's question.
6. Record generation, scope, path, filters, completeness and unresolved
   coverage. For graphs preserve total, matching and returned/emitted counts.
7. Read source only to explain behavior that indexed relations cannot establish.

Heavy edges, bidirectional pairs, hubs, scattered consumers and overlapping
rule failures are signals to investigate. None is a refactor verdict alone.

## Interpret with general architecture language

Evaluate candidates through broadly applicable concerns:

- responsibility and ownership clarity;
- cohesion inside the selected unit;
- coupling and dependency direction across boundaries;
- size and stability of the exposed contract;
- encapsulation of invariants and failure modes;
- locality of change and verification;
- testability through observable behavior;
- change propagation and operational risk.

Apply an optional lens only when it sharpens a real hypothesis:

- Use a deep/shallow lens to ask whether callers learn too much for the behavior
  they receive.
- Use the deletion test to ask whether removing an indirection concentrates
  complexity or merely redistributes it.
- Name a seam only where behavior genuinely varies.
- Recommend an adapter only for a concrete alternate implementation, test
  substitute or external dependency strategy.
- Express leverage and locality with available evidence such as caller count,
  dependency spread or change concentration.

Do not force every finding into depth, seams or adapters.

## Present candidates before designing interfaces

For a broad review, present a small candidate set before proposing detailed
interfaces. Each candidate includes:

- affected scopes and returned monikers;
- indexed facts and coverage;
- architectural friction;
- proposed direction in plain language;
- expected benefit and trade-offs;
- recommendation strength: `Strong`, `Worth exploring`, or `Speculative`.

Ask the user which candidate to deepen when the choice changes the design
space. Only then compare interface or contract alternatives.

Use this output order so provenance stays visible:

1. **Facts** — exact Code Moniker language, counts and generation.
2. **Interpretation** — general software-architecture language.
3. **Lens** — optional heuristic, explicitly named.
4. **Coverage** — omissions, unresolved references and uncertainty.
5. **Recommendation** — prioritized action and why.

Stop when the evidence answers the scoped question. Do not broaden the index or
manufacture more candidates merely to make the review look exhaustive.
