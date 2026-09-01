---
name: jsx
title: JSX component conventions
lang: jsx
blurb: JSX component and hook policies remain independent from TSX while sharing language-family linkage
learn_kind: language
learn_path: languages/javascript/jsx
learn_order: 10
tags: javascript,jsx,react,components,hooks
published: true
---

# JSX component conventions

JSX uses `jsx.*`; TSX rules do not apply to it. This recipe treats functions
under `components/` as components that must be PascalCase, and functions under
`hooks/` as hooks that must start with `use`. These are directory conventions,
not semantic React component detection: keep unrelated helpers outside those
folders or adapt the aliases to the project.

```toml cm:rules
default_rules = false

[aliases]
component = "moniker ~ '**/dir:components/**'"
hook = "moniker ~ '**/dir:hooks/**'"

[[jsx.function.where]]
id = "component-pascal-case"
expr = "$component => name =~ ^[A-Z][A-Za-z0-9]*$"
message = "JSX component `{name}` should be PascalCase."

[[jsx.function.where]]
id = "hook-use-prefix"
expr = "$hook => name =~ ^use[A-Z][A-Za-z0-9]*$"
message = "JSX hook `{name}` should start with use."
```

```jsx cm:file=src/components/accountCard.jsx
export function accountCard({ name }) {
	return <strong>{name}</strong>;
}
```

```jsx cm:file=src/hooks/account.jsx
export function account() {
	return null;
}
```

```cm:expect
jsx.function.component-pascal-case @ src/components/accountCard.jsx:L1-L3
jsx.function.hook-use-prefix @ src/hooks/account.jsx:L1-L3
```
