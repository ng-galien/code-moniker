#![cfg(unix)]

use std::future::Future;
use std::io::{Read, Seek, SeekFrom};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use code_moniker_query::{
	Command, CommandRequest, CommandResponse, DaemonRpcClient, DaemonWorkspaceConfig,
	HandshakeResponse, PROTOCOL_VERSION, QueryError, QueryRequest, QueryResponse,
	current_build_identity,
};
use jsonrpsee::core::ClientError;
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
use tokio::runtime::Runtime;

use code_moniker_query::{daemon_registry_heartbeat_expired, list_registry_files, pid_is_alive};

const DAEMON_SERVING_ATTEMPTS: usize = 50;
const DAEMON_SERVING_CONNECT_ATTEMPTS: usize = 10;
const DAEMON_SERVING_POLL: Duration = Duration::from_millis(100);

pub use code_moniker_query::{
	DaemonRegistryEntry, WorkspaceSourceDocumentDto, WorkspaceSourceSetDto,
	canonical_workspace_config, canonical_workspace_root, canonical_workspace_roots,
	config_from_roots, config_roots, daemon_log_path_for_config, daemon_workspace_config,
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
			return Err(no_daemon_registered_error(&config));
		};
		connect_entry(config, entry)
	}

	pub fn connect_endpoint(endpoint: &str) -> anyhow::Result<Self> {
		let entry = registry_entry_for_endpoint(endpoint)?;
		let config = config_from_registry_entry(&entry);
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

pub fn no_daemon_registered_error(config: &DaemonWorkspaceConfig) -> anyhow::Error {
	anyhow::anyhow!(
		"no daemon registered for {}{}",
		workspace_label(&config_roots(config)),
		daemon_diagnostic_suffix(config)
	)
}

pub fn registry_entry_for_endpoint(endpoint: &str) -> anyhow::Result<DaemonRegistryEntry> {
	let matches = list_registry_entries()?
		.into_iter()
		.filter(|entry| entry.endpoint == endpoint)
		.collect::<Vec<_>>();
	match matches.as_slice() {
		[] => anyhow::bail!(
			"no daemon registered at endpoint {endpoint}; run `code-moniker daemon list`"
		),
		[entry] => Ok(entry.clone()),
		_ => anyhow::bail!("multiple daemons registered at endpoint {endpoint}"),
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
		validate_compatibility(&self.handshake)?;
		self.block(self.ws.query(request)).map_err(rpc_client_error)
	}

	pub fn command(&self, request: CommandRequest) -> anyhow::Result<String> {
		Ok(self.command_response(request)?.message)
	}

	pub fn command_response(&self, request: CommandRequest) -> anyhow::Result<CommandResponse> {
		validate_compatibility(&self.handshake)?;
		self.block(self.ws.command(request))
			.map_err(rpc_client_error)
	}

	pub fn replace_source_set(
		&self,
		source_set: WorkspaceSourceSetDto,
	) -> anyhow::Result<CommandResponse> {
		self.command_response(CommandRequest {
			command: Command::WorkspaceSourceSetReplace { source_set },
		})
	}

	pub fn remove_source_set(&self, srcset: impl Into<String>) -> anyhow::Result<CommandResponse> {
		self.command_response(CommandRequest {
			command: Command::WorkspaceSourceSetRemove {
				srcset: srcset.into(),
			},
		})
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
		},
	};
	Ok(client)
}

fn config_from_registry_entry(entry: &DaemonRegistryEntry) -> DaemonWorkspaceConfig {
	let entry = entry.clone();
	DaemonWorkspaceConfig {
		roots: entry.workspace_roots,
		project: entry.project,
		cache_dir: entry.cache_dir,
		live_refresh: entry.live_refresh,
	}
}

fn validate_client_protocol(client: &DaemonClient) -> anyhow::Result<()> {
	let handshake = client.handshake("daemon-client")?;
	validate_compatibility(&handshake)
}

fn validate_compatibility(handshake: &HandshakeResponse) -> anyhow::Result<()> {
	validate_protocol(handshake)?;
	validate_build(handshake)
}

fn validate_protocol(handshake: &HandshakeResponse) -> anyhow::Result<()> {
	if handshake.protocol_version == PROTOCOL_VERSION {
		return Ok(());
	}
	if handshake.protocol_version < PROTOCOL_VERSION {
		anyhow::bail!(
			"daemon protocol {} is older than client protocol {} (daemon version {}); reconnect-or-start must recycle the daemon once so it can rebuild the index",
			handshake.protocol_version,
			PROTOCOL_VERSION,
			handshake.daemon_version
		)
	}
	anyhow::bail!(
		"client protocol {} is older than daemon protocol {} (daemon version {}); update the client, the newer daemon was left running",
		PROTOCOL_VERSION,
		handshake.protocol_version,
		handshake.daemon_version
	)
}

