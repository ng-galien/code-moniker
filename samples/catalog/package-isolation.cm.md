---
name: package-isolation
title: Package isolation
lang: ts
blurb: Two packages kept mutually isolated with a single symmetric disjoint rule
learn_kind: pattern
learn_path: architecture/package-isolation
learn_order: 30
tags: typescript,packages,isolation,disjoint
published: true
---

# Package isolation

Some boundaries have no direction. When two packages must stay fully isolated,
neither one is the source of the policy and neither is the target: the rule is
that they must not both appear in the same reference.

`disjoint` states exactly that. `A disjoint B` is `NOT (A AND B)`, so the rule
below is written once and rejects imports in both directions. Written with
`=>` it would take two rules repeating the same intent, one per direction.

Each alias matches a reference for which *either* endpoint belongs to that
package, which is what makes the predicate direction-free.

```toml cm:rules
default_rules = false

[aliases]
package_a = "source ~ '**/dir:package-a/**' OR target ~ '**/dir:package-a/**'"
package_b = "source ~ '**/dir:package-b/**' OR target ~ '**/dir:package-b/**'"

[[refs.where]]
id = "package-a-and-package-b-do-not-import-each-other"
severity = "error"
expr = """
  (kind = 'imports_symbol' OR kind = 'imports_module')
  => $package_a disjoint $package_b
"""
message = "`package-a` and `package-b` must not import each other."
rationale = "The two packages are fully isolated, so an import crossing the boundary in either direction breaks the same policy."
```

## Crossing the boundary

`package-a` reaches into `package-b`:

```ts cm:file=packages/package-a/order.ts
import { price } from "../package-b/pricing";

export function total(): number {
	return price();
}
```

And `package-b` reaches back into `package-a`. The relationship is symmetric,
so this fails for the same reason and under the same rule id:

```ts cm:file=packages/package-b/pricing.ts
import { discount } from "../package-a/rebate";

export function price(): number {
	return 100 - discount();
}
```

## Staying inside

A reference internal to `package-a` matches `$package_a` only, so the
conjunction is false and the rule passes:

```ts cm:file=packages/package-a/rebate.ts
export function discount(): number {
	return 5;
}
```

An unrelated package matches neither alias. `disjoint` is not XOR — matching
neither operand is valid:

```ts cm:file=packages/package-c/log.ts
export function log(message: string): void {
	console.log(message);
}
```

```cm:expect
refs.package-a-and-package-b-do-not-import-each-other @ packages/package-a/order.ts:L1
refs.package-a-and-package-b-do-not-import-each-other @ packages/package-b/pricing.ts:L1
```
