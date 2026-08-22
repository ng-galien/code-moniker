use super::*;

#[test]
fn identity_children_walks_the_symbolic_tree() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src_dir = temp.path().join("src");
	fs::create_dir_all(&src_dir).expect("src dir");
	fs::write(src_dir.join("lib.rs"), "pub mod engine;\n").expect("write lib");
	fs::write(
		src_dir.join("engine.rs"),
		"pub fn entry() { helper(); }\nfn helper() {}\n",
	)
	.expect("write engine");
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
	let mut children = |prefix: &str| {
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::IdentityChildren(code_moniker_query::IdentityChildrenQuery {
				workspace: None,
				prefix: prefix.to_string(),
				limit: 80,
			}),
			consistency: code_moniker_query::Consistency::Current,
			page: Page::default(),
		})));
		let ProtocolResponse::Query(query) = response else {
			panic!("expected query response");
		};
		let QueryResult::IdentityChildren(result) = query.result else {
			panic!("expected identity children, got {:?}", query.result);
		};
		result.children
	};

	// Walk organizational segments (lang, dir, module wrappers) down to
	// the level that holds the engine module.
	let mut prefix = String::new();
	let engine = loop {
		let rows = children(&prefix);
		assert!(!rows.is_empty(), "no children under `{prefix}`");
		if let Some(engine) = rows.iter().find(|row| row.name == "engine") {
			break engine.clone();
		}
		let next = rows
			.iter()
			.find(|row| row.has_children)
			.unwrap_or_else(|| panic!("no descent from `{prefix}`: {rows:?}"));
		assert!(next.symbol.is_none(), "organizational segment: {next:?}");
		assert!(next.defs > 0, "{next:?}");
		prefix = next.identity.clone();
	};
	assert!(engine.defs >= 2, "entry + helper below engine: {engine:?}");

	let functions = children(&engine.identity);
	let entry = functions
		.iter()
		.find(|row| row.name.starts_with("entry"))
		.unwrap_or_else(|| panic!("entry under engine: {functions:?}"));
	assert_eq!(entry.kind, "fn");
	let symbol = entry.symbol.as_ref().expect("entry is a definition");
	assert_eq!(symbol.kind, "fn");
	assert!(symbol.file.ends_with("engine.rs"), "{symbol:?}");
	assert!(!entry.has_children, "{entry:?}");
	assert!(
		functions.iter().any(|row| row.name.starts_with("helper")),
		"{functions:?}"
	);
}

#[test]
fn identity_graph_rolls_up_cross_module_calls() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src_dir = temp.path().join("src");
	fs::create_dir_all(&src_dir).expect("src dir");
	fs::write(src_dir.join("lib.rs"), "pub mod engine;\npub mod driver;\n").expect("write lib");
	fs::write(
			src_dir.join("engine.rs"),
			"pub fn entry() { crate::driver::remote(); crate::driver::remote(); helper(); }\nfn helper() {}\n",
		)
		.expect("write engine");
	fs::write(src_dir.join("driver.rs"), "pub fn remote() {}\n").expect("write driver");
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
	let mut graph = |prefix: &str| {
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::IdentityGraph(code_moniker_query::IdentityGraphQuery {
				workspace: None,
				prefix: prefix.to_string(),
				path: Vec::new(),
				min_count: 1,
			}),
			consistency: code_moniker_query::Consistency::Current,
			page: Page::default(),
		})));
		let ProtocolResponse::Query(query) = response else {
			panic!("expected query response");
		};
		let QueryResult::IdentityGraph(result) = query.result else {
			panic!("expected identity graph, got {:?}", query.result);
		};
		result
	};

	// At the level that holds both modules, the two calls roll up into one
	// aggregated engine -> driver edge.
	let modules = graph("lang:rs/dir:src");
	assert!(
		modules.nodes.iter().any(|node| node.name == "engine"),
		"{modules:?}"
	);
	let rollup = modules
		.edges
		.iter()
		.find(|edge| {
			edge.source.ends_with("module:engine") && edge.target.ends_with("module:driver")
		})
		.unwrap_or_else(|| panic!("engine -> driver rollup: {modules:?}"));
	assert_eq!(rollup.count, 2, "{rollup:?}");
	assert!(
		rollup.kinds.iter().any(|kind| kind == "calls"),
		"{rollup:?}"
	);
	// entry -> helper stays inside module:engine: not an edge at this level.
	assert!(
		!modules.edges.iter().any(|edge| edge.source == edge.target),
		"{modules:?}"
	);

	// One level deeper the boundary crossing becomes an outgoing port.
	let engine = graph("lang:rs/dir:src/module:engine");
	assert!(
		engine
			.edges
			.iter()
			.any(|edge| edge.source.ends_with("fn:entry()") && edge.target.ends_with("fn:helper()")),
		"{engine:?}"
	);
	let port = engine
		.ports_out
		.iter()
		.find(|port| port.identity.ends_with("module:driver") || port.identity.contains("driver"))
		.unwrap_or_else(|| panic!("outgoing port toward driver: {engine:?}"));
	assert_eq!(port.count, 2, "{port:?}");
	assert_identity_graph_filtering_and_pagination(&mut daemon);
}

