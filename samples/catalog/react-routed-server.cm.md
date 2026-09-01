---
name: react-routed-server
title: Routed and server React boundaries
lang: tsx
blurb: Use conservative directory and import proxies for Next.js, Remix, and server-rendered React
learn_kind: framework
learn_path: languages/typescript/tsx/react/routed-server
learn_order: 10
tags: typescript,tsx,react,nextjs,remix,rsc,server,client
published: true
---

# Routed and server React boundaries

Next.js, Remix, and React Server Components have framework- and version-specific
semantics that Code Moniker does not infer. Start with repository evidence you
control: server-owned directories and imports of browser-only packages. This
example treats `server/` as server-owned and keeps the browser-only
`react-dom/client` entrypoint out of it.

The rule does not prove that a module is a React Server Component, understand a
`use client` directive, or model a framework router. It therefore does not use
client markers as a signal. Adapt the directory alias to the actual stack;
for example, add a `routes/` path only when the repository proves that those
modules are server-owned.

```toml cm:rules
default_rules = false

[aliases]
src_server = "source ~ '**/dir:server/**'"
tgt_react_dom_client = "target ~ '**/external_pkg:react-dom/path:client/**'"

[[tsx.refs.where]]
id = "server-no-react-dom-client"
expr = "$src_server AND kind = 'imports_symbol' => NOT $tgt_react_dom_client"
message = "Server-owned TSX should not import react-dom/client."
```

```tsx cm:file=src/server/routes/account.tsx
import { createRoot } from "react-dom/client";

export function AccountRoute() {
	createRoot(document.body);
	return <section />;
}
```

```cm:expect
tsx.refs.server-no-react-dom-client @ src/server/routes/account.tsx:L1
```

`react-dom/server` is a legitimate server-rendering API and remains allowed:

```tsx cm:file=src/server/render.tsx
import { renderToPipeableStream } from "react-dom/server";

export function render() {
	return renderToPipeableStream(<main />, {});
}
```
