---
name: rust-naming
title: Rust function naming
lang: rs
blurb: Functions stay snake_case
learn_kind: language
learn_path: languages/rust/naming
learn_order: 10
tags: rust,naming,snake-case
published: true
---

# Rust naming

Rust functions follow `snake_case`. The rule matches every `fn` name against
a lowercase pattern; `DoThing` violates it, `tidy` does not.

```toml cm:rules
[[rust.fn.where]]
id      = "snake-case"
rationale = "Rust readers expect function names to be snake_case. Following that convention keeps ordinary code from looking surprising."
expr    = "name =~ ^[a-z][a-z0-9_]*$"
message = "Function `{name}` should be snake_case."
```

```rust cm:file=src/lib.rs
pub fn tidy() {}

pub fn DoThing() {}
```

Run this document with `code-moniker check . --scenario samples/catalog/rust-naming.cm.md`.

```cm:expect
rust.fn.snake-case @ src/lib.rs:L3
```
