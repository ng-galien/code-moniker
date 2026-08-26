use super::*;

#[cfg(windows)]
use crate::runtime::WindowsSupervisorProcess;

#[cfg(windows)]
#[test]
fn windows_supervisor_handle_observes_the_original_process_exit() {
	let mut child = std::process::Command::new(std::env::current_exe().expect("test binary"))
		.args([
			"--exact",
			"tests::runtime::windows_supervisor_handle_child",
			"--ignored",
		])
		.spawn()
		.expect("spawn supervisor child");
	let supervisor = WindowsSupervisorProcess::open(child.id()).expect("open supervisor handle");
	assert!(supervisor.is_running());
	assert!(child.wait().expect("wait for supervisor child").success());
	assert!(!supervisor.is_running());
}

#[cfg(windows)]
#[test]
#[ignore = "subprocess fixture"]
fn windows_supervisor_handle_child() {
	std::thread::sleep(std::time::Duration::from_millis(250));
}

#[tokio::test(flavor = "multi_thread")]
async fn initial_preload_publishes_ready_before_live_watcher_can_block() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(temp.path().join("lib.rs"), "pub struct Indexed;\n").expect("write fixture");
	let daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let daemon = Arc::new(Mutex::new(daemon));
	let daemon_probe = daemon.clone();
	let published = Arc::new(RwLock::new(None));
	let lifecycle = Arc::new(RwLock::new(WorkspaceLifecycle::loading()));
	let (events, _) = tokio::sync::broadcast::channel(8);
	let (watcher_entered_tx, watcher_entered_rx) = std::sync::mpsc::channel();
	let (release_watcher_tx, release_watcher_rx) = std::sync::mpsc::channel();
	let (probe_completed_tx, probe_completed_rx) = std::sync::mpsc::channel();
	let readiness_published = Arc::new(tokio::sync::Notify::new());
	let dependency_probe = spawn_runtime_dependency_probe_with(
		readiness_published.clone(),
		vec![temp.path().to_path_buf()],
		move |_| {
			probe_completed_tx
				.send(())
				.expect("announce dependency probe")
		},
	);

	let (_, worker) = spawn_initial_preload_with_watcher(
		daemon,
		published.clone(),
		lifecycle.clone(),
		events,
		readiness_published,
		move |registration| {
			watcher_entered_tx.send(()).expect("announce watcher start");
			release_watcher_rx.recv().expect("release watcher start");
			registration.start().map(Some)
		},
	);
	watcher_entered_rx
		.recv_timeout(std::time::Duration::from_secs(10))
		.expect("initial index reaches watcher startup");
	let phase_while_watcher_is_blocked = lifecycle
		.read()
		.unwrap_or_else(|error| error.into_inner())
		.phase;
	let snapshot_published_while_watcher_is_blocked = published
		.read()
		.unwrap_or_else(|error| error.into_inner())
		.is_some();
	let daemon_available_while_watcher_is_blocked = daemon_probe.try_lock().is_ok();
	probe_completed_rx
		.recv_timeout(std::time::Duration::from_secs(2))
		.expect("dependency probe runs after readiness without waiting for the watcher");
	release_watcher_tx.send(()).expect("release watcher");
	worker
		.await
		.expect("preload worker joins")
		.expect("preload succeeds");
	dependency_probe.await.expect("dependency probe joins");

	assert_eq!(phase_while_watcher_is_blocked, WorkspacePhase::Ready);
	assert!(snapshot_published_while_watcher_is_blocked);
	assert!(daemon_available_while_watcher_is_blocked);
	let daemon = daemon_probe
		.lock()
		.unwrap_or_else(|error| error.into_inner());
	assert_eq!(
		daemon
			.registry
			.queries()
			.snapshot()
			.unwrap()
			.generation
			.value(),
		1,
		"watcher arming must not invalidate cursors without a source refresh"
	);
	assert!(
		daemon.registry.queries().staleness().is_stale(),
		"watcher arming must expose its pre-registration observation gap"
	);
}

