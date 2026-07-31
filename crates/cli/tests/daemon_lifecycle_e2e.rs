#![cfg(unix)]

use std::ffi::CString;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use code_moniker_daemon_client::{
	DaemonClient, DaemonRegistryEntry, config_from_roots, daemon_workspace_config,
	read_registry_entry, registry_path_for_config, remove_registry_entry_if_own,
};
use code_moniker_query::write_registry_entry;

struct ChildGuard(Option<Child>);

impl ChildGuard {
	fn child_mut(&mut self) -> &mut Child {
		self.0.as_mut().expect("child already consumed")
	}

	fn wait(mut self) -> std::io::Result<std::process::ExitStatus> {
		self.0.take().expect("child already consumed").wait()
	}
}

impl Drop for ChildGuard {
	fn drop(&mut self) {
		if let Some(child) = &mut self.0 {
			let _ = child.kill();
			let _ = child.wait();
		}
	}
}

#[test]
fn supervised_daemon_exits_and_cleans_registry_when_supervisor_dies() {
	let _lifecycle = lifecycle_test_lock();
	let workspace = tempfile::tempdir().expect("workspace");
	std::fs::write(workspace.path().join("App.java"), "class App {}\n").expect("fixture");
	let root = workspace.path().canonicalize().expect("canonical root");
	let config = config_from_roots([root.clone()]).expect("daemon config");
	assert!(
		read_registry_entry(&config)
			.expect("initial registry read")
			.is_none(),
		"test workspace unexpectedly has a daemon"
	);

	let mut supervisor = ChildGuard(Some(
		Command::new("sleep")
			.arg("60")
			.stdin(Stdio::null())
			.stdout(Stdio::null())
			.stderr(Stdio::null())
			.spawn()
			.expect("spawn supervisor"),
	));
	let supervisor_pid = supervisor.child_mut().id();
	let mut daemon = ChildGuard(Some(
		Command::new(env!("CARGO_BIN_EXE_code-moniker"))
			.args(["daemon", "start"])
			.arg(&root)
			.args(["--supervisor-pid", &supervisor_pid.to_string()])
			.stdin(Stdio::null())
			.stdout(Stdio::null())
			.stderr(Stdio::null())
			.spawn()
			.expect("spawn supervised daemon"),
	));

	wait_for_ready_registry(&config, daemon.child_mut().id(), Duration::from_secs(15));
	let first_heartbeat = read_registry_entry(&config)
		.expect("registry read")
		.expect("registered daemon")
		.heartbeat_unix_ms;
	thread::sleep(Duration::from_millis(2_200));
	let renewed_heartbeat = read_registry_entry(&config)
		.expect("renewed registry read")
		.expect("registered daemon after heartbeat")
		.heartbeat_unix_ms;
	assert!(
		renewed_heartbeat > first_heartbeat,
		"daemon registry heartbeat did not advance"
	);
	assert!(
		daemon
			.child_mut()
			.try_wait()
			.expect("poll supervised daemon")
			.is_none(),
		"daemon exited while its supervisor was alive"
	);

	supervisor.child_mut().kill().expect("terminate supervisor");
	supervisor.wait().expect("reap supervisor");
	// Workspace-wide test runs can starve this subprocess while other language
	// suites are active. The supervisor itself is polled once per second; this
	// larger harness bound avoids turning scheduler delay into a false leak.
	wait_for_exit(daemon.child_mut(), Duration::from_secs(15));
	let status = daemon.wait().expect("reap daemon");
	assert!(status.success(), "supervised daemon status: {status}");
	assert!(
		read_registry_entry(&config)
			.expect("final registry read")
			.is_none(),
		"supervised daemon left a registry entry behind"
	);
}

