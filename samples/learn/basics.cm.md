---
name: basics
title: Rule blocks and expressions
summary: Define where-rules with ids, predicates, severity, messages, and rationale.
---

# Rule Blocks And Expressions

Rules live in `[[<lang>.<kind>.where]]`, `[[<lang>.shape.<shape>.where]]`,
or `[[refs.where]]` tables. `expr` is a boolean predicate over the current
symbol or reference. A false predicate emits a violation.

```toml
default_rules = false

[[rust.fn.where]]
id        = "function-snake-case"
expr      = "name =~ ^[a-z][a-z0-9_]*$"
severity  = "warn"
message   = "Function `{name}` should be snake_case."
rationale = "A familiar name shape lets Rust readers recognize functions without stopping to decode style differences."
```

Useful operators: `=`, `!=`, `=~`, `!~`, `<`, `<=`, `>`, `>=`, `AND`, `OR`,
`NOT`, and implication with `=>`.
