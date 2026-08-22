use super::*;

#[test]
fn rules_check_evaluates_the_selected_daemon_generation() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src = temp.path().join("src");
	fs::create_dir_all(&src).expect("src dir");
	let lib = src.join("lib.rs");
	fs::write(&lib, "pub fn indexed_name() {}\n").expect("write indexed source");
	let rules = temp.path().join("scratch-rules.toml");
	fs::write(
		&rules,
		r#"
default_rules = false

[[rust.fn.where]]
id = "indexed-name-is-visible"
expr = "name != 'indexed_name'"
message = "the rule must observe the indexed generation"
"#,
	)
	.expect("write rules");
	let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
		roots: vec![temp.path().display().to_string()],
		project: None,
		cache_dir: None,
		live_refresh: Some("on-demand".to_string()),
	})
	.expect("daemon");
	let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	assert!(matches!(refreshed, ProtocolResponse::Command(_)));

	fs::write(&lib, "pub fn filesystem_name() {}\n").expect("change filesystem source");
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
		query: Query::RulesCheck(RulesCheckQuery {
			workspace: None,
			profile: None,
			rules: Some(rules.display().to_string()),
			file: Vec::new(),
			report: true,
		}),
		consistency: code_moniker_query::Consistency::StaleOk,
		page: Page::default(),
	})));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected rules check response, got {response:?}");
	};
	assert_eq!(response.generation, Some(WorkspaceGeneration(1)));
	let QueryResult::RulesCheck(result) = response.result else {
		panic!("expected rules check result, got {:?}", response.result);
	};
	assert_eq!(result.summary.files_scanned, 1, "{result:?}");
	assert_eq!(result.summary.total_violations, 1, "{result:?}");
	assert_eq!(
		result.violations[0].rule_id,
		"rust.fn.indexed-name-is-visible"
	);

	fs::write(
		&rules,
		r#"
default_rules = false

[[rust.fn.where]]
id = "changed-rules-are-reloaded"
expr = "name == 'filesystem_name'"
message = "the current rules file must run against the pinned index"
"#,
	)
	.expect("change rules");
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
		query: Query::RulesCheck(RulesCheckQuery {
			workspace: None,
			profile: None,
			rules: Some(rules.display().to_string()),
			file: Vec::new(),
			report: true,
		}),
		consistency: code_moniker_query::Consistency::StaleOk,
		page: Page::default(),
	})));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected rules check response, got {response:?}");
	};
	assert_eq!(response.generation, Some(WorkspaceGeneration(1)));
	let QueryResult::RulesCheck(result) = response.result else {
		panic!("expected rules check result, got {:?}", response.result);
	};
	assert_eq!(result.summary.total_violations, 1, "{result:?}");
	assert_eq!(
		result.violations[0].rule_id,
		"rust.fn.changed-rules-are-reloaded"
	);
}

