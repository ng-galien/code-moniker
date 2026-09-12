use super::*;

#[test]
fn graph_corridor_includes_memory_source_set_intermediates() {
	let temp = tempfile::tempdir().expect("tempdir");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	replace_source_set(
		&mut daemon,
		WorkspaceSourceSetDto {
			srcset: "memory-graph".to_string(),
			revision: Some("1".to_string()),
			documents: vec![WorkspaceSourceDocumentDto {
				uri: "src/flow.rs".to_string(),
				language: "rs".to_string(),
				content: "pub fn memory_from() { memory_middle(); }\npub fn memory_middle() { memory_to(); }\npub fn memory_to() {}\n".to_string(),
			}],
		},
	);
	let QueryResult::SymbolList(from) = search_symbols_named(&mut daemon, "memory_from()") else {
		panic!("expected source symbol");
	};
	let QueryResult::SymbolList(to) = search_symbols_named(&mut daemon, "memory_to()") else {
		panic!("expected target symbol");
	};
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::GraphCorridor(GraphCorridorQuery {
			workspace: None,
			from: from.rows[0].uri.clone(),
			to: to.rows[0].uri.clone(),
			scope: GraphSymbolScope {
				shape: vec!["callable".to_string()],
				..Default::default()
			},
			relation: vec!["calls".to_string()],
			max_depth: 4,
			max_symbols: 20,
			max_edges: 20,
			min_coverage: 100,
		}),
	))));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected graph corridor response");
	};
	let QueryResult::GraphCorridor(corridor) = response.result else {
		panic!("expected graph corridor result");
	};
	assert_eq!(corridor.connected, Some(true), "{corridor:?}");
	assert!(
		corridor
			.members
			.iter()
			.any(|member| member.name.starts_with("memory_middle")),
		"{corridor:?}"
	);
}

#[test]
fn memory_source_set_replace_is_idempotent_and_removable() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(
		temp.path().join("local.rs"),
		"pub fn local_symbol_survives() {}\n",
	)
	.expect("write local source");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let refresh = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	let ProtocolResponse::Command(refresh) = refresh else {
		panic!("expected initial refresh, got {refresh:?}");
	};

	let source_set = WorkspaceSourceSetDto {
		srcset: "database".to_string(),
		revision: Some("1".to_string()),
		documents: vec![
			WorkspaceSourceDocumentDto {
				uri: "schema/accounts.sql".to_string(),
				language: "sql".to_string(),
				content: "CREATE TABLE app.virtual_accounts (id bigint);\n".to_string(),
			},
			WorkspaceSourceDocumentDto {
				uri: "schema/audit.sql".to_string(),
				language: "sql".to_string(),
				content: "CREATE TABLE app.virtual_audit (id bigint);\n".to_string(),
			},
		],
	};
	let replace = replace_source_set(&mut daemon, source_set.clone());
	assert!(
		replace.generation.expect("replace generation").0
			> refresh.generation.expect("refresh generation").0
	);

	let QueryResult::SymbolList(accounts) = search_symbols_named(&mut daemon, "virtual_accounts")
	else {
		panic!("expected symbol list");
	};
	assert_eq!(accounts.total, 1, "{accounts:?}");
	assert!(
		accounts.rows[0].uri.contains("/srcset:database/"),
		"the existing srcset identity facet must carry the supplied source set: {accounts:?}"
	);
	assert!(accounts.rows[0].file.ends_with("schema/accounts.sql"));
	assert_eq!(
		accounts.rows[0]
			.source
			.as_ref()
			.expect("in-memory source snippet")
			.lines[0]
			.text,
		"CREATE TABLE app.virtual_accounts (id bigint);"
	);

	let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	let ProtocolResponse::Command(refreshed) = refreshed else {
		panic!("expected full refresh, got {refreshed:?}");
	};
	assert_symbol_total(&mut daemon, "virtual_accounts", 1);

	let mut reordered = source_set.clone();
	reordered.documents.reverse();
	let duplicate = replace_source_set(&mut daemon, reordered);
	assert_eq!(duplicate.generation, refreshed.generation);

	replace_source_set(
		&mut daemon,
		WorkspaceSourceSetDto {
			srcset: "database".to_string(),
			revision: Some("2".to_string()),
			documents: vec![WorkspaceSourceDocumentDto {
				uri: "schema/accounts.sql".to_string(),
				language: "sql".to_string(),
				content: "CREATE TABLE app.virtual_customers (id bigint);\n".to_string(),
			}],
		},
	);
	assert_symbol_total(&mut daemon, "virtual_accounts", 0);
	assert_symbol_total(&mut daemon, "virtual_audit", 0);
	assert_symbol_total(&mut daemon, "virtual_customers", 1);
	assert_symbol_total(&mut daemon, "local_symbol_survives()", 1);

	let remove = remove_source_set(&mut daemon, "database");
	assert_symbol_total(&mut daemon, "virtual_customers", 0);
	assert_symbol_total(&mut daemon, "local_symbol_survives()", 1);

	let duplicate_remove = remove_source_set(&mut daemon, "database");
	assert_eq!(duplicate_remove.generation, remove.generation);

	let rules = temp.path().join("memory-lifecycle-rules.toml");
	fs::write(
		&rules,
		r#"
default_rules = false

[[rust.fn.where]]
id = "local-function-remains"
expr = "name =~ ."
message = "the local function remains visible"
"#,
	)
	.expect("lifecycle rules");
	assert_memory_root_absent_from_rules(&mut daemon, &rules);
}

