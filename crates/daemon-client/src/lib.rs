#![cfg(unix)]

use std::future::Future;
use std::ops::Deref;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use code_moniker_query::{
	CommandRequest, CommandResponse, DaemonRpcClient, DaemonWorkspaceConfig, HandshakeResponse,
	PROTOCOL_VERSION, QueryRequest, QueryResponse,
};
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
use tokio::runtime::Runtime;

use code_moniker_query::{
	DaemonRegistryState, daemon_registry_heartbeat_expired, list_registry_files, pid_is_alive,
};

const DAEMON_READY_ATTEMPTS: usize = 300;
const DAEMON_READY_CONNECT_ATTEMPTS: usize = 10;
const DAEMON_READY_POLL: Duration = Duration::from_millis(100);

pub use code_moniker_query::{
	DaemonRegistryEntry, canonical_workspace_config, canonical_workspace_root,
	canonical_workspace_roots, config_from_roots, config_roots, daemon_workspace_config,
	list_registry_entries, read_registry_entry, registry_dir, registry_path_for_config,
	registry_path_for_root, registry_path_for_roots, remove_registry_entry_if_own,
	validate_daemon_start_config, workspace_label,
};

#[derive(Clone)]
pub struct DaemonClient {
	connection: DaemonConnection,
	endpoint: DaemonEndpoint,
}

#[derive(Clone)]
struct DaemonEndpoint {
	config: DaemonWorkspaceConfig,
	roots: Vec<PathBuf>,
	address: String,
	_supervisor_guard: Option<Arc<UnixStream>>,
}

#[derive(Clone)]
struct RegisteredDaemon {
	path: PathBuf,
	entry: DaemonRegistryEntry,
}

#[derive(Clone)]
pub struct DaemonConnection {
	runtime: Arc<Runtime>,
	ws: Arc<WsClient>,
	handshake: HandshakeResponse,
}

impl DaemonClient {
	pub fn connect(roots: Vec<PathBuf>) -> anyhow::Result<Self> {
		Self::connect_config(config_from_roots(roots)?)
	}

	pub fn connect_config(config: DaemonWorkspaceConfig) -> anyhow::Result<Self> {
		let config = canonical_workspace_config(config)?;
		let Some(entry) = read_registry_entry(&config)? else {
			anyhow::bail!(
				"no daemon registered for {}",
				workspace_label(&config_roots(&config))
			);
		};
		connect_entry(config, entry)
	}

	pub fn connect_or_start(roots: Vec<PathBuf>) -> anyhow::Result<Self> {
		Self::connect_or_start_config(config_from_roots(roots)?)
	}

	pub fn connect_or_start_config(config: DaemonWorkspaceConfig) -> anyhow::Result<Self> {
		let config = canonical_workspace_config(config)?;
		validate_daemon_start_config(&config)?;
		if let Some(client) = connect_registered_daemon(&config)? {
			return Ok(client);
		}
		start_compatible_daemon(config)
	}

	pub fn connect_or_start_supporting(
		config: DaemonWorkspaceConfig,
		capability: &str,
	) -> anyhow::Result<Self> {
		let client = Self::connect_or_start_config(config.clone())?;
		if client.supports_query(capability)? {
			return Ok(client);
		}
		restart_for_capability(client, config, capability)
	}

	pub fn root(&self) -> &Path {
		&self.endpoint.roots[0]
	}

	pub fn roots(&self) -> &[PathBuf] {
		&self.endpoint.roots
	}

	pub fn config(&self) -> &DaemonWorkspaceConfig {
		&self.endpoint.config
	}

	pub fn endpoint(&self) -> &str {
		&self.endpoint.address
	}
}

impl Deref for DaemonClient {
	type Target = DaemonConnection;

	fn deref(&self) -> &Self::Target {
		&self.connection
	}
}

impl DaemonConnection {
	pub fn handshake(&self, _client: &str) -> anyhow::Result<HandshakeResponse> {
		Ok(self.handshake.clone())
	}

	pub fn supports_query(&self, capability: &str) -> anyhow::Result<bool> {
		let handshake = self.handshake("daemon-client")?;
		Ok(handshake
			.capabilities
			.queries
			.iter()
			.any(|verb| verb == capability))
	}