#[test]
fn query_targets_the_exact_registered_daemon_endpoint() {
	let _lifecycle = lifecycle_test_lock();
	let workspace = tempfile::tempdir().expect("workspace");
	std::fs::write(workspace.path().join("App.java"), "class App {}\n").expect("fixture");
	let root = workspace.path().canonicalize().expect("canonical root");
	let config = config_from_roots([root.clone()]).expect("daemon config");
	let mut supervisor = spawn_supervisor();
	let mut daemon = spawn_supervised_daemon(&root, supervisor.child_mut().id());
	wait_for_ready_registry(&config, daemon.child_mut().id(), Duration::from_secs(15));
	let endpoint = read_registry_entry(&config)
		.expect("registry read")
		.expect("registered daemon")
		.endpoint;

	assert_exact_daemon_query_surface(workspace.path(), &root, &endpoint);
	stop_daemon_endpoint(workspace.path(), &endpoint);
	wait_for_exit(daemon.child_mut(), Duration::from_secs(15));
	assert!(daemon.wait().expect("reap daemon").success());
	wait_for_registry_removal(&config, Duration::from_secs(2));
	assert_missing_endpoint_has_no_fallback(&config, &endpoint);

	supervisor.child_mut().kill().expect("terminate supervisor");
	supervisor.wait().expect("reap supervisor");
}

#[test]
fn daemon_identity_control_uses_the_ambient_cache_directory() {
	let _lifecycle = lifecycle_test_lock();
	let workspace = tempfile::tempdir().expect("workspace");
	std::fs::write(workspace.path().join("App.java"), "class App {}\n").expect("fixture");
	let root = workspace.path().canonicalize().expect("canonical root");
	let cache = workspace.path().join("cache");
	let config = daemon_workspace_config(
		[root.clone()],
		None,
		Some(cache.clone()),
		Some("on-demand".to_string()),
	)
	.expect("daemon config");
	let mut supervisor = spawn_supervisor();
	let mut daemon = spawn_cached_supervised_daemon(&root, &cache, supervisor.child_mut().id());
	wait_for_ready_registry(&config, daemon.child_mut().id(), Duration::from_secs(15));

	for command in ["status", "stop"] {
		let output = Command::new(env!("CARGO_BIN_EXE_code-moniker"))
			.args(["daemon", command])
			.arg(&root)
			.env("CODE_MONIKER_CACHE_DIR", &cache)
			.output()
			.expect("daemon identity control");
		assert!(
			output.status.success(),
			"daemon {command}: {}\nstdout:\n{}\nstderr:\n{}",
			output.status,
			String::from_utf8_lossy(&output.stdout),
			String::from_utf8_lossy(&output.stderr)
		);
	}
	wait_for_exit(daemon.child_mut(), Duration::from_secs(15));
	assert!(daemon.wait().expect("reap daemon").success());
	wait_for_registry_removal(&config, Duration::from_secs(2));
	supervisor.child_mut().kill().expect("terminate supervisor");
	supervisor.wait().expect("reap supervisor");
}

fn assert_exact_daemon_query_surface(workspace: &Path, root: &Path, endpoint: &str) {
	let output = Command::new(env!("CARGO_BIN_EXE_code-moniker"))
		.args(["query", "--daemon", endpoint, "workspace.status"])
		.env("CODE_MONIKER_CACHE_DIR", workspace.join("ambient-cache"))
		.output()
		.expect("query exact daemon endpoint");

	assert!(
		output.status.success(),
		"query status: {}\nstdout:\n{}\nstderr:\n{}",
		output.status,
		String::from_utf8_lossy(&output.stdout),
		String::from_utf8_lossy(&output.stderr)
	);
	assert!(
		String::from_utf8_lossy(&output.stdout).contains(&root.display().to_string()),
		"query must answer for the selected daemon: {}",
		String::from_utf8_lossy(&output.stdout)
	);

	let rules = workspace.join("scratch-rules.toml");
	std::fs::write(
		&rules,
		r#"
default_rules = false

[[java.class.where]]
id = "endpoint-index-is-pinned"
expr = "name != 'App'"
message = "the selected daemon generation must remain the source corpus"
"#,
	)
	.expect("rules fixture");
	std::fs::write(workspace.join("App.java"), "class Changed {}\n")
		.expect("change filesystem source");
	let rules_query = format!(
		r#"rules.check rules:"{}" consistency:stale-ok"#,
		rules.display()
	);
	let output = Command::new(env!("CARGO_BIN_EXE_code-moniker"))
		.args(["query", "--daemon", endpoint, "--json", &rules_query])
		.output()
		.expect("rules check exact daemon endpoint");
	let stdout = String::from_utf8_lossy(&output.stdout);
	assert!(
		output.status.success(),
		"rules query status: {}\nstdout:\n{stdout}\nstderr:\n{}",
		output.status,
		String::from_utf8_lossy(&output.stderr)
	);
	assert!(
		stdout.contains("java.class.endpoint-index-is-pinned"),
		"rules must observe the pinned indexed App class: {stdout}"
	);
	assert!(
		stdout.contains("\"generation\": 1"),
		"response must identify the selected index generation: {stdout}"
	);

	let status = Command::new(env!("CARGO_BIN_EXE_code-moniker"))
		.args(["daemon", "status", "--daemon", endpoint])
		.env("CODE_MONIKER_CACHE_DIR", workspace.join("ambient-cache"))
		.output()
		.expect("status exact daemon endpoint");
	let status_stdout = String::from_utf8_lossy(&status.stdout);
	assert!(
		status.status.success(),
		"daemon status: {}\nstdout:\n{status_stdout}\nstderr:\n{}",
		status.status,
		String::from_utf8_lossy(&status.stderr)
	);
	assert!(status_stdout.contains(&format!("endpoint: {endpoint}")));
	assert!(status_stdout.contains("generation: 1"));
}

