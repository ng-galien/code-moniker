use std::sync::{Arc, Mutex};

use code_moniker_daemon::WorkspaceDaemon;
use code_moniker_daemon_client::DaemonClient;
use code_moniker_query::{
	CommandRequest, CommandResponse, ProtocolRequest, ProtocolResponse, QueryError, QueryRequest,
	QueryResponse,
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

#[derive(Clone)]
pub(crate) enum DaemonRuntime {
	Client(DaemonClient),
	InProcess(Arc<Mutex<WorkspaceDaemon>>),
}

impl DaemonRuntime {
	pub(crate) fn client(client: DaemonClient) -> Self {
		Self::Client(client)
	}

	#[cfg(test)]
	pub(crate) fn in_process(daemon: WorkspaceDaemon) -> Self {
		Self::InProcess(Arc::new(Mutex::new(daemon)))
	}

	fn query(&self, request: QueryRequest) -> anyhow::Result<QueryResponse> {
		match self {
			Self::Client(client) => client.query(request),
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
			Self::Client(client) => client.command_response(request),
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

fn query_error(error: QueryError) -> anyhow::Error {
	anyhow::anyhow!("{error}")
}
