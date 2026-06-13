---
name: metrics
title: Local numeric metrics
summary: Use local metrics such as fan-out, WMC, RFC, CBO, LCOM4, CV, Gini, and entropy.
---

# Local Numeric Metrics

Numeric metrics operate on the current local graph item and its direct local
relationships.

```toml
[[rust.shape.type.where]]
id        = "balanced-method-fanout"
rationale = "When one method talks to far more collaborators than its neighbors, it may be carrying a hidden responsibility."
expr      = "count(shape:callable) >= 5 => cv(shape:callable, fan_out(each)) <= 0.8"

[[java.class.where]]
id        = "class-budget"
rationale = "Metrics are review prompts. This rule points to classes that may be too coupled, too broad, or too scattered."
expr      = "cbo(self) <= 14 AND rfc(self) <= 50 AND wmc(self) <= 47 AND lcom4(self) <= 1"
```

Use metrics as warning-first review heuristics unless your project has already
calibrated thresholds.