#[test]
fn rules_check_scopes_nested_roots_by_source_identity() {
	let temp = tempfile::tempdir().expect("tempdir");
	let child = temp.path().join("apps/child");
	fs::create_dir_all(&child).expect("child root");
	let parent = temp.path().canonicalize().expect("canonical parent");
	let child = child.canonicalize().expect("canonical child");
	fs::write(parent.join("parent.rs"), "pub fn parent_fn() {}\n").expect("parent source");
	let child_file = child.join("child.rs");
	fs::write(&child_file, "pub fn child_fn() {}\n").expect("child source");
	let rules = parent.join("scratch-rules.toml");
	fs::write(
		&rules,
		r#"
default_rules = false

[[rust.fn.where]]
id = "count-selected-source-root"
expr = "name == 'never'"
message = "every selected function is observable"
"#,
	)
	.expect("rules");
	let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
		roots: vec![parent.display().to_string(), child.display().to_string()],
		project: None,
		cache_dir: None,
		live_refresh: Some("on-demand".to_string()),
	})
	.expect("daemon");
	let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	assert!(matches!(refreshed, ProtocolResponse::Command(_)));

	let parent_result = rules_check_result(&mut daemon, &rules, &parent, Vec::new());
	assert_eq!(parent_result.summary.files_scanned, 2, "{parent_result:?}");
	assert_eq!(
		parent_result.summary.total_violations, 2,
		"{parent_result:?}"
	);
	let parent_child_moniker = parent_result
		.violations
		.iter()
		.find(|violation| violation.path == child_file.display().to_string())
		.expect("child file through parent source root")
		.moniker
		.clone();

	let nested = rules_check_result(&mut daemon, &rules, &child, Vec::new());
	assert_eq!(nested.summary.files_scanned, 1, "{nested:?}");
	assert_eq!(nested.summary.total_violations, 1, "{nested:?}");
	let nested_moniker = nested.violations[0].moniker.clone();
	assert_ne!(
		parent_child_moniker, nested_moniker,
		"the same physical file must keep the selected source root anchor"
	);

	for file in [
		vec!["apps/child/child.rs".to_string()],
		vec![child_file.display().to_string()],
	] {
		let filtered = rules_check_result(&mut daemon, &rules, &parent, file);
		assert_eq!(filtered.summary.files_scanned, 1, "{filtered:?}");
		assert_eq!(filtered.violations[0].moniker, parent_child_moniker);
	}

	for file in [
		vec!["child.rs".to_string()],
		vec![child_file.display().to_string()],
	] {
		let filtered = rules_check_result(&mut daemon, &rules, &child, file);
		assert_eq!(filtered.summary.files_scanned, 1, "{filtered:?}");
		assert_eq!(filtered.summary.total_violations, 1, "{filtered:?}");
		assert_eq!(filtered.violations[0].moniker, nested_moniker);
	}
}

fn rules_check_result(
	daemon: &mut WorkspaceDaemon,
	rules: &Path,
	workspace: &Path,
	file: Vec<String>,
) -> code_moniker_query::RulesCheckResult {
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
		query: Query::RulesCheck(RulesCheckQuery {
			workspace: Some(workspace.display().to_string()),
			profile: None,
			rules: Some(rules.display().to_string()),
			file,
			report: true,
		}),
		consistency: code_moniker_query::Consistency::StaleOk,
		page: Page::default(),
	})));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected rules check response, got {response:?}");
	};
	let QueryResult::RulesCheck(result) = response.result else {
		panic!("expected rules check result, got {:?}", response.result);
	};
	result
}

