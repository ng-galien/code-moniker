use super::*;

#[test]
fn graph_queries_reject_unbounded_direct_protocol_requests() {
	let temp = tempfile::tempdir().expect("tempdir");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let invalid_path = QueryRequest::new(Query::GraphPath(GraphPathQuery {
		from: "from".to_string(),
		to: "to".to_string(),
		max_depth: code_moniker_query::MAX_GRAPH_DEPTH + 1,
		..Default::default()
	}));
	let invalid_corridor = QueryRequest {
		query: Query::GraphCorridor(GraphCorridorQuery {
			from: "from".to_string(),
			to: "to".to_string(),
			max_edges: code_moniker_query::MAX_GRAPH_EDGES + 1,
			..Default::default()
		}),
		consistency: code_moniker_query::Consistency::Current,
		page: Page::default(),
	};
	for request in [invalid_path, invalid_corridor] {
		let ProtocolResponse::Error(error) =
			daemon.handle_protocol(ProtocolRequest::Query(Box::new(request)))
		else {
			panic!("unbounded graph request reached workspace execution");
		};
		assert_eq!(error.code, "invalid_graph_limits", "{error:?}");
	}
	let invalid_cursor = QueryRequest {
		query: Query::GraphCorridor(GraphCorridorQuery {
			from: "from".to_string(),
			to: "to".to_string(),
			scope: GraphSymbolScope {
				shape: vec!["callable".to_string()],
				..Default::default()
			},
			relation: vec!["calls".to_string()],
			..Default::default()
		}),
		consistency: code_moniker_query::Consistency::Current,
		page: Page {
			limit: 1,
			cursor: Some(QueryCursor::new(1, None)),
		},
	};
	let ProtocolResponse::Error(error) =
		daemon.handle_protocol(ProtocolRequest::Query(Box::new(invalid_cursor)))
	else {
		panic!("corridor cursor reached workspace execution");
	};
	assert_eq!(error.code, "graph_corridor_not_paginated", "{error:?}");

	for (query, expected_code) in [
		(
			GraphCorridorQuery {
				from: "from".to_string(),
				to: "to".to_string(),
				relation: vec!["calls".to_string()],
				..Default::default()
			},
			"missing_required",
		),
		(
			GraphCorridorQuery {
				from: "from".to_string(),
				to: "to".to_string(),
				scope: GraphSymbolScope {
					shape: vec!["callable".to_string()],
					..Default::default()
				},
				..Default::default()
			},
			"missing_required",
		),
	] {
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::GraphCorridor(query),
		))));
		let ProtocolResponse::Error(error) = response else {
			panic!("invalid corridor contract reached workspace execution");
		};
		assert_eq!(error.code, expected_code, "{error:?}");
	}
}
