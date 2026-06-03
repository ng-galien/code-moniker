use std::io::Write;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use code_moniker_workspace::live::{
	LiveWorkspaceWatcher, WorkspaceLiveEvent, WorkspaceLiveRefreshPlan,
};
use code_moniker_workspace::registry::LocalWorkspaceOptions;
use code_moniker_workspace::snapshot::{WorkspaceRequest, WorkspaceTransition};
use tracing::{error, info, warn};

use crate::args::McpArgs;
use crate::mcp::McpContext;
use crate::session::SessionOptions;
use crate::workspace_index::SharedWorkspaceIndex;
use crate::{DEFAULT_SCHEME, Exit, mcp};

pub(crate) fn run<W1: Write, W2: Write>(args: &McpArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	match run_inner(args, stdout, stderr) {
		Ok(()) => Exit::Match,
		Err(error) => {
			let _ = writeln!(stderr, "code-moniker: {error:#}");
			Exit::UsageError
		}
	}
}

fn run_inner<W1: Write, W2: Write>(
	args: &McpArgs,
	_stdout: &mut W1,
	_stderr: &mut W2,
) -> anyhow::Result<()> {
	init_logging();
	let scheme = args.scheme.as_deref().unwrap_or(DEFAULT_SCHEME).to_string();
	let opts = SessionOptions {
		paths: args.paths.clone(),
		project: args.project.clone(),
		cache_dir: args.cache.clone(),
	};
	let index = SharedWorkspaceIndex::new(None);
	let runtime = tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.thread_name("code-moniker-mcp")
		.build()?;
	runtime.block_on(run_server(opts, scheme, args.port, index))
}

async fn run_server(
	opts: SessionOptions,
	scheme: String,
	port: u16,
	index: SharedWorkspaceIndex,
) -> anyhow::Result<()> {
	let load_opts = opts.clone();
	let load_index = index.clone();
	tokio::task::spawn_blocking(move || load_workspace_snapshots(load_opts, load_index));
	let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
	let addr = listener.local_addr()?;
	let router = mcp::router(McpContext::new(opts.clone(), scheme, index));
	info!(
		event = "http_transport_ready",
		endpoint = %format!("http://{addr}/mcp"),
		paths = %path_list(&opts),
		"mcp http transport ready"
	);
	axum::serve(listener, router).await?;
	Ok(())
}

fn load_workspace_snapshots(opts: SessionOptions, index: SharedWorkspaceIndex) {
	if let Err(error) = run_live_workspace(&opts, &index) {
		error!(event = "workspace_load_failed", error = %format!("{error:#}"), "workspace load failed");
	}
}

#[cfg(test)]
fn load_workspace_snapshots_inner(
	opts: &SessionOptions,
	index: &SharedWorkspaceIndex,
) -> anyhow::Result<()> {
	let _ = load_initial_workspace(opts, index)?;
	Ok(())
}

fn run_live_workspace(opts: &SessionOptions, index: &SharedWorkspaceIndex) -> anyhow::Result<()> {
	let mut registry = load_initial_workspace(opts, index)?;
	let (tx, rx) = mpsc::channel();
	let mut watcher = start_live_watcher(&registry, tx.clone())?;
	for event in rx {
		if handle_live_event(opts, index, &mut registry, event) {
			watcher = start_live_watcher(&registry, tx.clone())?;
		}
	}
	drop(watcher);
	Ok(())
}

fn start_live_watcher(
	registry: &code_moniker_workspace::LocalWorkspaceRegistry,
	tx: mpsc::Sender<WorkspaceLiveEvent>,
) -> anyhow::Result<LiveWorkspaceWatcher> {
	let watcher = LiveWorkspaceWatcher::start(registry.watch_roots(), move |event| {
		info!(event = "workspace_live_event", kind = ?event, "workspace live event");
		let _ = tx.send(event);
	})?;
	if let Some(status) = watcher.status() {
		info!(
			event = "workspace_live_ready",
			status, "workspace live watcher ready"
		);
	}
	Ok(watcher)
}

fn load_initial_workspace(
	opts: &SessionOptions,
	index: &SharedWorkspaceIndex,
) -> anyhow::Result<code_moniker_workspace::LocalWorkspaceRegistry> {
	let started = Instant::now();
	info!(
		event = "workspace_phase_started",
		phase = "index",
		paths = %path_list(opts),
		"workspace phase started"
	);
	let mut registry = code_moniker_workspace::LocalWorkspaceRegistry::local(
		LocalWorkspaceOptions::new(opts.paths.clone(), opts.project.clone())
			.with_cache_dir(opts.cache_dir.clone()),
	);
	info!(
		event = "workspace_registry_ready",
		paths = %path_list(opts),
		"workspace registry ready"
	);
	match registry
		.commands()
		.load_index(WorkspaceRequest::new("mcp-index"))
	{
		WorkspaceTransition::Ready { .. } => {
			let snapshot = registry
				.queries()
				.snapshot_arc()
				.ok_or_else(|| anyhow::anyhow!("workspace index snapshot is unavailable"))?;
			log_snapshot_ready("index", started.elapsed(), &snapshot);
			index.publish(registry.queries().snapshot_arc());
			if let Err(error) = index.reload_notes(&opts.paths) {
				warn!(event = "notes_reload_failed", error = %error, "notes reload failed");
			}
		}
		WorkspaceTransition::Failed { failure, .. } => anyhow::bail!(failure.message),
	}
	let started = Instant::now();
	info!(
		event = "workspace_phase_started",
		phase = "linkage",
		paths = %path_list(opts),
		"workspace phase started"
	);
	match registry
		.commands()
		.resolve_linkage(WorkspaceRequest::new("mcp-linkage"))
	{
		WorkspaceTransition::Ready { .. } => {
			let snapshot = registry
				.queries()
				.snapshot_arc()
				.ok_or_else(|| anyhow::anyhow!("workspace linkage snapshot is unavailable"))?;
			log_snapshot_ready("linkage", started.elapsed(), &snapshot);
			index.publish(registry.queries().snapshot_arc());
			if let Err(error) = index.reload_notes(&opts.paths) {
				warn!(event = "notes_reload_failed", error = %error, "notes reload failed");
			}
			Ok(registry)
		}
		WorkspaceTransition::Failed { failure, .. } => anyhow::bail!(failure.message),
	}
}

