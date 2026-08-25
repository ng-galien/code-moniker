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
	let published = Arc::new(RwLock::new(None));
	let lifecycle = Arc::new(RwLock::new(WorkspaceLifecycle::loading()));
	let (events, _) = tokio::sync::broadcast::channel(8);
	let (watcher_entered_tx, watcher_entered_rx) = std::sync::mpsc::channel();
	let (release_watcher_tx, release_watcher_rx) = std::sync::mpsc::channel();

	let (_, worker) = spawn_initial_preload_with_watcher(
		daemon,
		published.clone(),
		lifecycle.clone(),
		events,
		move |_| {
			watcher_entered_tx.send(()).expect("announce watcher start");
			release_watcher_rx.recv().expect("release watcher start");
			Ok(())
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
	release_watcher_tx.send(()).expect("release watcher");
	worker
		.await
		.expect("preload worker joins")
		.expect("preload succeeds");

	assert_eq!(phase_while_watcher_is_blocked, WorkspacePhase::Ready);
	assert!(snapshot_published_while_watcher_is_blocked);
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