fn stop_daemon_endpoint(workspace: &Path, endpoint: &str) {
	let stopped = Command::new(env!("CARGO_BIN_EXE_code-moniker"))
		.args(["daemon", "stop", "--daemon", endpoint])
		.env("CODE_MONIKER_CACHE_DIR", workspace.join("ambient-cache"))
		.output()
		.expect("stop exact daemon endpoint");
	assert!(
		stopped.status.success(),
		"daemon stop: {}\nstdout:\n{}\nstderr:\n{}",
		stopped.status,
		String::from_utf8_lossy(&stopped.stdout),
		String::from_utf8_lossy(&stopped.stderr)
	);
}

fn assert_missing_endpoint_has_no_fallback(
	config: &code_moniker_query::DaemonWorkspaceConfig,
	endpoint: &str,
) {
	let missing = Command::new(env!("CARGO_BIN_EXE_code-moniker"))
		.args(["query", "--daemon", endpoint, "workspace.status"])
		.output()
		.expect("query removed daemon endpoint");
	assert!(!missing.status.success(), "missing daemon must fail closed");
	assert!(
		String::from_utf8_lossy(&missing.stderr).contains("no daemon registered at endpoint"),
		"unexpected missing-daemon error: {}",
		String::from_utf8_lossy(&missing.stderr)
	);
	assert!(
		read_registry_entry(config)
			.expect("registry after failed direct query")
			.is_none(),
		"an exact endpoint miss must not auto-start or register a replacement"
	);
}

#[test]
fn supervised_daemon_exits_if_supervisor_dies_during_initial_index() {
	let _lifecycle = lifecycle_test_lock();
	let workspace = tempfile::tempdir().expect("workspace");
	for index in 0..2_000 {
		std::fs::write(
			workspace.path().join(format!("Class{index}.java")),
			format!("class Class{index} {{ void call() {{}} }}\n"),
		)
		.expect("fixture");
	}
	let root = workspace.path().canonicalize().expect("canonical root");
	let config = config_from_roots([root.clone()]).expect("daemon config");
	let mut supervisor = spawn_supervisor();
	let supervisor_pid = supervisor.child_mut().id();
	let mut daemon = spawn_supervised_daemon(&root, supervisor_pid);

	wait_for_registry_entry(&config, daemon.child_mut().id(), Duration::from_secs(10));
	supervisor.child_mut().kill().expect("terminate supervisor");
	supervisor.wait().expect("reap supervisor");
	wait_for_exit(daemon.child_mut(), Duration::from_secs(15));
	assert!(daemon.wait().expect("reap daemon").success());
	wait_for_registry_removal(&config, Duration::from_secs(2));
}

#[test]
fn supervised_daemon_exits_while_a_source_read_is_blocked() {
	let _lifecycle = lifecycle_test_lock();
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
	let config = config_from_roots([root.clone()]).expect("daemon config");
	let mut supervisor = spawn_supervisor();
	let mut daemon = spawn_supervised_daemon(&root, supervisor.child_mut().id());
	wait_for_registry_entry(&config, daemon.child_mut().id(), Duration::from_secs(10));

	supervisor.child_mut().kill().expect("terminate supervisor");
	supervisor.wait().expect("reap supervisor");
	wait_for_exit(daemon.child_mut(), Duration::from_secs(2));
	assert!(daemon.wait().expect("reap daemon").success());
	wait_for_registry_removal(&config, Duration::from_secs(2));
}

