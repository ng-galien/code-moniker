---
name: rust
lang: rust
blurb: Naming, size, test prefixes, and layering for a Rust crate
published: true
---

# Rust check sample

A starter rule set for a Rust crate: public traits and structs stay
PascalCase, public functions stay short, test names announce themselves with a
`test_`/`should_`/`it_` prefix, and domain modules never reach into
infrastructure directly.

```toml cm:rules
default_rules = false

[aliases]
src = "moniker ~ '**/dir:src/**'"
tests = "moniker ~ '**/dir:tests/**'"
domain = "moniker ~ '**/dir:domain/**'"
infra = "moniker ~ '**/dir:/^(infra|infrastructure)$/**'"

src_domain = "source ~ '**/dir:domain/**'"
tgt_infra = "target ~ '**/dir:/^(infra|infrastructure)$/**'"

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
rationale = "A test name should say what behavior it protects before the reader opens the body."
expr = "name =~ ^(test_|should_|it_)"
message = "Rust test `{name}` should start with test_, should_, or it_."

[[rust.refs.where]]
id = "domain-no-infra"
rationale = "Domain code should express business decisions. Infrastructure details belong behind adapters so the domain stays easy to test and move."
expr = "$src_domain => NOT $tgt_infra"
message = "Domain code must not depend directly on infrastructure."
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

The domain module concentrates the violations: a lowercase public trait and
struct, a function body padded past the 80-line budget, a test whose name has
no recognized prefix, and a direct dependency on `crate::infra` (which the
`domain-no-infra` rule would like to flag — see the note below):

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

Note on `domain-no-infra`: extracted Rust reference targets spell foreign
modules with `module:`/`path:` segments (`dir:` segments only mirror the
referencing file's own path on disk), so the `tgt_infra` alias —
`target ~ '**/dir:/^(infra|infrastructure)$/**'` — can never match a Rust
ref target and the rule stays silent in any layout.

```cm:expect
! rust.refs.domain-no-infra extracted Rust ref targets use module:/path: segments, never dir:, so the tgt_infra dir pattern cannot match
rust.trait.trait-pascalcase @ src/domain/mod.rs:L3-L5
rust.struct.struct-pascalcase @ src/domain/mod.rs:L7-L9
rust.fn.public-fn-small @ src/domain/mod.rs:L19-L102
rust.test.tests-start-with-describes-or_should @ src/domain/mod.rs:L112-L114
```
