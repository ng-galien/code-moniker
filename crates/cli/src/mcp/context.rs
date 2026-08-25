use std::path::PathBuf;
use std::sync::{Arc, Mutex, TryLockError};

use code_moniker_daemon::WorkspaceDaemon;
use code_moniker_daemon_client::DaemonClient;
use code_moniker_query::{
	CommandRequest, CommandResponse, Consistency, DaemonWorkspaceConfig, Page, ProtocolRequest,
	ProtocolResponse, Query, QueryError, QueryRequest, QueryResponse, QueryResult,
	WorkspaceLifecycle, WorkspacePhase,
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

	pub(super) fn runtime_label(&self) -> &'static str {
		match &self.daemon {
			DaemonRuntime::Client { .. } => "detached-daemon",
			DaemonRuntime::InProcess { .. } => "stdio-worker",
		}
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
		lifecycle: Arc<Mutex<WorkspaceLifecycle>>,
	},
}

pub(crate) struct InProcessPreloadParts {
	pub(crate) daemon_slot: Arc<Mutex<WorkspaceDaemon>>,
	pub(crate) config: DaemonWorkspaceConfig,
	pub(crate) lifecycle: Arc<Mutex<WorkspaceLifecycle>>,
}

impl DaemonRuntime {
	pub(crate) fn client(client: DaemonClient, config: DaemonWorkspaceConfig) -> Self {
		Self::Client {
			client: Arc::new(Mutex::new(client)),
			config,
		}
	}

	#[cfg(test)]
	pub(crate) fn in_process(daemon: WorkspaceDaemon) -> Self {
		Self::InProcess {
			daemon: Arc::new(Mutex::new(daemon)),
			preload_config: None,
			lifecycle: Arc::new(Mutex::new(WorkspaceLifecycle::ready())),
		}
	}

	pub(crate) fn in_process_preload(config: DaemonWorkspaceConfig) -> anyhow::Result<Self> {
		Ok(Self::InProcess {
			daemon: Arc::new(Mutex::new(WorkspaceDaemon::new_with_config(
				config.clone(),
			)?)),
			preload_config: Some(config),
			lifecycle: Arc::new(Mutex::new(WorkspaceLifecycle::loading())),
		})
	}

	pub(crate) fn in_process_preload_parts(&self) -> Option<InProcessPreloadParts> {
		let Self::InProcess {
			daemon,
			preload_config,
			lifecycle,
		} = self
		else {
			return None;
		};
		Some(InProcessPreloadParts {
			daemon_slot: daemon.clone(),
			config: preload_config.clone()?,
			lifecycle: lifecycle.clone(),
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
				lifecycle,
			} => {
				let current = current_lifecycle(lifecycle)?;
				if request.query.requires_workspace_snapshot() {
					ensure_workspace_available(&current)?;
				}
				let mut daemon = lock_daemon(daemon)?;
				let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(request)));
				match response {
					ProtocolResponse::Query(response) => {
						let mut response = *response;
						project_workspace_lifecycle(&mut response, &current);
						Ok(response)
					}
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
				lifecycle,
			} => {
				ensure_workspace_available(&current_lifecycle(lifecycle)?)?;
				let mut daemon = lock_daemon(daemon)?;
				let response = daemon.handle_protocol(ProtocolRequest::Command(request));
				match response {
					ProtocolResponse::Command(response) => {
						if let Some(status) = &response.status {
							*lifecycle.lock().map_err(|_| {
								anyhow::anyhow!("workspace lifecycle lock poisoned")
							})? = WorkspaceLifecycle {
								phase: status.phase,
								failure: status.failure.clone(),
							};
						}
						Ok(response)
					}
					ProtocolResponse::Error(error) => Err(query_error(error)),
					other => anyhow::bail!("unexpected daemon command response: {other:?}"),
				}
			}
		}
	}
}

fn current_lifecycle(lifecycle: &Mutex<WorkspaceLifecycle>) -> anyhow::Result<WorkspaceLifecycle> {
	lifecycle
		.lock()
		.map(|state| state.clone())
		.map_err(|_| anyhow::anyhow!("workspace lifecycle lock poisoned"))
}

fn ensure_workspace_available(lifecycle: &WorkspaceLifecycle) -> anyhow::Result<()> {
	match lifecycle.phase {
		WorkspacePhase::Ready => Ok(()),
		WorkspacePhase::Loading | WorkspacePhase::Refreshing => {
			anyhow::bail!("workspace_loading: workspace snapshot is still loading")
		}
		WorkspacePhase::Failed => anyhow::bail!(
			"workspace_load_failed: {}",
			lifecycle
				.failure
				.as_ref()
				.map(|failure| failure.message.as_str())
				.unwrap_or("workspace initial index failed")
		),
	}
}

fn project_workspace_lifecycle(response: &mut QueryResponse, lifecycle: &WorkspaceLifecycle) {
	let QueryResult::WorkspaceStatus(status) = &mut response.result else {
		return;
	};
	status.phase = lifecycle.phase;
	status.failure = lifecycle.failure.clone();
	if lifecycle.phase == WorkspacePhase::Ready {
		return;
	}
	let summary = lifecycle
		.failure
		.as_ref()
		.map(|failure| failure.message.clone())
		.unwrap_or_else(|| lifecycle.phase.to_string());
	status.stale_summary = summary.clone();
	for root in &mut status.roots {
		root.stale_summary.clone_from(&summary);
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

fn lock_daemon(
	daemon: &Mutex<WorkspaceDaemon>,
) -> anyhow::Result<std::sync::MutexGuard<'_, WorkspaceDaemon>> {
	match daemon.try_lock() {
		Ok(guard) => Ok(guard),
		Err(TryLockError::WouldBlock) => {
			anyhow::bail!(
				"workspace_busy: this stdio-worker is applying an exclusive mutation; a detached daemon is an independent runtime"
			)
		}
		Err(TryLockError::Poisoned(_)) => anyhow::bail!("daemon lock poisoned"),
	}
}

#[cfg(test)]
mod tests {
	use code_moniker_query::{Consistency, Page, Query, WorkspaceLifecycle};

	use super::{ensure_workspace_available, refreshed_query_request};

	#[test]
	fn curated_queries_refresh_stale_workspaces() {
		let request = refreshed_query_request(Query::WorkspaceStatus, Page::default());

		assert_eq!(request.consistency, Consistency::RefreshIfStale);
	}

	#[test]
	fn loading_is_reported_without_waiting() {
		let error = ensure_workspace_available(&WorkspaceLifecycle::loading())
			.expect_err("loading must be returned to the caller");

		assert!(error.to_string().contains("workspace_loading"), "{error}");
	}

	#[test]
	fn preload_failure_is_reported_without_restart_or_retry() {
		let error = ensure_workspace_available(&WorkspaceLifecycle::failed("broken index"))
			.expect_err("failed preload must be observable");

		assert!(
			error.to_string().contains("workspace_load_failed"),
			"{error}"
		);
		assert!(error.to_string().contains("broken index"), "{error}");
	}
}