	pub fn query(&self, request: QueryRequest) -> anyhow::Result<QueryResponse> {
		validate_protocol(&self.handshake)?;
		self.block(self.ws.query(request))
			.map_err(|err| anyhow::anyhow!("{err}"))
	}

	pub fn command(&self, request: CommandRequest) -> anyhow::Result<String> {
		Ok(self.command_response(request)?.message)
	}

	pub fn command_response(&self, request: CommandRequest) -> anyhow::Result<CommandResponse> {
		validate_protocol(&self.handshake)?;
		self.block(self.ws.command(request))
			.map_err(|err| anyhow::anyhow!("{err}"))
	}

	pub fn shutdown(&self) -> anyhow::Result<()> {
		self.block(self.ws.shutdown())
			.map_err(|err| anyhow::anyhow!("{err}"))
	}

	fn block<F: Future>(&self, fut: F) -> F::Output {
		self.runtime.block_on(fut)
	}
}

fn connect_entry(
	config: DaemonWorkspaceConfig,
	entry: DaemonRegistryEntry,
) -> anyhow::Result<DaemonClient> {
	let runtime = Arc::new(build_runtime()?);
	let url = format!("ws://{}", entry.endpoint);
	let ws = runtime.block_on(async { WsClientBuilder::default().build(&url).await })?;
	let handshake = runtime
		.block_on(ws.handshake("daemon-client".to_string()))
		.map_err(|err| anyhow::anyhow!("{err}"))?;
	validate_workspace(config_roots(&config), &handshake)?;
	let client = DaemonClient {
		connection: DaemonConnection {
			runtime,
			ws: Arc::new(ws),
			handshake,
		},
		endpoint: DaemonEndpoint {
			roots: config_roots(&config),
			config,
			address: entry.endpoint,
			_supervisor_guard: None,
		},
	};
	Ok(client)
}

fn validate_client_protocol(client: &DaemonClient) -> anyhow::Result<()> {
	let handshake = client.handshake("daemon-client")?;
	validate_protocol(&handshake)
}

fn validate_protocol(handshake: &HandshakeResponse) -> anyhow::Result<()> {
	if handshake.protocol_version == PROTOCOL_VERSION {
		return Ok(());
	}
	anyhow::bail!(
		"daemon protocol {} does not match client protocol {} (daemon version {}); reinstall code-moniker so the client and daemon versions match",
		handshake.protocol_version,
		PROTOCOL_VERSION,
		handshake.daemon_version
	)
}

fn validate_workspace(
	expected_roots: Vec<PathBuf>,
	handshake: &HandshakeResponse,
) -> anyhow::Result<()> {
	let mut expected = expected_roots
		.into_iter()
		.map(|root| root.display().to_string())
		.collect::<Vec<_>>();
	let mut actual = handshake.workspace_roots.clone();
	expected.sort();
	actual.sort();
	if expected == actual {
		return Ok(());
	}
	anyhow::bail!(
		"daemon workspace mismatch: expected [{}], daemon serves [{}]",
		expected.join(", "),
		actual.join(", ")
	)
}

fn connect_registered_daemon(
	config: &DaemonWorkspaceConfig,
) -> anyhow::Result<Option<DaemonClient>> {
	let Some(registered) = registry_entry_for(config)? else {
		return Ok(None);
	};
	let client = match wait_for_daemon(config.clone()) {
		Ok(client) => client,
		Err(error) => {
			let current = read_registry_entry(config)?;
			let same_fresh_claim = current.as_ref().is_some_and(|current| {
				current.pid == registered.entry.pid
					&& current.token == registered.entry.token
					&& pid_is_alive(current.pid)
					&& !daemon_registry_heartbeat_expired(current)
			});
			if same_fresh_claim {
				anyhow::bail!(
					"registered daemon pid {} for {} is alive but its endpoint is unavailable; stop that process before retrying: {error:#}",
					registered.entry.pid,
					registered.entry.workspace_root
				)
			}
			remove_registry_entry_if_own(&registered.path, &registered.entry);
			return Ok(None);
		}
	};
	let handshake = client.handshake("daemon-client")?;
	if handshake.protocol_version == PROTOCOL_VERSION {
		return Ok(Some(client));
	}
	let _ = client.shutdown();
	drop(client);
	wait_for_deregistration(config);
	let _ = cleanup_stale_config(config);
	Ok(None)
}

