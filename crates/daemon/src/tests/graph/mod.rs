use super::*;

fn assert_filtered_outgoing_graph(daemon: &mut WorkspaceDaemon, entry_uri: String) {
	let filtered = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
		query: Query::SymbolGraph(code_moniker_query::SymbolGraphQuery {
			workspace: None,
			focus: entry_uri,
			direction: code_moniker_query::UsageDirection::Outgoing,
			relation: vec!["calls".to_string()],
			min_count: 2,
			include_internal: false,
			limit: 40,
		}),
		consistency: code_moniker_query::Consistency::Current,
		page: Page::default(),
	})));
	let ProtocolResponse::Query(filtered) = filtered else {
		panic!("expected filtered graph response");
	};
	let QueryResult::SymbolGraph(filtered) = filtered.result else {
		panic!("expected filtered graph, got {:?}", filtered.result);
	};
	assert!(filtered.callers.is_empty(), "{filtered:?}");
	assert_eq!(filtered.coverage.callers.total, 1, "{filtered:?}");
	assert_eq!(filtered.coverage.callers.matching, 0, "{filtered:?}");
	assert_eq!(filtered.coverage.callers.returned, 0, "{filtered:?}");
	assert!(filtered.internal_edges.is_empty(), "{filtered:?}");
	assert_eq!(filtered.callees.len(), 1, "{filtered:?}");
	assert_eq!(filtered.coverage.callees.total, 2, "{filtered:?}");
	assert_eq!(filtered.coverage.callees.matching, 1, "{filtered:?}");
	assert_eq!(filtered.coverage.callees.returned, 1, "{filtered:?}");
	assert!(filtered.callees[0].symbol.name.starts_with("helper"));
}

fn graph_path(
	daemon: &mut WorkspaceDaemon,
	from: &str,
	to: &str,
	expect: GraphPathExpectation,
	max_depth: usize,
) -> GraphPathResult {
	graph_path_with_limits(
		daemon,
		from,
		to,
		expect,
		BoundedPathLimits {
			max_depth,
			max_symbols: 10_000,
			max_edges: 50_000,
		},
	)
}

fn graph_path_with_limits(
	daemon: &mut WorkspaceDaemon,
	from: &str,
	to: &str,
	expect: GraphPathExpectation,
	limits: BoundedPathLimits,
) -> GraphPathResult {
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::GraphPath(GraphPathQuery {
			workspace: None,
			from: from.to_string(),
			to: to.to_string(),
			expect,
			relation: vec!["calls".to_string(), "method_call".to_string()],
			max_depth: limits.max_depth,
			max_symbols: limits.max_symbols,
			max_edges: limits.max_edges,
			min_coverage: 100,
		}),
	))));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected graph path response, got {response:?}");
	};
	let rendered = code_moniker_query::format_query_response(&response);
	assert!(rendered.contains("reachable:"), "{rendered}");
	assert!(rendered.contains("coverage:"), "{rendered}");
	let QueryResult::GraphPath(result) = response.result else {
		panic!("expected graph path result, got {:?}", response.result);
	};
	*result
}

fn graph_corridor(
	daemon: &mut WorkspaceDaemon,
	from: &str,
	to: &str,
	limits: BoundedPathLimits,
) -> GraphCorridorResult {
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::GraphCorridor(GraphCorridorQuery {
			workspace: None,
			from: from.to_string(),
			to: to.to_string(),
			scope: GraphSymbolScope {
				shape: vec!["callable".to_string()],
				..Default::default()
			},
			relation: vec!["calls".to_string(), "method_call".to_string()],
			max_depth: limits.max_depth,
			max_symbols: limits.max_symbols,
			max_edges: limits.max_edges,
			min_coverage: 100,
		}),
	))));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected graph corridor response, got {response:?}");
	};
	let rendered = code_moniker_query::format_query_response(&response);
	assert!(rendered.contains("connected:"), "{rendered}");
	assert!(rendered.contains("members:"), "{rendered}");
	let QueryResult::GraphCorridor(result) = response.result else {
		panic!("expected graph corridor result, got {:?}", response.result);
	};
	*result
}

struct GraphPathFixture {
	_temp: tempfile::TempDir,
	daemon: WorkspaceDaemon,
	uris: BTreeMap<&'static str, String>,
}

impl GraphPathFixture {
	fn uri(&self, name: &'static str) -> String {
		self.uris
			.get(name)
			.unwrap_or_else(|| panic!("missing fixture symbol {name}"))
			.clone()
	}
}

fn graph_path_fixture() -> GraphPathFixture {
	let temp = tempfile::tempdir().expect("tempdir");
	let src_dir = temp.path().join("src");
	fs::create_dir_all(&src_dir).expect("src dir");
	fs::write(
		src_dir.join("lib.rs"),
		concat!(
			"mod excluded;\n",
			"pub fn callback() { service(); alternative(); }\n",
			"fn service() { repository(); }\n",
			"fn alternative() { repository(); }\n",
			"fn repository() {}\n",
			"pub fn safe() { audit(); }\n",
			"fn audit() {}\n",
			"pub fn uncertain() { missing(); }\n",
			"pub fn cyclic() { cycle_a(); }\n",
			"fn cycle_a() { cycle_b(); }\n",
			"fn cycle_b() { cycle_a(); }\n",
			"pub fn scoped_entry() { excluded::excluded(); scoped_target(); }\n",
			"fn scoped_target() {}\n",
		),
	)
	.expect("write lib");
	fs::write(src_dir.join("excluded.rs"), "pub fn excluded() {}\n")
		.expect("write excluded module");
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
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::SymbolGraph(SymbolGraphQuery {
			workspace: None,
			focus: "src/lib.rs".to_string(),
			..Default::default()
		}),
	))));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected symbol graph response");
	};
	let QueryResult::SymbolGraph(graph) = response.result else {
		panic!("expected symbol graph, got {:?}", response.result);
	};
	let uris = [
		"callback",
		"service",
		"alternative",
		"repository",
		"safe",
		"uncertain",
		"cyclic",
		"cycle_a",
		"cycle_b",
		"scoped_entry",
		"scoped_target",
	]
	.into_iter()
	.map(|name| {
		let uri = graph
			.members
			.iter()
			.find(|member| member.name.starts_with(name))
			.unwrap_or_else(|| panic!("missing {name}: {graph:?}"))
			.uri
			.clone();
		(name, uri)
	})
	.collect();
	GraphPathFixture {
		_temp: temp,
		daemon,
		uris,
	}
}

mod contracts;
mod corridor;
mod path;
mod resolution;
