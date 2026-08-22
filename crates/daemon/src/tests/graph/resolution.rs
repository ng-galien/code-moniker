use super::*;

#[test]
fn natural_symbol_inputs_resolve_without_changing_exact_symbol_identity() {
	let mut fixture = graph_path_fixture();
	let response = fixture
		.daemon
		.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::SymbolSearch(SymbolSearchQuery {
				path: vec!["src/**".to_string()],
				shape: vec!["callable".to_string()],
				name: Some("^callback$".to_string()),
				..Default::default()
			}),
		))));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected natural callable search response");
	};
	let QueryResult::SymbolList(symbols) = response.result else {
		panic!("expected symbol list");
	};
	assert_eq!(symbols.total, 1, "{symbols:?}");
	assert!(
		symbols
			.hint
			.as_deref()
			.is_some_and(|hint| hint.contains("retried as `^callback\\(`")),
		"{symbols:?}"
	);

	let path = graph_path(
		&mut fixture.daemon,
		"rs:src/lib.fn:callback",
		"repository",
		GraphPathExpectation::Reachable,
		6,
	);
	assert_eq!(path.verdict, GraphPathVerdict::Pass, "{path:?}");
	assert_eq!(path.path.len(), 2, "{path:?}");
}

#[test]
fn ambiguous_natural_symbol_input_returns_concrete_candidates() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src = temp.path().join("src");
	fs::create_dir_all(&src).expect("src dir");
	fs::write(
		src.join("lib.rs"),
		"mod other; fn duplicate() {} fn target() {}\n",
	)
	.expect("lib source");
	fs::write(src.join("other.rs"), "pub fn duplicate() {}\n").expect("other source");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	assert!(matches!(
		daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		})),
		ProtocolResponse::Command(_)
	));
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::GraphPath(GraphPathQuery {
			from: "duplicate".to_string(),
			to: "target".to_string(),
			..Default::default()
		}),
	))));
	let ProtocolResponse::Error(error) = response else {
		panic!("ambiguous natural name must not be guessed");
	};
	assert_eq!(error.code, "symbol_ambiguous", "{error:?}");
	assert!(
		error.message.contains("matches multiple symbols"),
		"{error:?}"
	);
	assert!(
		error.message.contains("next: choose a returned moniker"),
		"{error:?}"
	);

	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::SymbolGraph(SymbolGraphQuery {
			focus: "duplicate".to_string(),
			..Default::default()
		}),
	))));
	let ProtocolResponse::Error(error) = response else {
		panic!("ambiguous graph focus must not be downgraded to an unknown file");
	};
	assert_eq!(error.code, "symbol_ambiguous", "{error:?}");
	assert!(
		error.message.contains("matches multiple symbols"),
		"{error:?}"
	);
}

#[test]
fn natural_symbol_resolution_is_scoped_to_the_selected_workspace() {
	let first = tempfile::tempdir().expect("first root");
	let second = tempfile::tempdir().expect("second root");
	fs::write(
		first.path().join("lib.rs"),
		"fn duplicate() { target(); } fn target() {}\n",
	)
	.expect("first source");
	fs::write(second.path().join("lib.rs"), "fn duplicate() {}\n").expect("second source");
	let mut daemon = WorkspaceDaemon::new(vec![
		first.path().to_path_buf(),
		second.path().to_path_buf(),
	])
	.expect("daemon");
	assert!(matches!(
		daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		})),
		ProtocolResponse::Command(_)
	));
	let workspace = first
		.path()
		.file_name()
		.and_then(|name| name.to_str())
		.expect("root name")
		.to_string();
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::GraphPath(GraphPathQuery {
			workspace: Some(workspace),
			from: "duplicate".to_string(),
			to: "target".to_string(),
			max_depth: 2,
			..Default::default()
		}),
	))));
	let ProtocolResponse::Query(response) = response else {
		panic!("workspace-scoped natural symbols should resolve: {response:?}");
	};
	let QueryResult::GraphPath(path) = response.result else {
		panic!("expected graph path");
	};
	assert_eq!(path.reachable, Some(true), "{path:?}");
}

