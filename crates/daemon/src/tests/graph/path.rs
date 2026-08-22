use super::*;

#[test]
fn graph_path_returns_minimal_witness_and_tri_state_confidence() {
	let mut fixture = graph_path_fixture();
	let callback = fixture.uri("callback");
	let repository = fixture.uri("repository");
	let safe = fixture.uri("safe");
	let uncertain = fixture.uri("uncertain");

	let reachable = graph_path(
		&mut fixture.daemon,
		&callback,
		&repository,
		GraphPathExpectation::Reachable,
		6,
	);
	assert_eq!(reachable.verdict, GraphPathVerdict::Pass, "{reachable:?}");
	assert_eq!(reachable.reachable, Some(true), "{reachable:?}");
	assert_eq!(reachable.no_path, Some(false), "{reachable:?}");
	assert_eq!(reachable.path.len(), 2, "{reachable:?}");
	assert!(
		reachable.path[0].target.name.starts_with("service"),
		"deterministic shortest witness: {reachable:?}"
	);
	assert!(
		reachable.path[1].target.name.starts_with("repository"),
		"{reachable:?}"
	);

	let safe = graph_path(
		&mut fixture.daemon,
		&safe,
		&repository,
		GraphPathExpectation::NoPath,
		6,
	);
	assert_eq!(safe.verdict, GraphPathVerdict::Pass, "{safe:?}");
	assert_eq!(safe.reachable, Some(false), "{safe:?}");
	assert_eq!(safe.no_path, Some(true), "{safe:?}");
	assert_eq!(safe.coverage.percent, 100, "{safe:?}");

	let uncertain = graph_path(
		&mut fixture.daemon,
		&uncertain,
		&repository,
		GraphPathExpectation::NoPath,
		6,
	);
	assert_eq!(
		uncertain.verdict,
		GraphPathVerdict::Inconclusive,
		"{uncertain:?}"
	);
	assert_eq!(uncertain.reachable, None, "{uncertain:?}");
	assert!(uncertain.coverage.unresolved > 0, "{uncertain:?}");
	assert!(!uncertain.coverage.gap_reasons.is_empty(), "{uncertain:?}");

	let bounded = graph_path(
		&mut fixture.daemon,
		&callback,
		&repository,
		GraphPathExpectation::Reachable,
		1,
	);
	assert_eq!(
		bounded.verdict,
		GraphPathVerdict::Inconclusive,
		"{bounded:?}"
	);
	assert!(bounded.search.depth_limit_reached, "{bounded:?}");
	assert!(bounded.path.is_empty(), "{bounded:?}");
}

#[test]
fn graph_path_bounds_cycles_and_exploration_limits() {
	let mut fixture = graph_path_fixture();
	let callback = fixture.uri("callback");
	let repository = fixture.uri("repository");
	let cyclic = fixture.uri("cyclic");
	let cycle = graph_path(
		&mut fixture.daemon,
		&cyclic,
		&repository,
		GraphPathExpectation::NoPath,
		6,
	);
	assert_eq!(cycle.verdict, GraphPathVerdict::Pass, "{cycle:?}");
	assert_eq!(cycle.no_path, Some(true), "{cycle:?}");
	assert!(!cycle.search.depth_limit_reached, "{cycle:?}");
	assert!(cycle.search.explored_symbols <= 3, "{cycle:?}");

	let limited = graph_path_with_limits(
		&mut fixture.daemon,
		&callback,
		&repository,
		GraphPathExpectation::Reachable,
		BoundedPathLimits {
			max_depth: 6,
			max_symbols: 1,
			max_edges: 50_000,
		},
	);
	assert_eq!(
		limited.verdict,
		GraphPathVerdict::Inconclusive,
		"{limited:?}"
	);
	assert!(limited.search.symbol_limit_reached, "{limited:?}");
	assert!(
		limited.reasons.iter().any(|reason| reason.contains(
			"symbol_limit:used=1,max=1,next=increase max_symbols or narrow relation/workspace"
		)),
		"{limited:?}"
	);
	assert!(
		limited
			.reasons
			.iter()
			.all(|reason| !reason.contains("path/lang/kind/shape/srcset")),
		"graph.path must not suggest graph.corridor-only facets: {limited:?}"
	);
	let edge_limited = graph_path_with_limits(
		&mut fixture.daemon,
		&callback,
		&repository,
		GraphPathExpectation::Reachable,
		BoundedPathLimits {
			max_depth: 6,
			max_symbols: 10_000,
			max_edges: 1,
		},
	);
	assert_eq!(
		edge_limited.verdict,
		GraphPathVerdict::Inconclusive,
		"{edge_limited:?}"
	);
	assert!(edge_limited.search.edge_limit_reached, "{edge_limited:?}");
	assert_eq!(edge_limited.search.admitted_references, 1);
	assert!(
		edge_limited.reasons.iter().any(|reason| reason.contains(
			"edge_limit:used=1,max=1,next=increase max_edges or narrow relation/workspace"
		)),
		"{edge_limited:?}"
	);
}

#[test]
fn graph_path_limit_hints_remain_executable_at_protocol_ceilings() {
	let coverage = BoundedPathCoverage {
		total: code_moniker_query::MAX_GRAPH_EDGES,
		decided: code_moniker_query::MAX_GRAPH_EDGES,
		..Default::default()
	};
	let assessment = graph_search_assessment(
		&coverage,
		GraphSearchLimitStatus {
			operation: GraphSearchOperation::Path,
			max_depth: code_moniker_query::MAX_GRAPH_DEPTH,
			depth_reached: code_moniker_query::MAX_GRAPH_DEPTH,
			max_symbols: code_moniker_query::MAX_GRAPH_SYMBOLS,
			explored_symbols: code_moniker_query::MAX_GRAPH_SYMBOLS,
			max_edges: code_moniker_query::MAX_GRAPH_EDGES,
			admitted_references: code_moniker_query::MAX_GRAPH_EDGES,
			depth_limit_reached: true,
			symbol_limit_reached: true,
			edge_limit_reached: true,
		},
		0,
	);
	assert_eq!(assessment.reasons.len(), 3, "{assessment:?}");
	assert!(
		assessment
			.reasons
			.iter()
			.all(|reason| reason.contains("next=narrow relation/workspace")),
		"{assessment:?}"
	);
	assert!(
		assessment
			.reasons
			.iter()
			.all(|reason| !reason.contains("increase")),
		"{assessment:?}"
	);
}
