#![cfg(feature = "mcp")]

use std::ffi::CString;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::{Duration, Instant};

use code_moniker_daemon_client::{config_from_roots, read_registry_entry};

static STDIO_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn stdio_handshake_and_eof_do_not_wait_for_a_blocked_preload() {
	let _test_guard = STDIO_TEST_LOCK.lock().expect("stdio test lock");
	let workspace = tempfile::tempdir().expect("workspace");
	let fifo = workspace.path().join("Blocked.rs");
	let fifo_name = CString::new(fifo.as_os_str().as_bytes()).expect("FIFO path");
	// SAFETY: fifo_name is a valid, NUL-terminated path owned for this call.
	let created = unsafe { libc::mkfifo(fifo_name.as_ptr(), 0o600) };
	assert_eq!(
		created,
		0,
		"create FIFO: {}",
		std::io::Error::last_os_error()
	);
	let root = fifo.canonicalize().expect("canonical FIFO");
	let mut child = spawn_stdio_read(&root);
	let stdout = child.stdout.take().expect("child stdout");
	let (response_tx, response_rx) = std::sync::mpsc::channel();
	let reader = thread::spawn(move || {
		for line in BufReader::new(stdout).lines() {
			if response_tx.send(line).is_err() {
				break;
			}
		}
	});

	let response = receive_response(&response_rx, 1, Duration::from_secs(2)).unwrap_or_else(|_| {
		let _ = child.kill();
		panic!("MCP initialize waited for the blocked workspace preload")
	});
	assert_eq!(response["id"].as_u64(), Some(1), "{response:#}");

	request_stdio_read(&mut child, &root, 2);
	let started = Instant::now();
	let response = receive_response(&response_rx, 2, Duration::from_secs(2)).unwrap_or_else(|_| {
		let _ = child.kill();
		panic!("MCP tool call waited for the blocked workspace preload")
	});
	assert!(
		started.elapsed() < Duration::from_secs(2),
		"workspace_loading response was not bounded"
	);
	assert_eq!(response["result"]["isError"].as_bool(), Some(true));
	let text = response["result"]["content"][0]["text"]
		.as_str()
		.expect("tool error text");
	assert!(text.contains("workspace_loading"), "{text}");

	drop(child.stdin.take());
	wait_for_exit(&mut child, Duration::from_secs(2));
	assert!(child.wait().expect("reap MCP").success());
	reader.join().expect("stdout reader");
	assert_no_daemon_registry(&root);
}

#[test]
fn stdio_failed_preload_exposes_one_failure_to_status_and_data_queries() {
	let _test_guard = STDIO_TEST_LOCK.lock().expect("stdio test lock");
	let workspace = tempfile::tempdir().expect("workspace");
	std::fs::create_dir_all(workspace.path().join("src/generated")).expect("source roots");
	std::fs::write(
		workspace.path().join(".code-moniker.toml"),
		r#"
[[workspace.source_group]]
roots = ["src"]

[[workspace.source_group]]
roots = ["src/generated"]
"#,
	)
	.expect("invalid source-group config");
	let root = workspace.path().canonicalize().expect("canonical root");
	let mut child = spawn_stdio_read(&root);
	let stdout = child.stdout.take().expect("child stdout");
	let (response_tx, response_rx) = std::sync::mpsc::channel();
	let reader = thread::spawn(move || {
		for line in BufReader::new(stdout).lines() {
			if response_tx.send(line).is_err() {
				break;
			}
		}
	});
	receive_response(&response_rx, 1, Duration::from_secs(2)).expect("MCP initialize");

	let deadline = Instant::now() + Duration::from_secs(5);
	let mut id = 2;
	let failure_text = loop {
		request_stdio_workspace_status(&mut child, id);
		let remaining = deadline
			.checked_duration_since(Instant::now())
			.unwrap_or_else(|| panic!("workspace.status did not expose the failed preload"));
		let response =
			receive_response(&response_rx, id, remaining).unwrap_or_else(|error| panic!("{error}"));
		let text = response["result"]["content"][0]["text"]
			.as_str()
			.expect("workspace.status text")
			.to_string();
		if text.contains("phase: failed") {
			break text;
		}
		assert!(text.contains("phase: loading"), "{text}");
		thread::sleep(Duration::from_millis(20));
		id += 1;
	};
	assert!(failure_text.contains("overlap"), "{failure_text}");

	id += 1;
	request_stdio_read(&mut child, &root, id);
	let response = receive_response(&response_rx, id, Duration::from_secs(2))
		.expect("failed data query response");
	let data_error = response["result"]["content"][0]["text"]
		.as_str()
		.expect("data error text");
	assert!(data_error.contains("workspace_load_failed"), "{data_error}");
	assert!(data_error.contains("overlap"), "{data_error}");

	drop(child.stdin.take());
	wait_for_exit(&mut child, Duration::from_secs(2));
	assert!(child.wait().expect("reap MCP").success());
	reader.join().expect("stdout reader");
	assert_no_daemon_registry(&root);
}

