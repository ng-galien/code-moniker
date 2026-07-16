use std::sync::{Arc, Mutex};

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
	pub(crate) fn new(opts: SessionOptions, scheme: String, daemon: DaemonRuntime) -> Self {
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
	InProcess(Arc<Mutex<WorkspaceDaemon>>),
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
		Self::InProcess(Arc::new(Mutex::new(daemon)))
	}

	fn query(&self, request: QueryRequest) -> anyhow::Result<QueryResponse> {
		match self {
			Self::Client { client, config } => {
				with_reconnect(client, config, |client| client.query(request.clone()))
			}
			Self::InProcess(daemon) => {
				let response = daemon
					.lock()
					.map_err(|_| anyhow::anyhow!("daemon lock poisoned"))?
					.handle_protocol(ProtocolRequest::Query(Box::new(request)));
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
			Self::InProcess(daemon) => {
				let response = daemon
					.lock()
					.map_err(|_| anyhow::anyhow!("daemon lock poisoned"))?
					.handle_protocol(ProtocolRequest::Command(request));
				match response {
					ProtocolResponse::Command(response) => Ok(response),
					ProtocolResponse::Error(error) => Err(query_error(error)),
					other => anyhow::bail!("unexpected daemon command response: {other:?}"),
				}
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

#[cfg(test)]
mod tests {
	use code_moniker_query::{Consistency, Page, Query};

	use super::refreshed_query_request;

	#[test]
	fn curated_queries_refresh_stale_workspaces() {
		let request = refreshed_query_request(Query::WorkspaceStatus, Page::default());

		assert_eq!(request.consistency, Consistency::RefreshIfStale);
	}
}
