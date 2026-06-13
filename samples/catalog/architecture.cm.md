---
name: architecture
lang: java
blurb: MVC, DDD, and hexagonal boundaries explained with a small Java project
published: true
---

# Architecture boundaries

This sample shows how to protect three common architecture styles: MVC, DDD
layering, and hexagonal ports & adapters. The rules name familiar folders
such as `controller`, `domain`, and `adapter`, then check that dependencies
move in the expected direction.

```toml cm:rules
default_rules = false

[aliases]
mvc_controller = "moniker ~ '**/*:/^(controller|api|web)$/**'"
mvc_service = "moniker ~ '**/*:/^(service|application)$/**'"
mvc_model = "moniker ~ '**/*:/^(model|domain)$/**'"
mvc_view = "moniker ~ '**/*:/^(view|ui)$/**'"

ddd_domain = "moniker ~ '**/*:domain/**'"
ddd_application = "moniker ~ '**/*:application/**'"
ddd_infrastructure = "moniker ~ '**/*:/^(infrastructure|infra)$/**'"

hex_core = "moniker ~ '**/*:/^(domain|application)$/**'"
hex_port = "moniker ~ '**/*:port/**' OR name =~ Port$"
hex_adapter = "moniker ~ '**/*:adapter/**' OR name =~ Adapter$"

src_controller = "source ~ '**/*:/^(controller|api|web)$/**'"
src_service = "source ~ '**/*:/^(service|application)$/**'"
src_domain = "source ~ '**/*:domain/**'"
src_application = "source ~ '**/*:application/**'"
src_infrastructure = "source ~ '**/*:/^(infrastructure|infra)$/**'"
src_adapter = "source ~ '**/*:adapter/**'"

tgt_controller = "target ~ '**/*:/^(controller|api|web)$/**'"
tgt_service = "target ~ '**/*:/^(service|application)$/**'"
tgt_model = "target ~ '**/*:/^(model|domain)$/**'"
tgt_view = "target ~ '**/*:/^(view|ui)$/**'"
tgt_domain = "target ~ '**/*:domain/**'"
tgt_application = "target ~ '**/*:application/**'"
tgt_infrastructure = "target ~ '**/*:/^(infrastructure|infra)$/**'"
tgt_port = "target ~ '**/*:port/**' OR target.name =~ Port$"
tgt_adapter = "target ~ '**/*:adapter/**' OR target.name =~ Adapter$"

[[refs.where]]
id = "mvc-controller-does-not-call-view"
rationale = "Controllers should orchestrate a request and return a response model. Calling view code directly mixes routing with rendering."
expr = "$src_controller => NOT $tgt_view"
message = "MVC controller code must not depend directly on view code."

[[refs.where]]
id = "mvc-view-does-not-call-service"
rationale = "Views should present data, not start application workflows. Route behavior through controllers or presentation models."
expr = "source ~ '**/*:/^(view|ui)$/**' => NOT $tgt_service"
message = "MVC view code must not depend directly on service/application code."

[[refs.where]]
id = "mvc-model-does-not-depend-on-controller"
rationale = "Model and domain code should be reusable without the presentation layer. Controller dependencies point the wrong way."
expr = "$tgt_controller => NOT ($src_domain OR source ~ '**/*:model/**')"
message = "MVC model/domain code must not depend on controllers."

[[refs.where]]
id = "ddd-domain-is-pure"
rationale = "Domain code should express business rules. Application flows and infrastructure details belong outside it."
expr = "$src_domain => NOT ($tgt_application OR $tgt_infrastructure)"
message = "DDD domain code must not depend on application or infrastructure layers."

[[refs.where]]
id = "ddd-application-depends-inward"
rationale = "Application code coordinates use cases. It should talk to domain contracts rather than concrete infrastructure."
expr = "$src_application => NOT $tgt_infrastructure"
message = "Application layer code must not depend directly on infrastructure."

[[refs.where]]
id = "ddd-infrastructure-may-depend-inward-only"
rationale = "Infrastructure may implement adapters, but it should not reach upward into presentation code."
expr = "$src_infrastructure => NOT $tgt_controller"
message = "Infrastructure code must not depend on presentation controllers."

[[default.class.where]]
id = "ddd-entity-name"
rationale = "If your team uses an Entity suffix, this makes domain entities visible in reviews and search results."
expr = "$ddd_domain AND name =~ Entity$ => $ddd_domain"
message = "DDD entity `{name}` should live in the domain layer."

[[refs.where]]
id = "hex-core-does-not-depend-on-adapters"
rationale = "The core should describe what it needs through ports. Concrete adapters belong outside the core."
expr = "source ~ '**/*:/^(domain|application)$/**' => NOT $tgt_adapter"
message = "Hexagonal core must not depend on adapters."

[[refs.where]]
id = "hex-adapter-depends-on-port-not-peer-adapter"
rationale = "Adapters should plug into ports, not coordinate with each other directly."
expr = "$src_adapter => NOT $tgt_adapter"
message = "Adapters must not depend directly on other adapters."

[[default.interface.where]]
id = "hex-port-interface-name"
rationale = "A Port suffix makes the boundary between the core and adapters visible."
expr = "$hex_port => name =~ Port$"
message = "Hexagonal port interface `{name}` should end with Port."

[[default.class.where]]
id = "hex-adapter-class-name"
rationale = "An Adapter suffix tells readers that the class connects the core to an outside technology or system."
expr = "$hex_adapter => name =~ Adapter$"
message = "Hexagonal adapter class `{name}` should end with Adapter."

[profiles.architecture]
enable = [
  "^refs\\.mvc-",
  "^refs\\.ddd-",
  "^refs\\.hex-",
  "^default\\.(class|interface)\\.hex-",
]
```