#[test]
fn applicable_rules_and_change_context_are_symbol_scoped_with_canonical_source_groups() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src = temp.path().join("src");
	fs::create_dir_all(&src).expect("src dir");
	fs::write(
		temp.path().join(".code-moniker.toml"),
		concat!(
			"default_rules = false\n\n",
			"[[workspace.source_group]]\n",
			"roots = [\"src\"]\n",
			"\n",
			"[[rust.fn.where]]\n",
			"id = \"function-snake-case\"\n",
			"expr = \"name =~ ^[a-z][a-z0-9_]*$\"\n",
			"severity = \"warn\"\n",
			"message = \"Function `{name}` should be snake_case.\"\n",
			"\n[[rust.shape.type.where]]\n",
			"id = \"type-rule\"\n",
			"expr = \"name =~ .\"\n",
			"message = \"Type rule.\"\n",
			"\n[[refs.where]]\n",
			"id = \"reference-rule\"\n",
			"expr = \"source ~ '**'\"\n",
			"message = \"Reference rule.\"\n",
		),
	)
	.expect("rules config");
	fs::write(
		src.join("lib.rs"),
		"pub fn entry() { helper(); }\nfn helper() {}\n",
	)
	.expect("source");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	assert!(matches!(refreshed, ProtocolResponse::Command(_)));
	let QueryResult::SymbolList(symbols) = search_symbols(&mut daemon, "entry") else {
		panic!("expected symbol search result");
	};
	let entry = symbols
		.rows
		.iter()
		.find(|symbol| symbol.name.starts_with("entry"))
		.expect("entry symbol");
	let entry_uri = entry.uri.clone();

	let applicable = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::RulesApplicable(code_moniker_query::RulesApplicableQuery {
			workspace: None,
			focus: entry_uri.clone(),
			profile: None,
			rules: None,
		}),
	))));
	let ProtocolResponse::Query(applicable) = applicable else {
		panic!("expected applicable rules response, got {applicable:?}");
	};
	let QueryResult::RulesApplicable(applicable) = applicable.result else {
		panic!("expected applicable rules, got {:?}", applicable.result);
	};
	assert_eq!(applicable.file, "src/lib.rs");
	assert_eq!(applicable.symbol_kind.as_deref(), Some("fn"));
	assert!(
		applicable
			.rows
			.iter()
			.any(|row| row.status == "applicable" && row.rule.id.contains("function-snake-case")),
		"{applicable:?}"
	);
	assert!(
		applicable
			.rows
			.iter()
			.any(|row| row.status == "ignored" && row.rule.id.contains("type-rule")),
		"{applicable:?}"
	);
	assert!(
		applicable
			.rows
			.iter()
			.any(|row| row.status == "potential" && row.rule.id.contains("reference-rule")),
		"{applicable:?}"
	);

	let context = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::ChangeContext(code_moniker_query::ChangeContextQuery {
			workspace: None,
			focus: entry_uri.clone(),
			profile: None,
			max_items: 1,
		}),
	))));
	let ProtocolResponse::Query(context) = context else {
		panic!("expected change context response");
	};
	let QueryResult::ChangeContext(context) = context.result else {
		panic!("expected change context, got {:?}", context.result);
	};
	assert!(
		matches!(&context.focus, code_moniker_query::SymbolGraphFocus::Symbol { symbol } if symbol.uri == entry_uri),
		"{context:?}"
	);
	assert_eq!(context.coverage.callees_emitted, 1);
	assert!(context.coverage.callees_total >= context.coverage.callees_emitted);
	assert_eq!(context.coverage.rules_emitted, 1);
	assert_eq!(context.suggested_checks.len(), 1);
	assert!(
		context.suggested_checks[0].starts_with("code_moniker_rules "),
		"{:?}",
		context.suggested_checks
	);
	assert!(!context.suggested_checks[0].contains("@m"));
}

#[test]
fn rules_config_root_searches_above_common_multi_root() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(temp.path().join(".code-moniker.toml"), "").expect("rules config");
	let first = temp.path().join("crates").join("first");
	let second = temp.path().join("crates").join("second");
	fs::create_dir_all(&first).expect("first");
	fs::create_dir_all(&second).expect("second");
	let roots = canonical_workspace_roots([&first, &second]).expect("roots");
	let common = temp
		.path()
		.join("crates")
		.canonicalize()
		.expect("canonical common");
	assert_eq!(common_workspace_root(&roots).expect("common root"), common);
	assert_eq!(
		rules_config_root(&roots).expect("rules config root"),
		temp.path().canonicalize().expect("canonical temp")
	);
}

#[test]
fn aggregate_check_summary_reconciles_unspecified_srcsets_across_roots() {
	let root = |root: &str, total_violations, violations_by_srcset| RulesCheckRootResult {
		root: root.to_string(),
		verdict: RulesCheckVerdict::Fail,
		exit: "no_match".to_string(),
		summary: CheckSummaryDto {
			total_violations,
			violations_by_srcset,
			..Default::default()
		},
		violations: Vec::new(),
		errors: Vec::new(),
		rule_reports: Vec::new(),
		skip_reason: None,
	};
	let summary = helpers::aggregate_check_summary(&[
		root("legacy", 2, BTreeMap::new()),
		root("indexed", 1, BTreeMap::from([("main".to_string(), 1)])),
	]);

	assert_eq!(summary.total_violations, 3);
	assert_eq!(
		summary.violations_by_srcset,
		BTreeMap::from([("main".to_string(), 1), ("unspecified".to_string(), 2)])
	);
}
