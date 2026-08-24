# Rules — write and read project memory

Project rules are executable architectural memory. They preserve decisions,
dangerous boundaries and project vocabulary discovered by previous agents so a
later agent can regain context without reconstructing the design from scratch.
They are not merely a list of forbidden operations.

Use this reference in two ordered developer modes:

- **Discover and develop**: orient on arrival from the project taxonomy and
  corpus, then follow relevant rules and aliases into indexed code before
  choosing a change.
- **Maintain testimony** after development by updating, adding or removing
  rules when the project knowledge or executable invariant changed.

The same rule supports both modes through five connected forms of evidence:

| Element | What it contributes |
|---|---|
| Id | A natural, compact statement of the enforced invariant |
| Taxonomy anchors | The architectural pattern and concrete project components involved |
| Aliases | Named coordinates from the rule to actual code zones or symbols |
| Expression | The executable claim the rule really checks |
| Message and rationale | The failure explanation, design reason and risk of regression |

Origin, fragment/view context, indexed coverage and Git history add provenance.

## Shared grammar

The canonical project `.code-moniker.toml` may declare closed vocabularies:

```toml
[rules.taxonomy]
patterns = ["ownership", "dependency", "call-flow", "lifecycle"]
components = ["mcp", "daemon", "workspace", "index", "index@workspace"]
```

A rule id is first a natural kebab-case statement. Patterns and components are
semantic anchors inside that statement, not ordered syntax slots. A component
may carry one explicit context as `component@scope`; for example,
`index@workspace` identifies the Workspace Index without also classifying the
rule as the independent `index` and `workspace` components.

A classified project id should contain:

- exactly one declared pattern;
- at least one declared component;
- enough articulation to state the invariant intelligibly.

Anchor order is free. Do not force a
`<pattern>-<components>-<articulation>` prefix and do not repeat a pattern as a
mechanical verb merely to satisfy a template.

```text
mcp-runtime-ownership-is-on-server
graph-corridor-call-flow-uses-roaring-bitmap-index
index@workspace-lifecycle-is-ready
workspace-status-lifecycle-is-typed
code-hygiene-rejects-placeholder-names
```

Prefer the smallest natural statement that remains understandable without
opening the expression. A longer id is useful when it carries project memory;
a short pile of nouns is not. Multiword anchors such as `call-flow`,
`dependency-injection` and `roaring-bitmap` count as their most specific
declared term. Two distinct pattern anchors are ambiguous.

Scoped components are declared vocabulary, not inferred hierarchy. Use at most
one `@` in a component term. It expresses context only, never ownership or
dependency. Direct filters and metrics count the scoped component atomically;
any future parent roll-up must be requested and reported separately.

### Warning rules as negotiation sentinels

Do not invent a `review`, `todo`, or `unclear` pattern for an architectural
question. Keep the real architectural pattern and components in the id, then
use `severity = "warn"` when the executable boundary is provisional, already
crossed, or awaiting an explicit design decision.

A negotiation warning must still detect a concrete code fact. Its rationale
must state:

- the proposed boundary and the evidence that made it worth preserving;
- the unresolved choice or known exception;
- the condition for promoting the rule to `error`, replacing it with the
  accepted invariant, or removing it.

Do not create an always-failing reminder or a warning with no executable
architectural observation. A warning is queryable architectural debt, not a
substitute for an issue description.

The taxonomy belongs to the project, not to an individual rule. Do not add a
pattern or component merely to make one historical id pass. Inspect the corpus
and discuss whether the vocabulary names a stable project concept. Components
may evolve as concrete areas become important enough to navigate and measure.

## Aliases connect testimony to code

Prefer named aliases for stable project zones, symbols and sides of a
relationship. A project-bound alias should normally carry the relevant
component anchor in snake_case:

```text
$mcp_server
$mcp_runtime_wiring
$workspace_runtime_target
$index_at_workspace_target
$daemon_graph_corridor_response
$roaring_bitmap_index
```

Normalize kebab-case taxonomy terms when reading alias names:
`roaring-bitmap` maps to `roaring_bitmap`. Match complete terms, not accidental
substrings. Preserve a scoped component with `_at_` in aliases:
`index@workspace` maps to `index_at_workspace`.

Do not manufacture an alias for every atomic predicate. Generic aliases such
as `$public`, `$imports` or `$http_runtime_target` may legitimately have no
project component. A simple hygiene rule may legitimately use no alias. The
useful discipline is:

- project-specific selectors are named instead of buried repeatedly in raw
  URI, moniker, source or target expressions;
- components named in the id can be related to aliases used by the rule;
- a component-bearing alias that introduces another architectural party is
  reflected in the rule testimony when material;
- aliases remain semantic handles, not indirection added only to satisfy a
  metric.

Read direct alias references before expression expansion. `expanded_expr`
proves executable meaning but loses the alias vocabulary that makes the rule
navigable.

## Mode 1 — Discover and develop

Start with one unfiltered corpus summary. It is a bounded vocabulary map, not a
detailed dump of every compiled rule.