fn start_compatible_daemon(config: DaemonWorkspaceConfig) -> anyhow::Result<DaemonClient> {
	let supervisor_guard = Arc::new(start_daemon_process(&config)?);
	let mut client = wait_for_daemon(config)?;
	client.endpoint._supervisor_guard = Some(supervisor_guard);
	validate_client_protocol(&client)?;
	Ok(client)
}

fn restart_for_capability(
	client: DaemonClient,
	config: DaemonWorkspaceConfig,
	capability: &str,
) -> anyhow::Result<DaemonClient> {
	let _ = client.shutdown();
	drop(client);
	let config = canonical_workspace_config(config)?;
	wait_for_deregistration(&config);
	let _ = cleanup_stale_config(&config);
	let client = start_compatible_daemon(config)?;
	if !client.supports_query(capability)? {
		anyhow::bail!(
			"the code-moniker daemon binary predates `{capability}`; update code-moniker and retry"
		);
	}
	Ok(client)
}

fn registry_entry_for(config: &DaemonWorkspaceConfig) -> anyhow::Result<Option<RegisteredDaemon>> {
	let registry_path = registry_path_for_config(config)?;
	if let Some(entry) = read_registry_entry(config)? {
		if pid_is_alive(entry.pid) {
			return Ok(Some(RegisteredDaemon {
				path: registry_path,
				entry,
			}));
		}
		remove_registry_entry_if_own(&registry_path, &entry);
	}
	for (path, entry) in list_registry_files()? {
		if !pid_is_alive(entry.pid) {
			remove_registry_entry_if_own(&path, &entry);
			continue;
		}
		if registry_entry_matches_config(config, &entry) && pid_is_alive(entry.pid) {
			return Ok(Some(RegisteredDaemon { path, entry }));
		}
	}
	Ok(None)
}

fn registry_entry_matches_config(
	config: &DaemonWorkspaceConfig,
	entry: &DaemonRegistryEntry,
) -> bool {
	let mut expected_roots = config.roots.clone();
	let mut actual_roots = entry.workspace_roots.clone();
	expected_roots.sort();
	actual_roots.sort();
	expected_roots == actual_roots
		&& config.project == entry.project
		&& config.cache_dir == entry.cache_dir
}

fn build_runtime() -> anyhow::Result<Runtime> {
	Ok(tokio::runtime::Builder::new_multi_thread()
		.worker_threads(2)
		.enable_all()
		.thread_name("code-moniker-daemon-client")
		.build()?)
}

// After asking an outdated daemon to shut down, give it a moment to leave
// the registry so the fresh start does not race its guarded removal.
fn wait_for_deregistration(config: &DaemonWorkspaceConfig) {
	for _ in 0..30 {
		match read_registry_entry(config) {
			Ok(Some(entry)) if pid_is_alive(entry.pid) => {
				thread::sleep(Duration::from_millis(100));
			}
			_ => return,
		}
	}
}

fn wait_for_daemon(config: DaemonWorkspaceConfig) -> anyhow::Result<DaemonClient> {
	wait_for_daemon_with_limits(
		config,
		DAEMON_READY_ATTEMPTS,
		DAEMON_READY_CONNECT_ATTEMPTS,
		DAEMON_READY_POLL,
	)
}

fn wait_for_daemon_with_limits(
	config: DaemonWorkspaceConfig,
	ready_attempts: usize,
	ready_connect_attempts: usize,
	poll: Duration,
) -> anyhow::Result<DaemonClient> {
	let mut last_error = None;
	let mut ready_connect_failures = 0;
	for _ in 0..ready_attempts {
		if let Some(registered) = registry_entry_for(&config)?
			&& registered.entry.state == DaemonRegistryState::Ready
		{
			match connect_entry(config.clone(), registered.entry) {
				Ok(client) => return Ok(client),
				Err(error) if error.to_string().contains("daemon workspace mismatch") => {
					return Err(error);
				}
				Err(error) => {
					ready_connect_failures += 1;
					last_error = Some(error);
					if ready_connect_failures >= ready_connect_attempts {
						break;
					}
				}
			}
		}
		thread::sleep(poll);
	}
	let workspace = workspace_label(&config_roots(&config));
	match last_error {
		Some(error) => anyhow::bail!(
			"daemon ready endpoint remained unusable for {workspace} after {ready_connect_failures} connection attempts: {error:#}"
		),
		None => {
			let timeout_seconds = (ready_attempts as u128 * poll.as_millis()) / 1_000;
			anyhow::bail!("daemon did not become ready for {workspace} after {timeout_seconds}s")
		}
	}
}

