---
name: paths
title: Moniker path patterns and aliases
summary: Match architectural locations with moniker globs and reusable aliases.
---

# Moniker Path Patterns And Aliases

Path expressions match moniker segments, not raw filesystem strings. Use
`**` for descendants and segment regexes such as `dir:/^(app|domain)$/`.

```toml cm:rules
default_rules = false

[aliases]
domain = "source ~ '**/dir:domain/**'"
infra  = "target ~ '**/dir:infrastructure/**'"

[[refs.where]]
id      = "domain-no-infra"
rationale = "A path alias can express an architectural boundary in plain project language: domain code should not call infrastructure directly."
expr    = "$domain => NOT $infra"
message = "Domain code must not depend on infrastructure."
```

```ts cm:file=src/domain/order-service.ts
import { saveOrder } from "../infrastructure/order-store";

export function placeOrder() {
  saveOrder();
}
```

```ts cm:file=src/infrastructure/order-store.ts
export function saveOrder() {}
```

Common fields include `uri`, `moniker`, `source`, `target`, `source.parent`,
`target.parent`, `name`, `kind`, `shape`, `visibility`, and `lines`.

```cm:expect
refs.domain-no-infra @ src/domain/order-service.ts:L1
refs.domain-no-infra @ src/domain/order-service.ts:L4
```
