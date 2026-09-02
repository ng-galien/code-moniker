use super::*;

#[test]
fn auto_policy_applies_live_edits_before_plain_queries() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src = temp.path().join("src");
	fs::create_dir_all(&src).expect("src dir");
	let lib = src.join("lib.rs");
	fs::write(&lib, "pub fn before_auto_edit() {}\n").expect("write lib");
	let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
		roots: vec![temp.path().display().to_string()],
		project: None,
		cache_dir: None,
		live_refresh: Some("auto".to_string()),
	})
	.expect("daemon");
	let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	assert!(matches!(refreshed, ProtocolResponse::Command(_)));

	fs::write(&lib, "pub fn after_auto_edit() {}\n").expect("rewrite lib");
	daemon
		.live
		.tx
		.send(WorkspaceLiveEvent::SourcesChanged(vec![lib.clone()]))
		.expect("send live event");

	match search_symbols(&mut daemon, "after_auto_edit") {
		QueryResult::SymbolList(symbols) => {
			assert_eq!(
				symbols.rows.len(),
				1,
				"auto policy should apply the edit before a plain query"
			);
		}
		other => panic!("expected symbols result, got {other:?}"),
	}

	fs::write(
		src.join("fresh_auto.rs"),
		"pub fn fresh_auto_created() {}\n",
	)
	.expect("create file");
	daemon
		.live
		.tx
		.send(WorkspaceLiveEvent::SourcesChanged(vec![
			src.join("fresh_auto.rs"),
		]))
		.expect("send create event");

	match search_symbols(&mut daemon, "fresh_auto_created") {
		QueryResult::SymbolList(symbols) => {
			assert_eq!(
				symbols.rows.len(),
				1,
				"auto policy should index created files before a plain query"
			);
		}
		other => panic!("expected symbols result, got {other:?}"),
	}
}

#[test]
fn auto_policy_coalesces_a_burst_into_one_workspace_generation() {
	let temp = tempfile::tempdir().expect("tempdir");
	let source = temp.path().join("lib.rs");
	fs::write(&source, "pub fn before() {}\n").expect("write source");
	let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
		roots: vec![temp.path().display().to_string()],
		project: None,
		cache_dir: None,
		live_refresh: Some("auto".to_string()),
	})
	.expect("daemon");
	let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	assert!(matches!(refreshed, ProtocolResponse::Command(_)));
	let before = daemon
		.registry
		.queries()
		.snapshot()
		.expect("initial snapshot")
		.generation
		.value();

	fs::write(&source, "pub fn after() {}\n").expect("rewrite source");
	for _ in 0..2 {
		daemon
			.live
			.tx
			.send(WorkspaceLiveEvent::SourcesChanged(vec![source.clone()]))
			.expect("queue duplicate live event");
	}

	match search_symbols(&mut daemon, "after") {
		QueryResult::SymbolList(symbols) => assert_eq!(symbols.rows.len(), 1),
		other => panic!("expected symbols result, got {other:?}"),
	}
	let after = daemon
		.registry
		.queries()
		.snapshot()
		.expect("refreshed snapshot")
		.generation
		.value();
	assert_eq!(after, before + 1, "one drained burst publishes once");
}

#[test]
fn query_error_carries_structured_code_in_data() {
	let error = query_error(QueryError::new("workspace_loading", "still loading"));
	assert_eq!(error.message(), "still loading");
	let data = error.data().expect("error should carry structured data");
	let value: serde_json::Value = serde_json::from_str(data.get()).unwrap();
	assert_eq!(value["code"], "workspace_loading");
	assert_eq!(value["message"], "still loading");
}

#[test]
fn initial_refresh_failure_is_a_typed_observable_workspace_state() {
	let temp = tempfile::tempdir().expect("tempdir");
	let workspace = temp.path().join("workspace");
	let unavailable = temp.path().join("workspace-unavailable");
	fs::create_dir_all(&workspace).expect("workspace");
	let mut daemon = WorkspaceDaemon::new(vec![workspace.clone()]).expect("daemon");
	fs::rename(&workspace, &unavailable).expect("make workspace unavailable");

	daemon
		.refresh_cancellable(WorkspaceCancellation::default())
		.expect_err("initial refresh must fail");
	let status = workspace_status_result(&daemon.roots, &daemon.registry);

	assert_eq!(status.phase, WorkspacePhase::Failed);
	let failure = status.failure.expect("typed failure");
	assert_eq!(failure.resource.as_deref(), Some("source_catalog"));
	assert!(!failure.message.is_empty());
}

