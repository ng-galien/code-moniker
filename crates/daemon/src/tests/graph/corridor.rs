use super::*;

#[test]
fn graph_corridor_returns_the_complete_diamond_and_tri_state_connectivity() {
	let mut fixture = graph_path_fixture();
	let callback = fixture.uri("callback");
	let repository = fixture.uri("repository");
	let safe = fixture.uri("safe");
	let uncertain = fixture.uri("uncertain");
	let limits = BoundedPathLimits {
		max_depth: 6,
		max_symbols: 10_000,
		max_edges: 50_000,
	};

	let corridor = graph_corridor(&mut fixture.daemon, &callback, &repository, limits);
	assert_eq!(corridor.connected, Some(true), "{corridor:?}");
	assert!(corridor.result_complete, "{corridor:?}");
	assert!(corridor.search_complete, "{corridor:?}");
	let member_names = corridor
		.members
		.iter()
		.map(|member| member.name.split('(').next().unwrap_or(&member.name))
		.collect::<BTreeSet<_>>();
	assert_eq!(
		member_names,
		BTreeSet::from(["alternative", "callback", "repository", "service"]),
		"both branches must survive: {corridor:?}"
	);
	assert_eq!(corridor.edges.len(), 4, "{corridor:?}");
	assert!(
		corridor
			.edges
			.iter()
			.all(|edge| edge.count == 1 && edge.relations.iter().any(|kind| kind == "calls")),
		"{corridor:?}"
	);

	let disconnected = graph_corridor(&mut fixture.daemon, &safe, &repository, limits);
	assert_eq!(disconnected.connected, Some(false), "{disconnected:?}");
	assert!(disconnected.result_complete, "{disconnected:?}");
	assert!(disconnected.search_complete, "{disconnected:?}");
	assert!(disconnected.members.is_empty(), "{disconnected:?}");

	let uncertain = graph_corridor(&mut fixture.daemon, &uncertain, &repository, limits);
	assert_eq!(uncertain.connected, None, "{uncertain:?}");
	assert!(uncertain.result_complete, "{uncertain:?}");
	assert!(!uncertain.search_complete, "{uncertain:?}");
	assert!(uncertain.coverage.unresolved > 0, "{uncertain:?}");
}

#[test]
fn graph_corridor_preserves_participating_cycles_and_explicit_bounds() {
	let mut fixture = graph_path_fixture();
	let cyclic = fixture.uri("cyclic");
	let cycle_b = fixture.uri("cycle_b");
	let repository = fixture.uri("repository");
	let callback = fixture.uri("callback");
	let cycle = graph_corridor(
		&mut fixture.daemon,
		&cyclic,
		&cycle_b,
		BoundedPathLimits {
			max_depth: 6,
			max_symbols: 10_000,
			max_edges: 50_000,
		},
	);
	assert_eq!(cycle.connected, Some(true), "{cycle:?}");
	assert!(
		cycle.edges.iter().any(|edge| {
			edge.source.name.starts_with("cycle_b") && edge.target.name.starts_with("cycle_a")
		}),
		"the participating back edge is corridor structure: {cycle:?}"
	);

	let bounded = graph_corridor(
		&mut fixture.daemon,
		&callback,
		&repository,
		BoundedPathLimits {
			max_depth: 1,
			max_symbols: 10_000,
			max_edges: 50_000,
		},
	);
	assert_eq!(bounded.connected, None, "{bounded:?}");
	assert!(bounded.result_complete, "{bounded:?}");
	assert!(!bounded.search_complete, "{bounded:?}");
	assert!(bounded.search.depth_limit_reached, "{bounded:?}");
	let edge_limited = graph_corridor(
		&mut fixture.daemon,
		&callback,
		&repository,
		BoundedPathLimits {
			max_depth: 6,
			max_symbols: 10_000,
			max_edges: 1,
		},
	);
	assert_eq!(edge_limited.connected, None, "{edge_limited:?}");
	assert!(
		edge_limited.search.edge_limit_reached
			&& edge_limited
				.reasons
				.iter()
				.any(|reason| reason.starts_with("edge_limit:")),
		"{edge_limited:?}"
	);
	assert_eq!(edge_limited.search.max_edges, 1);
	assert_eq!(edge_limited.search.admitted_references, 1);
	assert!(
		edge_limited.reasons.iter().any(|reason| reason.contains(
			"edge_limit:used=1,max=1,next=narrow relation or path/lang/kind/shape/srcset"
		)),
		"{edge_limited:?}"
	);

	let same = graph_corridor(
		&mut fixture.daemon,
		&repository,
		&repository,
		BoundedPathLimits {
			max_depth: 0,
			max_symbols: 1,
			max_edges: 1,
		},
	);
	assert_eq!(same.connected, Some(true), "{same:?}");
	assert!(same.result_complete, "{same:?}");
	assert!(same.search_complete, "{same:?}");
	assert_eq!(same.members.len(), 1, "{same:?}");
	assert!(same.edges.is_empty(), "{same:?}");
}