#[test]
fn inherited_supervision_channel_wins_even_while_supervisor_pid_is_alive() {
	let _lifecycle = lifecycle_test_lock();
	let workspace = tempfile::tempdir().expect("workspace");
	std::fs::write(workspace.path().join("App.java"), "class App {}\n").expect("fixture");
	let root = workspace.path().canonicalize().expect("canonical root");
	let config = config_from_roots([root.clone()]).expect("daemon config");
	let (mut daemon, supervisor_guard) = spawn_channel_supervised_daemon(&root, std::process::id());
	wait_for_ready_registry(&config, daemon.child_mut().id(), Duration::from_secs(15));

	drop(supervisor_guard);
	wait_for_exit(daemon.child_mut(), Duration::from_secs(10));
	assert!(daemon.wait().expect("reap daemon").success());
	wait_for_registry_removal(&config, Duration::from_secs(2));
}

#[test]
fn daemon_exits_if_its_live_registry_claim_disappears() {
	let _lifecycle = lifecycle_test_lock();
	let workspace = tempfile::tempdir().expect("workspace");
	std::fs::write(workspace.path().join("App.java"), "class App {}\n").expect("fixture");
	let root = workspace.path().canonicalize().expect("canonical root");
	let config = config_from_roots([root.clone()]).expect("daemon config");
	let mut supervisor = spawn_supervisor();
	let mut daemon = spawn_supervised_daemon_with_stderr(&root, supervisor.child_mut().id());
	wait_for_ready_registry(&config, daemon.child_mut().id(), Duration::from_secs(15));
	let entry = read_registry_entry(&config)
		.expect("registry read")
		.expect("registered daemon");
	remove_registry_entry_if_own(
		&registry_path_for_config(&config).expect("registry path"),
		&entry,
	);
	// Claim ownership is polled every 250 ms. Keep CI scheduling headroom here;
	// the blocked-read shutdown contract has its own strict two-second test.
	wait_for_exit(daemon.child_mut(), Duration::from_secs(15));
	let mut diagnostic = String::new();
	daemon
		.child_mut()
		.stderr
		.take()
		.expect("daemon stderr")
		.read_to_string(&mut diagnostic)
		.expect("read daemon diagnostic");
	let status = daemon.wait().expect("reap daemon");
	assert!(!status.success(), "claim loss must be an abnormal exit");
	assert!(
		diagnostic.contains("daemon registry claim lost")
			&& diagnostic.contains("registry entry disappeared"),
		"missing claim-loss diagnostic:\n{diagnostic}"
	);
}

#[test]
fn daemon_honors_rpc_shutdown_during_initial_index() {
	let _lifecycle = lifecycle_test_lock();
	let workspace = tempfile::tempdir().expect("workspace");
	for index in 0..2_000 {
		std::fs::write(
			workspace.path().join(format!("Class{index}.java")),
			format!("class Class{index} {{ void call() {{}} }}\n"),
		)
		.expect("fixture");
	}
	let root = workspace.path().canonicalize().expect("canonical root");
	let config = config_from_roots([root.clone()]).expect("daemon config");
	let mut supervisor = spawn_supervisor();
	let mut daemon = spawn_supervised_daemon(&root, supervisor.child_mut().id());
	wait_for_registry_entry(&config, daemon.child_mut().id(), Duration::from_secs(10));
	let client = DaemonClient::connect_config(config.clone()).expect("connect indexing daemon");
	client.shutdown().expect("request shutdown during index");
	drop(client);
	wait_for_exit(daemon.child_mut(), Duration::from_secs(5));
	assert!(daemon.wait().expect("reap daemon").success());
	wait_for_registry_removal(&config, Duration::from_secs(2));
}

