#![cfg(unix)]

use std::ffi::CString;
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
	DaemonClient, DaemonRegistryEntry, config_from_roots, read_registry_entry,
	registry_path_for_config, remove_registry_entry_if_own,
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

	wait_for_registry_state(
		&config,
		daemon.child_mut().id(),
		code_moniker_query::DaemonRegistryState::Indexing,
		Duration::from_secs(10),
	);
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
	wait_for_registry_state(
		&config,
		daemon.child_mut().id(),
		code_moniker_query::DaemonRegistryState::Indexing,
		Duration::from_secs(10),
	);

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
	let mut daemon = spawn_supervised_daemon(&root, supervisor.child_mut().id());
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
	assert!(daemon.wait().expect("reap daemon").success());
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
	wait_for_registry_state(
		&config,
		daemon.child_mut().id(),
		code_moniker_query::DaemonRegistryState::Indexing,
		Duration::from_secs(10),
	);
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
		heartbeat_unix_ms: 0,
		state: code_moniker_query::DaemonRegistryState::Ready,
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
fn repeated_auto_spawned_clients_leave_no_daemons_or_registry_entries() {
	let _lifecycle = lifecycle_test_lock();
	for iteration in 0..3 {
		let workspace = tempfile::tempdir().expect("workspace");
		std::fs::write(
			workspace.path().join("App.java"),
			format!("class App{iteration} {{}}\n"),
		)
		.expect("fixture");
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
		wait_for_registry_removal(&config, Duration::from_secs(5));
	}
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
			&& entry.state == code_moniker_query::DaemonRegistryState::Ready
		{
			return;
		}
		thread::sleep(Duration::from_millis(50));
	}
	panic!("daemon did not become ready for {}", root_label(config));
}

fn wait_for_registry_state(
	config: &code_moniker_query::DaemonWorkspaceConfig,
	expected_pid: u32,
	expected_state: code_moniker_query::DaemonRegistryState,
	timeout: Duration,
) {
	let deadline = Instant::now() + timeout;
	while Instant::now() < deadline {
		if let Some(entry) = read_registry_entry(config).expect("registry read")
			&& entry.pid == expected_pid
			&& entry.state == expected_state
		{
			return;
		}
		thread::sleep(Duration::from_millis(5));
	}
	panic!(
		"daemon did not reach {expected_state:?} for {}",
		root_label(config)
	);
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
