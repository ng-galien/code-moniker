use super::*;

#[test]
fn usage_prefix_distinguishes_workspace_crates() {
	assert_eq!(path_prefix("crates/daemon/src/lib.rs"), "crates/daemon");
	assert_eq!(
		path_prefix("crates/workspace/src/live/watcher.rs"),
		"crates/workspace"
	);
	assert_eq!(path_prefix("src/a.rs"), "src");
	assert_eq!(path_prefix("src/b.rs"), "src");
}

#[test]
fn symbol_search_lists_production_before_tests() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::create_dir_all(temp.path().join("src")).expect("create source directory");
	fs::create_dir_all(temp.path().join("benches")).expect("create bench directory");
	fs::write(
		temp.path().join("src/lib.rs"),
		r#"
#[cfg(test)]
mod tests {
	fn helper() {}

	#[test]
	fn early_test() {}
}

pub fn production_entry() {}
"#,
	)
	.expect("write fixture");
	fs::write(
		temp.path().join("benches/speed.rs"),
		"pub fn benchmark_helper() {}\n",
	)
	.expect("write bench fixture");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let refresh = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	assert!(matches!(refresh, ProtocolResponse::Command(_)));

	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
		query: Query::SymbolSearch(code_moniker_query::SymbolSearchQuery {
			shape: vec!["callable".to_string()],
			..Default::default()
		}),
		consistency: code_moniker_query::Consistency::Current,
		page: Page {
			cursor: None,
			limit: 1,
		},
	})));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected query response");
	};
	let QueryResult::SymbolList(list) = response.result else {
		panic!("expected symbol list, got {:?}", response.result);
	};

	assert_eq!(list.rows.len(), 1);
	assert_eq!(
		list.rows[0].name, "production_entry()",
		"production symbols must precede test symbols on the default page"
	);
}
