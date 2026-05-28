#[cfg(test)]
mod tests {
	// cm: def builtin unit test
	#[test]
	fn parses_order() {}

	// cm: def ignored test without reason
	#[test]
	#[ignore]
	fn skipped_without_reason() {}

	// cm: def ignored test with reason
	#[ignore = "requires external service"]
	#[test]
	fn skipped_with_reason() {}

	mod nested {
		// cm: def nested unit test
		#[test]
		fn keeps_hierarchy() {}
	}
}

mod integration_style {
	// cm: def module scoped unit test
	#[test]
	fn top_level_module_test() {}
}

// cm: def tokio async is normal function
#[tokio::test]
async fn async_runtime_test_is_not_builtin_rust_test() {}

// cm: ref proptest macro external
proptest::proptest! {
	// cm: def proptest generated test
	#[test]
	fn round_trips(bytes in proptest::collection::vec(any::<u8>(), 0..16), size in 0usize..4) {
		let _ = (bytes, size);
	}

	// cm: def ignored proptest generated test
	#[ignore = "slow property"]
	#[test]
	fn ignored_property(value in 0usize..8) {
		let _ = value;
	}
}
