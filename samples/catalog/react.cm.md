---
name: react
title: React + TypeScript client SPA conventions
lang: tsx
blurb: Opt-in TS/TSX conventions for components, hooks, layers, and a browser entrypoint
learn_kind: framework
learn_path: languages/typescript/tsx/react
learn_order: 10
tags: react,tsx,components,hooks,entrypoint
published: true
---

# React client SPA conventions

This opt-in pack targets a conventional client-rendered React SPA. Component functions use
PascalCase, custom hooks live in the hooks layer, pages do not call adapters
directly, and `react-dom` stays in the browser entrypoint.

The checks do not identify React components, hooks, router conventions, or
server components semantically. Directory location, a `use[A-Z]` name, and a
root-level entry module are configurable proxies. Next.js, Remix, React Server
Components, and other server or routed stacks need their own project rules.
Open the routed/server child for conservative directory and import proxies.

```toml cm:rules
default_rules = false

[aliases]
component_src = "moniker ~ '**/dir:components/**'"
hook_src = "moniker ~ '**/dir:hooks/**'"
src_entry = "source ~ '**/dir:src/module:/^(main|index|client|entry)$' OR source ~ '**/dir:src/module:/^(main|index|client|entry)$/**'"

src_pages = "source ~ '**/dir:pages/**'"
tgt_adapters = "target ~ '**/dir:adapters/**'"
tgt_react_dom = "target ~ '**/external_pkg:react-dom/**'"

[[tsx.function.where]]
id = "component-pascalcase"
rationale = "A capitalized function identifier is treated as a component in JSX. This directory convention keeps component candidates distinct from helpers and intrinsic tags."
expr = "$component_src AND NOT name =~ ^use[A-Z].* => name =~ ^[A-Z][A-Za-z0-9]*"
message = "React component `{name}` must use PascalCase."

[[tsx.function.where]]
id = "hooks-live-in-hooks"
rationale = "Custom hooks carry shared stateful behavior. Keeping `use*` functions in hooks/ makes reuse and testing explicit."
expr = "name =~ ^use[A-Z].* => $hook_src"
message = "Custom hook `{name}` must live under hooks/."

[[ts.function.where]]
id = "hooks-live-in-hooks"
rationale = "Hooks without JSX normally live in .ts files. The same name-and-directory policy therefore belongs to both TypeScript namespaces."
expr = "name =~ ^use[A-Z].* => $hook_src"
message = "Custom hook `{name}` must live under hooks/."

[[refs.where]]
id = "pages-do-not-call-adapters"
rationale = "Pages should compose UI and data flow, not bind directly to transport or persistence adapters."
expr = "$src_pages => NOT $tgt_adapters"
message = "React pages must not depend directly on adapters."

[[tsx.refs.where]]
id = "react-dom-entrypoint-only"
rationale = "`react-dom` bootstraps the application. Importing it from components or pages couples render code to the browser entrypoint."
expr = "kind = 'imports_symbol' AND $tgt_react_dom => $src_entry"
message = "`react-dom` imports are only allowed from the React entrypoint."
```

This pack deliberately treats non-hook functions below `components/` as
component candidates. A colocated helper would therefore be a false positive;
move helpers outside that directory or narrow the alias. The button component
is lower-case, which makes JSX readers wonder whether it
is an intrinsic element or an application component:

```tsx cm:file=src/components/save_button.tsx
export function save_button() {
	return <button>Save</button>;
}
```

The custom hook is useful, but it is defined beside components instead of in
the hooks layer:

```tsx cm:file=src/components/profile_card.tsx
export function useProfileCard() {
	return { name: "Ada" };
}

export function ProfileCard() {
	const profile = useProfileCard();
	return <section>{profile.name}</section>;
}
```

A hook that does not render JSX normally belongs to TypeScript rather than
TSX; it must still follow the same layer policy:

```ts cm:file=src/components/use_account.ts
export function useAccount() {
	return { id: "account-1" };
}
```

The page reaches into the adapter layer directly. It also imports `react-dom`,
which should only appear in the entrypoint:

```tsx cm:file=src/pages/home.tsx
import { createRoot } from "react-dom/client";

import { fetchProfile } from "../adapters/profile_api";
import { ProfileCard } from "../components/profile_card";

export function HomePage() {
	createRoot(document.body);
	fetchProfile();
	return <ProfileCard />;
}
```

Adapters are allowed to stay behind the page boundary:

```ts cm:file=src/adapters/profile_api.ts
export function fetchProfile() {
	return { name: "Ada" };
}
```

The real React entrypoint is the only place where `react-dom` belongs:

```tsx cm:file=src/main.tsx
import { createRoot } from "react-dom/client";

import { HomePage } from "./pages/home";

createRoot(document.body).render(<HomePage />);
```

```cm:expect
tsx.function.component-pascalcase @ src/components/save_button.tsx:L1-L3
tsx.function.hooks-live-in-hooks @ src/components/profile_card.tsx:L1-L3
ts.function.hooks-live-in-hooks @ src/components/use_account.ts:L1-L3
refs.pages-do-not-call-adapters @ src/pages/home.tsx:L3
tsx.refs.react-dom-entrypoint-only @ src/pages/home.tsx:L1
refs.pages-do-not-call-adapters @ src/pages/home.tsx:L8
```

The two page-boundary findings come from separate graph edges: the import and
the later call are evaluated independently even though they represent the same
architectural dependency.
