---
name: taxonomy
title: Project taxonomy and architectural testimony
summary: Classify natural rule ids with project patterns and components, connect them to code with aliases, and interpret diagnostics without gaming their counts.
learn_kind: general
learn_path: rules/taxonomy
learn_order: 100
---

# Project Taxonomy And Architectural Testimony

The canonical project `.code-moniker.toml` can declare the closed vocabulary
used by architectural rule ids. A **pattern** names the architectural
relationship or form being protected, such as dependency, ownership, call
flow, or hygiene. A **component** names a concrete concept in this project. It
may be a crate or module, but it may also be a runtime, product surface,
bounded context, protocol, or other stable project area.

A classified id is a natural kebab-case statement containing exactly one
declared pattern and at least one declared component. The anchors may appear in
any order; do not force a mechanical prefix or add vocabulary merely to make a
counter reach zero.

```toml cm:rules
default_rules = false

[rules.taxonomy]
patterns = ["dependency", "hygiene"]
components = ["code", "workspace", "index@workspace"]

[aliases]
workspace_source = "source ~ '**/dir:workspace/**' AND kind = 'imports_symbol'"
index_at_workspace_target = "target ~ '**/dir:index/**'"

[[refs.where]]
id        = "workspace-dependency-avoids-index@workspace"
expr      = "$workspace_source => NOT $index_at_workspace_target"
message   = "Workspace code must not import the Workspace Index directly."
rationale = "The Workspace Index is reached through the project boundary rather than coupled to interactive workspace code."

[[ts.function.where]]
id        = "code-hygiene-rejects-placeholder-names"
expr      = "name != 'placeholder'"
message   = "Replace the placeholder function name with project vocabulary."
rationale = "This generic hygiene predicate is already readable and does not need a fabricated alias."
```

```ts cm:file=src/workspace/editor.ts
import { snapshot } from "../index/snapshot";

export function placeholder() {
  return snapshot;
}
```

```ts cm:file=src/index/snapshot.ts
export const snapshot = {};
```

## Scoped Components Are Atomic

`index@workspace` names one component in the context of `workspace`. It does
not also classify the rule as the independent components `index` and
`workspace`, and `@` does not imply ownership or dependency. Declare and filter
the scoped component as one exact term. In aliases it is normalized with
`_at_`, so `index@workspace` matches `$index_at_workspace_target`.

## Aliases Connect Taxonomy To Code

Project-specific zones and symbols should normally have semantic aliases such
as `$workspace_source`. Aliases keep the rule navigable before expansion and
give readers coordinates into the code.

Do not manufacture an alias for every predicate:

- a generic alias such as `$public` or `$imports` may legitimately carry no
  component anchor;
- a simple metric or hygiene rule may legitimately use no alias;
- a component belongs in an alias or rule id only when it is material to the
  architectural testimony.

Corpus diagnostics are review aids. `rule-uses-no-alias` asks whether a
project-specific selector is hidden in the expression; it does not require an
alias when the expression is already generic and clear. Likewise,
`alias-has-no-taxonomy-anchor` can describe a legitimate generic alias. Review
the affected rule instead of optimizing the diagnostic count.
Reaching zero is not a conformance target.

## Keep Four Kinds Of Evidence Separate

- **Taxonomy conformance** says whether rule ids contain the declared anchors
  and whether aliases align with them.
- **Executable coverage** says what the current index and selectors actually
  evaluate; implication antecedent counts help reveal empty rules.
- **Historical context** explains why the rule and its protected boundary were
  introduced.
- **Architectural interpretation** is the evidence-backed human judgment that
  the vocabulary and invariant describe the project correctly.

A classified rule with zero violations is not proof of exhaustive coverage.
Use `rules show --details` to read its aliases, expressions, message, and
rationale, then use `check --report` to inspect current executable evidence.

```cm:expect
refs.workspace-dependency-avoids-index@workspace @ src/workspace/editor.ts:L1
ts.function.code-hygiene-rejects-placeholder-names @ src/workspace/editor.ts:L3-L5
```
