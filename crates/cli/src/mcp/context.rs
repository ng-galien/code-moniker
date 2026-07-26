use std::path::PathBuf;
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

use code_moniker_daemon::WorkspaceDaemon;
use code_moniker_daemon_client::DaemonClient;
use code_moniker_query::{
	CommandRequest, CommandResponse, Consistency, DaemonWorkspaceConfig, Page, ProtocolRequest,
	ProtocolResponse, Query, QueryError, QueryRequest, QueryResponse,
};

use crate::session::SessionOptions;

#[derive(Clone)]
pub(crate) struct McpContext {
	opts: SessionOptions,
	scheme: String,
	daemon: DaemonRuntime,
}

impl McpContext {
	pub(crate) fn new(mut opts: SessionOptions, scheme: String, daemon: DaemonRuntime) -> Self {
		opts.paths = opts
			.paths
			.into_iter()
			.map(|path| match path.canonicalize() {
				Ok(canonical) => canonical,
				Err(_) => path,
			})
			.collect();
		Self {
			opts,
			scheme,
			daemon,
		}
	}

	pub(super) fn query(&self, request: QueryRequest) -> anyhow::Result<QueryResponse> {
		self.daemon.query(request)
	}

	pub(super) fn query_refreshed(
		&self,
		query: Query,
		page: Page,
	) -> anyhow::Result<QueryResponse> {
		self.daemon.query(refreshed_query_request(query, page))
	}

	pub(super) fn command(&self, request: CommandRequest) -> anyhow::Result<CommandResponse> {
		self.daemon.command(request)
	}

	pub(super) fn opts(&self) -> &SessionOptions {
		&self.opts
	}

	pub(super) fn scheme(&self) -> &str {
		&self.scheme
	}

	pub(super) fn workspace_roots(&self) -> &[PathBuf] {
		&self.opts.paths
	}

	pub(super) fn workspace_label(&self) -> String {
		self.opts
			.paths
			.iter()
			.map(|path| path.display().to_string())
			.collect::<Vec<_>>()
			.join(", ")
	}

	pub(crate) fn in_process_preload_parts(&self) -> Option<InProcessPreloadParts> {
		self.daemon.in_process_preload_parts()
	}

	pub(super) fn verify_expected_roots(&self, expected: &[PathBuf]) -> anyhow::Result<()> {
		let mut expected = canonical_roots(expected)?;
		let mut actual = canonical_roots(&self.opts.paths)?;
		expected.sort();
		actual.sort();
		if expected == actual {
			return Ok(());
		}
		anyhow::bail!(
			"workspace_mismatch: expected [{}], server bound to [{}]",
			root_list(&expected),
			root_list(&actual)
		)
	}
}

fn canonical_roots(paths: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
	paths
		.iter()
		.map(|path| {
			path.canonicalize().map_err(|error| {
				anyhow::anyhow!(
					"cannot canonicalize workspace root {}: {error}",
					path.display()
				)
			})
		})
		.collect()
}

fn root_list(paths: &[PathBuf]) -> String {
	paths
		.iter()
		.map(|path| path.display().to_string())
		.collect::<Vec<_>>()
		.join(", ")
}

fn refreshed_query_request(query: Query, page: Page) -> QueryRequest {
	QueryRequest {
		query,
		consistency: Consistency::RefreshIfStale,
		page,
	}
}

#[derive(Clone)]
pub(crate) enum DaemonRuntime {
	Client {
		client: Arc<Mutex<DaemonClient>>,
		config: DaemonWorkspaceConfig,
	},
	InProcess {
		daemon: Arc<Mutex<WorkspaceDaemon>>,
		preload_config: Option<DaemonWorkspaceConfig>,
		preload_status: Arc<Mutex<PreloadStatus>>,
	},
}

#[derive(Clone, Debug)]
pub(crate) enum PreloadStatus {
	Loading,
	Ready,
	Failed(String),
}