#[test]
fn failed_initial_index_rejects_data_queries_without_a_restart_loop() {
	let lifecycle = RwLock::new(WorkspaceLifecycle::failed("broken corpus"));
	let response = workspace_unavailable_response(
		ProtocolRequest::Query(Box::new(QueryRequest::new(Query::SymbolSearch(
			SymbolSearchQuery::default(),
		)))),
		&lifecycle,
	);

	let ProtocolResponse::Error(error) = response else {
		panic!("expected typed workspace failure")
	};
	assert_eq!(error.code, "workspace_load_failed");
	assert_eq!(error.message, "broken corpus");
}

#[test]
fn failed_workspace_status_without_snapshot_carries_the_failure_summary() {
	let response = workspace_status_without_snapshot(
		&[PathBuf::from("/workspace")],
		WorkspaceLifecycle::failed("broken corpus"),
	);
	let QueryResult::WorkspaceStatus(status) = response.result else {
		panic!("expected workspace status")
	};

	assert_eq!(status.phase, WorkspacePhase::Failed);
	assert_eq!(status.stale_summary, "broken corpus");
	assert_eq!(status.roots[0].stale_summary, "broken corpus");
}

#[test]
fn live_watcher_failure_remains_a_typed_failed_status() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(temp.path().join("lib.rs"), "pub fn watched() {}\n").expect("fixture");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	assert!(matches!(refreshed, ProtocolResponse::Command(_)));
	daemon.inject_live_watcher_failure("watch registration failed");

	for _ in 0..2 {
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::WorkspaceStatus,
		))));
		let ProtocolResponse::Query(response) = response else {
			panic!("expected status response, got {response:?}");
		};
		let QueryResult::WorkspaceStatus(status) = response.result else {
			panic!("expected workspace status, got {:?}", response.result);
		};
		assert_eq!(status.phase, WorkspacePhase::Failed);
		assert_eq!(
			status
				.failure
				.as_ref()
				.and_then(|failure| failure.resource.as_deref()),
			Some("live_watcher")
		);
		assert_eq!(status.stale_summary, "watch registration failed");
	}
}

#[test]
fn terminal_runtime_watcher_failure_remains_failed_on_later_requests() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(temp.path().join("lib.rs"), "pub fn watched() {}\n").expect("fixture");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	assert!(matches!(refreshed, ProtocolResponse::Command(_)));

	let error = daemon.stop_failed_live_watcher();
	assert!(error.to_string().contains("automatic restart stopped"));

	for _ in 0..2 {
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::WorkspaceStatus,
		))));
		let ProtocolResponse::Query(response) = response else {
			panic!("expected status response, got {response:?}");
		};
		let QueryResult::WorkspaceStatus(status) = response.result else {
			panic!("expected workspace status, got {:?}", response.result);
		};
		assert_eq!(status.phase, WorkspacePhase::Failed);
		assert_eq!(
			status
				.failure
				.as_ref()
				.and_then(|failure| failure.resource.as_deref()),
			Some("live_watcher")
		);
		assert!(status.stale_summary.contains("automatic restart stopped"));
	}
}

#[test]
fn automatic_polling_fallback_is_retained_for_later_registrations() {
	let temp = tempfile::tempdir().expect("tempdir");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");

	daemon.remember_live_watcher_backend_state(false, true);
	assert!(!daemon.live.force_polling);
	daemon.remember_live_watcher_backend_state(true, false);
	assert!(!daemon.live.force_polling);
	daemon.remember_live_watcher_backend_state(true, true);
	assert!(daemon.live.force_polling);
	daemon.remember_live_watcher_backend_state(false, false);
	assert!(
		daemon.live.force_polling,
		"a later topology rebuild must not retry the failed native backend"
	);
}

#[test]
fn watcher_activation_reconciles_a_mutation_from_the_registration_gap() {
	let temp = tempfile::tempdir().expect("tempdir");
	let source = temp.path().join("lib.rs");
	fs::write(&source, "pub fn before_registration() {}\n").expect("fixture");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	crate::lifecycle::refresh_full_cancellable(&mut daemon, WorkspaceCancellation::default())
		.expect("initial refresh");
	let registration = daemon.live_watcher_registration();

	fs::write(&source, "pub fn mutation_inside_registration_gap() {}\n")
		.expect("mutate before watcher activation");
	let watcher = registration.start().expect("arm watcher after mutation");
	daemon.queue_live_watcher_update_for_test(watcher);
	crate::lifecycle::drain_live_events(&mut daemon, true).expect("install watcher update");
	assert!(
		daemon.registry.queries().staleness().is_stale(),
		"watcher activation must require reconciliation"
	);

	crate::lifecycle::refresh_stale(&mut daemon).expect("reconcile registration gap");
	match search_symbols(&mut daemon, "mutation_inside_registration_gap") {
		QueryResult::SymbolList(symbols) => assert_eq!(symbols.rows.len(), 1),
		other => panic!("expected symbols result, got {other:?}"),
	}
}