#[test]
fn memory_source_set_refresh_extracts_only_added_or_modified_documents() {
	let temp = tempfile::tempdir().expect("tempdir");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	daemon
		.refresh_cancellable(WorkspaceCancellation::default())
		.expect("initial refresh");

	let source_set = WorkspaceSourceSetDto {
		srcset: "database".to_string(),
		revision: Some("1".to_string()),
		documents: (0..8)
			.map(|index| WorkspaceSourceDocumentDto {
				uri: format!("schema/table_{index}.sql"),
				language: "sql".to_string(),
				content: format!("CREATE TABLE app.table_{index} (id bigint);\n"),
			})
			.collect(),
	};
	replace_source_set(&mut daemon, source_set.clone());
	let initial = daemon
		.registry
		.queries()
		.snapshot()
		.expect("initial source set");
	assert_eq!(initial.index.timings.extraction_jobs, 8);
	assert!((1..=8).contains(&initial.index.timings.extraction_workers));
	assert_eq!(
		initial.timings.memory_source_refresh,
		Some(MemorySourceRefreshMetrics {
			mode: MemorySourceRefreshMode::Bulk,
			documents_total: 8,
			added: 8,
			modified: 0,
			removed: 0,
			unchanged: 0,
			extraction_jobs: 8,
			extraction_workers: initial.index.timings.extraction_workers,
			linkage_invocations: 1,
		})
	);

	let mut one_modified = source_set.clone();
	one_modified.revision = Some("2".to_string());
	one_modified.documents[3]
		.content
		.push_str("-- catalog metadata changed\n");
	replace_source_set(&mut daemon, one_modified.clone());
	let modified = daemon
		.registry
		.queries()
		.snapshot()
		.expect("modified source set");
	assert_eq!(modified.index.timings.extraction_jobs, 1);
	let refresh = modified
		.timings
		.memory_source_refresh
		.expect("memory refresh metrics");
	assert_eq!(refresh.mode, MemorySourceRefreshMode::Incremental);
	assert_eq!(refresh.modified, 1);
	assert_eq!(refresh.unchanged, 7);
	assert_eq!(refresh.extraction_jobs, 1);
	assert_eq!(refresh.linkage_invocations, 1);

	let mut one_added = one_modified.clone();
	one_added.revision = Some("3".to_string());
	one_added.documents.push(WorkspaceSourceDocumentDto {
		uri: "schema/table_8.sql".to_string(),
		language: "sql".to_string(),
		content: "CREATE TABLE app.table_8 (id bigint);\n".to_string(),
	});
	replace_source_set(&mut daemon, one_added.clone());
	let added = daemon.registry.queries().snapshot().expect("added source");
	assert_eq!(added.index.timings.extraction_jobs, 1);

	let mut one_removed = one_added.clone();
	one_removed.revision = Some("4".to_string());
	one_removed.documents.remove(4);
	replace_source_set(&mut daemon, one_removed.clone());
	let removed = daemon
		.registry
		.queries()
		.snapshot()
		.expect("removed source");
	assert_eq!(removed.index.timings.extraction_jobs, 0);
	assert_symbol_total(&mut daemon, "table_4", 0);

	let mut revision_only = one_removed;
	revision_only.revision = Some("5".to_string());
	replace_source_set(&mut daemon, revision_only);
	let revision = daemon
		.registry
		.queries()
		.snapshot()
		.expect("revision-only source set");
	assert_eq!(revision.index.timings.extraction_jobs, 0);
}

#[test]
fn first_memory_source_set_on_a_mixed_workspace_reports_incremental_path() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(temp.path().join("local.rs"), "pub fn local_symbol() {}\n").expect("local source");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	daemon
		.refresh_cancellable(WorkspaceCancellation::default())
		.expect("initial filesystem refresh");

	replace_source_set(
		&mut daemon,
		WorkspaceSourceSetDto {
			srcset: "database".to_string(),
			revision: Some("1".to_string()),
			documents: vec![WorkspaceSourceDocumentDto {
				uri: "schema/table.sql".to_string(),
				language: "sql".to_string(),
				content: "CREATE TABLE app.table (id bigint);\n".to_string(),
			}],
		},
	);

	let snapshot = daemon
		.registry
		.queries()
		.snapshot()
		.expect("mixed workspace snapshot");
	let refresh = snapshot
		.timings
		.memory_source_refresh
		.expect("memory refresh metrics");
	assert_eq!(refresh.mode, MemorySourceRefreshMode::Incremental);
	assert_eq!(refresh.added, 1);
	assert_eq!(refresh.extraction_jobs, 1);
	assert_eq!(refresh.linkage_invocations, 1);
	assert_symbol_total(&mut daemon, "local_symbol()", 1);
}