#[tokio::test(flavor = "multi_thread")]
async fn initial_watcher_failure_is_exposed_by_rpc_workspace_status() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(temp.path().join("lib.rs"), "pub struct Indexed;\n").expect("write fixture");
	let roots = vec![temp.path().to_path_buf()];
	let daemon = Arc::new(Mutex::new(
		WorkspaceDaemon::new(roots.clone()).expect("daemon"),
	));
	let published = Arc::new(RwLock::new(None));
	let lifecycle = Arc::new(RwLock::new(WorkspaceLifecycle::loading()));
	let (events, _) = tokio::sync::broadcast::channel(8);
	let (_, worker) = spawn_initial_preload_with_watcher(
		daemon.clone(),
		published.clone(),
		lifecycle.clone(),
		events.clone(),
		Arc::new(tokio::sync::Notify::new()),
		move |_| anyhow::bail!("watch registration failed"),
	);
	worker
		.await
		.expect("preload worker joins")
		.expect_err("watcher failure must fail preload completion");

	let service = DaemonRpcService {
		daemon,
		published,
		lifecycle,
		roots: Arc::from(roots),
		events,
		shutdown: Arc::new(tokio::sync::Notify::new()),
		handshake: HandshakeResponse {
			protocol_version: code_moniker_query::PROTOCOL_VERSION,
			daemon_version: "test".to_string(),
			build: producer_identity(),
			workspace_root: "test".to_string(),
			workspace_roots: Vec::new(),
			capabilities: CapabilitySet::default(),
		},
	};
	let response = service
		.dispatch(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::WorkspaceStatus,
		))))
		.await
		.expect("RPC dispatch");
	let ProtocolResponse::Query(response) = response else {
		panic!("expected workspace status, got {response:?}");
	};
	let QueryResult::WorkspaceStatus(status) = response.result else {
		panic!("expected workspace status, got {:?}", response.result);
	};
	assert_eq!(status.phase, WorkspacePhase::Failed);
	assert_eq!(status.stale_summary, "watch registration failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn stale_initial_watcher_success_remains_fallback_coverage() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(temp.path().join("lib.rs"), "pub struct Indexed;\n").expect("fixture");
	let daemon = Arc::new(Mutex::new(
		WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon"),
	));
	let controller = daemon.clone();
	let published = Arc::new(RwLock::new(None));
	let lifecycle = Arc::new(RwLock::new(WorkspaceLifecycle::loading()));
	let (events, _) = tokio::sync::broadcast::channel(8);
	let (_, worker) = spawn_initial_preload_with_watcher(
		daemon.clone(),
		published,
		lifecycle.clone(),
		events,
		Arc::new(tokio::sync::Notify::new()),
		move |registration| {
			controller
				.lock()
				.unwrap_or_else(|error| error.into_inner())
				.restart_live_watcher()?;
			registration.start().map(Some)
		},
	);
	worker
		.await
		.expect("worker joins")
		.expect("preload succeeds");

	let daemon = daemon.lock().unwrap_or_else(|error| error.into_inner());
	assert!(daemon.live.watcher.is_some());
	assert!(daemon.registry.queries().staleness().is_stale());
	assert_eq!(
		lifecycle
			.read()
			.unwrap_or_else(|error| error.into_inner())
			.phase,
		WorkspacePhase::Ready
	);
}

#[tokio::test(flavor = "multi_thread")]
async fn stale_initial_watcher_failure_cannot_override_a_replacement() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(temp.path().join("lib.rs"), "pub struct Indexed;\n").expect("fixture");
	let daemon = Arc::new(Mutex::new(
		WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon"),
	));
	let controller = daemon.clone();
	let published = Arc::new(RwLock::new(None));
	let lifecycle = Arc::new(RwLock::new(WorkspaceLifecycle::loading()));
	let (events, _) = tokio::sync::broadcast::channel(8);
	let (_, worker) = spawn_initial_preload_with_watcher(
		daemon.clone(),
		published,
		lifecycle.clone(),
		events,
		Arc::new(tokio::sync::Notify::new()),
		move |_| {
			controller
				.lock()
				.unwrap_or_else(|error| error.into_inner())
				.restart_live_watcher()?;
			anyhow::bail!("obsolete initial watcher failed")
		},
	);
	worker
		.await
		.expect("worker joins")
		.expect("obsolete failure is ignored");

	let daemon = daemon.lock().unwrap_or_else(|error| error.into_inner());
	assert!(daemon.registry.queries().staleness().is_stale());
	assert_eq!(
		lifecycle
			.read()
			.unwrap_or_else(|error| error.into_inner())
			.phase,
		WorkspacePhase::Ready
	);
}

#[test]
fn daemon_token_is_128_bits_encoded_as_hex() {
	let token = generate_token().expect("generate daemon token");
	assert_eq!(token.len(), 32);
	assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
fn daemon_answers_status_and_symbol_search() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(
		temp.path().join("lib.rs"),
		"pub struct Customer;\nimpl Customer { pub fn id(&self) -> u64 { 42 } }\n",
	)
	.expect("write fixture");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let status = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::WorkspaceStatus,
	))));
	match status {
		ProtocolResponse::Query(response) => {
			assert!(matches!(response.result, QueryResult::WorkspaceStatus(_)));
		}
		other => panic!("unexpected response: {other:?}"),
	}
	let refresh = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	assert!(
		matches!(refresh, ProtocolResponse::Command(_)),
		"unexpected response: {refresh:?}"
	);
	let search = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::SymbolSearch(code_moniker_query::SymbolSearchQuery {
			text: Some("Customer".to_string()),
			..Default::default()
		}),
	))));
	match search {
		ProtocolResponse::Query(response) => match response.result {
			QueryResult::SymbolList(list) => {
				assert!(list.rows.iter().any(|row| row.name == "Customer"));
			}
			other => panic!("unexpected result: {other:?}"),
		},
		other => panic!("unexpected response: {other:?}"),
	}
}
