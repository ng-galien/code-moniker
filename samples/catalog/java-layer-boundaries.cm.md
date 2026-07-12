---
name: java-layer-boundaries
lang: java
blurb: Domain code never depends on infrastructure
published: true
---

# Layer boundaries

A DDD-flavoured boundary: domain code may be *used by* outer layers, but must
never reference infrastructure itself. The rule inspects every cross-file
reference and rejects the ones whose source lives under `domain` and whose
resolved target lives under `infrastructure`.

```toml cm:rules
[aliases]
src_domain = "source ~ '**/*:domain/**'"
tgt_infrastructure = "target ~ '**/*:/^(infrastructure|infra)$/**'"

[[refs.where]]
id      = "domain-depends-only-inward"
rationale = "Domain code should model business rules without knowing how data is stored. Keep infrastructure behind an application boundary."
expr    = "$src_domain => NOT $tgt_infrastructure"
message = "Domain code must not depend on infrastructure."
```

`Order` breaks the boundary by instantiating a persistence table directly:

```java cm:file=src/main/java/com/acme/domain/Order.java
package com.acme.domain;

import com.acme.infrastructure.OrderTable;

public class Order {
	private final OrderTable table = new OrderTable();

	public String id() {
		return table.key();
	}
}
```

The infrastructure side is a plain adapter:

```java cm:file=src/main/java/com/acme/infrastructure/OrderTable.java
package com.acme.infrastructure;

public class OrderTable {
	public String key() {
		return "order";
	}
}
```

The application layer may depend on the domain — this file stays clean:

```java cm:file=src/main/java/com/acme/application/PlaceOrder.java
package com.acme.application;

import com.acme.domain.Order;

public class PlaceOrder {
	public String run() {
		return new Order().id();
	}
}
```

```cm:expect
refs.domain-depends-only-inward @ src/main/java/com/acme/domain/Order.java:L3
refs.domain-depends-only-inward @ src/main/java/com/acme/domain/Order.java:L6
refs.domain-depends-only-inward @ src/main/java/com/acme/domain/Order.java:L6
refs.domain-depends-only-inward @ src/main/java/com/acme/domain/Order.java:L6
refs.domain-depends-only-inward @ src/main/java/com/acme/domain/Order.java:L6
refs.domain-depends-only-inward @ src/main/java/com/acme/domain/Order.java:L9
```
