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
