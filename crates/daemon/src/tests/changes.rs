use super::*;

#[test]
fn virtual_diff_impact_is_transactional_and_does_not_mutate_workspace_state() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(temp.path().join("lib.rs"), "pub fn workspace_symbol() {}\n")
		.expect("write workspace fixture");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	daemon
		.refresh_cancellable(WorkspaceCancellation::default())
		.expect("initial refresh");
	let before = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::WorkspaceStatus,
	))));

	let source_set = |revision: &str, source: &str| WorkspaceSourceSetDto {
		srcset: "diff-impact".to_string(),
		revision: Some(revision.to_string()),
		documents: vec![WorkspaceSourceDocumentDto {
			uri: "src/lib.rs".to_string(),
			language: "rs".to_string(),
			content: source.to_string(),
		}],
	};
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::DiffImpactCompare(code_moniker_query::DiffImpactCompareQuery {
			scope: "base..head".to_string(),
			project: Some("sample".to_string()),
			base: source_set("base", "pub fn changed() { old(); }\n"),
			head: source_set("head", "pub fn changed() { new(); }\n"),
			files: vec![code_moniker_query::DiffImpactCompareFile {
				status: DiffImpactFileStatus::Modified,
				old_uri: Some("src/lib.rs".to_string()),
				new_uri: Some("src/lib.rs".to_string()),
				old_hunks: vec![code_moniker_query::DiffImpactLineSpan { start: 1, end: 1 }],
				new_hunks: vec![code_moniker_query::DiffImpactLineSpan { start: 1, end: 1 }],
				rename_score: None,
			}],
		}),
	))));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected virtual diff-impact response, got {response:?}");
	};
	assert_eq!(response.generation, None);
	let QueryResult::DiffImpact(impact) = response.result else {
		panic!("expected diff-impact result, got {:?}", response.result);
	};
	assert_eq!(impact.scope, "base..head");
	assert_eq!(impact.summary.files, 1);
	assert_eq!(impact.summary.symbol_changes, 1);

	let after = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::WorkspaceStatus,
	))));
	assert_eq!(
		before, after,
		"virtual comparison must leave no workspace state behind"
	);
}

#[test]
fn virtual_diff_impact_rejects_invalid_line_spans() {
	for span in [
		code_moniker_query::DiffImpactLineSpan { start: 0, end: 1 },
		code_moniker_query::DiffImpactLineSpan { start: 5, end: 3 },
	] {
		let error = validate_diff_impact_file(&code_moniker_query::DiffImpactCompareFile {
			status: DiffImpactFileStatus::Modified,
			old_uri: Some("src/lib.rs".to_string()),
			new_uri: Some("src/lib.rs".to_string()),
			old_hunks: vec![span],
			new_hunks: Vec::new(),
			rename_score: None,
		})
		.expect_err("invalid spans must fail closed");
		assert_eq!(error.code, "invalid_diff_impact_span");
	}
}

#[test]
fn change_review_query_builds_semantic_facts_on_demand() {
	let temp = tempfile::tempdir().expect("tempdir");
	let git = |args: &[&str]| {
		let output = std::process::Command::new("git")
			.arg("-C")
			.arg(temp.path())
			.args(args)
			.output()
			.expect("run git");
		assert!(
			output.status.success(),
			"git {args:?}: {}",
			String::from_utf8_lossy(&output.stderr)
		);
	};
	git(&["init"]);
	git(&["config", "user.email", "cm@example.test"]);
	git(&["config", "user.name", "Code Moniker"]);
	let src = temp.path().join("src");
	fs::create_dir_all(&src).expect("src dir");
	fs::write(
		src.join("util.rs"),
		"pub fn assist() { work(); }\npub fn sidekick() { rest(); }\n",
	)
	.expect("write util");
	git(&["add", "."]);
	git(&["commit", "-m", "initial"]);
	git(&["mv", "src/util.rs", "src/support.rs"]);
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
	let before_generation = daemon
		.registry
		.queries()
		.snapshot()
		.expect("initial snapshot")
		.generation
		.value();
	daemon
		.live
		.tx
		.send(WorkspaceLiveEvent::RescanRequired)
		.expect("queue stale source event");

	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
		query: Query::ChangeReview(code_moniker_query::ChangeReviewQuery { workspace: None }),
		consistency: code_moniker_query::Consistency::StaleOk,
		page: Page::default(),
	})));
	let after_generation = daemon
		.registry
		.queries()
		.snapshot()
		.expect("refreshed snapshot")
		.generation
		.value();
	assert_eq!(after_generation, before_generation + 1);
	assert!(!daemon.registry.queries().staleness().is_stale());

	let ProtocolResponse::Query(query) = response else {
		panic!("expected query response");
	};
	let QueryResult::ChangeReview(result) = query.result else {
		panic!("expected change review result, got {:?}", query.result);
	};
	assert_eq!(result.scope, "HEAD..worktree");
	assert!(
		result
			.files
			.iter()
			.any(|file| file.disposition == "moved" && file.coverage_explained),
		"{result:?}"
	);
	assert!(
		result
			.symbol_changes
			.iter()
			.all(|change| change.kind == "moved" && change.file_moved),
		"{result:?}"
	);

	let tree = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
		query: Query::TreeChildren(code_moniker_query::TreeChildrenQuery {
			workspace: None,
			path: Vec::new(),
			depth: 1,
			lang: Vec::new(),
			projection: Vec::new(),
		}),
		consistency: code_moniker_query::Consistency::Current,
		page: Page::default(),
	})));
	let ProtocolResponse::Query(tree) = tree else {
		panic!("expected tree response");
	};
	let QueryResult::TreeChildren(tree) = tree.result else {
		panic!("expected tree children, got {:?}", tree.result);
	};
	assert!(
		tree.rows.iter().any(|row| row.change_count > 0),
		"tree rows must carry the change count: {:?}",
		tree.rows
	);
}