#[test]
fn graph_corridor_returns_one_full_semantically_scoped_result() {
	let mut fixture = graph_path_fixture();
	let callback = fixture.uri("callback");
	let repository = fixture.uri("repository");
	let request = QueryRequest {
		query: Query::GraphCorridor(GraphCorridorQuery {
			workspace: None,
			from: callback.clone(),
			to: repository.clone(),
			scope: GraphSymbolScope {
				lang: vec!["rs".to_string()],
				shape: vec!["callable".to_string()],
				..Default::default()
			},
			relation: vec!["calls".to_string()],
			max_depth: 6,
			max_symbols: 256,
			max_edges: 1_024,
			min_coverage: 100,
		}),
		consistency: code_moniker_query::Consistency::Current,
		page: Page::default(),
	};
	let ProtocolResponse::Query(response) = fixture
		.daemon
		.handle_protocol(ProtocolRequest::Query(Box::new(request)))
	else {
		panic!("expected scoped corridor response");
	};
	assert!(response.next_cursor.is_none(), "{response:?}");
	let QueryResult::GraphCorridor(result) = response.result else {
		panic!("expected graph corridor result");
	};
	assert!(result.result_complete, "{result:?}");
	assert!(result.search_complete, "{result:?}");
	assert_eq!(result.connected, Some(true), "{result:?}");
	assert_eq!(result.member_count, result.members.len());
	assert_eq!(result.edge_count, result.edges.len());
	assert_eq!((result.member_count, result.edge_count), (4, 4));
	assert_eq!(result.search.max_symbols, 256);
	assert_eq!(result.search.max_edges, 1_024);

	let endpoints_exceed_limit = QueryRequest::new(Query::GraphCorridor(GraphCorridorQuery {
		workspace: None,
		from: callback.clone(),
		to: repository.clone(),
		scope: GraphSymbolScope {
			lang: vec!["java".to_string()],
			..Default::default()
		},
		relation: vec!["calls".to_string()],
		max_depth: 6,
		max_symbols: 1,
		max_edges: 1_024,
		min_coverage: 100,
	}));
	let ProtocolResponse::Error(error) = fixture
		.daemon
		.handle_protocol(ProtocolRequest::Query(Box::new(endpoints_exceed_limit)))
	else {
		panic!("endpoint floor exceeded max_symbols without an error");
	};
	assert_eq!(error.code, "graph_scope_too_large", "{error:?}");
	assert!(
			error.message.contains(
				"corridor combined endpoint scope resolves naturally to 2 owner-and-descendant symbols, above max_symbols=1"
			),
			"{error:?}"
		);
	assert!(
		error
			.message
			.contains("or select a more specific member endpoint")
	);
	assert!(!error.message.contains("facet_matches"), "{error:?}");

	let too_large = QueryRequest::new(Query::GraphCorridor(GraphCorridorQuery {
		workspace: None,
		from: callback.clone(),
		to: repository.clone(),
		scope: GraphSymbolScope {
			lang: vec!["rs".to_string()],
			shape: vec!["callable".to_string()],
			..Default::default()
		},
		relation: vec!["calls".to_string()],
		max_depth: 6,
		max_symbols: 2,
		max_edges: 1_024,
		min_coverage: 100,
	}));
	let ProtocolResponse::Error(error) = fixture
		.daemon
		.handle_protocol(ProtocolRequest::Query(Box::new(too_large)))
	else {
		panic!("oversized semantic scope reached corridor traversal");
	};
	assert_eq!(error.code, "graph_scope_too_large", "{error:?}");
	assert!(error.message.contains("facet_matches:lang="), "{error:?}");
	assert!(error.message.contains(",shape="), "{error:?}");
	assert!(error.message.contains("; next:"), "{error:?}");

	let excluded = QueryRequest::new(Query::GraphCorridor(GraphCorridorQuery {
		workspace: None,
		from: callback,
		to: repository,
		scope: GraphSymbolScope {
			lang: vec!["java".to_string()],
			..Default::default()
		},
		relation: vec!["calls".to_string()],
		max_depth: 6,
		max_symbols: 256,
		max_edges: 1_024,
		min_coverage: 100,
	}));
	let ProtocolResponse::Query(response) = fixture
		.daemon
		.handle_protocol(ProtocolRequest::Query(Box::new(excluded)))
	else {
		panic!("expected excluded corridor response");
	};
	let QueryResult::GraphCorridor(result) = response.result else {
		panic!("expected graph corridor result");
	};
	assert!(result.result_complete, "{result:?}");
	assert!(result.search_complete, "{result:?}");
	assert_eq!(result.connected, Some(false), "{result:?}");
	assert!(result.members.is_empty(), "{result:?}");
	assert!(result.edges.is_empty(), "{result:?}");

	let scoped_entry = fixture.uri("scoped_entry");
	let scoped_target = fixture.uri("scoped_target");
	let scoped_limits = QueryRequest::new(Query::GraphCorridor(GraphCorridorQuery {
		workspace: None,
		from: scoped_entry,
		to: scoped_target,
		scope: GraphSymbolScope {
			path: vec!["src/lib.rs".to_string()],
			..Default::default()
		},
		relation: vec!["calls".to_string(), "method_call".to_string()],
		max_depth: 2,
		max_symbols: 16,
		max_edges: 1,
		min_coverage: 100,
	}));
	let ProtocolResponse::Query(response) = fixture
		.daemon
		.handle_protocol(ProtocolRequest::Query(Box::new(scoped_limits)))
	else {
		panic!("expected bitmap-scoped limit response");
	};
	let QueryResult::GraphCorridor(result) = response.result else {
		panic!("expected graph corridor result");
	};
	assert!(
		result.search_complete,
		"out-of-scope method consumed max_edges: {result:?}"
	);
	assert_eq!(result.connected, Some(true), "{result:?}");
	assert_eq!(result.edge_count, 1, "{result:?}");
}
