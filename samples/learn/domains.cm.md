---
name: domains
title: Item domains, descendants, pairs, and quantifiers
summary: Iterate item domains with any, all, none; group kinds with shape:<S>; reach nested defs with descendants(D); compare items with pairs(D) and a/b.
---

# Item Domains, Descendants, Pairs, And Quantifiers

An *item domain* is a collection of concrete local graph items. `<kind>` is the
direct child defs of that kind; `shape:<S>` groups several kinds into one
language-agnostic bucket; `descendants(D)` walks strict descendant defs so a
rule can reach nested scopes; `pairs(D)` enumerates unordered pairs of distinct
items. The quantifiers `any`, `all`, and `none` test an expression against the
items of a domain.

A `[[shape.<shape>.where]]` scope is **cross-language**: the rule below
evaluates on a Java type and a TypeScript type alike, and `shape:callable`
counts methods regardless of how each language spells them.

```toml cm:rules
default_rules = false

[[shape.type.where]]
id        = "type-callable-budget"
rationale = "shape:callable groups functions, methods, and constructors. A cross-language scope states one budget for every type."
expr      = "count(shape:callable) <= 1"

[[java.class.where]]
id        = "methods-all-public"
rationale = "all(D, P) holds when every item satisfies P. This rule flags any class with a non-public method."
expr      = "all(method, visibility = 'public')"

[[java.class.where]]
id        = "no-draw-methods"
rationale = "none(D, P) holds when no item satisfies P. This rule flags classes that declare a draw method."
expr      = "none(method, name =~ ^draw)"

[[java.class.where]]
id        = "requires-render"
rationale = "any(D, P) holds when at least one item satisfies P. This rule flags classes that lack a render method."
expr      = "any(method, name = 'render')"

[[java.class.where]]
id        = "no-overloaded-names"
rationale = "pairs(method) enumerates unordered method pairs; a and b bind the two items, so a.name = b.name detects overloaded names."
expr      = "count(pairs(method), a.name = b.name) = 0"

[[java.class.where]]
id        = "few-params-total"
rationale = "descendants(param) reaches params nested under each method, so the rule reasons across scopes the direct-child domain cannot see."
expr      = "count(descendants(param)) <= 1"
```

`Panel` declares two overloaded `draw` methods and a private `hide`, so it
trips every class rule. `Box` is TypeScript, yet the same `shape.type` budget
applies to it.

```java cm:file=src/main/java/app/Panel.java
package app;

public class Panel {
	private int state;

	public void draw(int x, int y) {}

	public void draw(int x) {}

	private void hide() {}
}
```

```ts cm:file=src/box.ts
export class Box {
  open() {}
  close() {}
}
```

```cm:expect
shape.type.type-callable-budget @ src/box.ts:L1-L4
java.class.few-params-total @ src/main/java/app/Panel.java:L3-L11
java.class.no-draw-methods @ src/main/java/app/Panel.java:L3-L11
java.class.no-overloaded-names @ src/main/java/app/Panel.java:L3-L11
java.class.requires-render @ src/main/java/app/Panel.java:L3-L11
shape.type.type-callable-budget @ src/main/java/app/Panel.java:L3-L11
java.class.methods-all-public @ src/main/java/app/Panel.java:L10
```
