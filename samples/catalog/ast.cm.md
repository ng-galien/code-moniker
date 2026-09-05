---
name: ast
title: AST source-structure rules
lang: ts
blurb: Reuse ordinary rule quantifiers to constrain real syntax nodes, not matching text
summary: Detect forbidden constructions, assertion budgets, JSX attributes, and unsafe blocks through the existing rule DSL.
learn_kind: pattern
learn_path: rules/ast
learn_order: 35
tags: ast,source-structure,typescript,tsx,rust,switch,casts,jsx,unsafe
published: true
---

# AST source-structure rules

The `ast` domain adds the current definition's named syntax nodes to the
ordinary local-rule context. It is not a second rule language: `count`, `any`,
`all`, `none`, boolean operators, messages, severities, and reports keep their
usual meaning.

Use it for policies that depend on a construction recognized by the selected
language grammar. These rules distinguish executable syntax from comments and
strings, and a misspelled exact `kind` fails ruleset compilation instead of
quietly matching nothing.

```toml cm:rules
default_rules = false

[[ts.module.where]]
id = "no-switch-dispatch"
expr = "none(ast, kind = 'switch_statement')"
message = "Use the project's exhaustive matcher instead of switch dispatch."
rationale = "Closed unions stay compiler-checked when dispatch goes through the project's exhaustive matcher."

[[ts.module.where]]
id = "type-assertion-budget"
expr = "count(ast, kind = 'as_expression' OR kind = 'type_assertion') <= 1"
severity = "warn"
message = "This module exceeds its type-assertion budget."
rationale = "Repeated assertions often indicate that boundary validation or exhaustive narrowing is being bypassed."

[[tsx.module.where]]
id = "no-dangerous-inner-html"
expr = "none(ast, kind = 'property_identifier' AND parent.kind = 'jsx_attribute' AND text = 'dangerouslySetInnerHTML')"
message = "Route trusted HTML through the project's rendering authority."
rationale = "Direct dangerouslySetInnerHTML use bypasses the component responsible for sanitization and rendering policy."

[[rust.fn.where]]
id = "no-unsafe-blocks"
expr = "none(ast, kind = 'unsafe_block')"
message = "Keep unsafe code behind the crate's designated low-level boundary."
rationale = "Concentrating unsafe blocks in an explicit boundary makes their invariants reviewable."
```

This dispatch contains a real `switch`. The same word also appears in a string
and a comment; those textual lookalikes are not `switch_statement` nodes:

```ts cm:file=src/dispatch.ts
export function route(kind: string) {
	const example = "switch (ignored) {}";
	// switch (also ignored) {}
	switch (kind) {
		default: return example;
	}
}
```

This separate module is the negative control: text alone must not trigger the
construction rule.

```ts cm:file=src/documentation.ts
export const switchExample = "switch (kind) { default: break; }";
// switch is discussed here, but no switch statement is executed.
```

This budget is a warning because it is a maintainability heuristic, not a syntax
error. The second assertion is the first item beyond the declared budget, so the
diagnostic points to that construction rather than to the whole module:

```ts cm:file=src/assertions.ts
const first = input as string;
const second = input as number;
```

The TSX rule combines existing `kind`, `parent.kind`, and `text` projections to
identify one attribute precisely:

```tsx cm:file=src/preview.tsx
export function Preview({ html }: { html: string }) {
	return <section dangerouslySetInnerHTML={{ __html: html }} />;
}
```

The same domain works through the Rust grammar. The safe sibling function is
evaluated normally and produces no finding:

```rust cm:file=src/raw.rs
pub fn read_raw(pointer: *const u8) -> u8 {
	unsafe { *pointer }
}

pub fn zero() -> u8 {
	0
}
```

```cm:expect
ts.module.no-switch-dispatch @ src/dispatch.ts:L4-L6
ts.module.type-assertion-budget @ src/assertions.ts:L2
tsx.module.no-dangerous-inner-html @ src/preview.tsx:L2
rust.fn.no-unsafe-blocks @ src/raw.rs:L2
```

AST kind names belong to each language grammar. In this first version,
`kind` and `parent.kind` use exact names; regex comparisons on them are
rejected. Parse errors, syntax injections inside the current scope, and symbol
ranges that do not map to one exact AST node are reported as unavailable or
inconclusive, never as an empty passing domain. AST domains nested below
another iterated domain are also rejected until those paths can propagate the
same availability contract.
An AST branch that is unreachable because an `AND`, `OR`, or implication has
already decided the result is not evaluated and produces no analysis warning.

Use graph refs, workspace rules, type-aware analysis, or control-flow analysis
for claims that are not local syntax facts. CSS support is a separate language
capability and is not implied by this recipe. See the
[complete DSL reference](../../docs/cli/check-dsl.md#quantifiers) for the exact
projections and current limits.
