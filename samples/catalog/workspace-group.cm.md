---
name: workspace-group
lang: java
blurb: Workspace-wide uniqueness rules over stable symbol groups
published: true
default_rules: false
---

# Workspace group uniqueness

Group rules select symbols from the shared inventory, bucket them by stable
projections, and emit one diagnostic for each failing group.

```toml cm:rules
[[workspace.group.where]]
id        = "unique-type-name-per-package"
severity  = "warn"
members   = "shape = 'type'"
group_by  = ["lang", "segment('package')", "name"]
expr      = "count(member) <= 1"
message   = "Duplicate type group {group}: {members}"
rationale = "A logical package must not expose two types with the same name."
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
workspace.group.unique-type-name-per-package @ src/main/java/com/acme/sales/SalesA.java:L4
```
