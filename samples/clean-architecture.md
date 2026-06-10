---
name: clean-architecture
lang: java
blurb: Dependencies point inward and inner rings stay framework-free
published: true
---

# Clean Architecture

Ruleset inspired by Robert C. Martin, *Clean Architecture* (2017). The rings,
from inner to outer, are `entities -> usecases -> adapters -> frameworks`.
The Dependency Rule says source dependencies point only inward; two annotation
rules keep framework markers out of the inner rings; two structural heuristics
encode the Stable Abstractions and Interface Segregation principles.

```toml cm:rules
# Clean Architecture check sample.
#
# Ruleset inspired by Robert C. Martin, "Clean Architecture: A Craftsman's
# Guide to Software Structure and Design" (Prentice Hall, 2017). Community-
# authored encoding of structural principles from the book; not endorsed by
# the author. Principles that require semantic judgement (SRP, OCP, LSP) and
# whole-graph analyses (Acyclic Dependencies Principle) are not encoded here;
# the latter belongs in a SQL query against an ingested code_graph.
#
# Layer convention used below, from inner to outer:
#   entities -> use_cases -> adapters -> frameworks
#
# Adapt the package and directory names to match your repository before use.

default_rules = false

[aliases]
# Def-scope aliases.
entities    = "moniker ~ '**/*:/^(entities|domain)$/**'"
use_cases   = "moniker ~ '**/*:/^(usecases|use_cases|application)$/**'"
adapters    = "moniker ~ '**/*:/^(adapters|interface_adapters)$/**'"
frameworks  = "moniker ~ '**/*:/^(frameworks|infrastructure)$/**'"

# Ref-scope aliases.
src_entities   = "source ~ '**/*:/^(entities|domain)$/**'"
src_use_cases  = "source ~ '**/*:/^(usecases|use_cases|application)$/**'"
src_adapters   = "source ~ '**/*:/^(adapters|interface_adapters)$/**'"

tgt_use_cases  = "target ~ '**/*:/^(usecases|use_cases|application)$/**'"
tgt_adapters   = "target ~ '**/*:/^(adapters|interface_adapters)$/**'"
tgt_frameworks = "target ~ '**/*:/^(frameworks|infrastructure)$/**'"

# Framework annotations that should not bleed into inner rings. Extend the
# list with framework markers used in your stack.
framework_annotation = "target.name =~ ^(Component|Service|Repository|Controller|RestController|Configuration|Bean|Autowired|Entity|Table|Column|Inject|Path|GET|POST|PUT|DELETE)$"

# The Dependency Rule -------------------------------------------------------
# Chapter 22. Source code dependencies point only inward, from outer rings
# to inner rings.

[[refs.where]]
id   = "clean-arch-entities-depend-on-nothing-outer"
expr = "$src_entities => NOT ($tgt_use_cases OR $tgt_adapters OR $tgt_frameworks)"
message = "Clean Architecture: entities must not depend on outer rings."

[[refs.where]]
id   = "clean-arch-use-cases-depend-on-nothing-outer"
expr = "$src_use_cases => NOT ($tgt_adapters OR $tgt_frameworks)"
message = "Clean Architecture: use cases must not depend on adapters or frameworks."

[[refs.where]]
id   = "clean-arch-adapters-depend-on-nothing-outer"
expr = "$src_adapters => NOT $tgt_frameworks"
message = "Clean Architecture: adapters must not depend on frameworks."

# Independence from frameworks ---------------------------------------------
# Chapter 20. Entities and use cases must not carry framework annotations.
# The rule inspects annotation refs.

[[refs.where]]
id   = "clean-arch-entities-no-framework-annotation"
expr = "$src_entities AND kind = 'annotates' => NOT $framework_annotation"
message = "Clean Architecture: entity must not carry framework annotations."

[[refs.where]]
id   = "clean-arch-use-cases-no-framework-annotation"
expr = "$src_use_cases AND kind = 'annotates' => NOT $framework_annotation"
message = "Clean Architecture: use case must not carry framework annotations."

# Stable Abstractions Principle ---------------------------------------------
# Chapter 14. A class with many incoming dependencies is stable and should be
# depended on through an abstraction. The DSL does not expose abstract-class
# modifiers portably, so this heuristic flags high fan-in classes. Threshold is
# conventional. `code-moniker check` evaluates a source file graph at a time;
# use SQL over an ingested code_graph for project-wide fan-in metrics.

[[default.class.where]]
id   = "clean-arch-stable-abstractions"
expr = "count(in_refs) < 20"
message = "Clean Architecture (SAP): class `{name}` has many dependents; introduce or depend on an abstraction."

# Interface Segregation -----------------------------------------------------
# SOLID I. Threshold is conventional; tighten per project.

[[default.interface.where]]
id   = "clean-arch-interface-segregation"
expr = "count(method) <= 8"
message = "Clean Architecture (ISP): interface `{name}` exposes too many methods."

[profiles.clean-architecture]
enable = [
  "^refs\\.clean-arch-",
  "^default\\.(class|interface)\\.clean-arch-",
]
```

