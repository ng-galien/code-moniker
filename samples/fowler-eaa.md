---
name: fowler-eaa
lang: java
blurb: PoEAA layering, repositories, DTOs and a non-anemic domain
published: true
---

# Fowler — Patterns of Enterprise Application Architecture

Ruleset inspired by Martin Fowler et al., *Patterns of Enterprise Application
Architecture* (2002). The layering is `presentation -> service -> domain`,
with the data source layer on the outside. Four `refs` rules police the layer
edges; three Java class heuristics encode the Repository, Data Transfer
Object and Domain Model patterns.

```toml cm:rules
# Fowler — Patterns of Enterprise Application Architecture check sample.
#
# Ruleset inspired by Martin Fowler et al., "Patterns of Enterprise
# Application Architecture" (Addison-Wesley, 2002). Community-authored
# encoding of structural patterns from the book; not endorsed by the author.
#
# Layer convention used below: presentation -> service (application) ->
# domain, with data (persistence/infrastructure) on the outside. This encodes
# the common dependency-inverted variant where the domain may own repository
# interfaces but does not depend on persistence implementations.
#
# Class heuristics are Java-scoped because they rely on Java method and field
# vocabulary. Adapt the package names to match your repository before use.

default_rules = false

[aliases]
# Def-scope aliases.
presentation = "moniker ~ '**/package:/^(presentation|web|controller|api)$/**'"
service      = "moniker ~ '**/package:/^(service|application)$/**'"
domain       = "moniker ~ '**/package:/^(domain|model)$/**'"
data         = "moniker ~ '**/package:/^(data|persistence|repository|infrastructure)$/**'"

# Ref-scope aliases.
src_presentation = "source ~ '**/package:/^(presentation|web|controller|api)$/**'"
src_service      = "source ~ '**/package:/^(service|application)$/**'"
src_domain       = "source ~ '**/package:/^(domain|model)$/**'"
src_data         = "source ~ '**/package:/^(data|persistence|repository|infrastructure)$/**'"

tgt_presentation = "target ~ '**/package:/^(presentation|web|controller|api)$/**'"
tgt_service      = "target ~ '**/package:/^(service|application)$/**'"
tgt_domain       = "target ~ '**/package:/^(domain|model)$/**'"
tgt_data         = "target ~ '**/package:/^(data|persistence|repository|infrastructure)$/**'"

# Layered Architecture ------------------------------------------------------
# Chapter 1. Higher layers depend on lower; lower layers must not depend
# back on higher. Annotation refs are excluded because persistence
# annotations on domain entities (JPA `@Column`, `@Entity`) are descriptive
# metadata, not behavioural coupling.

[[refs.where]]
id   = "fowler-layered-domain-no-presentation"
expr = "$src_domain AND kind != 'annotates' => NOT $tgt_presentation"
message = "Layered Architecture: domain must not depend on presentation."

[[refs.where]]
id   = "fowler-layered-domain-no-data"
expr = "$src_domain AND kind != 'annotates' => NOT $tgt_data"
message = "Layered Architecture: domain must not depend on the data source layer."

[[refs.where]]
id   = "fowler-layered-data-no-presentation"
expr = "$src_data AND kind != 'annotates' => NOT $tgt_presentation"
message = "Layered Architecture: data source layer must not depend on presentation."

# Service Layer -------------------------------------------------------------
# Presentation talks to the service layer; the service layer orchestrates
# domain logic and repositories. Presentation reaching the data layer
# directly is the smell this rule catches.

[[refs.where]]
id   = "fowler-presentation-no-direct-data"
expr = "$src_presentation AND kind != 'annotates' => NOT $tgt_data"
message = "Service Layer: presentation should call the service layer, not the data source layer directly."

# Repository ----------------------------------------------------------------
# Repository implementations live in the data source layer. Repository
# interfaces may live with the domain (the canonical Repository pattern in
# DDD-influenced PoEAA).

[[java.class.where]]
id   = "fowler-repository-naming"
expr = "name =~ Repository$ => ($data OR $domain)"
message = "Repository: class `{name}` should live in a data source or domain package."

# Data Transfer Object ------------------------------------------------------
# DTOs are data carriers: accessors, constructor, equality. Business
# behaviour belongs in a domain or service class. Detection is heuristic
# based on name suffix + accessor pattern; builder-style or record-style
# projects should tune this rule.

[[java.class.where]]
id   = "fowler-dto-only-accessors"
expr = "name =~ (Dto|DTO|Request|Response)$ => all(method, name =~ ^(get|set|is)[A-Z_].* OR name =~ ^(equals|hashCode|toString)$)"
message = "DTO `{name}` should only expose accessors; business logic belongs elsewhere."

# Domain Model --------------------------------------------------------------
# Related to Fowler's "AnemicDomainModel" bliki entry: a domain class with
# zero behaviour is a smell. Excludes obvious value-only classes by name.

[[java.class.where]]
id   = "fowler-domain-not-anemic"
expr = "$domain AND name !~ (Dto|DTO|Request|Response|Event|Command|Query|Id|Vo|VO)$ => count(method, name !~ ^(get|set|is)[A-Z_].* AND name !~ ^(equals|hashCode|toString)$) >= 1"
message = "Anemic Domain Model: domain class `{name}` has no non-accessor methods."

[profiles.fowler-eaa]
enable = [
  "^refs\\.fowler-",
  "^java\\.class\\.fowler-",
]
```

