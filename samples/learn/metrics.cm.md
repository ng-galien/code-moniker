---
name: metrics
title: Local numeric metrics
summary: Use local metrics such as fan-out, WMC, RFC, CBO, LCOM4, CV, Gini, and entropy.
---

# Local Numeric Metrics

Numeric metrics operate on the current local graph item and its direct local
relationships.

```toml cm:rules
default_rules = false

[[rust.fn.where]]
id        = "short-function"
rationale = "A small line budget is an easy numeric rule to understand before moving to richer metrics."
expr      = "lines <= 3"

[[java.class.where]]
id        = "class-method-budget"
rationale = "Metrics are review prompts. This rule points to classes that may be carrying too many entry points."
expr      = "count(method) <= 2"
```

```rust cm:file=src/lib.rs
pub fn calculate() {
    let base = 1;
    let tax = 2;
    let total = base + tax;
    println!("{total}");
}
```

```java cm:file=src/InvoiceService.java
class InvoiceService {
  void load() {}
  void price() {}
  void print() {}
}
```

Use metrics as warning-first review heuristics unless your project has already
calibrated thresholds.

```cm:expect
java.class.class-method-budget @ src/InvoiceService.java:L1-L5
rust.fn.short-function @ src/lib.rs:L1-L6
```
