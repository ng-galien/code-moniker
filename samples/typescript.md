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
# TypeScript / JavaScript check sample.
# Copy to `.code-moniker.toml` and delete rules that do not fit your repo.

# Make this file the complete rule pack. Remove this line or set it to true
# if you also want the embedded default naming rules.
default_rules = false

[aliases]
# Def-scope aliases use `moniker`.
src = "moniker ~ '**/dir:src/**'"
tests = "moniker ~ '**/dir:/^tests?$/**'"
domain = "moniker ~ '**/dir:domain/**'"
infra = "moniker ~ '**/dir:infrastructure/**'"

# Ref-scope aliases must name source/target explicitly.
src_domain = "source ~ '**/dir:domain/**'"
tgt_domain = "target ~ '**/dir:domain/**'"
tgt_infra = "target ~ '**/dir:infrastructure/**'"

[[ts.class.where]]
id = "class-pascalcase"
# Every TypeScript class should use PascalCase.
expr = "name =~ ^[A-Z][A-Za-z0-9]*$"
message = "Class `{name}` must use PascalCase."

[[ts.class.where]]
id = "class-budget"
# Keep classes small: at most 20 direct methods, and no direct method longer
# than 60 lines.
expr = "count(method) <= 20 AND all(method, lines <= 60)"
message = "Class `{name}` is too large."

[[ts.interface.where]]
id = "repository-lives-in-domain"
# Interfaces named *Repository must live under the domain folder.
expr = "name =~ Repository$ => $domain"
message = "Repository interfaces must live in the domain layer."

[[refs.where]]
id = "domain-no-infra"
# Direct refs from domain code to infrastructure code are forbidden.
expr = "$src_domain => NOT $tgt_infra"
message = "Domain code must not depend directly on infrastructure."

[[ts.refs.where]]
id = "no-framework-imports-in-domain"
# Domain files must not import framework packages directly.
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