pub(crate) struct InProcessPreloadParts {
	pub(crate) daemon_slot: Arc<Mutex<WorkspaceDaemon>>,
	pub(crate) config: DaemonWorkspaceConfig,
	pub(crate) preload_status: Arc<Mutex<PreloadStatus>>,
}

impl DaemonRuntime {
	pub(crate) fn client(client: DaemonClient, config: DaemonWorkspaceConfig) -> Self {
		Self::Client {
			client: Arc::new(Mutex::new(client)),
			config,
		}
	}

	pub(crate) fn in_process(daemon: WorkspaceDaemon) -> Self {
		Self::InProcess {
			daemon: Arc::new(Mutex::new(daemon)),
			preload_config: None,
			preload_status: Arc::new(Mutex::new(PreloadStatus::Ready)),
		}
	}

	pub(crate) fn in_process_preload(config: DaemonWorkspaceConfig) -> anyhow::Result<Self> {
		Ok(Self::InProcess {
			daemon: Arc::new(Mutex::new(WorkspaceDaemon::new_with_config(
				config.clone(),
			)?)),
			preload_config: Some(config),
			preload_status: Arc::new(Mutex::new(PreloadStatus::Loading)),
		})
	}

	pub(crate) fn in_process_preload_parts(&self) -> Option<InProcessPreloadParts> {
		let Self::InProcess {
			daemon,
			preload_config,
			preload_status,
		} = self
		else {
			return None;
		};
		Some(InProcessPreloadParts {
			daemon_slot: daemon.clone(),
			config: preload_config.clone()?,
			preload_status: preload_status.clone(),
		})
	}

	fn query(&self, request: QueryRequest) -> anyhow::Result<QueryResponse> {
		match self {
			Self::Client { client, config } => {
				with_reconnect(client, config, |client| client.query(request.clone()))
			}
			Self::InProcess {
				daemon,
				preload_config: _,
				preload_status,
			} => {
				ensure_preload_ready(preload_status)?;
				let mut daemon = lock_daemon_bounded(daemon)?;
				let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(request)));
				match response {
					ProtocolResponse::Query(response) => Ok(*response),
					ProtocolResponse::Error(error) => Err(query_error(error)),
					other => anyhow::bail!("unexpected daemon query response: {other:?}"),
				}
			}
		}
	}

	fn command(&self, request: CommandRequest) -> anyhow::Result<CommandResponse> {
		match self {
			Self::Client { client, config } => with_reconnect(client, config, |client| {
				client.command_response(request.clone())
			}),
			Self::InProcess {
				daemon,
				preload_config: _,
				preload_status,
			} => {
				ensure_preload_ready(preload_status)?;
				let mut daemon = lock_daemon_bounded(daemon)?;
				let response = daemon.handle_protocol(ProtocolRequest::Command(request));
				match response {
					ProtocolResponse::Command(response) => Ok(response),
					ProtocolResponse::Error(error) => Err(query_error(error)),
					other => anyhow::bail!("unexpected daemon command response: {other:?}"),
				}
			}
		}
	}
}

fn ensure_preload_ready(preload_status: &Mutex<PreloadStatus>) -> anyhow::Result<()> {
	wait_for_workspace(loading_wait_timeout(), LOADING_WAIT_INTERVAL, || {
		let Ok(status) = preload_status.lock() else {
			return LoadPoll::Failed(anyhow::anyhow!("preload status lock poisoned"));
		};
		match &*status {
			PreloadStatus::Loading => LoadPoll::Loading,
			PreloadStatus::Ready => LoadPoll::Ready(()),
			PreloadStatus::Failed(error) => {
				LoadPoll::Failed(anyhow::anyhow!("workspace_load_failed: {error}"))
			}
		}
	})
}

// Bounded wait mirroring the CLI query command: a first tool call during
// the initial index gets grace instead of an immediate workspace_loading
// failure. Tests with a wedged preload shrink the budget through
// CODE_MONIKER_MCP_LOADING_TIMEOUT_MS.
const LOADING_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const LOADING_WAIT_INTERVAL: Duration = Duration::from_millis(250);

