---
name: rust
title: Rust starter pack
lang: rs
blurb: Naming, size budgets, and test prefixes for a Rust crate
learn_kind: language
learn_path: languages/rust
learn_order: 40
tags: rust,naming,tests
published: true
---

# Rust check sample

A starter rule set for Rust files (parser tag: `rs`, rule prefix: `rust`): public traits and structs stay
PascalCase and public functions stay short. The test-name prefix below is an
example project policy, not a universal Rust convention.

```toml cm:rules
default_rules = false

[aliases]
src = "moniker ~ '**/dir:src/**'"
tests = "moniker ~ '**/dir:tests/**'"

[[rust.trait.where]]
id = "trait-pascalcase"
rationale = "Public traits are part of the crate vocabulary. PascalCase makes them look like Rust types and keeps APIs easy to scan."
expr = "visibility = 'public' => name =~ ^[A-Z][A-Za-z0-9]*$"
message = "Public trait `{name}` must use PascalCase."

[[rust.struct.where]]
id = "struct-pascalcase"
rationale = "Public structs introduce named concepts. PascalCase helps readers recognize them as Rust types immediately."
expr = "visibility = 'public' => name =~ ^[A-Z][A-Za-z0-9]*$"
message = "Public struct `{name}` must use PascalCase."

[[rust.fn.where]]
id = "public-fn-small"
rationale = "A public function is harder to change once other code depends on it. Keeping it short makes its contract easier to understand."
expr = "visibility = 'public' => lines <= 80"
message = "Public function `{name}` is too long."

[[rust.test.where]]
id = "tests-start-with-describes-or_should"
rationale = "This is an example project policy, not a universal Rust convention. Teams that use descriptive prefixes can make test intent visible before opening the body."
expr = "name =~ ^(test_|should_|it_)"
message = "Rust test `{name}` should start with test_, should_, or it_."

```

The infrastructure side is a small adapter — nothing to flag here:

```rust cm:file=src/infra/mod.rs
pub struct Store;

impl Store {
	pub fn fetch(&self) -> u32 {
		42
	}
}
```

The crate root just wires the modules:

```rust cm:file=src/lib.rs
pub mod domain;
pub mod infra;
```

The domain module concentrates the demonstrated violations: a lowercase public
trait and struct, a function body padded past the 80-line budget, and a test
whose name has no recognized prefix. Its import is fixture context, not a
claimed dependency-boundary check.

```rust cm:file=src/domain/mod.rs
use crate::infra::Store;

pub trait repository {
	fn load(&self) -> u32;
}

pub struct order_record {
	pub total: u32,
}

pub struct Order {
	pub total: u32,
}

pub fn order_total() -> u32 {
	Store.fetch()
}

pub fn settle_everything() -> u32 {
	let mut total = 0;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total += 1;
	total
}

#[cfg(test)]
mod tests {
	#[test]
	fn test_order_total_is_positive() {
		assert!(super::order_total() > 0);
	}

	#[test]
	fn totals_accumulate() {
		assert_eq!(super::order_total(), 42);
	}
}
```

```cm:expect
rust.trait.trait-pascalcase @ src/domain/mod.rs:L3-L5
rust.struct.struct-pascalcase @ src/domain/mod.rs:L7-L9
rust.fn.public-fn-small @ src/domain/mod.rs:L19-L102
rust.test.tests-start-with-describes-or_should @ src/domain/mod.rs:L112-L114
```
