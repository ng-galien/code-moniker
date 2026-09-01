use super::*;

#[test]
fn stateless_syntax_parse_does_not_require_a_loaded_snapshot() {
	let temp = tempfile::tempdir().expect("tempdir");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::SyntaxParse(code_moniker_query::SyntaxParseQuery {
			language: "plpgsql".to_string(),
			source: "DECLARE total numeric; BEGIN total := 1; RETURN total; END;".to_string(),
			uri: None,
			max_depth: 12,
			max_nodes: 200,
			named_only: true,
			include_text: true,
			max_text_chars: 40,
		}),
	))));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected stateless syntax response, got {response:?}");
	};
	assert_eq!(response.generation, None);
	let QueryResult::SyntaxTree(tree) = response.result else {
		panic!("expected syntax tree, got {:?}", response.result);
	};
	assert_eq!(tree.file, "snippet.plpgsql");
	assert_eq!(tree.language, "plpgsql");
	assert_eq!(tree.root.kind, "source_file");
	assert!(syntax_node_contains(&tree.root, "decl_statement", None));
	assert!(syntax_node_contains(&tree.root, "stmt_assign", None));
	assert!(syntax_node_contains(&tree.root, "stmt_return", None));
}

#[test]
fn stateless_syntax_parse_accepts_client_owned_limits_for_deep_postgres_lists() {
	let temp = tempfile::tempdir().expect("tempdir");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let source = r#"SELECT
  address_1.id,
  address_1.label,
  address_1.line1,
  address_1.line2,
  address_1.postal_code,
  address_1.city,
  address_1.country_code,
  address_1.created_at,
  shipment_2.id,
  shipment_2.sales_order_id,
  shipment_2.warehouse_id,
  shipment_2.shipping_address_id,
  shipment_2.carrier,
  shipment_2.tracking_number,
  shipment_2.status,
  shipment_2.shipped_at,
  shipment_2.delivered_at
FROM
  shop.address AS address_1
  LEFT JOIN shop.shipment AS shipment_2 ON address_1.id = shipment_2.shipping_address_id;"#;
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::SyntaxParse(code_moniker_query::SyntaxParseQuery {
			language: "sql".to_string(),
			source: source.to_string(),
			uri: Some("qualified-columns.sql".to_string()),
			max_depth: 64,
			max_nodes: 2_000,
			named_only: true,
			include_text: false,
			max_text_chars: 80,
		}),
	))));
	let ProtocolResponse::Query(response) = response else {
		panic!("client-selected syntax limits must be accepted, got {response:?}");
	};
	let QueryResult::SyntaxTree(tree) = response.result else {
		panic!("expected syntax tree result");
	};
	assert!(!tree.has_error, "valid PostgreSQL must parse: {tree:#?}");
	assert!(
		!tree.truncated,
		"depth 64 must retain the complete target list"
	);
	assert_eq!(tree.emitted_nodes, tree.total_nodes);
}

#[test]
fn stateless_syntax_parse_accepts_large_explicit_client_limits() {
	let temp = tempfile::tempdir().expect("tempdir");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	for (max_depth, max_nodes) in [(1_000, 100), (6, 20_000)] {
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::SyntaxParse(code_moniker_query::SyntaxParseQuery {
				language: "sql".to_string(),
				source: "SELECT account.id FROM public.account AS account;".to_string(),
				uri: None,
				max_depth,
				max_nodes,
				named_only: true,
				include_text: false,
				max_text_chars: 80,
			}),
		))));
		assert!(
			matches!(response, ProtocolResponse::Query(_)),
			"explicit max_depth={max_depth} max_nodes={max_nodes} must be accepted: {response:?}"
		);
	}
}

#[test]
fn stateless_syntax_parse_rejects_zero_node_limit() {
	let temp = tempfile::tempdir().expect("tempdir");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::SyntaxParse(code_moniker_query::SyntaxParseQuery {
			language: "sql".to_string(),
			source: "SELECT 1;".to_string(),
			uri: None,
			max_depth: 64,
			max_nodes: 0,
			named_only: true,
			include_text: false,
			max_text_chars: 80,
		}),
	))));
	let ProtocolResponse::Error(error) = response else {
		panic!("zero max_nodes must be rejected, got {response:?}");
	};
	assert_eq!(error.code, "invalid_syntax_node_limit");
}