pub fn cleanup_stale_entry(roots: Vec<PathBuf>) -> anyhow::Result<()> {
	cleanup_stale_config(&config_from_roots(roots)?)
}

pub fn cleanup_stale_config(config: &DaemonWorkspaceConfig) -> anyhow::Result<()> {
	let path = registry_path_for_config(config)?;
	if let Some(entry) = read_registry_entry(config)?
		&& !pid_is_alive(entry.pid)
	{
		remove_registry_entry_if_own(&path, &entry);
	}
	Ok(())
}

fn start_daemon_process(config: &DaemonWorkspaceConfig) -> anyhow::Result<UnixStream> {
	let exe = std::env::current_exe()?;
	let (supervisor_guard, child_supervisor) = UnixStream::pair()?;
	let supervisor_fd = child_supervisor.as_raw_fd();
	let mut command = ProcessCommand::new(exe);
	command
		.arg("daemon")
		.arg("start")
		.arg("--supervisor-pid")
		.arg(std::process::id().to_string())
		.arg("--supervisor-fd")
		.arg(supervisor_fd.to_string())
		.stdin(Stdio::null())
		.stdout(Stdio::null())
		.stderr(Stdio::null());
	// SAFETY: the closure only clears FD_CLOEXEC on the already-open socket.
	// No allocation or lock-taking operation runs in the forked child.
	unsafe {
		command.pre_exec(move || {
			if libc::fcntl(supervisor_fd, libc::F_SETFD, 0) == -1 {
				return Err(std::io::Error::last_os_error());
			}
			Ok(())
		});
	}
	if let Some(project) = &config.project {
		command.arg("--project").arg(project);
	}
	if let Some(cache_dir) = &config.cache_dir {
		command.arg("--cache").arg(cache_dir);
	}
	if let Some(live_refresh) = &config.live_refresh {
		command.arg("--live-refresh").arg(live_refresh);
	}
	for root in config_roots(config) {
		command.arg(root);
	}
	command.spawn().map(|_| supervisor_guard).map_err(|err| {
		anyhow::anyhow!(
			"cannot start daemon for {}: {err}",
			workspace_label(&config_roots(config))
		)
	})
}

#[cfg(test)]
mod tests {
	use std::net::TcpListener;

	use code_moniker_query::CapabilitySet;
	use code_moniker_query::write_registry_entry;

	use super::*;

	fn handshake(protocol_version: u32) -> HandshakeResponse {
		HandshakeResponse {
			protocol_version,
			daemon_version: "test".to_string(),
			workspace_root: "/workspace".to_string(),
			workspace_roots: vec!["/workspace".to_string()],
			capabilities: CapabilitySet::default(),
		}
	}

	fn registry_entry(roots: &[&str]) -> DaemonRegistryEntry {
		DaemonRegistryEntry {
			workspace_root: roots.join(","),
			workspace_roots: roots.iter().map(|root| (*root).to_string()).collect(),
			project: None,
			cache_dir: None,
			live_refresh: Some("on-demand".to_string()),
			endpoint: "127.0.0.1:1234".to_string(),
			token: "test".to_string(),
			pid: std::process::id(),
			heartbeat_unix_ms: code_moniker_query::registry_heartbeat_unix_ms(),
			state: DaemonRegistryState::Ready,
		}
	}

	#[test]
	fn accepts_current_protocol() {
		validate_protocol(&handshake(PROTOCOL_VERSION)).expect("current protocol");
	}