#[test]
fn memory_source_set_parallel_refresh_matches_a_complete_publication() {
	let temp = tempfile::tempdir().expect("tempdir");
	let incremental_root = temp.path().join("incremental");
	let complete_root = temp.path().join("complete");
	fs::create_dir_all(&incremental_root).expect("incremental root");
	fs::create_dir_all(&complete_root).expect("complete root");

	let initial = memory_equivalence_source_set("1", false);
	let final_set = memory_equivalence_source_set("2", true);
	let mut incremental = WorkspaceDaemon::new(vec![incremental_root]).expect("incremental daemon");
	incremental
		.refresh_cancellable(WorkspaceCancellation::default())
		.expect("initial incremental workspace");
	replace_source_set(&mut incremental, initial);
	replace_source_set(&mut incremental, final_set.clone());
	let incremental_snapshot = incremental
		.registry
		.queries()
		.snapshot()
		.expect("incremental snapshot")
		.clone();
	assert_eq!(incremental_snapshot.index.timings.extraction_jobs, 1);

	let mut complete = WorkspaceDaemon::new(vec![complete_root]).expect("complete daemon");
	complete
		.refresh_cancellable(WorkspaceCancellation::default())
		.expect("initial complete workspace");
	replace_source_set(&mut complete, final_set);
	let complete_snapshot = complete
		.registry
		.queries()
		.snapshot()
		.expect("complete snapshot");

	assert_eq!(
		incremental_snapshot.index.sources,
		complete_snapshot.index.sources
	);
	assert_eq!(
		incremental_snapshot.index.symbols,
		complete_snapshot.index.symbols
	);
	assert_eq!(
		incremental_snapshot.index.references,
		complete_snapshot.index.references
	);
	assert_eq!(
		incremental_snapshot.linkage.resolved,
		complete_snapshot.linkage.resolved
	);
	assert_eq!(
		incremental_snapshot.linkage.candidates,
		complete_snapshot.linkage.candidates
	);
	assert_eq!(
		incremental_snapshot.linkage.external,
		complete_snapshot.linkage.external
	);
	assert_eq!(
		incremental_snapshot.linkage.dynamic,
		complete_snapshot.linkage.dynamic
	);
	assert_eq!(
		incremental_snapshot.linkage.blocked,
		complete_snapshot.linkage.blocked
	);
	assert_eq!(
		incremental_snapshot.linkage.unresolved,
		complete_snapshot.linkage.unresolved
	);
}

fn memory_equivalence_source_set(revision: &str, modified: bool) -> WorkspaceSourceSetDto {
	WorkspaceSourceSetDto {
			srcset: "database".to_string(),
			revision: Some(revision.to_string()),
			documents: vec![
				WorkspaceSourceDocumentDto {
					uri: "schema/accounts.sql".to_string(),
					language: "sql".to_string(),
					content: "CREATE TABLE app.accounts (id bigint, name text);\n".to_string(),
				},
				WorkspaceSourceDocumentDto {
					uri: "schema/orders.sql".to_string(),
					language: "sql".to_string(),
					content: "CREATE TABLE app.orders (id bigint, account_id bigint REFERENCES app.accounts(id));\n"
						.to_string(),
				},
				WorkspaceSourceDocumentDto {
					uri: "schema/account_orders.sql".to_string(),
					language: "sql".to_string(),
					content: format!(
						"CREATE VIEW app.account_orders AS SELECT a.id, o.id FROM app.accounts a JOIN app.orders o ON o.account_id = a.id;\n{}",
						if modified { "-- refreshed\n" } else { "" }
					),
				},
			],
		}
}

#[test]
fn memory_source_set_refreshes_linkage_from_local_sources() {
	let temp = tempfile::tempdir().expect("tempdir");
	let source_dir = temp.path().join("src/main/java/app");
	fs::create_dir_all(&source_dir).expect("create Java source directory");
	fs::write(
		source_dir.join("Local.java"),
		"package app; public class Local { Generated value; }\n",
	)
	.expect("write local source");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	daemon
		.refresh_cancellable(WorkspaceCancellation::default())
		.expect("initial refresh");

	replace_source_set(
		&mut daemon,
		WorkspaceSourceSetDto {
			srcset: "main".to_string(),
			revision: Some("1".to_string()),
			documents: vec![WorkspaceSourceDocumentDto {
				uri: "src/main/java/app/Generated.java".to_string(),
				language: "java".to_string(),
				content: "package app; public class Generated {}\n".to_string(),
			}],
		},
	);
	let snapshot = daemon
		.registry
		.queries()
		.snapshot()
		.expect("indexed snapshot");
	let target = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.name == "Generated" && symbol.kind == "class")
		.expect("virtual target");
	assert!(
		snapshot
			.linkage
			.resolved
			.iter()
			.any(|edge| edge.target == target.id),
		"an unchanged local reference must be reconsidered when its in-memory target appears; \
			 refs={:?}; unresolved={:?}; target={}",
		snapshot
			.index
			.references
			.iter()
			.map(|reference| reference.target_identity.to_string())
			.collect::<Vec<_>>(),
		snapshot.linkage.unresolved,
		target.identity
	);
}

