---
name: relations
title: Moniker relation operators
summary: Compare a moniker to a literal URI with @> (ancestor of), <@ (descendant of), and ?= (bind_match).
---

# Moniker Relation Operators

Besides path matching with `~`, the DSL relates a moniker to a **literal URI**:

| Op | Meaning |
| -- | ------- |
| `a @> b` | `a` is an ancestor of `b` |
| `a <@ b` | `a` is a descendant of `b` |
| `a ?= b` | `bind_match`: equal up to the last segment, which matches modulo call arity and bare name |

The right-hand side is a full `code+moniker://` URI, not a projection. `a <@ b`
is exactly `~ '<segments-of-b>/**'`. `?=` is the interesting one: it ignores
the trailing call arity, so a bare `method:alpha` binds the extracted
`method:alpha()` — this is how an arity-bearing call site resolves to its bare
definition across files.

```toml cm:rules
default_rules = false

[[ts.method.where]]
id        = "stay-in-foo"
rationale = "<@ asserts the method lives under Foo. A method declared on Bar is not a descendant, so the rule fires."
expr      = "moniker <@ 'code+moniker://./lang:ts/dir:src/module:shop/class:Foo'"

[[ts.class.where]]
id        = "contains-alpha"
rationale = "@> asserts the class is an ancestor of Foo.alpha. Only Foo qualifies; Bar is not an ancestor."
expr      = "uri @> 'code+moniker://./lang:ts/dir:src/module:shop/class:Foo/method:alpha()'"

[[ts.method.where]]
id        = "binds-alpha"
rationale = "?= binds the method moniker to a bare alpha signature. The arity-free literal still matches alpha(); beta does not match at all."
expr      = "moniker ?= 'code+moniker://./lang:ts/dir:src/module:shop/class:Foo/method:alpha'"
```

The workspace mounts `shop.ts` under `dir:src/module:shop`, so `Foo.alpha`
satisfies all three rules and the `Bar` side trips them.

```ts cm:file=src/shop.ts
export class Foo {
  alpha() {}
}

export class Bar {
  beta() {}
}
```

```cm:expect
ts.class.contains-alpha @ src/shop.ts:L5-L7
ts.method.binds-alpha @ src/shop.ts:L6
ts.method.stay-in-foo @ src/shop.ts:L6
```