#[test]
fn stdio_transport_serves_the_bound_workspace_without_stdout_noise() {
	let _test_guard = STDIO_TEST_LOCK.lock().expect("stdio test lock");
	let workspace = tempfile::tempdir().expect("workspace");
	std::fs::write(workspace.path().join("App.java"), "class App {}\n").expect("fixture");
	let root = workspace
		.path()
		.canonicalize()
		.expect("canonical workspace");
	let mut child = spawn_stdio_read(&root);
	assert_no_daemon_registry_while_alive(&mut child, &root);
	let text = collect_stdio_read(child, &root);
	assert_no_daemon_registry(&root);
	assert!(text.contains("workspace:\n  roots:"), "{text}");
	assert!(text.contains(&root.display().to_string()), "{text}");
	assert!(text.contains("App.java"), "{text}");
}

#[test]
fn simultaneous_stdio_servers_keep_workspace_facts_isolated() {
	let _test_guard = STDIO_TEST_LOCK.lock().expect("stdio test lock");
	let first = tempfile::tempdir().expect("first workspace");
	let second = tempfile::tempdir().expect("second workspace");
	std::fs::write(first.path().join("First.java"), "class First {}\n").expect("first fixture");
	std::fs::write(second.path().join("Second.java"), "class Second {}\n").expect("second fixture");
	let first_root = first.path().canonicalize().expect("first root");
	let second_root = second.path().canonicalize().expect("second root");
	let mut first_child = spawn_stdio_read(&first_root);
	let mut second_child = spawn_stdio_read(&second_root);
	assert_no_daemon_registry_while_alive(&mut first_child, &first_root);
	assert_no_daemon_registry_while_alive(&mut second_child, &second_root);
	let first_text = collect_stdio_read(first_child, &first_root);
	let second_text = collect_stdio_read(second_child, &second_root);
	assert_no_daemon_registry(&first_root);
	assert_no_daemon_registry(&second_root);

	assert!(
		first_text.contains(&first_root.display().to_string()),
		"{first_text}"
	);
	assert!(first_text.contains("First.java"), "{first_text}");
	assert!(!first_text.contains("Second.java"), "{first_text}");
	assert!(
		second_text.contains(&second_root.display().to_string()),
		"{second_text}"
	);
	assert!(second_text.contains("Second.java"), "{second_text}");
	assert!(!second_text.contains("First.java"), "{second_text}");
}

fn spawn_stdio_read(root: &std::path::Path) -> std::process::Child {
	let mut child = Command::new(env!("CARGO_BIN_EXE_code-moniker"))
		.args(["mcp"])
		.arg(&root)
		.args(["--transport", "stdio"])
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.expect("spawn stdio MCP");

	let initialize = serde_json::json!({
		"jsonrpc": "2.0",
		"id": 1,
		"method": "initialize",
		"params": {
			"protocolVersion": "2025-11-25",
			"capabilities": {},
			"clientInfo": { "name": "code-moniker-test", "version": "1" }
		}
	});
	let initialized = serde_json::json!({
		"jsonrpc": "2.0",
		"method": "notifications/initialized",
		"params": {}
	});
	{
		let stdin = child.stdin.as_mut().expect("child stdin");
		for message in [initialize, initialized] {
			writeln!(stdin, "{message}").expect("write MCP message");
		}
	}
	child
}