#[test]
fn stateless_syntax_parse_applies_small_client_limit_deterministically() {
	let temp = tempfile::tempdir().expect("tempdir");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let query = code_moniker_query::SyntaxParseQuery {
		language: "sql".to_string(),
		source: "SELECT account.id, account.email, account.status FROM public.account AS account;"
			.to_string(),
		uri: None,
		max_depth: 64,
		max_nodes: 12,
		named_only: true,
		include_text: false,
		max_text_chars: 80,
	};
	let parse = |daemon: &mut WorkspaceDaemon| {
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::SyntaxParse(query.clone()),
		))));
		let ProtocolResponse::Query(response) = response else {
			panic!("small client-selected limit must be accepted, got {response:?}");
		};
		let QueryResult::SyntaxTree(tree) = response.result else {
			panic!("expected syntax tree result");
		};
		tree
	};
	let first = parse(&mut daemon);
	let second = parse(&mut daemon);
	assert_eq!(first, second);
	assert_eq!(first.emitted_nodes, 12);
	assert!(first.truncated);
	assert!(!first.has_error);
}

#[test]
fn stateless_syntax_parse_handles_realistic_postgres_with_client_owned_limits() {
	let temp = tempfile::tempdir().expect("tempdir");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let source = r#"WITH recent_orders AS (
  SELECT
    orders.customer_id,
    orders.payload ->> 'status' AS status,
    sum(orders.amount) FILTER (WHERE orders.amount > 0) AS positive_total,
    row_number() OVER (
      PARTITION BY orders.customer_id
      ORDER BY orders.created_at DESC
    ) AS row_number
  FROM sales.orders AS orders
  JOIN sales.customers AS customers ON customers.id = orders.customer_id
  WHERE orders.created_at >= now() - interval '30 days'
    AND EXISTS (
      SELECT 1
      FROM sales.order_items AS items
      WHERE items.order_id = orders.id
        AND items.metadata @> '{"active": true}'::jsonb
    )
  GROUP BY orders.customer_id, orders.payload, orders.created_at
)
SELECT
  recent_orders.customer_id,
  jsonb_build_object(
    'status', recent_orders.status,
    'total', recent_orders.positive_total
  ) AS summary,
  count(*) OVER () AS result_count
FROM recent_orders
WHERE recent_orders.row_number = 1
ORDER BY recent_orders.positive_total DESC;"#;
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::SyntaxParse(code_moniker_query::SyntaxParseQuery {
			language: "sql".to_string(),
			source: source.to_string(),
			uri: Some("recent-orders.sql".to_string()),
			max_depth: 1_000,
			max_nodes: 20_000,
			named_only: true,
			include_text: false,
			max_text_chars: 80,
		}),
	))));
	let ProtocolResponse::Query(response) = response else {
		panic!("realistic PostgreSQL limits must be accepted, got {response:?}");
	};
	let QueryResult::SyntaxTree(tree) = response.result else {
		panic!("expected syntax tree result");
	};
	assert!(
		!tree.has_error,
		"realistic PostgreSQL must parse: {tree:#?}"
	);
	assert!(!tree.truncated, "client limit must retain the full tree");
	assert_eq!(tree.emitted_nodes, tree.total_nodes);
}

