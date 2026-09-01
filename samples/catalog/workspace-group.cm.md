---
name: workspace-group
title: Workspace group invariants
lang: java
blurb: Workspace-wide uniqueness and distribution rules over stable symbol groups
learn_kind: workspace
learn_path: workspace/groups
learn_order: 10
tags: workspace,groups,uniqueness,aggregates
published: true
default_rules: false
---

# Workspace group invariants

Group rules select symbols from the shared inventory, bucket them by stable
projections, and emit one diagnostic for each failing group. Their predicates
can combine member counts with descriptive statistics over inclusive symbol
line spans.

```toml cm:rules
[[workspace.group.where]]
id        = "unique-type-name-per-package"
severity  = "warn"
members   = "shape = 'type'"
group_by  = ["lang", "srcset", "segment('package')", "name"]
expr      = "count(member) <= 1"
message   = "Duplicate type group {group}: {members}"
rationale = "A logical package must not expose two types with the same name."

[[workspace.group.where]]
id        = "balanced-type-sizes-per-package"
severity  = "warn"
members   = "shape = 'type'"
group_by  = ["lang", "srcset", "segment('package')"]
expr      = "count(member) >= 4 => gini(member, lines) <= 0.2"
message   = "Uneven type sizes in {group}: {observations}"
rationale = "A sufficiently large package should not concentrate most code in a few types."
```

The two nested types have distinct monikers but collide on language, package
and type name:

```java cm:file=src/main/java/com/acme/sales/SalesA.java
package com.acme.sales;

class SalesA {
	class Invoice {}
}
```

```java cm:file=src/main/java/com/acme/sales/SalesB.java
package com.acme.sales;

class SalesB {
	class Invoice {}
}
```

```cm:expect
workspace.group.balanced-type-sizes-per-package @ src/main/java/com/acme/sales/SalesA.java:L3-L5
workspace.group.unique-type-name-per-package @ src/main/java/com/acme/sales/SalesA.java:L4
```