## Layer edges, broken in both directions

`Customer` lives in the domain but reaches sideways into presentation and
outward into persistence. Its `rename` method keeps it out of the anemic rule
— the layering rules are what flag it:

```java cm:file=src/main/java/com/shop/domain/Customer.java
package com.shop.domain;

import com.shop.persistence.CustomerGateway;
import com.shop.web.CustomerView;

public class Customer {
	private String name;

	public String getName() {
		return name;
	}

	public void rename(String next) {
		name = next;
		new CustomerGateway().save(this);
	}

	public String show() {
		return CustomerView.render(getName());
	}
}
```

`Invoice` is the textbook anemic domain object: accessors only, no
behaviour:

```java cm:file=src/main/java/com/shop/domain/Invoice.java
package com.shop.domain;

public class Invoice {
	private long amount;

	public long getAmount() {
		return amount;
	}

	public void setAmount(long next) {
		amount = next;
	}
}
```

The controller skips the service layer and talks to the gateway directly:

```java cm:file=src/main/java/com/shop/web/CheckoutController.java
package com.shop.web;

import com.shop.persistence.CustomerGateway;

public class CheckoutController {
	public String checkout(String name) {
		new CustomerGateway().save(null);
		return CustomerView.render(name);
	}
}
```

```java cm:file=src/main/java/com/shop/web/CustomerView.java
package com.shop.web;

public class CustomerView {
	public static String render(String name) {
		return "<p>" + name + "</p>";
	}

	public static String empty() {
		return "<p></p>";
	}
}
```

The gateway, in the data source layer, depends back on a presentation view:

```java cm:file=src/main/java/com/shop/persistence/CustomerGateway.java
package com.shop.persistence;

import com.shop.web.CustomerView;

public class CustomerGateway {
	public void save(Object row) {
		CustomerView.render("saved");
	}
}
```

`OrderRepository` is parked in the service layer instead of data or domain,
and `OrderDto` smuggles pricing logic into a data carrier:

```java cm:file=src/main/java/com/shop/service/OrderRepository.java
package com.shop.service;

public class OrderRepository {
	public Object findById(String id) {
		return id;
	}

	public void save(Object order) {
	}
}
```

```java cm:file=src/main/java/com/shop/service/OrderDto.java
package com.shop.service;

public class OrderDto {
	private long total;

	public long getTotal() {
		return total;
	}

	public long applyDiscount(long percent) {
		return total - total * percent / 100;
	}
}
```

A clean service for contrast — it may depend on both the domain and the data
source layer:

```java cm:file=src/main/java/com/shop/service/CheckoutService.java
package com.shop.service;

import com.shop.domain.Customer;
import com.shop.persistence.CustomerGateway;

public class CheckoutService {
	public void checkout(Customer customer) {
		new CustomerGateway().save(customer);
	}
}
```

```cm:expect
refs.fowler-layered-domain-no-data @ src/main/java/com/shop/domain/Customer.java:L3
refs.fowler-layered-domain-no-presentation @ src/main/java/com/shop/domain/Customer.java:L4
refs.fowler-layered-domain-no-data @ src/main/java/com/shop/domain/Customer.java:L15
refs.fowler-layered-domain-no-data @ src/main/java/com/shop/domain/Customer.java:L15
refs.fowler-layered-domain-no-data @ src/main/java/com/shop/domain/Customer.java:L15
refs.fowler-layered-domain-no-presentation @ src/main/java/com/shop/domain/Customer.java:L19
java.class.fowler-domain-not-anemic @ src/main/java/com/shop/domain/Invoice.java:L3-L13
refs.fowler-layered-data-no-presentation @ src/main/java/com/shop/persistence/CustomerGateway.java:L3
refs.fowler-layered-data-no-presentation @ src/main/java/com/shop/persistence/CustomerGateway.java:L7
java.class.fowler-dto-only-accessors @ src/main/java/com/shop/service/OrderDto.java:L10-L12
java.class.fowler-repository-naming @ src/main/java/com/shop/service/OrderRepository.java:L3-L10
refs.fowler-presentation-no-direct-data @ src/main/java/com/shop/web/CheckoutController.java:L3
refs.fowler-presentation-no-direct-data @ src/main/java/com/shop/web/CheckoutController.java:L7
refs.fowler-presentation-no-direct-data @ src/main/java/com/shop/web/CheckoutController.java:L7
refs.fowler-presentation-no-direct-data @ src/main/java/com/shop/web/CheckoutController.java:L7
```