#[test]
fn stateless_syntax_parse_distinguishes_dollar_quoted_default_and_body() {
	let temp = tempfile::tempdir().expect("tempdir");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let source = "CREATE FUNCTION app.with_default(value text DEFAULT $$fallback$$)\n\
			RETURNS text\n\
			LANGUAGE plpgsql\n\
			AS $body$ BEGIN RETURN value; END; $body$;";
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::SyntaxParse(code_moniker_query::SyntaxParseQuery {
			language: "sql".to_string(),
			source: source.to_string(),
			uri: Some("default.sql".to_string()),
			max_depth: 20,
			max_nodes: 500,
			named_only: true,
			include_text: true,
			max_text_chars: 80,
		}),
	))));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected stateless syntax response, got {response:?}");
	};
	let QueryResult::SyntaxTree(tree) = response.result else {
		panic!("expected syntax tree, got {:?}", response.result);
	};
	assert!(!tree.has_error, "valid routine must parse: {tree:#?}");

	let function =
		syntax_node_find(&tree.root, "CreateFunctionStmt", None).expect("function declaration");
	assert_eq!(function.byte_range, (0, source.len() - 1));
	let parameter = syntax_node_find(&tree.root, "func_arg_with_default", None)
		.expect("parameter with default");
	let parameter_start = source.find("value text").expect("parameter start");
	let default_end = source.find(")\n").expect("parameter list end");
	assert_eq!(parameter.byte_range, (parameter_start, default_end));

	let default = syntax_node_find(&tree.root, "dollar_quoted_string", Some("$$fallback$$"))
		.expect("dollar-quoted default");
	let default_start = source.find("$$fallback$$").expect("default start");
	assert_eq!(
		default.byte_range,
		(default_start, default_start + "$$fallback$$".len())
	);

	let body =
		syntax_node_find_language(&tree.root, "source_file", "plpgsql").expect("PL/pgSQL body");
	let body_start = source.find("BEGIN").expect("body start");
	let body_end = source.rfind("$body$").expect("body end");
	assert_eq!(body.byte_range, (body_start, body_end));
	assert!(syntax_node_contains(body, "stmt_return", None));
}

#[test]
fn stateless_syntax_parse_accepts_quoted_plpgsql_labels() {
	let temp = tempfile::tempdir().expect("tempdir");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let source = r#"BEGIN
  <<"outer ""loop">>
  FOR i IN 1..10 LOOP
    EXIT "outer ""loop";
  END LOOP "outer ""loop";
END;"#;
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::SyntaxParse(code_moniker_query::SyntaxParseQuery {
			language: "plpgsql".to_string(),
			source: source.to_string(),
			uri: Some("quoted-label.plpgsql".to_string()),
			max_depth: 16,
			max_nodes: 300,
			named_only: false,
			include_text: true,
			max_text_chars: 80,
		}),
	))));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected stateless syntax response, got {response:?}");
	};
	let QueryResult::SyntaxTree(tree) = response.result else {
		panic!("expected syntax tree, got {:?}", response.result);
	};
	assert!(
		!tree.has_error,
		"quoted PL/pgSQL label must parse: {tree:#?}"
	);
	assert!(syntax_node_contains(&tree.root, "loop_label", None));
	assert!(syntax_node_contains(
		&tree.root,
		"quoted_identifier",
		Some("\"outer \"\"loop\""),
	));
	let quoted_label =
		syntax_node_find(&tree.root, "quoted_identifier", Some("\"outer \"\"loop\""))
			.expect("quoted loop label");
	let label_text = "\"outer \"\"loop\"";
	let label_start = source.find(label_text).expect("label text in source");
	assert_eq!(
		quoted_label.byte_range,
		(label_start, label_start + label_text.len())
	);
	assert_eq!((quoted_label.start.line, quoted_label.start.column), (2, 4));
	assert_eq!((quoted_label.end.line, quoted_label.end.column), (2, 18));
}

#[test]
fn stateless_syntax_parse_rejects_unsupported_languages_and_large_sources() {
	let temp = tempfile::tempdir().expect("tempdir");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	for (query, expected_code) in [
		(
			code_moniker_query::SyntaxParseQuery {
				language: "brainfuck".to_string(),
				source: "+++".to_string(),
				uri: None,
				max_depth: 6,
				max_nodes: 100,
				named_only: true,
				include_text: false,
				max_text_chars: 80,
			},
			"syntax_language_unsupported",
		),
		(
			code_moniker_query::SyntaxParseQuery {
				language: "rs".to_string(),
				source: "x".repeat(code_moniker_query::SYNTAX_PARSE_MAX_SOURCE_BYTES + 1),
				uri: None,
				max_depth: 6,
				max_nodes: 100,
				named_only: true,
				include_text: false,
				max_text_chars: 80,
			},
			"syntax_source_too_large",
		),
	] {
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::SyntaxParse(query),
		))));
		let ProtocolResponse::Error(error) = response else {
			panic!("expected {expected_code}, got {response:?}");
		};
		assert_eq!(error.code, expected_code);
	}
}

