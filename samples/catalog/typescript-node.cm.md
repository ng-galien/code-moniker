---
name: typescript-node
title: TypeScript Node backend boundaries
lang: ts
blurb: Keep domain code independent from Node web and persistence adapters using explicit graph evidence
learn_kind: framework
learn_path: languages/typescript/node
learn_order: 20
tags: typescript,node,express,nestjs,backend,layering
learn_aliases: node,nestjs,express
published: true
---

# TypeScript Node backend boundaries

This opt-in recipe fits layered Node backends, including projects using
Express or Nest. It does not infer controllers, services, dependency injection,
or decorators semantically. Instead it checks evidence Code Moniker exposes
today: source directories, imports, and resolved project references.

The first rule keeps runtime frameworks out of `domain/`. The second prevents
controller modules from reaching directly into persistence adapters. Rename the
directories and package patterns to match the repository.

```toml cm:rules
default_rules = false

[aliases]
src_domain = "source ~ '**/dir:domain/**'"
src_controller = "source ~ '**/dir:/^(controllers|http|web)$/**'"
tgt_persistence = "target ~ '**/dir:/^(persistence|repositories|infrastructure)$/**'"

[[ts.refs.where]]
id = "domain-no-node-framework-imports"
expr = """
  $src_domain AND kind = 'imports_symbol'
  => NOT (
       target ~ '**/external_pkg:/^(express|typeorm)$/**'
       OR target ~ '**/external_pkg:/^@nestjs\\/.+$/**'
     )
"""
message = "TypeScript domain code should not import a Node delivery or persistence framework."

[[refs.where]]
id = "controller-no-persistence-direct"
expr = "$src_controller => NOT $tgt_persistence"
message = "A controller should delegate through an application service instead of calling persistence directly."
```

```ts cm:file=src/persistence/order_store.ts
export class OrderStore {
	load(id: string): string { return id; }
}
```

```ts cm:file=src/domain/order.ts
import express from "express";

export function loadDomainOrder(): string {
	return express.application.get("id");
}
```

Scoped Nest packages keep the full package name (for example
`@nestjs/common`) in the external-package segment, so the pattern includes the
scope and its slash explicitly:

```ts cm:file=src/domain/nest_order.ts
import { Injectable } from "@nestjs/common";

@Injectable()
export class NestOrder {}
```

```ts cm:file=src/controllers/order_controller.ts
import { OrderStore } from "../persistence/order_store";

export function loadOrder(id: string): string {
	return new OrderStore().load(id);
}
```

```cm:expect
ts.refs.domain-no-node-framework-imports @ src/domain/order.ts:L1
ts.refs.domain-no-node-framework-imports @ src/domain/nest_order.ts:L1
refs.controller-no-persistence-direct @ src/controllers/order_controller.ts:L1
refs.controller-no-persistence-direct @ src/controllers/order_controller.ts:L4
```
