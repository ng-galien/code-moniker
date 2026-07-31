use std::io::Write;
use std::path::PathBuf;

use code_moniker_daemon as daemon;
use code_moniker_daemon_client as daemon_client;
use code_moniker_query::{
	DaemonRegistryEntry, DaemonWorkspaceConfig, PROTOCOL_VERSION, Query, QueryRequest, QueryResult,
	pid_is_alive, remove_registry_entry_if_own,
};

use crate::Exit;
use crate::args::{
	DaemonArgs, DaemonCommand, DaemonRootArgs, DaemonStartArgs, DaemonTargetArgs, LiveRefresh,
};

pub(crate) fn run_daemon<W1: Write, W2: Write>(
	args: &DaemonArgs,
	stdout: &mut W1,
	stderr: &mut W2,
) -> Exit {
	let result = match &args.command {
		DaemonCommand::Start(args) => daemon_start(args),
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

fn daemon_start(args: &DaemonStartArgs) -> anyhow::Result<()> {
	let config = daemon_config(&args.root)?;
	daemon::serve_foreground_config_supervised(config, args.supervisor_pid, args.supervisor_fd)
}

fn daemon_status<W: Write>(args: &DaemonTargetArgs, stdout: &mut W) -> anyhow::Result<()> {
	let (config, entry) = resolve_daemon_target(args)?;
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
	let client = match connect_daemon_target(args, config.clone()) {
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
	writeln!(stdout, "process: serving")?;
	write_overlap_warning(stdout, client.config(), Some(&entry))?;
	writeln!(stdout, "protocol: {}", handshake.protocol_version)?;
	writeln!(stdout, "daemon: {}", handshake.daemon_version)?;
	writeln!(
		stdout,
		"build: {} {}",
		handshake.build.version, handshake.build.fingerprint
	)?;
	writeln!(
		stdout,
		"queries: {}",
		handshake.capabilities.queries.join(", ")
	)?;
	if handshake.protocol_version != PROTOCOL_VERSION {
		writeln!(
			stdout,
			"compatibility: incompatible (client protocol {PROTOCOL_VERSION})"
		)?;
		return Ok(());
	}
	let client_build = code_moniker_query::current_build_identity(env!("CARGO_PKG_VERSION"))?;
	if handshake.build != client_build {
		writeln!(
			stdout,
			"compatibility: incompatible (client build {} {})",
			client_build.version, client_build.fingerprint
		)?;
		return Ok(());
	}
	let response = client.query(QueryRequest::new(Query::WorkspaceStatus))?;
	if let QueryResult::WorkspaceStatus(status) = response.result {
		writeln!(stdout, "state: {}", status.phase)?;
		if let Some(failure) = status.failure {
			writeln!(stdout, "failure: {}", failure.message)?;
		}
		if let Some(generation) = status.generation {
			writeln!(stdout, "generation: {}", generation.0)?;
		}
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

fn daemon_stop<W: Write>(args: &DaemonTargetArgs, stdout: &mut W) -> anyhow::Result<()> {
	let client = if let Some(endpoint) = args.daemon.as_deref() {
		daemon_client::DaemonClient::connect_endpoint(endpoint)?
	} else {
		daemon_client::DaemonClient::connect_config(daemon_target_config(args)?)?
	};
	client.shutdown()?;
	writeln!(stdout, "stopped: {}", root_label(client.roots()))?;
	Ok(())
}

fn resolve_daemon_target(
	args: &DaemonTargetArgs,
) -> anyhow::Result<(DaemonWorkspaceConfig, DaemonRegistryEntry)> {
	if let Some(endpoint) = args.daemon.as_deref() {
		let entry = daemon_client::registry_entry_for_endpoint(endpoint)?;
		return Ok((registry_entry_config(&entry), entry));
	}
	let config = daemon_target_config(args)?;
	let Some(entry) = daemon_client::read_registry_entry(&config)? else {
		let conflicts = overlapping_daemons(&config, None)?;
		if conflicts.is_empty() {
			return Err(daemon_client::no_daemon_registered_error(&config));
		}
		anyhow::bail!(
			"no daemon registered for {}; overlapping daemon roots: {}",
			daemon_client::workspace_label(&daemon_client::config_roots(&config)),
			format_daemon_roots(&conflicts)
		);
	};
	Ok((config, entry))
}

fn connect_daemon_target(
	args: &DaemonTargetArgs,
	config: DaemonWorkspaceConfig,
) -> anyhow::Result<daemon_client::DaemonClient> {
	if let Some(endpoint) = args.daemon.as_deref() {
		daemon_client::DaemonClient::connect_endpoint(endpoint)
	} else {
		daemon_client::DaemonClient::connect_config(config)
	}
}

fn registry_entry_config(entry: &DaemonRegistryEntry) -> DaemonWorkspaceConfig {
	let entry = entry.clone();
	DaemonWorkspaceConfig {
		roots: entry.workspace_roots,
		project: entry.project,
		cache_dir: entry.cache_dir,
		live_refresh: entry.live_refresh,
	}
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
		crate::args::cache_dir_with_env(&args.cache),
		Some(live_refresh_label(args.live_refresh)),
	)
}

fn daemon_target_config(args: &DaemonTargetArgs) -> anyhow::Result<DaemonWorkspaceConfig> {
	daemon::daemon_workspace_config(
		daemon_roots(&args.workspace_roots),
		args.project.clone(),
		crate::args::cache_dir_with_env(&args.cache),
		Some(live_refresh_label(args.live_refresh)),
	)
}

fn live_refresh_label(policy: LiveRefresh) -> String {
	match policy {
		LiveRefresh::OnDemand => "on-demand",
		LiveRefresh::Auto => "auto",
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