#[test]
fn path_and_corridor_expand_owner_endpoints_to_navigable_members() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(
		temp.path().join("lib.rs"),
		"pub struct Target; pub struct Owner { pub target: Target }\n",
	)
	.expect("Rust source");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	assert!(matches!(
		daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		})),
		ProtocolResponse::Command(_)
	));
	let QueryResult::SymbolList(owner_rows) = search_symbols_named(&mut daemon, "Owner") else {
		panic!("owner search");
	};
	let QueryResult::SymbolList(target_rows) = search_symbols_named(&mut daemon, "Target") else {
		panic!("target search");
	};
	let owner = &owner_rows.rows[0].uri;
	let target = &target_rows.rows[0].uri;

	let path_response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(
		QueryRequest::new(Query::GraphPath(GraphPathQuery {
			from: owner.clone(),
			to: target.clone(),
			relation: vec!["uses_type".to_string(), "typed_as".to_string()],
			max_depth: 2,
			max_symbols: 16,
			max_edges: 16,
			min_coverage: 0,
			..Default::default()
		})),
	)));
	let ProtocolResponse::Query(path_response) = path_response else {
		panic!("owner path response: {path_response:?}");
	};
	let QueryResult::GraphPath(path) = path_response.result else {
		panic!("owner path result");
	};
	assert!(path.from_endpoint_symbols > 1, "{path:?}");
	assert_eq!(path.reachable, Some(true), "{path:?}");
	assert_eq!(path.path.len(), 1, "{path:?}");
	assert_eq!(path.path[0].source.kind, "field", "{path:?}");

	let corridor_response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(
		QueryRequest::new(Query::GraphCorridor(GraphCorridorQuery {
			from: owner.clone(),
			to: target.clone(),
			scope: GraphSymbolScope {
				lang: vec!["rs".to_string()],
				..Default::default()
			},
			relation: vec!["uses_type".to_string(), "typed_as".to_string()],
			max_depth: 2,
			max_symbols: 16,
			max_edges: 16,
			min_coverage: 0,
			..Default::default()
		})),
	)));
	let ProtocolResponse::Query(corridor_response) = corridor_response else {
		panic!("owner corridor response: {corridor_response:?}");
	};
	let QueryResult::GraphCorridor(corridor) = corridor_response.result else {
		panic!("owner corridor result");
	};
	assert!(corridor.from_endpoint_symbols > 1, "{corridor:?}");
	assert_eq!(corridor.connected, Some(true), "{corridor:?}");
	assert_eq!(corridor.edge_count, 1, "{corridor:?}");

	let self_response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(
		QueryRequest::new(Query::GraphCorridor(GraphCorridorQuery {
			from: owner.clone(),
			to: owner.clone(),
			scope: GraphSymbolScope {
				lang: vec!["rs".to_string()],
				..Default::default()
			},
			relation: vec!["uses_type".to_string()],
			max_symbols: 16,
			..Default::default()
		})),
	)));
	let ProtocolResponse::Query(self_response) = self_response else {
		panic!("owner self corridor response: {self_response:?}");
	};
	let QueryResult::GraphCorridor(self_corridor) = self_response.result else {
		panic!("owner self corridor result");
	};
	assert_eq!(self_corridor.from_endpoint_symbols, 1, "{self_corridor:?}");
	assert_eq!(self_corridor.to_endpoint_symbols, 1, "{self_corridor:?}");
	assert_eq!(self_corridor.member_count, 1, "{self_corridor:?}");
	assert_eq!(self_corridor.edge_count, 0, "{self_corridor:?}");
}

#[test]
fn symbol_graph_partitions_unit_boundary_edges() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src_dir = temp.path().join("src");
	fs::create_dir_all(&src_dir).expect("src dir");
	fs::write(src_dir.join("lib.rs"), "pub mod engine;\npub mod driver;\n").expect("write lib");
	fs::write(
		src_dir.join("engine.rs"),
		"pub fn entry() { helper(); helper(); crate::driver::remote(); }\nfn helper() { helper(); }\n",
	)
	.expect("write engine");
	fs::write(
		src_dir.join("driver.rs"),
		"pub fn remote() {}\npub fn boss() { crate::engine::entry(); }\n",
	)
	.expect("write driver");
	let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
		roots: vec![temp.path().display().to_string()],
		project: None,
		cache_dir: None,
		live_refresh: None,
	})
	.expect("daemon");
	let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	assert!(matches!(refreshed, ProtocolResponse::Command(_)));
	let mut graph = |focus: &str| {
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::SymbolGraph(code_moniker_query::SymbolGraphQuery {
				workspace: None,
				focus: focus.to_string(),
				..Default::default()
			}),
			consistency: code_moniker_query::Consistency::Current,
			page: Page::default(),
		})));
		let ProtocolResponse::Query(query) = response else {
			panic!("expected query response");
		};
		let QueryResult::SymbolGraph(result) = query.result else {
			panic!("expected symbol graph, got {:?}", query.result);
		};
		result
	};

	let file = graph("src/engine.rs");
	assert_eq!(file.members.len(), 2, "{file:?}");
	assert!(
		file.internal_edges.iter().any(|edge| edge.count == 2),
		"entry -> helper twice: {file:?}"
	);
	assert!(
		file.internal_edges
			.iter()
			.any(|edge| edge.source == edge.target),
		"helper recursion stays internal: {file:?}"
	);
	assert!(
		file.callers
			.iter()
			.any(|caller| caller.symbol.name.starts_with("boss")),
		"{file:?}"
	);
	assert!(
		file.callees
			.iter()
			.any(|callee| callee.symbol.name.starts_with("remote")),
		"{file:?}"
	);

	let entry_uri = file
		.members
		.iter()
		.find(|member| member.name.starts_with("entry"))
		.expect("entry member")
		.uri
		.clone();
	let unit = graph(&entry_uri);
	assert!(
		matches!(&unit.focus, code_moniker_query::SymbolGraphFocus::Symbol { symbol } if symbol.name.starts_with("entry")),
		"{unit:?}"
	);
	assert!(
		unit.callees
			.iter()
			.any(|callee| callee.symbol.name.starts_with("helper") && callee.count == 2),
		"same-file helper is OUTSIDE the fn unit: {unit:?}"
	);
	assert!(
		unit.callees
			.iter()
			.any(|callee| callee.symbol.name.starts_with("remote")),
		"{unit:?}"
	);
	assert!(
		unit.callers
			.iter()
			.any(|caller| caller.symbol.name.starts_with("boss")),
		"{unit:?}"
	);
	assert!(unit.internal_edges.is_empty(), "{unit:?}");
	assert_filtered_outgoing_graph(&mut daemon, entry_uri);
}