#[test]
fn memory_source_set_table_rename_identifies_readers_and_relinks_projection() {
	let temp = tempfile::tempdir().expect("tempdir");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	daemon
		.refresh_cancellable(WorkspaceCancellation::default())
		.expect("initial refresh");

	replace_source_set(
		&mut daemon,
		database_source_set(
			"before-table-rename",
			&[
				(
					"schema/tables/orders.sql",
					"CREATE TABLE sales.orders (id bigint);\n",
				),
				(
					"schema/views/orders_view.sql",
					"CREATE VIEW sales.orders_view AS SELECT id FROM sales.orders;\n",
				),
				(
					"schema/tables/audit.sql",
					"CREATE TABLE audit.events (id bigint);\n",
				),
			],
		),
	);

	let orders_uri = {
		let snapshot = daemon
			.registry
			.queries()
			.snapshot()
			.expect("indexed snapshot");
		let orders = snapshot
			.index
			.symbols
			.iter()
			.find(|symbol| symbol.name == "orders" && symbol.kind == "table")
			.expect("orders table");
		orders.identity.to_string()
	};
	let impact = incoming_usage_files(&mut daemon, &orders_uri);
	assert_eq!(
		impact,
		BTreeSet::from(["schema/views/orders_view.sql".to_string()]),
		"the existing graph identifies the table reader to reproject"
	);

	replace_source_set(
		&mut daemon,
		database_source_set(
			"after-table-rename",
			&[
				(
					"schema/tables/orders.sql",
					"CREATE TABLE sales.archived_orders (id bigint);\n",
				),
				(
					"schema/views/orders_view.sql",
					"CREATE VIEW sales.orders_view AS SELECT id FROM sales.archived_orders;\n",
				),
				(
					"schema/tables/audit.sql",
					"CREATE TABLE audit.events (id bigint);\n",
				),
			],
		),
	);

	assert_symbol_total(&mut daemon, "orders", 0);
	assert_symbol_total(&mut daemon, "archived_orders", 1);
	let archived_orders_uri = {
		let snapshot = daemon
			.registry
			.queries()
			.snapshot()
			.expect("indexed snapshot");
		let archived_orders = snapshot
			.index
			.symbols
			.iter()
			.find(|symbol| symbol.name == "archived_orders" && symbol.kind == "table")
			.expect("renamed table");
		archived_orders.identity.to_string()
	};
	assert_eq!(
		incoming_usage_files(&mut daemon, &archived_orders_uri),
		BTreeSet::from(["schema/views/orders_view.sql".to_string()]),
		"the reprojected view must follow the renamed table"
	);
}

#[test]
fn memory_source_set_function_rename_identifies_callers_and_relinks_projection() {
	let temp = tempfile::tempdir().expect("tempdir");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	daemon
		.refresh_cancellable(WorkspaceCancellation::default())
		.expect("initial refresh");

	replace_source_set(
		&mut daemon,
		database_source_set(
			"before-function-rename",
			&[
				(
					"schema/functions/total_orders.sql",
					"CREATE FUNCTION sales.total_orders() RETURNS integer LANGUAGE sql \
						AS $$ SELECT 1 $$;\n",
				),
				(
					"schema/views/dashboard.sql",
					"CREATE VIEW sales.dashboard AS SELECT sales.total_orders() AS total;\n",
				),
				(
					"schema/tables/audit.sql",
					"CREATE TABLE audit.events (id bigint);\n",
				),
			],
		),
	);

	let total_orders_uri = {
		let snapshot = daemon
			.registry
			.queries()
			.snapshot()
			.expect("indexed snapshot");
		let total_orders = snapshot
			.index
			.symbols
			.iter()
			.find(|symbol| symbol.name == "total_orders()" && symbol.kind == "function")
			.expect("total_orders function");
		total_orders.identity.to_string()
	};
	let impact = incoming_usage_files(&mut daemon, &total_orders_uri);
	assert_eq!(
		impact,
		BTreeSet::from(["schema/views/dashboard.sql".to_string()]),
		"the existing graph identifies the function caller to reproject"
	);

	replace_source_set(
		&mut daemon,
		database_source_set(
			"after-function-rename",
			&[
				(
					"schema/functions/total_orders.sql",
					"CREATE FUNCTION sales.total_orders_v2() RETURNS integer LANGUAGE sql \
						AS $$ SELECT 1 $$;\n",
				),
				(
					"schema/views/dashboard.sql",
					"CREATE VIEW sales.dashboard AS SELECT sales.total_orders_v2() AS total;\n",
				),
				(
					"schema/tables/audit.sql",
					"CREATE TABLE audit.events (id bigint);\n",
				),
			],
		),
	);

	assert_symbol_total(&mut daemon, "total_orders()", 0);
	assert_symbol_total(&mut daemon, "total_orders_v2()", 1);
	let total_orders_v2_uri = {
		let snapshot = daemon
			.registry
			.queries()
			.snapshot()
			.expect("indexed snapshot");
		let total_orders_v2 = snapshot
			.index
			.symbols
			.iter()
			.find(|symbol| symbol.name == "total_orders_v2()" && symbol.kind == "function")
			.expect("renamed function");
		total_orders_v2.identity.to_string()
	};
	assert_eq!(
		incoming_usage_files(&mut daemon, &total_orders_v2_uri),
		BTreeSet::from(["schema/views/dashboard.sql".to_string()]),
		"the reprojected view must call the renamed function"
	);
}

