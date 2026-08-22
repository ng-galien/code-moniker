---
name: fragments
title: Fragments, view URIs, and namespaced rule ids
summary: Keep fragment name, view id, and local rule id distinct; read view URIs from the listing; expect namespaced ids in check output.
---

# Fragments, View URIs, And Namespaced Rule Ids

A `code-moniker.fragment.toml` under the canonical `.code-moniker.toml` is one
file for two loaders. Do not treat these names as the same URI:

| In the file | Meaning | What an agent uses |
|---|---|---|
| `fragment = "domain"` | Rule/alias namespace | Never an URI |
| `[[views]] id = "billing-hexagon"` | View listing leaf | `workspace/views/billing-hexagon` from `workspace/views`, never invented |
| `id = "no-infra"` | Local rule id | In the fragment and in `rules = […]`. Check output is `refs.domain.no-infra` |

`fragment` and view `id` may match or differ. Always follow a **returned**
`workspace/views/<view.id>` call.

View `symbols = ["module:api/fn:save"]` values are identity **suffixes**
scoped to the fragment directory. They are not compact monikers.

This learn fixture cannot mount a real fragment file (the scenario overlay is
the root `.code-moniker.toml`). The executable rule below is the **effective**
form after merge: local `no-infra` in fragment `domain` becomes
`refs.domain.no-infra`. A fragment on disk would look like:

```toml
fragment = "domain"

[aliases]
domain = "source ~ '**/dir:domain/**'"
infra  = "target ~ '**/dir:infrastructure/**'"

[[refs.where]]
id      = "no-infra"
expr    = "$domain => NOT $infra"
message = "Domain code must not depend on infrastructure."
```

```toml cm:rules
default_rules = false

[aliases]
domain = "source ~ '**/dir:domain/**'"
infra  = "target ~ '**/dir:infrastructure/**'"

[[refs.where]]
id      = "domain.no-infra"
rationale = "After fragment merge the local id no-infra is namespaced with fragment domain, so check reports refs.domain.no-infra."
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

```cm:expect
refs.domain.no-infra @ src/domain/order-service.ts:L1
refs.domain.no-infra @ src/domain/order-service.ts:L4
```