1. Read the declared patterns, components, scoped components,
   pattern-by-component matrix, fragment origins and conformance summary.
   `rules show` does not require the daemon:

   ```sh
   code-moniker rules show .
   ```

   Use `rules learn taxonomy` only if the model needs explanation after seeing
   the project's actual vocabulary. Classification says the corpus follows the
   declared grammar; it does not prove that the vocabulary or architecture is
   correct.
2. Relate that vocabulary to the general workspace index, then filter by the
   task's component, pattern or exact rule and request details:

   ```sh
   code-moniker rules show . --component mcp --details
   code-moniker rules show . --pattern ownership --details
   code-moniker rules show . --rule mcp-runtime-ownership-is-on-server
   ```

3. Read the id as the decision, then inspect the aliases as entry points into
   the code. Expand only the aliases relevant to the task and follow their
   concrete modules or symbols.
4. Read the declared expression to preserve the author's local vocabulary, the
   effective expression to see fragment namespacing, and the expanded
   expression to learn the concrete enforced boundary. Read the message and
   rationale to learn why it exists and what regression previous work was
   preventing. Do not let a broad rationale overstate the executable evidence.
5. When indexed relationships matter, use the relevant project view, context,
   graph or usages call to verify current owners and consumers. Record coverage;
   a classified rule does not prove that every intended symbol is indexed or
   selected.
6. Before editing, summarize the applicable invariants and let them constrain
   candidate designs. Prefer the existing owner, call flow and dependency
   direction over a parallel mechanism.
7. After implementing and testing, return to the maintenance posture: keep,
   revise, add or remove testimony according to what the change actually taught
   or invalidated. Preserve user ownership of architectural choices.

This creates the navigable chain:

```text
taxonomy -> component -> rule -> aliases -> zones and symbols -> invariant -> rationale
```

## Mode 2 — Maintain testimony after development

Compare the implemented design with the corpus read during orientation. Revise
an existing rule when its boundary, vocabulary or rationale changed; add one
when the work establishes a durable invariant whose loss would recreate
architectural drift, duplication, unsafe ownership, an invalid dependency
direction, an important call flow, lifecycle confusion or a project-specific
hygiene failure; remove a rule when its protected decision no longer exists.
Do not create a rule merely because code changed, to restate implementation, or
to duplicate behavior already expressed more clearly by a focused test.

1. Inspect the implemented code and identify the exact guarantee worth
   preserving. Distinguish the broader design intention from what an
   executable selector can actually prove.
2. Re-read the project taxonomy and select the one architectural pattern and
   all material project components involved. Change that vocabulary only as an
   explicit project-language decision; do not invent it silently.
3. Compose a natural id containing those anchors. The id must describe the
   executable guarantee, not a stronger aspiration found only in the
   rationale.
4. Reuse or define component-qualified aliases for the concrete operands. Keep
   generic predicates generic and move repeated project-specific selectors out
   of the rule body.
5. Make the expression enforce exactly the invariant named by the id. Use the
   message for the immediate failure and the rationale for the design decision,
   historical danger and likely regression mode.
6. Inspect static corpus diagnostics. Resolve missing or ambiguous anchors and
   review alias mismatches without turning legitimate generic aliases into
   fake components.
7. Run the narrow executable check appropriate to the rule, then the configured
   broader project gate when warranted. `check ... --file ...` exercises
   file-scoped rules; omit `--file` when workspace/linkage rules must run.

An existing rule that no longer describes the desired architecture is a design
decision to revisit. Do not bypass it with an ignore or weaken it merely to
make a change pass. Present the conflict and change the rule explicitly only
when the task authorizes that decision.

## Use Git history as optional context

Git history can connect a rule to the change that introduced the component,
boundary or regression it protects. Use it when the rationale is insufficient,
the rule appears stale, its intent conflicts with the current task, or the
originating change would materially alter interpretation. It is not required
for every rule.

Useful read-only routes include:

```sh
git blame -L <start>,<end> -- .code-moniker.toml
git log -S'id = "<rule-id>"' --all -- .code-moniker.toml
git log -G'<stable rationale fragment|alias name>' --all -- <rules-file>
git show <commit> -- <rules-file> <related-code-paths>
```

For fragment rules, use the fragment path. After an id rename, search a stable
message, rationale fragment, alias or symbol name to recover earlier history.
Commit co-occurrence is evidence of context, not proof of causality; compare the
rule diff with the related code diff before drawing a conclusion.

## Interpret corpus diagnostics and metrics

Static corpus diagnostics are a migration aid. They can establish whether ids
contain known anchors, which aliases a rule names, whether rule and alias
anchors align, where project selectors remain inline, and how rules distribute
across the pattern-by-component map. They cannot judge whether an English id is
clear or whether a selector covers the intended symbols.

Keep these evidence classes separate:

- **Grammar coverage:** classified ids, alias alignment and migration
  diagnostics from rule files alone.
- **Indexed coverage:** current symbols, zones and rule applicability from a
  loaded workspace index.
- **Historical context:** commits and diffs associated with the rule and its
  protected code.
- **Architectural interpretation:** the agent's evidence-backed understanding,
  never an automatic metric.

Use diagnostics to find work, not to auto-generate ids or silently rewrite the
corpus. Natural phrasing, material components and the truth of the invariant
remain case-by-case architectural judgments.