fn validate_build(handshake: &HandshakeResponse) -> anyhow::Result<()> {
	let client = current_build_identity(env!("CARGO_PKG_VERSION"))?;
	if handshake.build == client {
		return Ok(());
	}
	anyhow::bail!(
		"daemon build {} ({}) does not match client build {} ({}); restart code-moniker so the snapshot producer matches the client",
		handshake.build.version,
		handshake.build.fingerprint,
		client.version,
		client.fingerprint
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
	let client_build = current_build_identity(env!("CARGO_PKG_VERSION"))?;
	match compatibility_action(&handshake, &client_build) {
		CompatibilityAction::Reuse => return Ok(Some(client)),
		CompatibilityAction::RejectClient => {
			return validate_client_protocol(&client).map(|()| Some(client));
		}
		CompatibilityAction::RestartDaemon => {}
	}
	let _ = client.shutdown();
	drop(client);
	wait_for_deregistration(config);
	let _ = cleanup_stale_config(config);
	Ok(None)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CompatibilityAction {
	Reuse,
	RestartDaemon,
	RejectClient,
}

fn compatibility_action(
	handshake: &HandshakeResponse,
	client_build: &code_moniker_query::BuildIdentity,
) -> CompatibilityAction {
	if handshake.protocol_version < PROTOCOL_VERSION {
		return CompatibilityAction::RestartDaemon;
	}
	if handshake.protocol_version > PROTOCOL_VERSION {
		return CompatibilityAction::RejectClient;
	}
	if &handshake.build != client_build {
		return CompatibilityAction::RestartDaemon;
	}
	CompatibilityAction::Reuse
}

fn start_compatible_daemon(config: DaemonWorkspaceConfig) -> anyhow::Result<DaemonClient> {
	start_daemon_process(&config)?;
	let client = wait_for_daemon(config)?;
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
		DAEMON_SERVING_ATTEMPTS,
		DAEMON_SERVING_CONNECT_ATTEMPTS,
		DAEMON_SERVING_POLL,
	)
}

fn wait_for_daemon_with_limits(
	config: DaemonWorkspaceConfig,
	serving_attempts: usize,
	serving_connect_attempts: usize,
	poll: Duration,
) -> anyhow::Result<DaemonClient> {
	let mut last_error = None;
	let mut serving_connect_failures = 0;
	for _ in 0..serving_attempts {
		if let Some(registered) = registry_entry_for(&config)? {
			match connect_entry(config.clone(), registered.entry) {
				Ok(client) => return Ok(client),
				Err(error) if error.to_string().contains("daemon workspace mismatch") => {
					return Err(error);
				}
				Err(error) => {
					serving_connect_failures += 1;
					last_error = Some(error);
					if serving_connect_failures >= serving_connect_attempts {
						break;
					}
				}
			}
		}
		thread::sleep(poll);
	}
	let workspace = workspace_label(&config_roots(&config));
	let diagnostic = daemon_diagnostic_suffix(&config);
	match last_error {
		Some(error) => anyhow::bail!(
			"daemon endpoint remained unusable for {workspace} after {serving_connect_failures} connection attempts: {error:#}{diagnostic}"
		),
		None => {
			let timeout_seconds = (serving_attempts as u128 * poll.as_millis()) / 1_000;
			anyhow::bail!(
				"daemon did not publish a serving endpoint for {workspace} after {timeout_seconds}s{diagnostic}"
			)
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

fn start_daemon_process(config: &DaemonWorkspaceConfig) -> anyhow::Result<()> {
	let exe = std::env::current_exe()?;
	let diagnostic_path = daemon_log_path_for_config(config)?;
	if let Some(parent) = diagnostic_path.parent() {
		std::fs::create_dir_all(parent)?;
	}
	let diagnostic = std::fs::OpenOptions::new()
		.create(true)
		.append(true)
		.open(&diagnostic_path)?;
	let mut command = ProcessCommand::new(exe);
	command
		.arg("daemon")
		.arg("start")
		.stdin(Stdio::null())
		.stdout(Stdio::null())
		.stderr(Stdio::from(diagnostic));
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

fn daemon_diagnostic_suffix(config: &DaemonWorkspaceConfig) -> String {
	const MAX_DIAGNOSTIC_BYTES: u64 = 8 * 1024;
	let Ok(path) = daemon_log_path_for_config(config) else {
		return String::new();
	};
	let Ok(mut file) = std::fs::File::open(&path) else {
		return format!("\ndaemon diagnostics: {} (not written)", path.display());
	};
	let length = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
	let start = length.saturating_sub(MAX_DIAGNOSTIC_BYTES);
	if file.seek(SeekFrom::Start(start)).is_err() {
		return format!("\ndaemon diagnostics: {} (unreadable)", path.display());
	}
	let mut bytes = Vec::new();
	if file.read_to_end(&mut bytes).is_err() {
		return format!("\ndaemon diagnostics: {} (unreadable)", path.display());
	}
	let text = String::from_utf8_lossy(&bytes);
	let text = if start > 0 {
		text.split_once('\n').map(|(_, tail)| tail).unwrap_or(&text)
	} else {
		&text
	};
	format!(
		"\ndaemon diagnostics ({}):\n{}",
		path.display(),
		text.trim()
	)
}

fn rpc_client_error(error: ClientError) -> anyhow::Error {
	if let ClientError::Call(error) = &error
		&& let Some(data) = error.data()
		&& let Ok(query_error) = serde_json::from_str::<QueryError>(data.get())
	{
		return anyhow::anyhow!("{query_error}");
	}
	anyhow::anyhow!("{error}")
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
			build: current_build_identity(env!("CARGO_PKG_VERSION")).expect("test build"),
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
			build: code_moniker_query::BuildIdentity::default(),
			heartbeat_unix_ms: code_moniker_query::registry_heartbeat_unix_ms(),
		}
	}

	#[test]
	fn accepts_current_protocol() {
		validate_compatibility(&handshake(PROTOCOL_VERSION)).expect("current compatibility");
	}

	#[test]
	fn preserves_structured_query_errors_without_json_rpc_debug_noise() {
		let uri = "rs:workspace.fn:missing()";
		let query_error = QueryError::new("symbol_not_found", format!("symbol not found: {uri}"));
		let rpc_error = jsonrpsee::types::ErrorObjectOwned::owned(
			jsonrpsee::types::error::INTERNAL_ERROR_CODE,
			query_error.message.clone(),
			Some(query_error),
		);

		let error = rpc_client_error(ClientError::Call(rpc_error));

		assert_eq!(
			error.to_string(),
			format!("symbol_not_found: symbol not found: {uri}")
		);
		assert!(!error.to_string().contains("ErrorObject"), "{error}");
		assert!(!error.to_string().contains("RawValue"), "{error}");
	}

	#[test]
	fn rejects_a_build_mismatch_even_when_protocol_matches() {
		let mut daemon = handshake(PROTOCOL_VERSION);
		daemon.build.fingerprint = "fnv1a64:0000000000000000".to_string();

		let error = validate_compatibility(&daemon).expect_err("mismatched build");
		assert!(error.to_string().contains("daemon build"));
		assert!(error.to_string().contains("does not match client build"));
		assert!(error.to_string().contains("snapshot producer"));
	}

	#[test]
	fn protocol_direction_controls_recovery() {
		let client_build = current_build_identity(env!("CARGO_PKG_VERSION")).expect("client build");
		assert_eq!(
			compatibility_action(&handshake(PROTOCOL_VERSION - 1), &client_build),
			CompatibilityAction::RestartDaemon,
			"an older daemon is recycled once so the current binary rebuilds its index"
		);
		assert_eq!(
			compatibility_action(&handshake(PROTOCOL_VERSION + 1), &client_build),
			CompatibilityAction::RejectClient,
			"an older client must not destroy a newer daemon"
		);

		let newer_error =
			validate_protocol(&handshake(PROTOCOL_VERSION + 1)).expect_err("newer daemon");
		assert!(newer_error.to_string().contains("update the client"));
		assert!(newer_error.to_string().contains("left running"));
		let older_error =
			validate_protocol(&handshake(PROTOCOL_VERSION - 1)).expect_err("older daemon");
		assert!(older_error.to_string().contains("recycle the daemon once"));
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
	fn reconstructs_the_exact_daemon_identity_from_its_registry_entry() {
		let mut entry = registry_entry(&["/workspace", "/other"]);
		entry.project = Some("backend".to_string());
		entry.cache_dir = Some("/cache".to_string());
		entry.live_refresh = Some("auto".to_string());

		let config = config_from_registry_entry(&entry);

		assert_eq!(config.roots, vec!["/workspace", "/other"]);
		assert_eq!(config.project.as_deref(), Some("backend"));
		assert_eq!(config.cache_dir.as_deref(), Some("/cache"));
		assert_eq!(config.live_refresh.as_deref(), Some("auto"));
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
			build: code_moniker_query::BuildIdentity::default(),
			heartbeat_unix_ms: code_moniker_query::registry_heartbeat_unix_ms(),
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

	#[test]
	fn startup_timeout_includes_the_captured_daemon_diagnostic() {
		let workspace = tempfile::tempdir().expect("workspace");
		let config = config_from_roots([workspace.path()]).expect("config");
		let path = daemon_log_path_for_config(&config).expect("diagnostic path");
		std::fs::create_dir_all(path.parent().expect("diagnostic parent"))
			.expect("diagnostic directory");
		std::fs::write(&path, "code-moniker daemon: fatal fixture\n").expect("diagnostic fixture");

		let error =
			match wait_for_daemon_with_limits(config.clone(), 0, 1, Duration::from_millis(0)) {
				Err(error) => error,
				Ok(_) => panic!("missing daemon must time out"),
			};
		let message = error.to_string();
		assert!(message.contains("fatal fixture"), "{message}");
		assert!(message.contains(&path.display().to_string()), "{message}");
		let missing = no_daemon_registered_error(&config).to_string();
		assert!(missing.contains("no daemon registered"), "{missing}");
		assert!(missing.contains("fatal fixture"), "{missing}");
		let _ = std::fs::remove_file(path);
	}
}
