use super::*;

#[test]
fn query_describe_does_not_require_a_loaded_snapshot() {
	let temp = tempfile::tempdir().expect("tempdir");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::QueryDescribe(code_moniker_query::QueryDescribeQuery {
			verb: Some("change.context".to_string()),
		}),
	))));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected query response, got {response:?}");
	};
	let QueryResult::QueryDescribe(result) = response.result else {
		panic!("expected query describe, got {:?}", response.result);
	};
	assert_eq!(result.capabilities.len(), 1);
	assert_eq!(result.capabilities[0].name, "change.context");

	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::QueryDescribe(code_moniker_query::QueryDescribeQuery {
			verb: Some("graph.corridor".to_string()),
		}),
	))));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected corridor describe response, got {response:?}");
	};
	let QueryResult::QueryDescribe(result) = response.result else {
		panic!("expected query describe, got {:?}", response.result);
	};
	let corridor = &result.capabilities[0];
	assert!(
		corridor
			.constraints
			.iter()
			.any(|constraint| constraint.contains("at least one semantic scope facet")),
		"{corridor:?}"
	);
	assert!(
		corridor
			.fields
			.iter()
			.find(|field| field.name == "srcset")
			.is_some_and(|field| field.multiple),
		"{corridor:?}"
	);

	let unknown = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::QueryDescribe(code_moniker_query::QueryDescribeQuery {
			verb: Some("graph.future".to_string()),
		}),
	))));
	let ProtocolResponse::Error(error) = unknown else {
		panic!("unknown query must return protocol coaching");
	};
	assert_eq!(error.code, "unknown_query");
	assert!(
		error.message.contains(&format!(
			"protocol {}",
			code_moniker_query::PROTOCOL_VERSION
		)),
		"{error:?}"
	);
	assert!(error.message.contains("available queries:"), "{error:?}");
	assert!(error.message.contains("recycle"), "{error:?}");
}
