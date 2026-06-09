use proptest::prelude::*;

use super::parse;
use super::test_support::{KINDS, TS};

proptest! {
	#![proptest_config(ProptestConfig {
		cases: 256,
		..ProptestConfig::default()
	})]

	#[test]
	fn arbitrary_text_never_panics(input in ".{0,512}") {
		let _ = parse(&input, TS, KINDS);
	}

	#[test]
	fn lossy_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
		let input = String::from_utf8_lossy(&bytes);
		let _ = parse(input.as_ref(), TS, KINDS);
	}

	#[test]
	fn operator_rich_expression_never_panics(
		lhs in prop_oneof![
			Just("name"),
			Just("text"),
			Just("kind"),
			Just("shape"),
			Just("visibility"),
			Just("lines"),
			Just("depth"),
			Just("moniker"),
			Just("source.parent"),
			Just("target.parent"),
		],
		op in prop_oneof![
			Just("="),
			Just("!="),
			Just("=~"),
			Just("!~"),
			Just("<"),
			Just("<="),
			Just(">"),
			Just(">="),
			Just("~"),
			Just("<@"),
		],
		rhs in ".{0,256}",
	) {
		let input = format!("{lhs} {op} {rhs}");
		let _ = parse(&input, TS, KINDS);
	}
}
