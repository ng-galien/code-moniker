---
name: languages
title: Language tags, rule namespaces, and shared ecosystems
summary: Browse language-specific rule packs and choose exact tags; shared analysis ecosystems can still keep independent policies.
learn_kind: general
learn_path: languages
published: true
---

# Language Tags, Rule Namespaces, And Shared Ecosystems

Language-specific recipes appear at the start of this page. Language tags are
policy boundaries: a rule under `[[ts.function.where]]` runs
only on TypeScript; it does not silently apply to TSX, JavaScript, or JSX.
List canonical parser tags:

```sh
code-moniker langs
```

Inspect the kinds and visibilities exposed by one tag:

```sh
code-moniker langs tsx
```

Replace `tsx` with any parser tag from the table.

Supported parser tags and rule prefixes:

| Parser tag | Files | Rule prefix |
| --- | --- | --- |
| `ts` | `.ts`, `.mts`, `.cts` | `ts.*` |
| `tsx` | `.tsx` | `tsx.*` |
| `js` | `.js`, `.mjs`, `.cjs` | `js.*` |
| `jsx` | `.jsx` | `jsx.*` |
| `rs` | `.rs` | `rust.*` |
| `java` | `.java` | `java.*` |
| `python` | `.py` | `python.*` |
| `go` | `.go` | `go.*` |
| `c` | `.c`, `.h` | `c.*` |
| `cs` | `.cs` | `cs.*` |
| `sql` | `.sql` | `sql.*` |

The TypeScript family shares one extraction and linkage pipeline. TS and JS use
the TypeScript grammar; TSX and JSX select its JSX-capable grammar. Imports can
still resolve across file variants, while monikers and rule sections remain
distinct so a component convention does not become a plain TypeScript or
JavaScript convention by accident.

`plpgsql` is accepted by on-demand syntax parsing and by injected PostgreSQL
bodies, but it is not an autonomous graph-rule namespace; graph checks over
database definitions use `sql.*`.

Open the TypeScript-family namespaces child for one compact executable
comparison of TS, TSX, JS, and JSX. Then use the language and framework guides
for project-ready recipes with narrower directory and stack signals.

This small smoke check also proves the boundary directly: the `ts.*` rule
reports the plain TypeScript function, but it does not run on the identically
named TSX function.

```toml cm:rules
default_rules = false

[[ts.function.where]]
id = "plain-typescript-camel-case"
expr = "name =~ ^[a-z][A-Za-z0-9]*$"
message = "Plain TypeScript function `{name}` should be camelCase."
```

```ts cm:file=src/account.ts
export function LoadAccount() {}
```

```tsx cm:file=src/account.tsx
export function LoadAccount() {
	return <section />;
}
```

```cm:expect
ts.function.plain-typescript-camel-case @ src/account.ts:L1
```
