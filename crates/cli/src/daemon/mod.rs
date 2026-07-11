use std::io::Write;
use std::path::PathBuf;

use code_moniker_daemon as daemon;
use code_moniker_daemon_client as daemon_client;
use code_moniker_query::{DaemonWorkspaceConfig, Query, QueryRequest, QueryResult};

use crate::Exit;
use crate::args::{DaemonArgs, DaemonCommand, DaemonRootArgs};

pub(crate) fn run_daemon<W1: Write, W2: Write>(
	args: &DaemonArgs,
	stdout: &mut W1,
	stderr: &mut W2,
) -> Exit {
	let result = match &args.command {
		DaemonCommand::Start(args) => daemon_config(args).and_then(daemon::serve_foreground_config),
		DaemonCommand::Status(args) => daemon_status(args, stdout),
		DaemonCommand::Stop(args) => daemon_stop(args, stdout),
		DaemonCommand::List => daemon_list(stdout),
	};
	match result {
		Ok(()) => Exit::Match,
		Err(error) => {
			let _ = writeln!(stderr, "code-moniker: {error:#}");
			Exit::UsageError
		}
	}
}

fn daemon_status<W: Write>(args: &DaemonRootArgs, stdout: &mut W) -> anyhow::Result<()> {
	let config = daemon_config(args)?;
	let entry = daemon_client::read_registry_entry(&config)?;
	let client = daemon_client::DaemonClient::connect_config(config)?;
	let handshake = client.handshake("code-moniker-cli")?;
	writeln!(stdout, "workspace: {}", handshake.workspace_root)?;
	for root in &handshake.workspace_roots {
		writeln!(stdout, "root: {root}")?;
	}
	writeln!(stdout, "endpoint: {}", client.endpoint())?;
	if let Some(project) = &client.config().project {
		writeln!(stdout, "project: {project}")?;
	}
	if let Some(cache_dir) = &client.config().cache_dir {
		writeln!(stdout, "cache: {cache_dir}")?;
	}
	if let Some(entry) = &entry {
		writeln!(stdout, "pid: {}", entry.pid)?;
		if let Some(live_refresh) = &entry.live_refresh {
			writeln!(stdout, "live_refresh: {live_refresh}")?;
		}
	}
	writeln!(stdout, "protocol: {}", handshake.protocol_version)?;
	writeln!(stdout, "daemon: {}", handshake.daemon_version)?;
	writeln!(
		stdout,
		"queries: {}",
		handshake.capabilities.queries.join(", ")
	)?;
	let response = client.query(QueryRequest::new(Query::WorkspaceStatus))?;
	if let QueryResult::WorkspaceStatus(status) = response.result {
		writeln!(
			stdout,
			"files: {} symbols: {} references: {} stale: {}",
			status.files, status.symbols, status.references, status.stale_summary
		)?;
		for root in status.roots {
			writeln!(
				stdout,
				"status_root: {} files={} symbols={} references={} stale={}",
				root.root, root.files, root.symbols, root.references, root.stale_summary
			)?;
		}
	}
	Ok(())
}

fn daemon_stop<W: Write>(args: &DaemonRootArgs, stdout: &mut W) -> anyhow::Result<()> {
	let client = daemon_client::DaemonClient::connect_config(daemon_config(args)?)?;
	client.shutdown()?;
	writeln!(stdout, "stopped: {}", root_label(client.roots()))?;
	Ok(())
}

fn daemon_list<W: Write>(stdout: &mut W) -> anyhow::Result<()> {
	let entries = daemon_client::list_registry_entries()?;
	if entries.is_empty() {
		writeln!(stdout, "<empty>")?;
		return Ok(());
	}
	for entry in entries {
		writeln!(
			stdout,
			"{} pid={} endpoint={}",
			entry.workspace_root, entry.pid, entry.endpoint
		)?;
		for root in entry.workspace_roots {
			writeln!(stdout, "  root: {root}")?;
		}
	}
	Ok(())
}

fn daemon_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
	if roots.is_empty() {
		vec![PathBuf::from(".")]
	} else {
		roots.to_vec()
	}
}

fn daemon_config(args: &DaemonRootArgs) -> anyhow::Result<DaemonWorkspaceConfig> {
	daemon::daemon_workspace_config(
		daemon_roots(&args.workspace_roots),
		args.project.clone(),
		args.cache.clone(),
		Some(live_refresh_label(args)),
	)
}

fn live_refresh_label(args: &DaemonRootArgs) -> String {
	match args.live_refresh {
		crate::args::LiveRefresh::OnDemand => "on-demand",
		crate::args::LiveRefresh::Auto => "auto",
	}
	.to_string()
}

fn root_label(roots: &[PathBuf]) -> String {
	roots
		.iter()
		.map(|root| root.display().to_string())
		.collect::<Vec<_>>()
		.join(";")
}
