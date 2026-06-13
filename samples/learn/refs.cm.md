---
name: refs
title: Reference rules
summary: Use refs rules for imports, calls, inheritance, annotations, and layer boundaries.
---

# Reference Rules

`[[refs.where]]` evaluates one emitted reference at a time. It is the right
domain for direct dependency boundaries, framework imports, call rules, and
annotation relationships.

```toml cm:rules
default_rules = false

[[refs.where]]
id        = "domain-imports-no-framework"
rationale = "Reference rules can protect boundaries directly: domain code stays easier to reuse when framework imports remain outside."
expr      = """
  source ~ '**/dir:domain/**' AND kind = 'imports_symbol'
  => NOT (target ~ '**/external_pkg:express/**'
          OR target ~ '**/external_pkg:nestjs/**')
"""
```

```ts cm:file=src/domain/controller.ts
import { Router } from "express";

export const router = Router();
```

Use `source.*` for the referencing symbol and `target.*` for the referenced
symbol when the target resolves.

```cm:expect
refs.domain-imports-no-framework @ src/domain/controller.ts:L1
```