#[test]
fn coupling_metrics_measure_cross_scope_and_internal_connections() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src_dir = temp.path().join("src");
	fs::create_dir_all(&src_dir).expect("src dir");
	fs::write(src_dir.join("lib.rs"), "pub mod engine;\npub mod driver;\n").expect("write lib");
	fs::write(
			src_dir.join("engine.rs"),
			"pub fn entry() { crate::driver::remote(); crate::driver::remote(); helper(); }\nfn helper() { helper(); }\n",
		)
		.expect("write engine");
	fs::write(src_dir.join("driver.rs"), "pub fn remote() {}\n").expect("write driver");
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

	let mut metrics = |expression: &str| {
		let request = code_moniker_query::parse_query(expression).expect("metrics query");
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(request)));
		let ProtocolResponse::Query(response) = response else {
			panic!("expected metrics response, got {response:?}");
		};
		let QueryResult::MetricsCoupling(result) = response.result else {
			panic!("expected coupling metrics, got {:?}", response.result);
		};
		result
	};

	let cross = metrics(
		"metrics.coupling from:\"lang:rs/dir:src/module:engine\" to:\"lang:rs/dir:src/module:driver\" relation:calls",
	);
	assert_eq!(cross.references, 2, "{cross:?}");
	assert_eq!(cross.snapshot, "current", "{cross:?}");
	assert!(cross.git.is_none(), "{cross:?}");
	assert!(!cross.export_requested, "{cross:?}");
	assert!(!cross.export_recorded, "{cross:?}");
	assert_eq!(cross.connections, 1, "{cross:?}");
	assert_eq!(cross.source_symbols, 1, "{cross:?}");
	assert_eq!(cross.target_symbols, 1, "{cross:?}");
	assert_eq!(cross.by_target.len(), 1, "{cross:?}");
	assert!(
		cross.by_target[0].moniker.ends_with("/fn:remote()"),
		"{cross:?}"
	);
	assert_eq!(cross.by_target[0].references, 2, "{cross:?}");
	assert_eq!(
		cross.by_kind,
		vec![CountDto {
			name: "calls".to_string(),
			count: 2,
		}],
		"{cross:?}"
	);

	let internal = metrics(
		"metrics.coupling from:\"lang:rs/dir:src/module:engine\" to:\"lang:rs/dir:src/module:engine\" relation:calls",
	);
	assert_eq!(internal.references, 1, "{internal:?}");
	assert_eq!(internal.connections, 1, "{internal:?}");
	assert!(internal.by_target.is_empty(), "{internal:?}");
	assert_eq!(internal.same_symbol_references, 1, "{internal:?}");
	assert_eq!(internal.coverage.source_references, 4, "{internal:?}");
	assert_eq!(
		internal.coverage.resolved_source_references, 4,
		"{internal:?}"
	);

	let export = metrics(
		"metrics.coupling from:\"lang:rs/dir:src/module:engine\" to:\"lang:rs/dir:src/module:driver\" relation:calls snapshot:test export:true",
	);
	assert_eq!(export.snapshot, "test", "{export:?}");
	assert!(export.export_requested, "{export:?}");
	assert!(!export.export_recorded, "{export:?}");
}

fn assert_identity_graph_filtering_and_pagination(daemon: &mut WorkspaceDaemon) {
	let filtered = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
		query: Query::IdentityGraph(code_moniker_query::IdentityGraphQuery {
			workspace: None,
			prefix: "lang:rs/dir:src".to_string(),
			path: Vec::new(),
			min_count: 3,
		}),
		consistency: code_moniker_query::Consistency::Current,
		page: Page::default(),
	})));
	let ProtocolResponse::Query(filtered) = filtered else {
		panic!("expected filtered identity graph response");
	};
	let QueryResult::IdentityGraph(filtered) = filtered.result else {
		panic!("expected filtered identity graph result");
	};
	assert_eq!(filtered.coverage.edges_total, 1, "{filtered:?}");
	assert_eq!(filtered.coverage.edges_matching, 0, "{filtered:?}");
	assert!(filtered.edges.is_empty(), "{filtered:?}");

	let mut cursor = None;
	let mut emitted = 0usize;
	let mut pages = 0usize;
	let mut expected_matching = None;
	loop {
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::IdentityGraph(code_moniker_query::IdentityGraphQuery {
				workspace: None,
				prefix: "lang:rs/dir:src".to_string(),
				path: Vec::new(),
				min_count: 1,
			}),
			consistency: code_moniker_query::Consistency::Current,
			page: Page { cursor, limit: 1 },
		})));
		let ProtocolResponse::Query(response) = response else {
			panic!("expected paged identity graph response");
		};
		cursor = response.next_cursor.clone();
		let QueryResult::IdentityGraph(page) = response.result else {
			panic!("expected paged identity graph result");
		};
		assert!(page.coverage.rows_emitted <= 1, "{page:?}");
		emitted += page.coverage.rows_emitted;
		pages += 1;
		match expected_matching {
			Some(expected) => assert_eq!(page.coverage.rows_matching, expected),
			None => expected_matching = Some(page.coverage.rows_matching),
		}
		if cursor.is_none() {
			break;
		}
	}
	assert!(pages > 1, "pagination must expose more than one page");
	assert_eq!(emitted, expected_matching.expect("matching row count"));
}

