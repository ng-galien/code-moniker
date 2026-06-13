---
name: collections
title: Child domains and collection predicates
summary: Use count, any, all, none, unique, subset, and set algebra over local child symbols.
---

# Child Domains And Collection Predicates

Shape and kind rules can inspect direct child domains such as `method`,
`field`, `segment`, `out_refs`, and `in_refs`.

```toml cm:rules
default_rules = false

[[ts.class.where]]
id        = "small-class"
rationale = "A class with fewer fields and shorter methods is easier to understand before changing it."
expr      = "count(method) <= 1 AND count(field) <= 1"

[[java.class.where]]
id        = "unique-method-names"
rationale = "Repeated method names can hide overload-heavy APIs. This rule asks whether every method name earns its place."
expr      = "size(unique(method.name)) = size(method.name)"
```

```ts cm:file=src/customer-report.ts
export class CustomerReport {
  private total = 0;
  private tax = 0;

  render() {}
  exportCsv() {}
}
```

```java cm:file=src/OrderService.java
class OrderService {
  void save(String id) {}
  void save(int id) {}
}
```

Collection operators are local to the current graph item. They do not compute
transitive dependencies or history-based smells.

```cm:expect
java.class.unique-method-names @ src/OrderService.java:L1-L4
ts.class.small-class @ src/customer-report.ts:L1-L7
```
