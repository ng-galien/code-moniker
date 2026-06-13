---
name: collections
title: Child domains and collection predicates
summary: Use count, any, all, none, unique, subset, and set algebra over local child symbols.
---

# Child Domains And Collection Predicates

Shape and kind rules can inspect direct child domains such as `method`,
`field`, `segment`, `out_refs`, and `in_refs`.

```toml
[[ts.class.where]]
id        = "small-class"
rationale = "A class with fewer fields and shorter methods is easier to understand before changing it."
expr      = "count(method) <= 20 AND count(field) <= 7 AND all(method, lines <= 60)"

[[java.class.where]]
id        = "unique-method-names"
rationale = "Repeated method names can hide overload-heavy APIs. This rule asks whether every method name earns its place."
expr      = "size(unique(method.name)) = size(method.name)"
```

Collection operators are local to the current graph item. They do not compute
transitive dependencies or history-based smells.
