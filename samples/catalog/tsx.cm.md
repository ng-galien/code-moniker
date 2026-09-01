---
name: tsx
title: TSX language conventions
lang: tsx
blurb: Apply TSX policies without assuming that every TSX file belongs to React
learn_kind: language
learn_path: languages/typescript/tsx
learn_order: 10
tags: typescript,tsx,jsx,components,naming
published: true
---

# TSX language conventions

`.tsx` files use the `tsx.*` rule namespace. They share TypeScript-family
extraction and linkage with `.ts`, `.js`, and `.jsx`, but select the JSX-capable
grammar and do not inherit rules from those other namespaces.

TSX is not synonymous with React: Preact, Solid and custom JSX runtimes use the
same syntax. Put framework-neutral policies here and open the React child only
for conventions that genuinely depend on a React project layout.

Inspect the available TSX kinds and visibilities:

```sh
code-moniker langs tsx
```

This example uses a directory as an explicit component signal. It does not
claim semantic component detection, so helpers should live elsewhere or use a
narrower project alias.

```toml cm:rules
default_rules = false

[aliases]
component_src = "moniker ~ '**/dir:components/**'"

[[tsx.function.where]]
id = "component-pascal-case"
expr = "$component_src => name =~ ^[A-Z][A-Za-z0-9]*$"
message = "TSX component candidate `{name}` should use PascalCase."
```

```tsx cm:file=src/components/account_card.tsx
export function account_card() {
	return <section />;
}
```

```cm:expect
tsx.function.component-pascal-case @ src/components/account_card.tsx:L1-L3
```