fn handle_live_event(
	opts: &SessionOptions,
	index: &SharedWorkspaceIndex,
	registry: &mut code_moniker_workspace::LocalWorkspaceRegistry,
	event: WorkspaceLiveEvent,
) -> bool {
	let plan = WorkspaceLiveRefreshPlan::from_event(event);
	let mut replace_watcher = false;
	let needs_rescan = plan.requires_rescan()
		|| (!plan.source_paths().is_empty()
			&& refresh_live_paths(index, registry, plan.source_paths().to_vec()));
	if needs_rescan {
		refresh_live_workspace(index, registry);
		replace_watcher = true;
	}
	if plan.includes_git_base() {
		refresh_live_changes(index, registry);
	}
	if plan.includes_notes() {
		reload_live_notes(opts, index);
	}
	replace_watcher
}

fn refresh_live_paths(
	index: &SharedWorkspaceIndex,
	registry: &mut code_moniker_workspace::LocalWorkspaceRegistry,
	paths: Vec<std::path::PathBuf>,
) -> bool {
	let started = Instant::now();
	match registry
		.commands()
		.refresh_paths(WorkspaceRequest::new("mcp-live-paths"), paths)
	{
		WorkspaceTransition::Ready { .. } => {
			if let Some(snapshot) = registry.queries().snapshot_arc() {
				log_snapshot_ready("live-paths", started.elapsed(), &snapshot);
				index.publish(Some(snapshot));
			}
			false
		}
		WorkspaceTransition::Failed { failure, .. } => {
			warn!(event = "workspace_live_failed", error = %failure.message, "workspace live refresh failed");
			true
		}
	}
}

fn refresh_live_workspace(
	index: &SharedWorkspaceIndex,
	registry: &mut code_moniker_workspace::LocalWorkspaceRegistry,
) {
	match registry
		.commands()
		.refresh(WorkspaceRequest::new("mcp-live-rescan"))
	{
		WorkspaceTransition::Ready { .. } => {
			index.publish(registry.queries().snapshot_arc());
		}
		WorkspaceTransition::Failed { failure, .. } => {
			warn!(event = "workspace_live_rescan_failed", error = %failure.message, "workspace live rescan failed");
		}
	}
}

fn refresh_live_changes(
	index: &SharedWorkspaceIndex,
	registry: &mut code_moniker_workspace::LocalWorkspaceRegistry,
) {
	match registry
		.commands()
		.refresh_changes(WorkspaceRequest::new("mcp-live-git-base"))
	{
		WorkspaceTransition::Ready { .. } => {
			index.publish(registry.queries().snapshot_arc());
		}
		WorkspaceTransition::Failed { failure, .. } => {
			warn!(event = "workspace_live_changes_failed", error = %failure.message, "workspace live changes refresh failed");
		}
	}
}

fn reload_live_notes(opts: &SessionOptions, index: &SharedWorkspaceIndex) {
	if let Err(error) = index.reload_notes(&opts.paths) {
		warn!(event = "notes_reload_failed", error = %error, "notes reload failed");
	}
}

fn log_snapshot_ready(
	phase: &str,
	elapsed: Duration,
	snapshot: &code_moniker_workspace::snapshot::WorkspaceSnapshot,
) {
	info!(
		event = "workspace_phase_ready",
		phase,
		elapsed_ms = elapsed.as_millis(),
		files = snapshot.index.sources.len(),
		symbols = snapshot.index.symbols.len(),
		refs = snapshot.index.references.len(),
		resolved_refs = snapshot.linkage.resolved_refs,
		unresolved_refs = snapshot.linkage.unresolved_refs,
		"workspace phase ready"
	);
}

fn init_logging() {
	let _ = tracing_subscriber::fmt()
		.with_writer(std::io::stderr)
		.with_target(false)
		.with_level(true)
		.compact()
		.try_init();
}

fn path_list(opts: &SessionOptions) -> String {
	opts.paths
		.iter()
		.map(|path| path.display().to_string())
		.collect::<Vec<_>>()
		.join(",")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn workspace_load_does_not_fail_on_malformed_notes_file() {
		let temp = tempfile::tempdir().expect("tempdir");
		std::fs::create_dir_all(temp.path().join("src/main/java")).expect("mkdir source");
		std::fs::write(temp.path().join("src/main/java/App.java"), "class App {}\n")
			.expect("write source");
		std::fs::create_dir_all(temp.path().join(".code-moniker")).expect("mkdir notes");
		std::fs::write(temp.path().join(".code-moniker/notes.toml"), "[[notes]\n")
			.expect("write malformed notes");
		let opts = SessionOptions {
			paths: vec![temp.path().to_path_buf()],
			project: None,
			cache_dir: None,
		};
		let index = SharedWorkspaceIndex::new(None);

		load_workspace_snapshots_inner(&opts, &index).expect("workspace should still load");

		assert!(index.index_snapshot().is_ok());
		assert!(index.notes_snapshot().is_ok());
	}
}