#[test]
fn stateless_syntax_parse_does_not_drain_or_refresh_live_workspace_events() {
	let temp = tempfile::tempdir().expect("tempdir");
	let changed = temp.path().join("later.rs");
	let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
		roots: vec![temp.path().display().to_string()],
		project: None,
		cache_dir: None,
		live_refresh: Some("auto".to_string()),
	})
	.expect("auto daemon");
	daemon
		.live
		.tx
		.send(WorkspaceLiveEvent::SourcesChanged(vec![changed.clone()]))
		.expect("queue live event");

	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::SyntaxParse(code_moniker_query::SyntaxParseQuery {
			language: "rs".to_string(),
			source: "fn answer() -> u32 { 42 }".to_string(),
			uri: None,
			max_depth: 6,
			max_nodes: 100,
			named_only: true,
			include_text: false,
			max_text_chars: 80,
		}),
	))));
	assert!(
		matches!(response, ProtocolResponse::Query(_)),
		"{response:?}"
	);
	let queued = daemon
		.live
		.rx
		.try_recv()
		.expect("live event remains queued");
	assert!(matches!(queued, WorkspaceLiveEvent::SourcesChanged(paths) if paths == vec![changed]));
	assert!(
		daemon.registry.queries().snapshot().is_none(),
		"stateless parse must not create a workspace snapshot"
	);
}

#[test]
fn daemon_returns_bounded_syntax_trees_for_files_and_symbols() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(
		temp.path().join("lib.rs"),
		"pub fn greet(name: &str) -> String {\n    format!(\"hello {name}\")\n}\n",
	)
	.expect("write fixture");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let refresh = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	assert!(matches!(refresh, ProtocolResponse::Command(_)));

	let file_response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(
		QueryRequest::new(Query::SyntaxTree(SyntaxTreeQuery {
			workspace: None,
			focus: "lib.rs".to_string(),
			max_depth: 1_000,
			max_nodes: 20_000,
			named_only: true,
			include_text: true,
			max_text_chars: 40,
		})),
	)));
	let ProtocolResponse::Query(file_response) = file_response else {
		panic!("expected syntax query response, got {file_response:?}");
	};
	let QueryResult::SyntaxTree(file_tree) = file_response.result else {
		panic!("expected syntax tree result");
	};
	assert_eq!(file_tree.file, "lib.rs");
	assert_eq!(file_tree.language, "rs");
	assert_eq!(file_tree.root.kind, "source_file");
	assert!(!file_tree.truncated);
	assert!(syntax_node_contains(&file_tree.root, "function_item", None));
	assert!(syntax_node_contains(
		&file_tree.root,
		"identifier",
		Some("greet")
	));

	let QueryResult::SymbolList(symbols) = search_symbols(&mut daemon, "greet") else {
		panic!("expected symbol list");
	};
	let symbol = symbols
		.rows
		.iter()
		.find(|symbol| symbol.name.starts_with("greet"))
		.expect("greet symbol");
	let symbol_response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(
		QueryRequest::new(Query::SyntaxTree(SyntaxTreeQuery {
			workspace: None,
			focus: symbol.uri.clone(),
			max_depth: 1,
			max_nodes: 2,
			named_only: true,
			include_text: false,
			max_text_chars: 0,
		})),
	)));
	let ProtocolResponse::Query(symbol_response) = symbol_response else {
		panic!("expected symbol syntax response, got {symbol_response:?}");
	};
	let QueryResult::SyntaxTree(symbol_tree) = symbol_response.result else {
		panic!("expected symbol syntax tree result");
	};
	assert_eq!(symbol_tree.root.kind, "function_item");
	assert_eq!(symbol_tree.emitted_nodes, 2);
	assert!(symbol_tree.truncated);
	assert!(symbol_tree.focus_line_range.is_some());
}

