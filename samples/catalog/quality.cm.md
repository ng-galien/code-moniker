---
name: quality
title: Quality and testing policies
blurb: Browse opt-in smell heuristics and test guardrails separately from architecture patterns
learn_kind: pattern
learn_path: rules/quality
learn_order: 110
tags: quality,testing,smells,guardrails
published: true
---

# Quality and testing policies

Quality rules are project policies and review signals, not architectural
styles. The child topics contain executable Fowler-inspired heuristics and test
guardrails. Read each rationale and limitation before enabling it.

This small example demonstrates the shape of an explicit local policy:

```toml cm:rules
default_rules = false

[[python.function.where]]
id = "no-debug-prefix"
expr = "name !~ ^debug_"
message = "Production function `{name}` should not keep a debug_ prefix."
```

```python cm:file=src/report.py
def debug_total():
    return 42
```

```cm:expect
python.function.no-debug-prefix @ src/report.py:L1-L2
```
