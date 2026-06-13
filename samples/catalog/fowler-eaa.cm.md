---
name: fowler-eaa
lang: java
blurb: Enterprise layering, repositories, DTOs, and domain behavior
published: true
---

# Fowler — Patterns of Enterprise Application Architecture

Ruleset inspired by Martin Fowler et al., *Patterns of Enterprise Application
Architecture* (2002). The sample teaches a practical enterprise shape:
presentation calls services, services coordinate domain behavior, and data
source code stays behind clear repository boundaries.

```toml cm:rules
default_rules = false

[aliases]
presentation = "moniker ~ '**/package:/^(presentation|web|controller|api)$/**'"
service      = "moniker ~ '**/package:/^(service|application)$/**'"
domain       = "moniker ~ '**/package:/^(domain|model)$/**'"
data         = "moniker ~ '**/package:/^(data|persistence|repository|infrastructure)$/**'"

src_presentation = "source ~ '**/package:/^(presentation|web|controller|api)$/**'"
src_service      = "source ~ '**/package:/^(service|application)$/**'"
src_domain       = "source ~ '**/package:/^(domain|model)$/**'"
src_data         = "source ~ '**/package:/^(data|persistence|repository|infrastructure)$/**'"

tgt_presentation = "target ~ '**/package:/^(presentation|web|controller|api)$/**'"
tgt_service      = "target ~ '**/package:/^(service|application)$/**'"
tgt_domain       = "target ~ '**/package:/^(domain|model)$/**'"
tgt_data         = "target ~ '**/package:/^(data|persistence|repository|infrastructure)$/**'"

[[refs.where]]
id   = "fowler-layered-domain-no-presentation"
rationale = "Domain code should not know how a request is presented. Keep presentation concerns outside the model."
expr = "$src_domain AND kind != 'annotates' => NOT $tgt_presentation"
message = "Layered Architecture: domain must not depend on presentation."

[[refs.where]]
id   = "fowler-layered-domain-no-data"
rationale = "The domain may define repository contracts, but it should not depend on concrete data source code."
expr = "$src_domain AND kind != 'annotates' => NOT $tgt_data"
message = "Layered Architecture: domain must not depend on the data source layer."

[[refs.where]]
id   = "fowler-layered-data-no-presentation"
rationale = "The data source layer should serve the application, not call back into controllers or views."
expr = "$src_data AND kind != 'annotates' => NOT $tgt_presentation"
message = "Layered Architecture: data source layer must not depend on presentation."

[[refs.where]]
id   = "fowler-presentation-no-direct-data"
rationale = "Presentation should ask the service layer to do work instead of reaching straight into persistence."
expr = "$src_presentation AND kind != 'annotates' => NOT $tgt_data"
message = "Service Layer: presentation should call the service layer, not the data source layer directly."

[[java.class.where]]
id   = "fowler-repository-naming"
rationale = "Repository types are persistence-facing contracts or implementations. Their package should make that role clear."
expr = "name =~ Repository$ => ($data OR $domain)"
message = "Repository: class `{name}` should live in a data source or domain package."

[[java.class.where]]
id   = "fowler-dto-only-accessors"
rationale = "DTOs are meant to carry data across a boundary. Business behavior belongs in domain or service objects."
expr = "name =~ (Dto|DTO|Request|Response)$ => all(method, name =~ ^(get|set|is)[A-Z_].* OR name =~ ^(equals|hashCode|toString)$)"
message = "DTO `{name}` should only expose accessors; business logic belongs elsewhere."

[[java.class.where]]
id   = "fowler-domain-not-anemic"
rationale = "A domain model should carry behavior, not only data. Classes with no meaningful methods often push business logic elsewhere."
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