#[test]
fn memory_source_set_rejects_ambiguous_input() {
	for srcset in ["bad/name", ".", ".."] {
		let error = parse_memory_source_set(WorkspaceSourceSetDto {
			srcset: srcset.to_string(),
			revision: None,
			documents: Vec::new(),
		})
		.expect_err("invalid srcset");
		assert_eq!(error.code, "invalid_workspace_srcset");
	}

	let error = parse_memory_source_set(WorkspaceSourceSetDto {
		srcset: "generated".to_string(),
		revision: None,
		documents: vec![WorkspaceSourceDocumentDto {
			uri: "generated.data".to_string(),
			language: "unknown".to_string(),
			content: String::new(),
		}],
	})
	.expect_err("invalid language");
	assert_eq!(error.code, "unsupported_workspace_source_language");

	let document = WorkspaceSourceDocumentDto {
		uri: "generated.rs".to_string(),
		language: "rs".to_string(),
		content: String::new(),
	};
	let error = parse_memory_source_set(WorkspaceSourceSetDto {
		srcset: "generated".to_string(),
		revision: None,
		documents: vec![document.clone(), document],
	})
	.expect_err("duplicate URI");
	assert_eq!(error.code, "duplicate_workspace_source_uri");
}

#[test]
fn memory_source_set_publishes_its_new_generation() {
	let temp = tempfile::tempdir().expect("tempdir");
	let (events, mut rx) = tokio::sync::broadcast::channel(4);
	let mut daemon = WorkspaceDaemon::with_events(
		DaemonWorkspaceConfig {
			roots: vec![temp.path().display().to_string()],
			project: None,
			cache_dir: None,
			live_refresh: Some("on-demand".to_string()),
		},
		events,
	)
	.expect("daemon");
	daemon
		.refresh_cancellable(WorkspaceCancellation::default())
		.expect("initial refresh");
	let response = replace_source_set(
		&mut daemon,
		WorkspaceSourceSetDto {
			srcset: "generated".to_string(),
			revision: Some("1".to_string()),
			documents: vec![WorkspaceSourceDocumentDto {
				uri: "generated.rs".to_string(),
				language: "rs".to_string(),
				content: "pub fn generated() {}\n".to_string(),
			}],
		},
	);
	let event = rx.try_recv().expect("refreshed event");
	assert_eq!(event.kind, WorkspaceEventKind::Refreshed);
	assert_eq!(event.generation, response.generation);
}

#[test]
fn memory_source_set_replace_rolls_back_after_refresh_failure() {
	let temp = tempfile::tempdir().expect("tempdir");
	let workspace = temp.path().join("workspace");
	let unavailable = temp.path().join("workspace-unavailable");
	fs::create_dir_all(&workspace).expect("workspace");
	let mut daemon = WorkspaceDaemon::new(vec![workspace.clone()]).expect("daemon");
	let source_set = WorkspaceSourceSetDto {
		srcset: "generated".to_string(),
		revision: Some("1".to_string()),
		documents: vec![WorkspaceSourceDocumentDto {
			uri: "generated.rs".to_string(),
			language: "rs".to_string(),
			content: "pub struct RetriedPublication;\n".to_string(),
		}],
	};

	fs::rename(&workspace, &unavailable).expect("make workspace unavailable");
	let failed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceSourceSetReplace {
			source_set: source_set.clone(),
		},
	}));
	assert!(
		matches!(failed, ProtocolResponse::Error(_)),
		"the unavailable workspace must reject publication: {failed:?}"
	);

	fs::rename(&unavailable, &workspace).expect("restore workspace");
	let retried = replace_source_set(&mut daemon, source_set);
	assert!(
		retried.generation.is_some(),
		"replaying a failed publication must rebuild and publish it"
	);
	assert_symbol_total(&mut daemon, "RetriedPublication", 1);
}

#[test]
fn memory_source_set_has_workspace_level_multi_root_identity() {
	let temp = tempfile::tempdir().expect("tempdir");
	let first = temp.path().join("first");
	let second = temp.path().join("second");
	fs::create_dir_all(&first).expect("first root");
	fs::create_dir_all(&second).expect("second root");
	let source_set = WorkspaceSourceSetDto {
		srcset: "generated".to_string(),
		revision: Some("1".to_string()),
		documents: vec![WorkspaceSourceDocumentDto {
			uri: "generated.rs".to_string(),
			language: "rs".to_string(),
			content: "pub struct WorkspaceOwned;\n".to_string(),
		}],
	};
	let coordinates = |roots: Vec<PathBuf>| {
		let mut daemon = WorkspaceDaemon::new(roots).expect("daemon");
		daemon
			.refresh_cancellable(WorkspaceCancellation::default())
			.expect("initial refresh");
		replace_source_set(&mut daemon, source_set.clone());
		let QueryResult::SymbolList(symbols) = search_symbols_named(&mut daemon, "WorkspaceOwned")
		else {
			panic!("expected symbol list");
		};
		let symbol = symbols.rows.first().expect("workspace-owned symbol");
		(symbol.root.clone(), symbol.uri.clone())
	};

	let forward = coordinates(vec![first.clone(), second.clone()]);
	let reversed = coordinates(vec![second, first]);
	assert_eq!(forward, reversed);
	assert_eq!(forward.0, MEMORY_SOURCE_ROOT_LABEL);
}