	#[test]
	fn rejects_any_protocol_mismatch_with_reinstall_guidance() {
		for mismatched in [PROTOCOL_VERSION - 1, PROTOCOL_VERSION + 1] {
			let error = validate_protocol(&handshake(mismatched)).expect_err("mismatched protocol");
			let message = error.to_string();
			assert!(message.contains(&format!("daemon protocol {mismatched}")));
			assert!(message.contains(&format!("client protocol {PROTOCOL_VERSION}")));
			assert!(message.contains("reinstall code-moniker"));
		}
	}

	#[test]
	fn accepts_only_the_exact_daemon_workspace() {
		let config = DaemonWorkspaceConfig {
			roots: vec!["/workspace".to_string()],
			project: None,
			cache_dir: None,
			live_refresh: Some("auto".to_string()),
		};
		assert!(registry_entry_matches_config(
			&config,
			&registry_entry(&["/workspace"])
		));
		assert!(!registry_entry_matches_config(
			&config,
			&registry_entry(&["/workspace", "/other"])
		));
		assert!(!registry_entry_matches_config(
			&config,
			&registry_entry(&["/other"])
		));
	}

	#[test]
	fn rejects_a_daemon_handshake_for_another_workspace() {
		validate_workspace(
			vec![PathBuf::from("/workspace")],
			&handshake(PROTOCOL_VERSION),
		)
		.expect("matching workspace");
		let error = validate_workspace(vec![PathBuf::from("/other")], &handshake(PROTOCOL_VERSION))
			.expect_err("mismatched workspace");
		assert!(error.to_string().contains("daemon workspace mismatch"));
		assert!(error.to_string().contains("/other"));
		assert!(error.to_string().contains("/workspace"));
	}

	#[test]
	fn preserves_a_live_registry_entry_with_an_unreachable_endpoint() {
		let workspace = tempfile::tempdir().expect("workspace");
		let config = config_from_roots([workspace.path()]).expect("config");
		let listener = TcpListener::bind("127.0.0.1:0").expect("reserve endpoint");
		let endpoint = listener.local_addr().expect("endpoint").to_string();
		drop(listener);
		let entry = DaemonRegistryEntry {
			workspace_root: config.roots[0].clone(),
			workspace_roots: config.roots.clone(),
			project: config.project.clone(),
			cache_dir: config.cache_dir.clone(),
			live_refresh: config.live_refresh.clone(),
			endpoint,
			token: "unreachable-live-entry".to_string(),
			pid: std::process::id(),
			heartbeat_unix_ms: code_moniker_query::registry_heartbeat_unix_ms(),
			state: DaemonRegistryState::Ready,
		};
		write_registry_entry(&config, &entry).expect("registry fixture");

		let error = match connect_registered_daemon(&config) {
			Err(error) => error,
			Ok(_) => panic!("a live but unreachable daemon must not be replaced"),
		};
		assert!(
			error
				.to_string()
				.contains("is alive but its endpoint is unavailable"),
			"{error:#}"
		);
		assert!(
			read_registry_entry(&config)
				.expect("read registry")
				.is_some(),
			"a live daemon claim must remain registered"
		);
		remove_registry_entry_if_own(
			&registry_path_for_config(&config).expect("registry path"),
			&entry,
		);
	}

	#[test]
	fn expires_an_unreachable_legacy_claim_even_if_its_pid_was_reused() {
		let workspace = tempfile::tempdir().expect("workspace");
		let config = config_from_roots([workspace.path()]).expect("config");
		let listener = TcpListener::bind("127.0.0.1:0").expect("reserve endpoint");
		let endpoint = listener.local_addr().expect("endpoint").to_string();
		drop(listener);
		let mut entry = registry_entry(&[config.roots[0].as_str()]);
		entry.workspace_root = config.roots[0].clone();
		entry.workspace_roots = config.roots.clone();
		entry.endpoint = endpoint;
		entry.heartbeat_unix_ms = 0;
		write_registry_entry(&config, &entry).expect("legacy registry fixture");

		let client = connect_registered_daemon(&config).expect("expire legacy claim");
		assert!(
			client.is_none(),
			"expired unreachable claim must be recyclable"
		);
		assert!(
			read_registry_entry(&config)
				.expect("read registry")
				.is_none(),
			"expired claim must be removed"
		);
	}
}
