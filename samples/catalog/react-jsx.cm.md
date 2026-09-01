---
name: react-jsx
title: React + JavaScript client conventions
lang: jsx
blurb: Apply React component and hook policies to JS and JSX without inheriting TSX rules
learn_kind: framework
learn_path: languages/javascript/jsx/react
learn_order: 10
tags: javascript,jsx,react,components,hooks,entrypoint
published: true
---

# React + JavaScript client conventions

React projects written in JavaScript need both namespaces: JSX component
candidates use `jsx.*`, while hooks that render no JSX normally live in `.js`
and use `js.*`. These are directory and naming proxies, not semantic React
detection. Imports from `react-dom/client` are kept in explicit browser
entrypoints for both JavaScript namespaces.

```toml cm:rules
default_rules = false

[aliases]
component_src = "moniker ~ '**/dir:components/**'"
hook_src = "moniker ~ '**/dir:hooks/**'"
src_entry = "source ~ '**/dir:src/module:/^(main|index|client|entry)$' OR source ~ '**/dir:src/module:/^(main|index|client|entry)$/**'"
tgt_react_dom_client = "target ~ '**/external_pkg:react-dom/path:client/**'"

[[jsx.function.where]]
id = "component-pascal-case"
expr = "$component_src AND NOT name =~ ^use[A-Z].* => name =~ ^[A-Z][A-Za-z0-9]*$"
message = "React JSX component candidate `{name}` should use PascalCase."

[[js.function.where]]
id = "hook-in-hooks-directory"
expr = "name =~ ^use[A-Z].* => $hook_src"
message = "React hook `{name}` should live under hooks/."

[[jsx.function.where]]
id = "hook-in-hooks-directory"
expr = "name =~ ^use[A-Z].* => $hook_src"
message = "React hook `{name}` should live under hooks/."

[[js.refs.where]]
id = "react-dom-client-entrypoint-only"
expr = "kind = 'imports_symbol' AND $tgt_react_dom_client => $src_entry"
message = "`react-dom/client` imports are only allowed from a React entrypoint."

[[jsx.refs.where]]
id = "react-dom-client-entrypoint-only"
expr = "kind = 'imports_symbol' AND $tgt_react_dom_client => $src_entry"
message = "`react-dom/client` imports are only allowed from a React entrypoint."
```

```jsx cm:file=src/components/account_card.jsx
export function account_card() {
	return <section />;
}
```

```js cm:file=src/components/use_account.js
export function useAccount() {
	return { id: "account-1" };
}
```

Hooks may also be authored in JSX; they follow the same directory policy:

```jsx cm:file=src/components/use_preferences.jsx
export function usePreferences() {
	return { theme: "system" };
}
```

Both a JSX module and a plain JavaScript module outside the configured
entrypoints violate the `react-dom/client` boundary:

```jsx cm:file=src/pages/home.jsx
import { createRoot } from "react-dom/client";

export function Home() {
	createRoot(document.body);
	return <main />;
}
```

```js cm:file=src/bootstrap/render.js
import { createRoot } from "react-dom/client";

export function render(node) {
	return createRoot(node);
}
```

The real JSX entrypoint remains valid:

```jsx cm:file=src/main.jsx
import { createRoot } from "react-dom/client";

createRoot(document.body).render(<main />);
```

```cm:expect
jsx.function.component-pascal-case @ src/components/account_card.jsx:L1-L3
js.function.hook-in-hooks-directory @ src/components/use_account.js:L1-L3
jsx.function.hook-in-hooks-directory @ src/components/use_preferences.jsx:L1-L3
jsx.refs.react-dom-client-entrypoint-only @ src/pages/home.jsx:L1
js.refs.react-dom-client-entrypoint-only @ src/bootstrap/render.js:L1
```
