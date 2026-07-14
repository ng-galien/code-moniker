use std::io::Write;
use std::path::PathBuf;

use code_moniker_daemon as daemon;
use code_moniker_daemon_client as daemon_client;
use code_moniker_query::{
	DaemonRegistryEntry, DaemonRegistryState, DaemonWorkspaceConfig, Query, QueryRequest,
	QueryResult, pid_is_alive, remove_registry_entry_if_own,
};

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
	let Some(entry) = daemon_client::read_registry_entry(&config)? else {
		let conflicts = overlapping_daemons(&config, None)?;
		if conflicts.is_empty() {
			anyhow::bail!(
				"no daemon registered for {}",
				daemon_client::workspace_label(&daemon_client::config_roots(&config))
			);
		}
		anyhow::bail!(
			"no daemon registered for {}; overlapping daemon roots: {}",
			daemon_client::workspace_label(&daemon_client::config_roots(&config)),
			format_daemon_roots(&conflicts)
		);
	};
	let registry_path = daemon_client::registry_path_for_config(&config)?;
	if !pid_is_alive(entry.pid) {
		remove_registry_entry_if_own(&registry_path, &entry);
		writeln!(
			stdout,
			"dead process: pid {} for {}; stale registry entry removed",
			entry.pid, entry.workspace_root
		)?;
		return Ok(());
	}
	if entry.state == DaemonRegistryState::Indexing {
		writeln!(stdout, "workspace: {}", entry.workspace_root)?;
		writeln!(stdout, "endpoint: {}", entry.endpoint)?;
		writeln!(stdout, "pid: {}", entry.pid)?;
		writeln!(stdout, "state: indexing")?;
		write_overlap_warning(stdout, &config, Some(&entry))?;
		return Ok(());
	}
	let client = match daemon_client::DaemonClient::connect_config(config.clone()) {
		Ok(client) => client,
		Err(error) => {
			writeln!(
				stdout,
				"stale registry: pid {} is alive but {} is unavailable ({error:#})",
				entry.pid, entry.endpoint
			)?;
			write_overlap_warning(stdout, &config, Some(&entry))?;
			return Ok(());
		}
	};
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
	writeln!(stdout, "pid: {}", entry.pid)?;
	if let Some(live_refresh) = &entry.live_refresh {
		writeln!(stdout, "live_refresh: {live_refresh}")?;
	}
	writeln!(stdout, "state: ready")?;
	write_overlap_warning(stdout, client.config(), Some(&entry))?;
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
			"{} pid={} endpoint={} state={}",
			entry.workspace_root,
			entry.pid,
			entry.endpoint,
			registry_state_label(&entry.state)
		)?;
		for root in entry.workspace_roots {
			writeln!(stdout, "  root: {root}")?;
		}
	}
	Ok(())
}

fn registry_state_label(state: &DaemonRegistryState) -> &'static str {
	match state {
		DaemonRegistryState::Indexing => "indexing",
		DaemonRegistryState::Ready => "ready",
	}
}

fn write_overlap_warning<W: Write>(
	stdout: &mut W,
	config: &DaemonWorkspaceConfig,
	current: Option<&DaemonRegistryEntry>,
) -> anyhow::Result<()> {
	let conflicts = overlapping_daemons(config, current)?;
	if !conflicts.is_empty() {
		writeln!(
			stdout,
			"warning: overlapping daemon roots: {}",
			format_daemon_roots(&conflicts)
		)?;
	}
	Ok(())
}

fn overlapping_daemons(
	config: &DaemonWorkspaceConfig,
	current: Option<&DaemonRegistryEntry>,
) -> anyhow::Result<Vec<DaemonRegistryEntry>> {
	let roots = daemon_client::config_roots(config);
	Ok(daemon_client::list_registry_entries()?
		.into_iter()
		.filter(|entry| current.is_none_or(|current| current.token != entry.token))
		.filter(|entry| {
			entry.workspace_roots.iter().any(|entry_root| {
				roots.iter().any(|root| {
					root.starts_with(entry_root) || PathBuf::from(entry_root).starts_with(root)
				})
			})
		})
		.collect())
}

fn format_daemon_roots(entries: &[DaemonRegistryEntry]) -> String {
	entries
		.iter()
		.map(|entry| format!("{} (pid {})", entry.workspace_root, entry.pid))
		.collect::<Vec<_>>()
		.join(", ")
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
