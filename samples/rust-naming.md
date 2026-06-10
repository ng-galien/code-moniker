---
name: rust-naming
lang: rust
blurb: Functions stay snake_case
published: true
---

# Rust naming

Rust functions follow `snake_case`. The rule matches every `fn` name against
a lowercase pattern; `DoThing` violates it, `tidy` does not.

```toml cm:rules
[[rust.fn.where]]
id      = "snake-case"
expr    = "name =~ ^[a-z][a-z0-9_]*$"
message = "Function `{name}` should be snake_case."
```

```rust cm:file=src/lib.rs
pub fn tidy() {}

pub fn DoThing() {}
```

Run this document with `code-moniker check . --scenario samples/rust-naming.md`.

```cm:expect
rust.fn.snake-case @ src/lib.rs:L3
```