fn request_stdio_read(child: &mut std::process::Child, root: &std::path::Path, id: u64) {
	let read = serde_json::json!({
		"jsonrpc": "2.0",
		"id": id,
		"method": "tools/call",
		"params": {
			"name": "code_moniker_read",
			"arguments": {
				"uri": "workspace",
				"expected_roots": [root.display().to_string()],
				"limit": 5
			}
		}
	});
	writeln!(child.stdin.as_mut().expect("child stdin"), "{read}").expect("write MCP read message");
}

fn request_stdio_workspace_status(child: &mut std::process::Child, id: u64) {
	let status = serde_json::json!({
		"jsonrpc": "2.0",
		"id": id,
		"method": "tools/call",
		"params": {
			"name": "code_moniker_query",
			"arguments": { "query": "workspace.status" }
		}
	});
	writeln!(child.stdin.as_mut().expect("child stdin"), "{status}")
		.expect("write MCP workspace.status");
}

fn collect_stdio_read(mut child: std::process::Child, root: &std::path::Path) -> String {
	let stdout = child.stdout.take().expect("child stdout");
	let (response_tx, response_rx) = std::sync::mpsc::channel();
	let reader = thread::spawn(move || {
		for line in BufReader::new(stdout).lines() {
			if response_tx.send(line).is_err() {
				break;
			}
		}
	});
	let deadline = Instant::now() + Duration::from_secs(45);
	let mut id = 2;
	let text = loop {
		request_stdio_read(&mut child, root, id);
		let remaining = deadline
			.checked_duration_since(Instant::now())
			.unwrap_or_else(|| panic!("workspace did not finish loading within 45s"));
		let response =
			receive_response(&response_rx, id, remaining).unwrap_or_else(|error| panic!("{error}"));
		let text = response["result"]["content"][0]["text"]
			.as_str()
			.expect("tool text")
			.to_string();
		if text.contains("workspace_loading") {
			thread::sleep(Duration::from_millis(50));
			id += 1;
			continue;
		}
		assert_ne!(
			response["result"]["isError"].as_bool(),
			Some(true),
			"{text}"
		);
		break text;
	};

	drop(child.stdin.take());
	wait_for_exit(&mut child, Duration::from_secs(45));
	let output = child.wait_with_output().expect("stdio MCP output");
	reader.join().expect("stdout reader");
	assert!(
		output.status.success(),
		"status: {}\nstderr:\n{}",
		output.status,
		String::from_utf8_lossy(&output.stderr)
	);
	text
}

fn assert_no_daemon_registry_while_alive(child: &mut std::process::Child, root: &std::path::Path) {
	for _ in 0..20 {
		assert!(
			child.try_wait().expect("poll MCP process").is_none(),
			"stdio MCP exited before its input was closed"
		);
		assert_no_daemon_registry(root);
		thread::sleep(Duration::from_millis(50));
	}
}

fn assert_no_daemon_registry(root: &std::path::Path) {
	let config = config_from_roots([PathBuf::from(root)]).expect("daemon config");
	let entry = read_registry_entry(&config).expect("read daemon registry");
	assert!(
		entry.is_none(),
		"stdio MCP must not register or launch a detached daemon: {entry:?}"
	);
}

fn wait_for_exit(child: &mut std::process::Child, timeout: Duration) {
	let polls = timeout.as_millis() / 100;
	for _ in 0..polls {
		if child.try_wait().expect("poll child").is_some() {
			return;
		}
		thread::sleep(Duration::from_millis(100));
	}
	let _ = child.kill();
	panic!("stdio MCP did not exit within {}s", timeout.as_secs());
}

fn receive_response(
	responses: &Receiver<std::io::Result<String>>,
	id: u64,
	timeout: Duration,
) -> Result<serde_json::Value, String> {
	let deadline = Instant::now() + timeout;
	loop {
		let remaining = deadline
			.checked_duration_since(Instant::now())
			.ok_or_else(|| format!("timed out waiting for response id {id}"))?;
		let line = responses
			.recv_timeout(remaining)
			.map_err(|error| format!("receive response id {id}: {error}"))?
			.map_err(|error| format!("read response id {id}: {error}"))?;
		let response: serde_json::Value = serde_json::from_str(&line)
			.map_err(|error| format!("invalid response JSON `{line}`: {error}"))?;
		if response["id"].as_u64() == Some(id) {
			return Ok(response);
		}
	}
}
