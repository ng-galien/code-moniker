#[cfg(test)]
mod tests {
	#[test]
	fn parses_order() {}

	#[test]
	#[ignore]
	fn skipped_without_reason() {}

	#[ignore = "requires external service"]
	#[test]
	fn skipped_with_reason() {}

	mod nested {
		#[test]
		fn keeps_hierarchy() {}
	}
}

mod integration_style {
	#[test]
	fn top_level_module_test() {}
}

#[tokio::test]
async fn async_runtime_test_is_not_builtin_rust_test() {}

proptest::proptest! {
	#[test]
	fn round_trips(bytes in proptest::collection::vec(any::<u8>(), 0..16), size in 0usize..4) {
		let _ = (bytes, size);
	}

	#[ignore = "slow property"]
	#[test]
	fn ignored_property(value in 0usize..8) {
		let _ = value;
	}
}