## MVC violations

The controller renders a view class directly instead of returning a response
model:

```java cm:file=src/main/java/com/acme/controller/OrderController.java
package com.acme.controller;

import com.acme.view.OrderView;

public class OrderController {
	public String show() {
		return new OrderView().render();
	}
}
```

The view reaches back into the service layer:

```java cm:file=src/main/java/com/acme/view/OrderView.java
package com.acme.view;

import com.acme.service.OrderQuery;

public class OrderView {
	public String render() {
		return new OrderQuery().total();
	}
}
```

The model knows its presentation controller:

```java cm:file=src/main/java/com/acme/model/OrderRecord.java
package com.acme.model;

import com.acme.controller.OrderController;

public class OrderRecord {
	public String preview() {
		return new OrderController().show();
	}
}
```

The service layer itself stays clean:

```java cm:file=src/main/java/com/acme/service/OrderQuery.java
package com.acme.service;

public class OrderQuery {
	public String total() {
		return "42";
	}
}
```

## DDD violations

Domain code instantiates a persistence table — the domain is no longer pure:

```java cm:file=src/main/java/com/acme/domain/Order.java
package com.acme.domain;

import com.acme.infrastructure.OrderTable;

public class Order {
	public String id() {
		return new OrderTable().key();
	}
}
```

The application layer skips its port and talks to infrastructure directly:

```java cm:file=src/main/java/com/acme/application/PlaceOrder.java
package com.acme.application;

import com.acme.infrastructure.OrderTable;

public class PlaceOrder {
	public String run() {
		return new OrderTable().key();
	}
}
```

Infrastructure depends upward on a presentation controller:

```java cm:file=src/main/java/com/acme/infrastructure/OrderTable.java
package com.acme.infrastructure;

import com.acme.controller.OrderController;

public class OrderTable {
	public String key() {
		return new OrderController().show();
	}
}
```

## Hexagonal violations

Core domain code news up an adapter instead of speaking through a port:

```java cm:file=src/main/java/com/acme/domain/Shipment.java
package com.acme.domain;

import com.acme.adapter.HttpNotifierAdapter;

public class Shipment {
	public void dispatch() {
		new HttpNotifierAdapter().notifyShipped();
	}
}
```

A port interface without the `Port` suffix, and an adapter class without the
`Adapter` suffix — plus that adapter depends on a peer adapter:

```java cm:file=src/main/java/com/acme/port/OrderGateway.java
package com.acme.port;

public interface OrderGateway {
	String fetch();
}
```

```java cm:file=src/main/java/com/acme/adapter/SqlOrderStore.java
package com.acme.adapter;

public class SqlOrderStore implements com.acme.port.OrderGateway {
	public String fetch() {
		new HttpNotifierAdapter().notifyShipped();
		return "order";
	}
}
```