#[test]
fn memory_source_set_runs_through_unscoped_indexed_rules() {
	let temp = tempfile::tempdir().expect("tempdir");
	let rules = temp.path().join("memory-rules.toml");
	fs::write(
		&rules,
		r#"
default_rules = false

[[rust.shape.type.where]]
id = "memory-type-is-visible"
expr = "name != 'WorkspaceOwned'"
message = "the indexed rule must observe the memory source"
"#,
	)
	.expect("rules");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	daemon
		.refresh_cancellable(WorkspaceCancellation::default())
		.expect("initial refresh");
	replace_source_set(
		&mut daemon,
		WorkspaceSourceSetDto {
			srcset: "generated".to_string(),
			revision: Some("1".to_string()),
			documents: vec![WorkspaceSourceDocumentDto {
				uri: "generated.rs".to_string(),
				language: "rs".to_string(),
				content: "pub struct WorkspaceOwned;\n".to_string(),
			}],
		},
	);

	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::RulesCheck(RulesCheckQuery {
			inline_rules: Vec::new(),
			workspace: None,
			profile: None,
			rules: Some(rules.display().to_string()),
			file: Vec::new(),
			report: true,
		}),
	))));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected rules response, got {response:?}");
	};
	let QueryResult::RulesCheck(result) = response.result else {
		panic!("expected rules result, got {:?}", response.result);
	};
	assert_eq!(result.summary.files_scanned, 1, "{result:?}");
	assert_eq!(result.summary.total_violations, 1, "{result:?}");
	assert_eq!(result.violations[0].root, MEMORY_SOURCE_ROOT);
	assert_eq!(
		result.violations[0].rule_id,
		"rust.shape.type.memory-type-is-visible"
	);
}

#[test]
fn memory_source_set_limits_bound_each_publication_and_global_usage() {
	let limits = MemorySourceLimits {
		max_source_sets: 1,
		max_documents_per_set: 1,
		max_uri_bytes: 8,
		max_document_bytes: 8,
		max_source_set_bytes: 20,
		max_total_bytes: 20,
	};
	let cache = LocalResourceCache::default();
	let first = MemorySourceSet {
		srcset: "first".to_string(),
		revision: None,
		documents: vec![MemorySourceDocument {
			uri: "a.rs".to_string(),
			lang: Lang::Rs,
			content: "fn a()".into(),
		}],
	};
	validate_memory_source_set_limits(&cache, &first, limits).expect("first publication fits");
	cache.replace_memory_source_set(first);

	let second = MemorySourceSet {
		srcset: "second".to_string(),
		revision: None,
		documents: vec![MemorySourceDocument {
			uri: "b.rs".to_string(),
			lang: Lang::Rs,
			content: "fn b()".into(),
		}],
	};
	let error = validate_memory_source_set_limits(&cache, &second, limits)
		.expect_err("global active-set limit");
	assert_eq!(error.code, "workspace_source_set_limit_exceeded");

	let oversized = MemorySourceSet {
		srcset: "first".to_string(),
		revision: None,
		documents: vec![MemorySourceDocument {
			uri: "long-uri.rs".to_string(),
			lang: Lang::Rs,
			content: "fn too_large()".into(),
		}],
	};
	let error = validate_memory_source_set_limits(&cache, &oversized, limits)
		.expect_err("per-publication limit");
	assert_eq!(error.code, "workspace_source_set_limit_exceeded");
}

#[test]
fn memory_source_sets_index_document_formats() {
	let temp = tempfile::tempdir().unwrap();
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).unwrap();
	replace_source_set(
		&mut daemon,
		WorkspaceSourceSetDto {
			srcset: "documents".to_string(),
			revision: Some("1".to_string()),
			documents: [
				("guide.md", "markdown", "# Guide\n"),
				("config.json", "json", "{\"json_key\":1}"),
				("config.yml", "yaml", "yaml_key: yes\n"),
			]
			.into_iter()
			.map(|(uri, language, content)| WorkspaceSourceDocumentDto {
				uri: uri.to_string(),
				language: language.to_string(),
				content: content.to_string(),
			})
			.collect(),
		},
	);
	for name in ["Guide", "json_key", "yaml_key"] {
		let QueryResult::SymbolList(result) = search_symbols_named(&mut daemon, name) else {
			panic!("symbol list")
		};
		assert_eq!(result.total, 1, "{name}: {result:?}");
	}
}
#[test]
fn inline_sql_index_rule_runs_on_memory_documents() {
	let temp = tempfile::tempdir().unwrap();
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).unwrap();
	daemon
		.refresh_cancellable(WorkspaceCancellation::default())
		.unwrap();
	for (revision, keys, expected) in [("1", "b,a", 1), ("2", "a,b", 0)] {
		replace_source_set(
			&mut daemon,
			WorkspaceSourceSetDto {
				srcset: "catalog".into(),
				revision: Some(revision.into()),
				documents: vec![WorkspaceSourceDocumentDto {
					uri: "postgres://db/app/table.sql".into(),
					language: "sql".into(),
					content: format!(
						"CREATE TABLE app.child (a int, b int, FOREIGN KEY (a,b) REFERENCES app.parent(a,b)); CREATE INDEX child_idx ON app.child ({keys});"
					),
				}],
			},
		);
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::RulesCheck(RulesCheckQuery {
				inline_rules: vec![
					include_str!("../../../../samples/sql-indexes/rules.toml").into(),
				],
				report: true,
				..RulesCheckQuery::default()
			}),
		))));
		let ProtocolResponse::Query(response) = response else {
			panic!("{response:?}")
		};
		let QueryResult::RulesCheck(result) = response.result else {
			panic!("{:?}", response.result)
		};
		assert!(result.errors.is_empty(), "{result:?}");
		assert_eq!(
			result.summary.total_violations, expected,
			"revision {revision}: {result:?}"
		);
	}
}