#[test]
fn expired_claim_with_a_reused_live_pid_does_not_block_startup() {
	let _lifecycle = lifecycle_test_lock();
	let workspace = tempfile::tempdir().expect("workspace");
	std::fs::write(workspace.path().join("App.java"), "class App {}\n").expect("fixture");
	let root = workspace.path().canonicalize().expect("canonical root");
	let config = config_from_roots([root.clone()]).expect("daemon config");
	let stale = DaemonRegistryEntry {
		workspace_root: config.roots[0].clone(),
		workspace_roots: config.roots.clone(),
		project: None,
		cache_dir: None,
		live_refresh: Some("on-demand".to_string()),
		endpoint: "127.0.0.1:9".to_string(),
		token: "expired-reused-pid".to_string(),
		pid: std::process::id(),
		build: code_moniker_query::BuildIdentity::default(),
		heartbeat_unix_ms: 0,
	};
	write_registry_entry(&config, &stale).expect("stale registry fixture");
	let mut supervisor = spawn_supervisor();
	let mut daemon = spawn_supervised_daemon(&root, supervisor.child_mut().id());
	wait_for_ready_registry(&config, daemon.child_mut().id(), Duration::from_secs(15));
	supervisor.child_mut().kill().expect("terminate supervisor");
	supervisor.wait().expect("reap supervisor");
	wait_for_exit(daemon.child_mut(), Duration::from_secs(5));
	assert!(daemon.wait().expect("reap daemon").success());
	wait_for_registry_removal(&config, Duration::from_secs(2));
}

#[test]
fn auto_spawned_daemon_survives_the_client_that_started_it() {
	let _lifecycle = lifecycle_test_lock();
	let workspace = tempfile::tempdir().expect("workspace");
	std::fs::write(workspace.path().join("App.java"), "class App {}\n").expect("fixture");
	let root = workspace.path().canonicalize().expect("canonical root");
	let config = config_from_roots([root.clone()]).expect("daemon config");
	let output = Command::new(env!("CARGO_BIN_EXE_code-moniker"))
		.args(["query", "-r"])
		.arg(&root)
		.arg("workspace.status")
		.output()
		.expect("run auto-spawned client");
	assert!(
		output.status.success(),
		"query status: {}\nstdout:\n{}\nstderr:\n{}",
		output.status,
		String::from_utf8_lossy(&output.stdout),
		String::from_utf8_lossy(&output.stderr)
	);
	let entry = read_registry_entry(&config)
		.expect("registry read")
		.expect("auto-spawned daemon must remain registered");
	assert!(
		code_moniker_query::pid_is_alive(entry.pid),
		"auto-spawned daemon must outlive its first client"
	);
	let client = DaemonClient::connect_config(config.clone()).expect("reconnect persistent daemon");
	client.shutdown().expect("stop persistent test daemon");
	drop(client);
	wait_for_registry_removal(&config, Duration::from_secs(5));
}