#[test]
fn identity_graph_applies_path_scope_before_java_package_rollup() {
	let temp = tempfile::tempdir().expect("tempdir");
	let main = temp.path().join("src/com/acme");
	let tests = temp.path().join("tests/com/acme");
	fs::create_dir_all(&main).expect("main sources");
	fs::create_dir_all(&tests).expect("test sources");
	fs::write(
		main.join("StorageService.java"),
		"package com.acme; public class StorageService { public static void save() {} }\n",
	)
	.expect("write main source");
	fs::write(
			tests.join("StorageServiceTest.java"),
			"package com.acme; public class StorageServiceTest { void testSave() { StorageService.save(); } }\n",
		)
		.expect("write test source");
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

	let mut graph = |expression: &str| {
		let request = code_moniker_query::parse_query(expression).expect("identity graph query");
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(request)));
		let ProtocolResponse::Query(response) = response else {
			panic!("expected identity graph response, got {response:?}");
		};
		let QueryResult::IdentityGraph(result) = response.result else {
			panic!("expected identity graph result, got {:?}", response.result);
		};
		result
	};

	let complete = graph("identity.graph prefix:\"lang:java\"");
	let main_only = graph("identity.graph prefix:\"lang:java\" path:\"src/**\"");
	assert_eq!(main_only.path, vec!["src/**"]);
	let complete_defs: usize = complete.nodes.iter().map(|node| node.defs).sum();
	let main_defs: usize = main_only.nodes.iter().map(|node| node.defs).sum();
	assert!(main_defs > 0, "{main_only:?}");
	assert!(
		main_defs < complete_defs,
		"the test package must not be merged into the selected main package: complete={complete:?} main={main_only:?}"
	);
	assert!(
		main_only.ports_in.iter().any(|port| port.count > 0),
		"references from excluded test sources must remain visible as incoming boundary crossings: {main_only:?}"
	);
}

#[test]
fn symbol_usages_rolls_up_singleton_member_activity_without_internal_refs() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src = temp.path().join("src/com/acme");
	fs::create_dir_all(&src).expect("sources");
	fs::write(
		src.join("StorageService.java"),
		concat!(
			"package com.acme; public class StorageService { ",
			"public static final StorageService instance = new StorageService(); ",
			"public void save() {} }\n"
		),
	)
	.expect("write singleton");
	fs::write(
		src.join("ClientA.java"),
		"package com.acme; public class ClientA { void run() { StorageService.instance.save(); } }\n",
	)
	.expect("write client A");
	fs::write(
		src.join("ClientB.java"),
		"package com.acme; public class ClientB { void run() { StorageService.instance.save(); } }\n",
	)
	.expect("write client B");
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
	let QueryResult::SymbolList(symbols) = search_symbols_named(&mut daemon, "StorageService")
	else {
		panic!("expected storage service symbol");
	};
	let service = symbols
		.rows
		.iter()
		.find(|symbol| symbol.kind == "class")
		.expect("storage service class")
		.uri
		.clone();

	let mut usages = |include_descendants| {
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::SymbolUsages(code_moniker_query::SymbolUsagesQuery {
				workspace: None,
				uri: service.clone(),
				direction: code_moniker_query::UsageDirection::Incoming,
				path: Vec::new(),
				lang: Vec::new(),
				include_descendants,
				projection: Vec::new(),
			}),
			consistency: code_moniker_query::Consistency::Current,
			page: Page {
				cursor: None,
				limit: 1_000,
			},
		})));
		let ProtocolResponse::Query(response) = response else {
			panic!("expected usage response");
		};
		let QueryResult::SymbolUsages(result) = response.result else {
			panic!("expected usage result");
		};
		result
	};

	let exact = usages(false);
	let rolled = usages(true);
	assert_eq!(exact.targets, 1, "{exact:?}");
	assert!(rolled.targets > 1, "{rolled:?}");
	assert!(
		exact
			.rows
			.iter()
			.all(|row| !row.context.contains("module:Client")),
		"exact type usages must keep their existing meaning: {exact:?}"
	);
	assert!(
		rolled
			.rows
			.iter()
			.any(|row| row.context.contains("module:ClientA"))
			&& rolled
				.rows
				.iter()
				.any(|row| row.context.contains("module:ClientB")),
		"member-mediated singleton consumers must become visible: {rolled:?}"
	);
	assert!(
		rolled
			.incoming_summary
			.as_ref()
			.is_some_and(|summary| summary.contexts >= 2),
		"{rolled:?}"
	);
	let unique_refs = rolled
		.rows
		.iter()
		.map(|row| row.reference.as_str())
		.collect::<BTreeSet<_>>();
	assert_eq!(unique_refs.len(), rolled.rows.len(), "{rolled:?}");
	assert!(
		rolled.rows.iter().all(|row| {
			row.context != service && identity_rest(&row.context, &service).is_none()
		}),
		"relations internal to the owner boundary must not count as coupling: {rolled:?}"
	);
}

