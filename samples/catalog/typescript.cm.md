---
name: typescript
lang: ts
blurb: PascalCase classes, class budgets, and a domain layer that ignores infrastructure
published: true
---

# TypeScript starter pack

The TypeScript sample combines naming, size budgets, and layering in one
overlay. Classes must be PascalCase and stay small; `*Repository` interfaces
belong to the domain folder; and domain code must not reach into
infrastructure — neither through project imports nor through framework
packages.

```toml cm:rules
default_rules = false

[aliases]
src = "moniker ~ '**/dir:src/**'"
tests = "moniker ~ '**/dir:/^tests?$/**'"
domain = "moniker ~ '**/dir:domain/**'"
infra = "moniker ~ '**/dir:infrastructure/**'"

src_domain = "source ~ '**/dir:domain/**'"
tgt_domain = "target ~ '**/dir:domain/**'"
tgt_infra = "target ~ '**/dir:infrastructure/**'"

[[ts.class.where]]
id = "class-pascalcase"
rationale = "PascalCase helps classes read as named concepts instead of values or functions."
expr = "name =~ ^[A-Z][A-Za-z0-9]*$"
message = "Class `{name}` must use PascalCase."

[[ts.class.where]]
id = "class-budget"
rationale = "Small classes are easier to explain, test, and review. This budget highlights classes that may be carrying too many responsibilities."
expr = "count(method) <= 20 AND all(method, lines <= 60)"
message = "Class `{name}` is too large."

[[ts.interface.where]]
id = "repository-lives-in-domain"
rationale = "Repository interfaces describe what the domain needs. Keeping them in the domain prevents persistence details from owning the contract."
expr = "name =~ Repository$ => $domain"
message = "Repository interfaces must live in the domain layer."

[[refs.where]]
id = "domain-no-infra"
rationale = "Domain code should not import persistence or delivery code directly. That keeps business logic independent from adapters."
expr = "$src_domain => NOT $tgt_infra"
message = "Domain code must not depend directly on infrastructure."

[[ts.refs.where]]
id = "no-framework-imports-in-domain"
rationale = "Framework packages belong at the edge of the application. Domain files should stay usable without Express, Nest, or TypeORM."
expr = """
  $src_domain AND kind = 'imports_symbol'
  => NOT target ~ '**/external_pkg:/^(express|nestjs|typeorm)$/**'
"""
```

The domain entity drags in `express` and a persistence class. The import and
the instantiation both cross the layer boundary, and `order_service` ignores
the PascalCase convention:

```ts cm:file=src/domain/order.ts
import express from "express";

import { OrderTable } from "../infrastructure/order_table";

export interface OrderRepository {
	find(id: string): string;
}

export class Order {
	constructor(readonly id: string) {}
}

export class order_service {
	loadOrder(id: string): string {
		const table = new OrderTable();
		return table.key(id);
	}
}
```

The infrastructure side hosts the persistence adapter — and an interface
named `*Repository`, which the layout rule sends back to the domain folder:

```ts cm:file=src/infrastructure/order_table.ts
export class OrderTable {
	key(id: string): string {
		return "order:" + id;
	}
}

export interface LegacyOrderRepository {
	load(id: string): string;
}
```

The application layer hosts a controller that grew past the 20-method
budget:

```ts cm:file=src/app/report_controller.ts
export class ReportController {
	renderHeader(): string { return "header"; }
	renderFooter(): string { return "footer"; }
	renderTitle(): string { return "title"; }
	renderSummary(): string { return "summary"; }
	renderSales(): string { return "sales"; }
	renderRefunds(): string { return "refunds"; }
	renderInventory(): string { return "inventory"; }
	renderShipping(): string { return "shipping"; }
	renderReturns(): string { return "returns"; }
	renderTaxes(): string { return "taxes"; }
	renderDiscounts(): string { return "discounts"; }
	renderCustomers(): string { return "customers"; }
	renderSuppliers(): string { return "suppliers"; }
	renderForecast(): string { return "forecast"; }
	renderBudget(): string { return "budget"; }
	renderAudit(): string { return "audit"; }
	renderExports(): string { return "exports"; }
	renderImports(): string { return "imports"; }
	renderAlerts(): string { return "alerts"; }
	renderTrends(): string { return "trends"; }
	renderAppendix(): string { return "appendix"; }
}
```

```cm:expect
ts.class.class-budget @ src/app/report_controller.ts:L1-L23
ts.refs.no-framework-imports-in-domain @ src/domain/order.ts:L1
refs.domain-no-infra @ src/domain/order.ts:L3
ts.class.class-pascalcase @ src/domain/order.ts:L13-L18
refs.domain-no-infra @ src/domain/order.ts:L15
ts.interface.repository-lives-in-domain @ src/infrastructure/order_table.ts:L7-L9
```
