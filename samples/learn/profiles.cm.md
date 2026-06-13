---
name: profiles
title: Profiles, defaults, and suppressions
summary: Control default rules, named profiles, warning severity, and inline suppressions.
---

# Profiles, Defaults, And Suppressions

Use `default_rules = false` for a standalone ruleset, or leave defaults on and
layer project rules on top. Profiles select subsets for different workflows.

```toml
default_rules = true

[[rust.fn.where]]
id        = "no-placeholder-name"
rationale = "Placeholder names are useful while sketching, but they make reviewed code harder to understand."
severity  = "warn"
expr      = "NOT name =~ ^(foo|bar|baz)$"

[profiles.agent]
enable = ["^rust\\.fn\\.no-placeholder-name$"]
```

Suppressions are source comments near the violating symbol or line. Prefer
fixing the rule or narrowing its predicate when suppressions become common.