#[test]
fn daemon_syntax_tree_uses_language_sdk_injections_for_plpgsql() {
	let temp = tempfile::tempdir().expect("tempdir");
	let source = "CREATE FUNCTION account_balance(p_id bigint) RETURNS numeric\n\
			 LANGUAGE plpgsql AS $$\n\
			 <<\"account block\">>\n\
			 DECLARE total numeric;\n\
			 BEGIN\n\
			   SELECT sum(amount) INTO total FROM ledger_entry WHERE account_id = p_id;\n\
			   IF total IS NULL THEN RETURN 0; END IF;\n\
			   RETURN total;\n\
			 EXCEPTION WHEN OTHERS THEN RETURN -1;\n\
			 END \"account block\";\n\
			 $$;\n";
	fs::write(temp.path().join("account.sql"), source).expect("write PL/pgSQL fixture");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let refresh = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	assert!(matches!(refresh, ProtocolResponse::Command(_)));

	let QueryResult::SymbolList(symbols) = search_symbols(&mut daemon, "account_balance") else {
		panic!("expected SQL symbol list");
	};
	let function = symbols
		.rows
		.iter()
		.find(|symbol| symbol.kind == "function")
		.expect("account_balance function");
	let compact = code_moniker_workspace::code::compact_identity(&function.uri, "code+moniker://")
		.expect("compact SQL moniker");
	assert!(
		!compact.contains('/'),
		"fixture must cover root-level compact monikers: {compact}"
	);
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::SyntaxTree(SyntaxTreeQuery {
			workspace: None,
			focus: compact,
			max_depth: 20,
			max_nodes: 500,
			named_only: true,
			include_text: true,
			max_text_chars: 40,
		}),
	))));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected PL/pgSQL syntax response, got {response:?}");
	};
	let QueryResult::SyntaxTree(tree) = response.result else {
		panic!("expected PL/pgSQL syntax tree result");
	};
	assert!(
		!tree.has_error,
		"indexed quoted PL/pgSQL label must parse: {tree:#?}"
	);
	assert_eq!(tree.root.kind, "toplevel_stmt");
	assert!(syntax_node_contains(&tree.root, "CreateFunctionStmt", None));
	assert!(syntax_node_contains_language(
		&tree.root,
		"source_file",
		"plpgsql"
	));
	assert!(syntax_node_contains(&tree.root, "stmt_if", None));
	assert!(syntax_node_contains(&tree.root, "stmt_return", None));
	assert!(syntax_node_contains(&tree.root, "sql_expression", None));
	assert!(syntax_node_contains(&tree.root, "block_label", None));
	assert!(syntax_node_contains(
		&tree.root,
		"quoted_identifier",
		Some("\"account block\""),
	));
	let quoted_label = syntax_node_find(&tree.root, "quoted_identifier", Some("\"account block\""))
		.expect("quoted block label");
	let label_text = "\"account block\"";
	let label_start = source.find(label_text).expect("label text in source");
	assert_eq!(
		quoted_label.byte_range,
		(label_start, label_start + label_text.len())
	);
	assert_eq!(syntax_node_language_count(&tree.root, "plpgsql"), 1);
	let plpgsql = syntax_node_find_language(&tree.root, "source_file", "plpgsql")
		.expect("PL/pgSQL injection root");
	assert_eq!(plpgsql.entry_point.as_deref(), Some("block"));
	assert_eq!(plpgsql.has_error, Some(false));
	assert!(
		syntax_node_language_count(&tree.root, "sql") >= 3,
		"SELECT, IF, and RETURN regions must be rendered through nested SQL injections"
	);
	assert!(syntax_node_contains_entry_point(
		&tree.root,
		"sql",
		"statement"
	));
	assert!(syntax_node_contains_entry_point(
		&tree.root,
		"sql",
		"expression"
	));
}

