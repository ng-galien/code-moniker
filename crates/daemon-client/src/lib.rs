#![cfg(unix)]

use std::future::Future;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use code_moniker_query::{
	CommandRequest, CommandResponse, DaemonRpcClient, DaemonWorkspaceConfig, HandshakeResponse,
	QueryRequest, QueryResponse,
};
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
use tokio::runtime::Runtime;

use code_moniker_query::{list_registry_files, pid_is_alive};

pub use code_moniker_query::{
	DaemonRegistryEntry, canonical_workspace_config, canonical_workspace_root,
	canonical_workspace_roots, config_from_roots, config_roots, daemon_workspace_config,
	list_registry_entries, read_registry_entry, registry_dir, registry_path_for_config,
	registry_path_for_root, registry_path_for_roots, workspace_label,
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
}

#[derive(Clone)]
pub struct DaemonConnection {
	runtime: Arc<Runtime>,
	ws: Arc<WsClient>,
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
		if let Some(entry) = read_registry_entry(&config)?
			&& let Ok(client) = connect_entry(config.clone(), entry)
		{
			return Ok(client);
		}
		let _ = cleanup_stale_config(&config);
		start_daemon_process(&config)?;
		wait_for_daemon(config)
	}

	pub fn connect_or_start_supporting(
		config: DaemonWorkspaceConfig,
		capability: &str,
	) -> anyhow::Result<Self> {
		let client = Self::connect_or_start_config(config.clone())?;
		if client.supports_query(capability)? {
			return Ok(client);
		}
		let _ = client.shutdown();
		drop(client);
		let config = canonical_workspace_config(config)?;
		wait_for_deregistration(&config);
		let _ = cleanup_stale_config(&config);
		start_daemon_process(&config)?;
		let client = wait_for_daemon(config)?;
		if !client.supports_query(capability)? {
			anyhow::bail!(
				"the code-moniker daemon binary predates `{capability}`; update code-moniker and retry"
			);
		}
		Ok(client)
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
	pub fn handshake(&self, client: &str) -> anyhow::Result<HandshakeResponse> {
		self.block(self.ws.handshake(client.to_string()))
			.map_err(|err| anyhow::anyhow!("{err}"))
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
		self.block(self.ws.query(request))
			.map_err(|err| anyhow::anyhow!("{err}"))
	}

	pub fn command(&self, request: CommandRequest) -> anyhow::Result<String> {
		Ok(self.command_response(request)?.message)
	}

	pub fn command_response(&self, request: CommandRequest) -> anyhow::Result<CommandResponse> {
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
	let client = DaemonClient {
		connection: DaemonConnection {
			runtime,
			ws: Arc::new(ws),
		},
		endpoint: DaemonEndpoint {
			roots: config_roots(&config),
			config,
			address: entry.endpoint,
		},
	};
	client.handshake("daemon-client")?;
	Ok(client)
}

fn registry_entry_for(
	config: &DaemonWorkspaceConfig,
) -> anyhow::Result<Option<DaemonRegistryEntry>> {
	if let Some(entry) = read_registry_entry(config)? {
		return Ok(Some(entry));
	}
	for (_, entry) in list_registry_files()? {
		let serves_all_roots = config
			.roots
			.iter()
			.all(|root| entry.workspace_roots.contains(root));
		if serves_all_roots && pid_is_alive(entry.pid) {
			return Ok(Some(entry));
		}
	}
	Ok(None)
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
	for _ in 0..50 {
		if let Some(entry) = registry_entry_for(&config)?
			&& let Ok(client) = connect_entry(config.clone(), entry)
		{
			return Ok(client);
		}
		thread::sleep(Duration::from_millis(100));
	}
	anyhow::bail!(
		"daemon did not become ready for {}",
		workspace_label(&config_roots(&config))
	)
}

pub fn cleanup_stale_entry(roots: Vec<PathBuf>) -> anyhow::Result<()> {
	cleanup_stale_config(&config_from_roots(roots)?)
}

pub fn cleanup_stale_config(config: &DaemonWorkspaceConfig) -> anyhow::Result<()> {
	let path = registry_path_for_config(config)?;
	if path.exists() {
		let _ = std::fs::remove_file(path);
	}
	Ok(())
}

fn start_daemon_process(config: &DaemonWorkspaceConfig) -> anyhow::Result<()> {
	let exe = std::env::current_exe()?;
	let mut command = ProcessCommand::new(exe);
	command
		.arg("daemon")
		.arg("start")
		.stdin(Stdio::null())
		.stdout(Stdio::null())
		.stderr(Stdio::null());
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
	command.spawn().map(|_| ()).map_err(|err| {
		anyhow::anyhow!(
			"cannot start daemon for {}: {err}",
			workspace_label(&config_roots(config))
		)
	})
}