## The Dependency Rule, broken at every ring

`Order` is an entity, the innermost ring — yet it reaches out to a use case
and carries JPA annotations:

```java cm:file=src/main/java/com/shop/entities/Order.java
package com.shop.entities;

import javax.persistence.Entity;
import javax.persistence.Table;

import com.shop.usecases.PlaceOrder;

@Entity
@Table
public class Order {
	private final Money total = new Money(0);

	public Money total() {
		return total;
	}

	public String describe() {
		return PlaceOrder.NAME + total.cents();
	}
}
```

`Money` is a clean value entity, but everything in its compilation unit binds
to it concretely — the accumulated fan-in trips the Stable Abstractions
heuristic, which asks for an abstraction (`count(in_refs)` sees the refs of
the analysed source file; project-wide fan-in belongs in SQL, as the rule's
comment notes):

```java cm:file=src/main/java/com/shop/entities/Money.java
package com.shop.entities;

public class Money {
	private final long cents;

	public Money(long cents) {
		this.cents = cents;
	}

	public long cents() {
		return cents;
	}

	public Money plus(Money other) {
		return new Money(cents + other.cents());
	}
}

class MoneyMath {
	static Money zero() {
		return new Money(0);
	}

	static Money min(Money a, Money b) {
		return a.cents() <= b.cents() ? a : b;
	}

	static Money max(Money a, Money b) {
		return a.cents() <= b.cents() ? b : a;
	}

	static Money clamp(Money value, Money low, Money high) {
		return min(max(value, low), high);
	}
}
```

The use case ring depends outward on an adapter and carries a Spring
stereotype:

```java cm:file=src/main/java/com/shop/usecases/PlaceOrder.java
package com.shop.usecases;

import org.springframework.stereotype.Service;

import com.shop.adapters.OrderPresenter;
import com.shop.entities.Order;

@Service
public class PlaceOrder {
	public static final String NAME = "place-order";

	public String run() {
		Order order = new Order();
		return OrderPresenter.render(order);
	}
}
```

A fat use-case interface violates Interface Segregation:

```java cm:file=src/main/java/com/shop/usecases/OrderingFacade.java
package com.shop.usecases;

public interface OrderingFacade {
	String create();
	String confirm();
	String pay();
	String pack();
	String ship();
	String track();
	String deliver();
	String invoice();
	String archive();
}
```

The adapter ring reaches into the frameworks ring:

```java cm:file=src/main/java/com/shop/adapters/OrderPresenter.java
package com.shop.adapters;

import com.shop.entities.Order;
import com.shop.frameworks.JsonCodec;

public class OrderPresenter {
	public static String render(Order order) {
		return JsonCodec.encode(order.describe());
	}
}
```

The frameworks ring may depend inward freely — these two files are clean,
but their heavy concrete use of `Money` is what pushes its fan-in over the
Stable Abstractions threshold:

```java cm:file=src/main/java/com/shop/frameworks/JsonCodec.java
package com.shop.frameworks;

public class JsonCodec {
	public static String encode(String payload) {
		return "{\"payload\":\"" + payload + "\"}";
	}

	public static String decode(String payload) {
		return payload;
	}
}
```

```java cm:file=src/main/java/com/shop/frameworks/MoneyLedger.java
package com.shop.frameworks;

import com.shop.entities.Money;

public class MoneyLedger {
	private Money opening;
	private Money closing;

	public Money zero() {
		return new Money(0);
	}

	public Money sum(Money a, Money b) {
		return a.plus(b);
	}

	public Money open(Money base) {
		opening = base;
		return opening;
	}

	public Money close(Money base) {
		closing = base;
		return closing;
	}
}
```

```cm:expect
refs.clean-arch-adapters-depend-on-nothing-outer @ src/main/java/com/shop/adapters/OrderPresenter.java:L4
refs.clean-arch-adapters-depend-on-nothing-outer @ src/main/java/com/shop/adapters/OrderPresenter.java:L8
java.class.clean-arch-stable-abstractions @ src/main/java/com/shop/entities/Money.java:L3-L17
refs.clean-arch-entities-depend-on-nothing-outer @ src/main/java/com/shop/entities/Order.java:L6
refs.clean-arch-entities-no-framework-annotation @ src/main/java/com/shop/entities/Order.java:L8
refs.clean-arch-entities-no-framework-annotation @ src/main/java/com/shop/entities/Order.java:L9
refs.clean-arch-entities-depend-on-nothing-outer @ src/main/java/com/shop/entities/Order.java:L18
java.interface.clean-arch-interface-segregation @ src/main/java/com/shop/usecases/OrderingFacade.java:L3-L13
refs.clean-arch-use-cases-depend-on-nothing-outer @ src/main/java/com/shop/usecases/PlaceOrder.java:L5
refs.clean-arch-use-cases-no-framework-annotation @ src/main/java/com/shop/usecases/PlaceOrder.java:L8
refs.clean-arch-use-cases-depend-on-nothing-outer @ src/main/java/com/shop/usecases/PlaceOrder.java:L14
```
