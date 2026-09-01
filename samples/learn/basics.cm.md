---
name: basics
title: Rule blocks and expressions
summary: Define where-rules with ids, predicates, severity, messages, and rationale.
learn_kind: general
learn_path: rules
learn_aliases: rules
---

# Rule Blocks And Expressions

Rules live in `[[<lang>.<kind>.where]]`, `[[<lang>.shape.<shape>.where]]`,
or `[[refs.where]]` tables. `expr` is a boolean predicate over the current
symbol or reference. A false predicate emits a violation.

```toml cm:rules
default_rules = false

[[rust.fn.where]]
id        = "function-snake-case"
expr      = "name =~ ^[a-z][a-z0-9_]*$"
severity  = "warn"
message   = "Function `{name}` should be snake_case."
rationale = "A familiar name shape lets Rust readers recognize functions without stopping to decode style differences."
```

```rust cm:file=src/lib.rs
pub fn tidy() {}

pub fn DoThing() {}
```

Useful operators: `=`, `!=`, `=~`, `!~`, `<`, `<=`, `>`, `>=`, `AND`, `OR`,
`NOT`, mutual exclusion with `disjoint`, and implication with `=>`.

`A disjoint B` is `NOT (A AND B)`: it fails only when both operands hold for
the same symbol, and it reads the same in either order. Because it binds
tighter than `AND` and `OR`, a compound operand is parenthesized —
`(A OR B) disjoint (C AND D)`.

```cm:expect
rust.fn.function-snake-case @ src/lib.rs:L3
```