#[test]
fn sql_index_workspace_rule_is_independent_of_statement_partition_and_order() {
	const TABLE: &str = "CREATE TABLE app.child (a int, b int);";
	const FK: &str = "ALTER TABLE app.child ADD CONSTRAINT child_fk FOREIGN KEY (a,b) REFERENCES app.parent(a,b);";
	const INDEX: &str = "CREATE INDEX child_idx ON app.child (a,b);";
	let temp = tempfile::tempdir().unwrap();
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).unwrap();
	daemon
		.refresh_cancellable(WorkspaceCancellation::default())
		.unwrap();
	let statements = [TABLE, FK, INDEX];
	let mut revision = 0;
	for order in [
		[0, 1, 2],
		[0, 2, 1],
		[1, 0, 2],
		[1, 2, 0],
		[2, 0, 1],
		[2, 1, 0],
	] {
		for grouped in [true, false] {
			revision += 1;
			let ordered: Vec<_> = order.iter().map(|i| statements[*i].to_string()).collect();
			let documents = if grouped {
				vec![ordered.join("\n")]
			} else {
				ordered
			};
			let result = check_sql_statements(&mut daemon, revision, &documents);
			assert_eq!(
				result.summary.total_violations, 0,
				"order {order:?}, grouped={grouped}: {result:?}"
			);
			let reports: Vec<_> = result
				.rule_reports
				.iter()
				.filter(|r| r.rule_id == "workspace.symbol.fk-index-prefix")
				.collect();
			assert!(
				reports.iter().any(|r| r.antecedent_matches == Some(1)),
				"FK was not exercised: {result:?}"
			);
			assert!(
				reports.iter().all(|r| r.inconclusive == Some(0)),
				"{result:?}"
			);
		}
	}
	for (index, expected) in [
		("CREATE INDEX child_idx ON app.child (b,a);", 1),
		("CREATE INDEX child_idx ON app.child (a);", 1),
		("CREATE INDEX child_idx ON app.child (a) INCLUDE (b);", 1),
		("CREATE INDEX child_idx ON app.child (a,b) WHERE a > 0;", 1),
		("CREATE INDEX child_idx ON app.child ((a+1),b);", 1),
		(
			"CREATE TABLE other.child(a int,b int); CREATE INDEX child_idx ON other.child (a,b);",
			1,
		),
		("", 1),
		(INDEX, 0),
	] {
		revision += 1;
		let result = check_sql_statements(
			&mut daemon,
			revision,
			&[TABLE.into(), FK.into(), index.into()],
		);
		assert_eq!(
			result.summary.total_violations, expected,
			"index {index}: {result:?}"
		);
		assert!(
			result
				.rule_reports
				.iter()
				.all(|r| r.inconclusive == Some(0)),
			"{result:?}"
		);
	}
}

#[test]
fn sql_index_workspace_rule_reports_uncertain_table_resolution() {
	let temp = tempfile::tempdir().unwrap();
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).unwrap();
	daemon
		.refresh_cancellable(WorkspaceCancellation::default())
		.unwrap();
	let result = check_sql_statements(
		&mut daemon,
		1,
		&[
			"ALTER TABLE missing.child ADD CONSTRAINT fk FOREIGN KEY(a) REFERENCES parent(id);"
				.into(),
			"CREATE INDEX i ON missing.child(a);".into(),
		],
	);
	assert_eq!(result.summary.total_violations, 0, "{result:?}");
	assert!(
		result
			.rule_reports
			.iter()
			.any(|r| r.inconclusive == Some(1)),
		"{result:?}"
	);
}

fn check_sql_statements(
	daemon: &mut WorkspaceDaemon,
	revision: usize,
	contents: &[String],
) -> code_moniker_query::RulesCheckResult {
	replace_source_set(
		daemon,
		WorkspaceSourceSetDto {
			srcset: "catalog".into(),
			revision: Some(revision.to_string()),
			documents: contents
				.iter()
				.enumerate()
				.filter(|(_, content)| !content.is_empty())
				.map(|(i, content)| WorkspaceSourceDocumentDto {
					uri: format!("postgres://db/statement-{i}.sql"),
					language: "sql".into(),
					content: content.clone(),
				})
				.collect(),
		},
	);
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::RulesCheck(RulesCheckQuery {
			inline_rules: vec![include_str!("../../../../samples/sql-indexes/rules.toml").into()],
			report: true,
			..RulesCheckQuery::default()
		}),
	))));
	let ProtocolResponse::Query(response) = response else {
		panic!("{response:?}")
	};
	let QueryResult::RulesCheck(result) = response.result else {
		panic!("{:?}", response.result)
	};
	assert!(result.errors.is_empty(), "{result:?}");
	result
}

