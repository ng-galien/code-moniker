use pgrx::prelude::*;

use super::moniker;

#[pg_operator(immutable, parallel_safe)]
#[opname(<)]
#[commutator(>)]
#[negator(>=)]
fn moniker_lt(a: moniker, b: moniker) -> bool {
	a.as_bytes() < b.as_bytes()
}

#[pg_operator(immutable, parallel_safe)]
#[opname(<=)]
#[commutator(>=)]
#[negator(>)]
fn moniker_le(a: moniker, b: moniker) -> bool {
	a.as_bytes() <= b.as_bytes()
}

#[pg_operator(immutable, parallel_safe)]
#[opname(>)]
#[commutator(<)]
#[negator(<=)]
fn moniker_gt(a: moniker, b: moniker) -> bool {
	a.as_bytes() > b.as_bytes()
}

#[pg_operator(immutable, parallel_safe)]
#[opname(>=)]
#[commutator(<=)]
#[negator(<)]
fn moniker_ge(a: moniker, b: moniker) -> bool {
	a.as_bytes() >= b.as_bytes()
}

#[pg_extern(immutable, parallel_safe)]
fn moniker_cmp(a: moniker, b: moniker) -> i32 {
	a.as_bytes().cmp(b.as_bytes()) as i32
}

const FNV_OFFSET_BASIS: u32 = 0x811c_9dc5;
const FNV_PRIME: u32 = 0x0100_0193;

#[pg_extern(immutable, parallel_safe)]
fn moniker_hash(m: moniker) -> i32 {
	m.as_bytes()
		.iter()
		.fold(FNV_OFFSET_BASIS, |h, &b| (h ^ b as u32).wrapping_mul(FNV_PRIME)) as i32
}

extension_sql!(
	r#"
	CREATE OPERATOR CLASS moniker_btree_ops
		DEFAULT FOR TYPE moniker USING btree AS
			OPERATOR 1 <,
			OPERATOR 2 <=,
			OPERATOR 3 =,
			OPERATOR 4 >=,
			OPERATOR 5 >,
			FUNCTION 1 moniker_cmp(moniker, moniker);

	CREATE OPERATOR CLASS moniker_hash_ops
		DEFAULT FOR TYPE moniker USING hash AS
			OPERATOR 1 =,
			FUNCTION 1 moniker_hash(moniker);
	"#,
	name = "moniker_opclasses",
	requires = [
		moniker_eq,
		moniker_lt,
		moniker_le,
		moniker_gt,
		moniker_ge,
		moniker_cmp,
		moniker_hash,
	]
);