#[test]
fn syntax_tree_disambiguates_one_line_csharp_symbols() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(
		temp.path().join("App.cs"),
		"class App { App() {} void Run() {} }\n",
	)
	.expect("write one-line nested fixture");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let refresh = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	assert!(matches!(refresh, ProtocolResponse::Command(_)));
	let QueryResult::SymbolList(symbols) = search_symbols(&mut daemon, "App") else {
		panic!("expected C# symbol list");
	};
	for (symbol_kind, expected_node_kind) in [
		("class", "class_declaration"),
		("constructor", "constructor_declaration"),
	] {
		let symbol = symbols
			.rows
			.iter()
			.find(|symbol| symbol.kind == symbol_kind)
			.unwrap_or_else(|| panic!("missing {symbol_kind} symbol: {symbols:?}"));
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::SyntaxTree(SyntaxTreeQuery {
				workspace: None,
				focus: symbol.uri.clone(),
				max_depth: 4,
				max_nodes: 20,
				named_only: true,
				include_text: false,
				max_text_chars: 0,
			}),
		))));
		let ProtocolResponse::Query(response) = response else {
			panic!("expected {symbol_kind} syntax response, got {response:?}");
		};
		let QueryResult::SyntaxTree(tree) = response.result else {
			panic!("expected {symbol_kind} syntax tree result");
		};
		assert_eq!(tree.root.kind, expected_node_kind);
	}

	let QueryResult::SymbolList(symbols) = search_symbols(&mut daemon, "Run") else {
		panic!("expected C# symbol list");
	};
	let method = symbols
		.rows
		.iter()
		.find(|symbol| symbol.name.starts_with("Run"))
		.expect("Run method");
	let nested_response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(
		QueryRequest::new(Query::SyntaxTree(SyntaxTreeQuery {
			workspace: None,
			focus: method.uri.clone(),
			max_depth: 4,
			max_nodes: 20,
			named_only: true,
			include_text: false,
			max_text_chars: 0,
		})),
	)));
	let ProtocolResponse::Query(nested_response) = nested_response else {
		panic!("expected nested syntax response, got {nested_response:?}");
	};
	let QueryResult::SyntaxTree(nested_tree) = nested_response.result else {
		panic!("expected nested syntax tree result");
	};
	assert_eq!(nested_tree.root.kind, "method_declaration");
}

#[test]
fn syntax_tree_accepts_an_empty_memory_source() {
	let temp = tempfile::tempdir().expect("tempdir");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let refresh = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	assert!(matches!(refresh, ProtocolResponse::Command(_)));
	replace_source_set(
		&mut daemon,
		WorkspaceSourceSetDto {
			srcset: "empty".to_string(),
			revision: None,
			documents: vec![WorkspaceSourceDocumentDto {
				uri: "empty.rs".to_string(),
				language: "rs".to_string(),
				content: String::new(),
			}],
		},
	);
	let empty_response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(
		QueryRequest::new(Query::SyntaxTree(SyntaxTreeQuery {
			workspace: None,
			focus: "empty.rs".to_string(),
			max_depth: 4,
			max_nodes: 20,
			named_only: true,
			include_text: true,
			max_text_chars: 40,
		})),
	)));
	let ProtocolResponse::Query(empty_response) = empty_response else {
		panic!("expected empty memory syntax response, got {empty_response:?}");
	};
	let QueryResult::SyntaxTree(empty_tree) = empty_response.result else {
		panic!("expected empty memory syntax tree result");
	};
	assert_eq!(empty_tree.file, "empty.rs");
	assert_eq!(empty_tree.root.kind, "source_file");
}

fn syntax_node_contains(node: &SyntaxNodeDto, kind: &str, text: Option<&str>) -> bool {
	syntax_node_find(node, kind, text).is_some()
}

fn syntax_node_find<'a>(
	node: &'a SyntaxNodeDto,
	kind: &str,
	text: Option<&str>,
) -> Option<&'a SyntaxNodeDto> {
	if node.kind == kind && text.is_none_or(|text| node.text.as_deref() == Some(text)) {
		return Some(node);
	}
	node.children
		.iter()
		.find_map(|child| syntax_node_find(child, kind, text))
}

fn syntax_node_contains_language(node: &SyntaxNodeDto, kind: &str, language: &str) -> bool {
	syntax_node_find_language(node, kind, language).is_some()
}

fn syntax_node_find_language<'a>(
	node: &'a SyntaxNodeDto,
	kind: &str,
	language: &str,
) -> Option<&'a SyntaxNodeDto> {
	if node.kind == kind && node.language.as_deref() == Some(language) {
		return Some(node);
	}
	node.children
		.iter()
		.find_map(|child| syntax_node_find_language(child, kind, language))
}

fn syntax_node_language_count(node: &SyntaxNodeDto, language: &str) -> usize {
	usize::from(node.language.as_deref() == Some(language))
		+ node
			.children
			.iter()
			.map(|child| syntax_node_language_count(child, language))
			.sum::<usize>()
}

fn syntax_node_contains_entry_point(
	node: &SyntaxNodeDto,
	language: &str,
	entry_point: &str,
) -> bool {
	(node.language.as_deref() == Some(language) && node.entry_point.as_deref() == Some(entry_point))
		|| node
			.children
			.iter()
			.any(|child| syntax_node_contains_entry_point(child, language, entry_point))
}