#[test]
fn symbol_graph_routes_directory_focus_to_identity_graph() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src_dir = temp.path().join("src");
	fs::create_dir_all(&src_dir).expect("src dir");
	fs::write(src_dir.join("lib.rs"), "pub fn entry() {}\n").expect("write lib");
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

	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
		query: Query::SymbolGraph(code_moniker_query::SymbolGraphQuery {
			workspace: None,
			focus: "src".to_string(),
			direction: code_moniker_query::UsageDirection::Both,
			relation: Vec::new(),
			min_count: 1,
			include_internal: true,
			limit: 40,
		}),
		consistency: code_moniker_query::Consistency::Current,
		page: Page::default(),
	})));
	let ProtocolResponse::Error(error) = response else {
		panic!("a directory focus must fail with routing guidance, got {response:?}");
	};
	assert_eq!(error.code, "focus_is_directory", "{error:?}");
	assert!(
		error.message.contains("identity.graph"),
		"the error must route to the scope graph, got {error:?}"
	);
}

#[test]
fn identity_graph_rejects_unknown_prefix_with_valid_heads() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src_dir = temp.path().join("src");
	fs::create_dir_all(&src_dir).expect("src dir");
	fs::write(src_dir.join("lib.rs"), "pub fn entry() {}\n").expect("write lib");
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
	let mut graph = |prefix: &str| {
		daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::IdentityGraph(code_moniker_query::IdentityGraphQuery {
				workspace: None,
				prefix: prefix.to_string(),
				path: Vec::new(),
				min_count: 1,
			}),
			consistency: code_moniker_query::Consistency::Current,
			page: Page::default(),
		})))
	};

	let response = graph("dir:src");
	let ProtocolResponse::Error(error) = response else {
		panic!("a prefix matching no identity must fail loudly, got {response:?}");
	};
	assert_eq!(error.code, "prefix_not_found", "{error:?}");
	assert!(
		error.message.contains("lang:rs"),
		"the error must list valid heads, got {error:?}"
	);

	let response = graph("lang:rs/dir:src/module:lib/fn:entry()");
	assert!(
		matches!(response, ProtocolResponse::Query(_)),
		"an exact leaf identity is a valid scope, got {response:?}"
	);
}

#[test]
fn identity_graph_separates_external_from_unresolved() {
	let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../workspace/tests/fixtures/projects/java/multiprojet");
	let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
		roots: vec![fixture.display().to_string()],
		project: None,
		cache_dir: None,
		live_refresh: None,
	})
	.expect("daemon");
	let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	assert!(matches!(refreshed, ProtocolResponse::Command(_)));
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
		query: Query::IdentityGraph(code_moniker_query::IdentityGraphQuery {
			workspace: None,
			prefix: String::new(),
			path: Vec::new(),
			min_count: 1,
		}),
		consistency: code_moniker_query::Consistency::Current,
		page: Page::default(),
	})));
	let ProtocolResponse::Query(query) = response else {
		panic!("expected query response");
	};
	let QueryResult::IdentityGraph(result) = query.result else {
		panic!("expected identity graph, got {:?}", query.result);
	};
	// The fixture explains every project-internal reference. Non-unique
	// candidates stay outside the graph but never masquerade as unresolved.
	assert!(result.unlinked.external > 0, "{:?}", result.unlinked);
	assert!(result.unlinked.candidate > 0, "{:?}", result.unlinked);
	assert_eq!(result.unlinked.unresolved, 0, "{:?}", result.unlinked);
	assert!(
		result.unlinked.unresolved_reasons.is_empty(),
		"{:?}",
		result.unlinked
	);
}
