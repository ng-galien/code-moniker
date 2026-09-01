---
name: typescript-family-namespaces
title: TypeScript-family rule namespaces
summary: Compare TS, TSX, JS, and JSX policies in one executable scenario without merging their rule namespaces.
learn_kind: language
learn_path: languages/typescript/namespaces
learn_order: 5
tags: typescript,tsx,javascript,jsx,namespaces
published: true
---

# TypeScript-family rule namespaces

The TypeScript family shares an extraction and linkage pipeline, but each file
variant keeps its own rule namespace. This compact scenario proves that
`ts.*`, `tsx.*`, `js.*`, and `jsx.*` policies can differ. The TSX and JSX rules
intentionally treat every demonstrated function as a component; project packs
should narrow that proxy to `components/` or another repository-specific
signal so ordinary helpers can remain camelCase.

```toml cm:rules
default_rules = false

[[ts.function.where]]
id = "typescript-functions-camel-case"
expr = "name =~ ^[a-z][A-Za-z0-9]*$"
message = "TypeScript function `{name}` should be camelCase."

[[tsx.function.where]]
id = "tsx-components-pascal-case"
expr = "name =~ ^[A-Z][A-Za-z0-9]*$"
message = "This TSX component `{name}` should be PascalCase."

[[js.function.where]]
id = "javascript-functions-camel-case"
expr = "name =~ ^[a-z][A-Za-z0-9]*$"
message = "JavaScript function `{name}` should be camelCase."

[[jsx.function.where]]
id = "jsx-components-pascal-case"
expr = "name =~ ^[A-Z][A-Za-z0-9]*$"
message = "This JSX component `{name}` should be PascalCase."
```

```ts cm:file=src/account.ts
export function LoadAccount() {}
```

```tsx cm:file=src/AccountCard.tsx
export function accountCard() {
	return <section />;
}
```

```js cm:file=src/account.js
export function LoadAccount() {}
```

```jsx cm:file=src/AccountCard.jsx
export function accountCard() {
	return <section />;
}
```

```cm:expect
ts.function.typescript-functions-camel-case @ src/account.ts:L1
tsx.function.tsx-components-pascal-case @ src/AccountCard.tsx:L1-L3
js.function.javascript-functions-camel-case @ src/account.js:L1
jsx.function.jsx-components-pascal-case @ src/AccountCard.jsx:L1-L3
```