#[test]
fn loading_index_returns_to_clients_and_keeps_building_in_the_same_daemon() {
	let _lifecycle = lifecycle_test_lock();
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
	let config = config_from_roots([root.clone()]).expect("daemon config");

	let started = Instant::now();
	let status = bounded_command_output(
		Command::new(env!("CARGO_BIN_EXE_code-moniker"))
			.args(["query", "-r"])
			.arg(&root)
			.arg("workspace.status"),
		Duration::from_secs(3),
	);
	assert!(
		status.status.success(),
		"{}",
		String::from_utf8_lossy(&status.stderr)
	);
	assert!(started.elapsed() < Duration::from_secs(3));
	assert!(String::from_utf8_lossy(&status.stdout).contains("phase: loading"));

	let entry = read_registry_entry(&config)
		.expect("registry read")
		.expect("serving daemon registration");
	assert!(code_moniker_query::pid_is_alive(entry.pid));
	let heartbeat_deadline = Instant::now() + Duration::from_secs(5);
	loop {
		let current = read_registry_entry(&config)
			.expect("heartbeat registry read")
			.expect("daemon remains registered while indexing");
		assert_eq!(
			current.pid, entry.pid,
			"heartbeat must keep the same daemon"
		);
		if current.heartbeat_unix_ms > entry.heartbeat_unix_ms {
			break;
		}
		assert!(
			Instant::now() < heartbeat_deadline,
			"serving daemon heartbeat did not advance during the blocked index"
		);
		thread::sleep(Duration::from_millis(50));
	}

	let loading = bounded_command_output(
		Command::new(env!("CARGO_BIN_EXE_code-moniker"))
			.args(["query", "-r"])
			.arg(&root)
			.arg("symbol.search name:Blocked"),
		Duration::from_secs(2),
	);
	assert!(!loading.status.success(), "data query must expose loading");
	assert!(
		String::from_utf8_lossy(&loading.stderr).contains("workspace_loading"),
		"{}",
		String::from_utf8_lossy(&loading.stderr)
	);
	assert_eq!(
		read_registry_entry(&config)
			.expect("registry read after retry")
			.expect("same daemon")
			.pid,
		entry.pid
	);

	let mut writer = ChildGuard(Some(
		Command::new("sh")
			.args([
				"-c",
				"while :; do printf 'pub struct Blocked;\\n' > \"$1\"; done",
				"fifo-writer",
			])
			.arg(&fifo)
			.stdin(Stdio::null())
			.stdout(Stdio::null())
			.stderr(Stdio::null())
			.spawn()
			.expect("release blocked source reads"),
	));
	wait_for_ready_registry(&config, entry.pid, Duration::from_secs(15));
	writer.child_mut().kill().expect("stop FIFO writer");
	writer.wait().expect("reap FIFO writer");
	let ready = bounded_command_output(
		Command::new(env!("CARGO_BIN_EXE_code-moniker"))
			.args(["query", "-r"])
			.arg(&root)
			.arg("symbol.search name:Blocked"),
		Duration::from_secs(3),
	);
	assert!(
		ready.status.success(),
		"{}",
		String::from_utf8_lossy(&ready.stderr)
	);
	assert!(String::from_utf8_lossy(&ready.stdout).contains("Blocked"));

	let client = DaemonClient::connect_config(config.clone()).expect("connect persistent daemon");
	client.shutdown().expect("stop persistent test daemon");
	drop(client);
	wait_for_registry_removal(&config, Duration::from_secs(5));
}

fn lifecycle_test_lock() -> MutexGuard<'static, ()> {
	static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
	LOCK.get_or_init(|| Mutex::new(()))
		.lock()
		.unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn wait_for_ready_registry(
	config: &code_moniker_query::DaemonWorkspaceConfig,
	expected_pid: u32,
	timeout: Duration,
) {
	let deadline = Instant::now() + timeout;
	while Instant::now() < deadline {
		if let Some(entry) = read_registry_entry(config).expect("registry read")
			&& entry.pid == expected_pid
		{
			let output = Command::new(env!("CARGO_BIN_EXE_code-moniker"))
				.args(["daemon", "status", "--daemon", &entry.endpoint])
				.output()
				.expect("probe daemon status");
			if output.status.success()
				&& String::from_utf8_lossy(&output.stdout).contains("state: ready")
			{
				return;
			}
		}
		thread::sleep(Duration::from_millis(50));
	}
	panic!("daemon did not become ready for {}", root_label(config));
}

fn wait_for_registry_entry(
	config: &code_moniker_query::DaemonWorkspaceConfig,
	expected_pid: u32,
	timeout: Duration,
) {
	let deadline = Instant::now() + timeout;
	while Instant::now() < deadline {
		if let Some(entry) = read_registry_entry(config).expect("registry read")
			&& entry.pid == expected_pid
		{
			return;
		}
		thread::sleep(Duration::from_millis(5));
	}
	panic!("daemon did not register for {}", root_label(config));
}

fn spawn_supervisor() -> ChildGuard {
	ChildGuard(Some(
		Command::new("sleep")
			.arg("60")
			.stdin(Stdio::null())
			.stdout(Stdio::null())
			.stderr(Stdio::null())
			.spawn()
			.expect("spawn supervisor"),
	))
}

fn spawn_supervised_daemon(root: &Path, supervisor_pid: u32) -> ChildGuard {
	ChildGuard(Some(
		Command::new(env!("CARGO_BIN_EXE_code-moniker"))
			.args(["daemon", "start"])
			.arg(root)
			.args(["--supervisor-pid", &supervisor_pid.to_string()])
			.stdin(Stdio::null())
			.stdout(Stdio::null())
			.stderr(Stdio::null())
			.spawn()
			.expect("spawn supervised daemon"),
	))
}

