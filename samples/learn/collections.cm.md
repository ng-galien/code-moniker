---
name: collections
title: Child domains and collection predicates
summary: Use count, unique, size, subset, and multiset algebra (intersect, union, diff) over local child symbols.
---

# Child Domains And Collection Predicates

Shape and kind rules can inspect direct child domains such as `method`,
`field`, `segment`, `out_refs`, and `in_refs`. A projection like `method.name`
returns a **multiset**: duplicates are kept unless `unique(...)` removes them,
and `size(...)` turns any multiset into a number.

```toml cm:rules
default_rules = false

[[ts.class.where]]
id        = "small-class"
rationale = "A class with fewer fields and shorter methods is easier to understand before changing it."
expr      = "count(method) <= 1 AND count(field) <= 1"

[[ts.class.where]]
id        = "members-no-name-clash"
rationale = "When a method and a field share a name, readers cannot tell state from behavior at the call site. `intersect` keeps the values present in both multisets."
expr      = "size(method.name intersect field.name) = 0"

[[ts.class.where]]
id        = "fields-have-accessor"
rationale = "Every field name should also appear as a method name, so state is reached through behavior. `A subset B` holds when every value count in A is present in B."
expr      = "field.name subset method.name"

[[ts.class.where]]
id        = "fields-fully-backed"
rationale = "`size(field.name diff method.name) = 0` is the numeric form of the subset rule: `diff` subtracts the method names, and a leftover means a field has no matching method."
expr      = "size(field.name diff method.name) = 0"

[[ts.class.where]]
id        = "compact-surface"
rationale = "`union` keeps the larger count per value, so its size is the number of distinct member names. A small surface is easier to learn at a glance."
expr      = "size(method.name union field.name) <= 2"

[[java.class.where]]
id        = "unique-method-names"
rationale = "Repeated method names can hide overload-heavy APIs. This rule asks whether every method name earns its place."
expr      = "size(unique(method.name)) = size(method.name)"
```

```ts cm:file=src/cart.ts
export class Cart {
  private items = 0;
  private total = 0;

  add() {}
  items() {}
}
```

```java cm:file=src/OrderService.java
class OrderService {
  void save(String id) {}
  void save(int id) {}
}
```

Collection operators are local to the current graph item. They do not compute
transitive dependencies or history-based smells. The four multiset operators
are `intersect` (minimum count per value), `union` (maximum count per value),
`diff` (saturating subtraction), and the boolean `subset`.

```cm:expect
java.class.unique-method-names @ src/OrderService.java:L1-L4
ts.class.compact-surface @ src/cart.ts:L1-L7
ts.class.fields-fully-backed @ src/cart.ts:L1-L7
ts.class.fields-have-accessor @ src/cart.ts:L1-L7
ts.class.members-no-name-clash @ src/cart.ts:L1-L7
ts.class.small-class @ src/cart.ts:L1-L7
```