```java cm:file=src/main/java/com/acme/adapter/HttpNotifierAdapter.java
package com.acme.adapter;

public class HttpNotifierAdapter {
	public void notifyShipped() {
	}
}
```

```cm:expect
! java.class.ddd-entity-name the expr is a tautology (`$ddd_domain AND name =~ Entity$ => $ddd_domain` repeats the premise in the conclusion), so no layout can make it fire
java.class.hex-adapter-class-name @ src/main/java/com/acme/adapter/SqlOrderStore.java:L3-L8
refs.hex-adapter-depends-on-port-not-peer-adapter @ src/main/java/com/acme/adapter/SqlOrderStore.java:L5
refs.hex-adapter-depends-on-port-not-peer-adapter @ src/main/java/com/acme/adapter/SqlOrderStore.java:L5
refs.hex-adapter-depends-on-port-not-peer-adapter @ src/main/java/com/acme/adapter/SqlOrderStore.java:L5
refs.ddd-application-depends-inward @ src/main/java/com/acme/application/PlaceOrder.java:L3
refs.ddd-application-depends-inward @ src/main/java/com/acme/application/PlaceOrder.java:L7
refs.ddd-application-depends-inward @ src/main/java/com/acme/application/PlaceOrder.java:L7
refs.ddd-application-depends-inward @ src/main/java/com/acme/application/PlaceOrder.java:L7
refs.mvc-controller-does-not-call-view @ src/main/java/com/acme/controller/OrderController.java:L3
refs.mvc-controller-does-not-call-view @ src/main/java/com/acme/controller/OrderController.java:L7
refs.mvc-controller-does-not-call-view @ src/main/java/com/acme/controller/OrderController.java:L7
refs.mvc-controller-does-not-call-view @ src/main/java/com/acme/controller/OrderController.java:L7
refs.ddd-domain-is-pure @ src/main/java/com/acme/domain/Order.java:L3
refs.ddd-domain-is-pure @ src/main/java/com/acme/domain/Order.java:L7
refs.ddd-domain-is-pure @ src/main/java/com/acme/domain/Order.java:L7
refs.ddd-domain-is-pure @ src/main/java/com/acme/domain/Order.java:L7
refs.hex-core-does-not-depend-on-adapters @ src/main/java/com/acme/domain/Shipment.java:L3
refs.hex-core-does-not-depend-on-adapters @ src/main/java/com/acme/domain/Shipment.java:L7
refs.hex-core-does-not-depend-on-adapters @ src/main/java/com/acme/domain/Shipment.java:L7
refs.hex-core-does-not-depend-on-adapters @ src/main/java/com/acme/domain/Shipment.java:L7
refs.ddd-infrastructure-may-depend-inward-only @ src/main/java/com/acme/infrastructure/OrderTable.java:L3
refs.ddd-infrastructure-may-depend-inward-only @ src/main/java/com/acme/infrastructure/OrderTable.java:L7
refs.ddd-infrastructure-may-depend-inward-only @ src/main/java/com/acme/infrastructure/OrderTable.java:L7
refs.ddd-infrastructure-may-depend-inward-only @ src/main/java/com/acme/infrastructure/OrderTable.java:L7
refs.mvc-model-does-not-depend-on-controller @ src/main/java/com/acme/model/OrderRecord.java:L3
refs.mvc-model-does-not-depend-on-controller @ src/main/java/com/acme/model/OrderRecord.java:L7
refs.mvc-model-does-not-depend-on-controller @ src/main/java/com/acme/model/OrderRecord.java:L7
refs.mvc-model-does-not-depend-on-controller @ src/main/java/com/acme/model/OrderRecord.java:L7
java.interface.hex-port-interface-name @ src/main/java/com/acme/port/OrderGateway.java:L3-L5
refs.mvc-view-does-not-call-service @ src/main/java/com/acme/view/OrderView.java:L3
refs.mvc-view-does-not-call-service @ src/main/java/com/acme/view/OrderView.java:L7
refs.mvc-view-does-not-call-service @ src/main/java/com/acme/view/OrderView.java:L7
refs.mvc-view-does-not-call-service @ src/main/java/com/acme/view/OrderView.java:L7
```
