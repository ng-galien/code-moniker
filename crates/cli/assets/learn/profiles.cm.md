---
name: profiles
title: Profiles, defaults, and suppressions
summary: Control default rules, named profiles, warning severity, and inline suppression comments.
---

# Profiles, Defaults, And Suppressions

Use `default_rules = false` for a standalone ruleset, or leave defaults on and
layer project rules on top. Profiles select subsets for different workflows.

```toml cm:rules
default_rules = false

[[rust.fn.where]]
id        = "no-placeholder-name"
rationale = "Placeholder names are useful while sketching, but they make reviewed code harder to understand."
severity  = "warn"
expr      = "NOT name =~ ^(foo|bar|baz)$"

[profiles.agent]
enable = ["^rust\\.fn\\.no-placeholder-name$"]
```

`severity = "warn"` reports the violation without failing the run. The
`[profiles.agent]` table selects this rule by an anchored regex on its full id,
so `code-moniker check --profile agent` runs only the rules a profile enables.

## Suppressions

Suppressions are source comments using the language line-comment marker (`//`,
`#`, or `--`). A bare `code-moniker: ignore` silences the next def;
`ignore[<id-suffix>]` narrows it to matching rules; `ignore-file` covers the
whole file. Prefer fixing the rule or narrowing its predicate when suppressions
become common.

In `lib.rs`, `foo` is reported, `bar` is silenced by a bare directive, and
`baz` is silenced by an id-filtered directive. Every line in `scratch.rs` is
silenced by a file-level directive.

```rust cm:file=src/lib.rs
pub fn foo() {}

// code-moniker: ignore
pub fn bar() {}

// code-moniker: ignore[no-placeholder-name]
pub fn baz() {}
```

```rust cm:file=src/scratch.rs
// code-moniker: ignore-file
pub fn foo() {}
pub fn bar() {}
```

```cm:expect
rust.fn.no-placeholder-name @ src/lib.rs:L1
```