fn loading_wait_timeout() -> Duration {
	std::env::var("CODE_MONIKER_MCP_LOADING_TIMEOUT_MS")
		.ok()
		.and_then(|value| value.parse::<u64>().ok())
		.map(Duration::from_millis)
		.unwrap_or(LOADING_WAIT_TIMEOUT)
}

enum LoadPoll<T> {
	Ready(T),
	Loading,
	Failed(anyhow::Error),
}

fn wait_for_workspace<T>(
	timeout: Duration,
	interval: Duration,
	mut poll: impl FnMut() -> LoadPoll<T>,
) -> anyhow::Result<T> {
	let deadline = Instant::now() + timeout;
	loop {
		match poll() {
			LoadPoll::Ready(value) => return Ok(value),
			LoadPoll::Failed(error) => return Err(error),
			LoadPoll::Loading if Instant::now() < deadline => std::thread::sleep(interval),
			LoadPoll::Loading => {
				anyhow::bail!("workspace_loading: workspace snapshot is still loading")
			}
		}
	}
}

// The MCP server outlives its daemon (binary swaps, restarts, crashes). A
// dropped connection is repaired by reconnecting-or-starting from the same
// config and replaying the request once, instead of demanding a restart.
fn with_reconnect<T>(
	client: &Arc<Mutex<DaemonClient>>,
	config: &DaemonWorkspaceConfig,
	call: impl Fn(&DaemonClient) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
	let first = {
		let client = client
			.lock()
			.map_err(|_| anyhow::anyhow!("client lock poisoned"))?;
		call(&client)
	};
	match first {
		Err(error) if connection_lost(&error) => {
			let fresh = DaemonClient::connect_or_start_config(config.clone())?;
			let mut slot = client
				.lock()
				.map_err(|_| anyhow::anyhow!("client lock poisoned"))?;
			*slot = fresh;
			call(&slot)
		}
		result => result,
	}
}

fn connection_lost(error: &anyhow::Error) -> bool {
	let text = format!("{error:#}");
	text.contains("closed") || text.contains("restart required") || text.contains("Networking")
}

fn query_error(error: QueryError) -> anyhow::Error {
	anyhow::anyhow!("{error}")
}

fn lock_daemon_bounded(
	daemon: &Mutex<WorkspaceDaemon>,
) -> anyhow::Result<std::sync::MutexGuard<'_, WorkspaceDaemon>> {
	wait_for_workspace(
		loading_wait_timeout(),
		LOADING_WAIT_INTERVAL,
		|| match daemon.try_lock() {
			Ok(guard) => LoadPoll::Ready(guard),
			Err(TryLockError::WouldBlock) => LoadPoll::Loading,
			Err(TryLockError::Poisoned(_)) => {
				LoadPoll::Failed(anyhow::anyhow!("daemon lock poisoned"))
			}
		},
	)
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use code_moniker_query::{Consistency, Page, Query};

	use super::{LoadPoll, refreshed_query_request, wait_for_workspace};

	#[test]
	fn curated_queries_refresh_stale_workspaces() {
		let request = refreshed_query_request(Query::WorkspaceStatus, Page::default());

		assert_eq!(request.consistency, Consistency::RefreshIfStale);
	}

	#[test]
	fn bounded_wait_retries_while_loading() {
		let mut polls = 0;
		let value = wait_for_workspace(Duration::from_secs(1), Duration::from_millis(1), || {
			polls += 1;
			if polls < 3 {
				LoadPoll::Loading
			} else {
				LoadPoll::Ready(42)
			}
		})
		.expect("ready after retries");

		assert_eq!(value, 42);
		assert!(polls >= 3, "the first loading answers must be retried");
	}

	#[test]
	fn bounded_wait_gives_up_with_the_loading_error() {
		let error =
			wait_for_workspace::<u32>(Duration::from_millis(5), Duration::from_millis(1), || {
				LoadPoll::Loading
			})
			.expect_err("deadline must expire");

		assert!(
			error.to_string().contains("workspace_loading"),
			"the deadline error must keep the workspace_loading signature, got {error}"
		);
	}
}