fn spawn_supervised_daemon_with_stderr(root: &Path, supervisor_pid: u32) -> ChildGuard {
	ChildGuard(Some(
		Command::new(env!("CARGO_BIN_EXE_code-moniker"))
			.args(["daemon", "start"])
			.arg(root)
			.args(["--supervisor-pid", &supervisor_pid.to_string()])
			.stdin(Stdio::null())
			.stdout(Stdio::null())
			.stderr(Stdio::piped())
			.spawn()
			.expect("spawn observable supervised daemon"),
	))
}

fn spawn_cached_supervised_daemon(root: &Path, cache: &Path, supervisor_pid: u32) -> ChildGuard {
	ChildGuard(Some(
		Command::new(env!("CARGO_BIN_EXE_code-moniker"))
			.args(["daemon", "start"])
			.arg(root)
			.args(["--cache"])
			.arg(cache)
			.args(["--supervisor-pid", &supervisor_pid.to_string()])
			.stdin(Stdio::null())
			.stdout(Stdio::null())
			.stderr(Stdio::null())
			.spawn()
			.expect("spawn cached supervised daemon"),
	))
}

fn spawn_channel_supervised_daemon(root: &Path, supervisor_pid: u32) -> (ChildGuard, UnixStream) {
	let (supervisor_guard, child_supervisor) = UnixStream::pair().expect("supervision socket pair");
	let supervisor_fd = child_supervisor.as_raw_fd();
	let mut command = Command::new(env!("CARGO_BIN_EXE_code-moniker"));
	command
		.args(["daemon", "start"])
		.arg(root)
		.args(["--supervisor-pid", &supervisor_pid.to_string()])
		.args(["--supervisor-fd", &supervisor_fd.to_string()])
		.stdin(Stdio::null())
		.stdout(Stdio::null())
		.stderr(Stdio::null());
	// SAFETY: this only clears FD_CLOEXEC on the inherited supervision socket.
	unsafe {
		command.pre_exec(move || {
			if libc::fcntl(supervisor_fd, libc::F_SETFD, 0) == -1 {
				return Err(std::io::Error::last_os_error());
			}
			Ok(())
		});
	}
	let daemon = command.spawn().expect("spawn channel-supervised daemon");
	(ChildGuard(Some(daemon)), supervisor_guard)
}

fn wait_for_exit(child: &mut Child, timeout: Duration) {
	let deadline = Instant::now() + timeout;
	while Instant::now() < deadline {
		if child.try_wait().expect("poll daemon").is_some() {
			return;
		}
		thread::sleep(Duration::from_millis(50));
	}
	panic!(
		"supervised daemon did not exit within {}s",
		timeout.as_secs()
	);
}

fn bounded_command_output(command: &mut Command, timeout: Duration) -> std::process::Output {
	command.stdout(Stdio::piped()).stderr(Stdio::piped());
	let mut child = command.spawn().expect("spawn bounded command");
	let deadline = Instant::now() + timeout;
	while Instant::now() < deadline {
		if child.try_wait().expect("poll bounded command").is_some() {
			return child
				.wait_with_output()
				.expect("collect bounded command output");
		}
		thread::sleep(Duration::from_millis(20));
	}
	let _ = child.kill();
	let output = child
		.wait_with_output()
		.expect("collect timed-out command output");
	panic!(
		"command did not finish within {}ms\nstdout:\n{}\nstderr:\n{}",
		timeout.as_millis(),
		String::from_utf8_lossy(&output.stdout),
		String::from_utf8_lossy(&output.stderr)
	);
}

fn wait_for_registry_removal(
	config: &code_moniker_query::DaemonWorkspaceConfig,
	timeout: Duration,
) {
	let deadline = Instant::now() + timeout;
	while Instant::now() < deadline {
		if read_registry_entry(config)
			.expect("registry read")
			.is_none()
		{
			return;
		}
		thread::sleep(Duration::from_millis(50));
	}
	panic!(
		"daemon registry remained after client exit for {}",
		root_label(config)
	);
}

fn root_label(config: &code_moniker_query::DaemonWorkspaceConfig) -> String {
	config
		.roots
		.first()
		.map(PathBuf::from)
		.as_deref()
		.unwrap_or_else(|| Path::new("<missing>"))
		.display()
		.to_string()
}