#[test]
fn sql_workspace_children_follow_semantic_ownership_in_every_document_layout() {
	let temp = tempfile::tempdir().unwrap();
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).unwrap();
	daemon
		.refresh_cancellable(WorkspaceCancellation::default())
		.unwrap();
	let statements = [
		"CREATE TABLE app.child(a int,b int);",
		"ALTER TABLE app.child ADD CONSTRAINT fk FOREIGN KEY(a,b) REFERENCES app.parent(a,b);",
		"CREATE INDEX i ON app.child(a,b);",
		"ALTER TABLE app.child ADD CONSTRAINT positive CHECK(a > 0);",
	];
	for (revision, order) in [[0, 1, 2, 3], [3, 2, 1, 0], [1, 2, 0, 3]]
		.iter()
		.enumerate()
	{
		for grouped in [true, false] {
			let content: Vec<_> = order.iter().map(|i| statements[*i].to_string()).collect();
			let documents = if grouped {
				vec![content.join("\n")]
			} else {
				content
			};
			check_sql_statements(&mut daemon, revision * 2 + usize::from(grouped), &documents);
			let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(
				QueryRequest::new(Query::RulesCheck(RulesCheckQuery {
					inline_rules: vec![
						r#"default_rules = false
[[workspace.symbol.where]]
id = "semantic-children"
expr = "kind = 'table' AND name = 'child' => count(constraint) = 2 AND count(index) = 1"
"#
						.into(),
					],
					report: true,
					..RulesCheckQuery::default()
				})),
			)));
			let ProtocolResponse::Query(response) = response else {
				panic!("{response:?}")
			};
			let QueryResult::RulesCheck(result) = response.result else {
				panic!("{:?}", response.result)
			};
			assert!(result.errors.is_empty(), "{result:?}");
			assert_eq!(
				result.summary.total_violations, 0,
				"grouped={grouped}, order={order:?}: {result:?}"
			);
			assert!(
				result
					.rule_reports
					.iter()
					.any(|r| r.antecedent_matches == Some(1)),
				"{result:?}"
			);
			assert!(
				result
					.rule_reports
					.iter()
					.all(|r| r.inconclusive == Some(0)),
				"{result:?}"
			);
		}
	}
}

#[test]
fn sql_workspace_index_prefix_preserves_long_and_quoted_keys_across_statements() {
	let temp = tempfile::tempdir().unwrap();
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).unwrap();
	daemon
		.refresh_cancellable(WorkspaceCancellation::default())
		.unwrap();
	let names: Vec<_> = (0..12).map(|i| format!("c{i}")).collect();
	let columns = names
		.iter()
		.map(|n| format!("{n} int"))
		.collect::<Vec<_>>()
		.join(",");
	let ordered = names.join(",");
	let mut reversed = names.clone();
	reversed.swap(10, 11);
	for (revision, keys, expected) in [(1, ordered.clone(), 0), (2, reversed.join(","), 1)] {
		let result = check_sql_statements(
			&mut daemon,
			revision,
			&[
				format!("CREATE TABLE app.child({columns});"),
				format!(
					"ALTER TABLE app.child ADD CONSTRAINT fk FOREIGN KEY({ordered}) REFERENCES parent({ordered});"
				),
				format!("CREATE INDEX i ON app.child({keys});"),
			],
		);
		assert_eq!(result.summary.total_violations, expected, "{result:?}");
	}
	for (revision, key, expected) in [(3, "\"A\"", 0), (4, "a", 1)] {
		let result=check_sql_statements(&mut daemon,revision,&[
			"CREATE TABLE app.\"Child\" (\"A\" int);".into(),
			"ALTER TABLE app.\"Child\" ADD CONSTRAINT fk FOREIGN KEY(\"A\") REFERENCES parent(id);".into(),
			format!("CREATE INDEX i ON app.\"Child\"({key});"),
		]);
		assert_eq!(result.summary.total_violations, expected, "{result:?}");
	}
}

#[test]
fn sql_workspace_rule_does_not_choose_an_ambiguous_table_owner() {
	let temp = tempfile::tempdir().unwrap();
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).unwrap();
	daemon
		.refresh_cancellable(WorkspaceCancellation::default())
		.unwrap();
	let result = check_sql_statements(
		&mut daemon,
		1,
		&[
			"CREATE TABLE app.child(a int);".into(),
			"CREATE TABLE app.child(a int);".into(),
			"ALTER TABLE app.child ADD CONSTRAINT fk FOREIGN KEY(a) REFERENCES parent(id);".into(),
			"CREATE INDEX i ON app.child(a);".into(),
		],
	);
	assert_eq!(result.summary.total_violations, 0, "{result:?}");
	assert!(
		result
			.rule_reports
			.iter()
			.any(|r| r.inconclusive == Some(1)),
		"{result:?}"
	);
}
