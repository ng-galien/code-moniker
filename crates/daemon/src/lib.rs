// code-moniker: ignore-file[smell-low-cohesion-module, smell-clone-reflex]
// Daemon bootstrap clones config and handles into independently owned runtime services.
#![cfg(unix)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, TryLockError, mpsc};
use std::time::{SystemTime, UNIX_EPOCH};

use code_moniker_check::{
	CheckRequest, CheckSkipReason, CheckSummary, CompiledRuleSpec, DefaultRulesSelection,
	RuleReport, RuleSetRequest, RuleSeverity, Violation,
};
use code_moniker_core::core::shape::Shape;
use code_moniker_core::lang::Lang;
use code_moniker_query::{
	CapabilitySet, CheckSummaryDto, Command, CommandRequest, CommandResponse, Consistency,
	CountDto, DaemonRpcServer, DaemonWorkspaceConfig, FailedRuleDto, FileErrorDto,
	HandshakeResponse, NoteDto, NoteResolutionDto, NotesAction, NotesQuery, NotesResult, Page,
	ProtocolRequest, ProtocolResponse, Query, QueryCursor, QueryError, QueryRequest, QueryResponse,
	QueryResult, RuleDto, RuleReportDto, RulesCheckResult, RulesCheckRootResult, RulesListResult,
	SourceLine, SourceSnippet, SymbolDetailResult, SymbolDto, SymbolInsightsResult,
	SymbolListResult, SymbolSearchQuery, SymbolUsagesQuery, SymbolUsagesResult, TreeChildrenQuery,
	TreeChildrenResult, TreeNode, TreeNodeKind, UsageDirection, UsageDto, UsageSummaryDto,
	ViewReadQuery, ViolationDto, WorkspaceEventDto, WorkspaceEventKind, WorkspaceGeneration,
	WorkspaceRootStatus, WorkspaceStatus,
};
use code_moniker_workspace::glob::FilePathFilter;
use code_moniker_workspace::live::{
	LiveWorkspaceWatcher, WorkspaceLiveEvent, WorkspaceLiveRefreshPlan,
};
use code_moniker_workspace::notes::{
	Note, NoteAuthor, NoteChanges, NoteId, NoteKind, NoteResolution, NoteStatus, NotesDocument,
	ResolvedNote, WorkspaceNotes, resolve_notes,
};
use code_moniker_workspace::registry::{LocalWorkspaceOptions, LocalWorkspaceRegistry};
use code_moniker_workspace::snapshot::{
	ReferenceId, ReferenceRecord, SourceFileRecord, SourceId, SymbolId, SymbolRecord,
	WorkspaceRequest, WorkspaceSnapshot, WorkspaceTransition, WorkspaceView,
};
use jsonrpsee::core::{SubscriptionResult, async_trait};
use jsonrpsee::server::{PendingSubscriptionSink, Server};
use jsonrpsee::types::ErrorObjectOwned;

const DEFAULT_SCHEME: &str = "code+moniker://";

use helpers::*;

pub mod views;

pub use code_moniker_query::{
	DaemonRegistryEntry, canonical_workspace_config, canonical_workspace_root,
	canonical_workspace_roots, config_from_roots, config_roots, daemon_workspace_config,
	list_registry_entries, registry_dir, registry_path_for_config, registry_path_for_root,
	registry_path_for_roots, workspace_label, write_registry_entry,
};

pub fn serve_foreground<I, P>(roots: I) -> anyhow::Result<()>
where
	I: IntoIterator<Item = P>,
	P: AsRef<Path>,
{
	serve_foreground_config(config_from_roots(roots)?)
}

pub fn serve_foreground_config(config: DaemonWorkspaceConfig) -> anyhow::Result<()> {
	let runtime = tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.thread_name("code-moniker-daemon")
		.build()?;
	runtime.block_on(serve_async(config))
}

async fn serve_async(config: DaemonWorkspaceConfig) -> anyhow::Result<()> {
	let config = canonical_workspace_config(config)?;
	let registry_path = registry_path_for_config(&config)?;
	let registry_project = config.project.clone();
	let registry_cache_dir = config.cache_dir.clone();
	let registry_live_refresh = config.live_refresh.clone();

	let (events, _) = tokio::sync::broadcast::channel(EVENT_BUFFER);
	let daemon = WorkspaceDaemon::with_events(config.clone(), events.clone())?;
	let workspace_root = workspace_label(&daemon.roots);
	let workspace_roots = root_labels(&daemon.roots);
	let shutdown = Arc::new(tokio::sync::Notify::new());
	let daemon = Arc::new(Mutex::new(daemon));
	let service = DaemonRpcService {
		daemon: daemon.clone(),
		roots: Arc::from(config_roots(&config)),
		events,
		shutdown: shutdown.clone(),
		handshake: HandshakeResponse {
			protocol_version: code_moniker_query::PROTOCOL_VERSION,
			daemon_version: env!("CARGO_PKG_VERSION").to_string(),
			workspace_root: workspace_root.clone(),
			workspace_roots: workspace_roots.clone(),
			capabilities: CapabilitySet::default(),
		},
	};

	let server = Server::builder().build("127.0.0.1:0").await?;
	let addr = server.local_addr()?;
	let entry = DaemonRegistryEntry {
		workspace_root,
		workspace_roots,
		project: registry_project,
		cache_dir: registry_cache_dir,
		live_refresh: registry_live_refresh,
		endpoint: addr.to_string(),
		token: generate_token()?,
		pid: std::process::id(),
	};
	reject_conflicting_daemons(&config)?;
	write_registry_entry(&config, &entry)?;

	let handle = server.start(service.into_rpc());
	let preload_daemon = daemon.clone();
	tokio::task::spawn_blocking(move || {
		let mut daemon = preload_daemon.lock().unwrap_or_else(|err| err.into_inner());
		if daemon.registry.queries().snapshot().is_none() {
			let _ = refresh_full(&mut daemon);
			let _ = restart_live_watcher(&mut daemon);
		}
	});
	tokio::select! {
		_ = shutdown.notified() => {}
		_ = handle.clone().stopped() => {}
	}
	let _ = handle.stop();
	handle.stopped().await;
	let _ = fs::remove_file(&registry_path);
	Ok(())
}

const EVENT_BUFFER: usize = 256;

struct DaemonRpcService {
	daemon: Arc<Mutex<WorkspaceDaemon>>,
	roots: Arc<[PathBuf]>,
	events: tokio::sync::broadcast::Sender<WorkspaceEventDto>,
	shutdown: Arc<tokio::sync::Notify>,
	handshake: HandshakeResponse,
}

impl DaemonRpcService {
	async fn dispatch(
		&self,
		request: ProtocolRequest,
	) -> Result<ProtocolResponse, ErrorObjectOwned> {
		let daemon = self.daemon.clone();
		let roots = self.roots.clone();
		tokio::task::spawn_blocking(move || match daemon.try_lock() {
			Ok(mut guard) => guard.handle_protocol(request),
			Err(TryLockError::WouldBlock) => workspace_loading_response(request, &roots),
			Err(TryLockError::Poisoned(err)) => {
				let mut guard = err.into_inner();
				guard.handle_protocol(request)
			}
		})
		.await
		.map_err(|err| internal_error(err.to_string()))
	}
}

#[async_trait]
impl DaemonRpcServer for DaemonRpcService {
	async fn handshake(&self, _client: String) -> Result<HandshakeResponse, ErrorObjectOwned> {
		Ok(self.handshake.clone())
	}

	async fn query(&self, request: QueryRequest) -> Result<QueryResponse, ErrorObjectOwned> {
		match self
			.dispatch(ProtocolRequest::Query(Box::new(request)))
			.await?
		{
			ProtocolResponse::Query(response) => Ok(*response),
			ProtocolResponse::Error(error) => Err(query_error(error)),
			other => Err(internal_error(format!(
				"unexpected query response: {other:?}"
			))),
		}
	}

	async fn command(&self, request: CommandRequest) -> Result<CommandResponse, ErrorObjectOwned> {
		match self.dispatch(ProtocolRequest::Command(request)).await? {
			ProtocolResponse::Command(response) => Ok(response),
			ProtocolResponse::Error(error) => Err(query_error(error)),
			other => Err(internal_error(format!(
				"unexpected command response: {other:?}"
			))),
		}
	}

	async fn shutdown(&self) -> Result<(), ErrorObjectOwned> {
		self.shutdown.notify_one();
		Ok(())
	}

	async fn subscribe_events(&self, pending: PendingSubscriptionSink) -> SubscriptionResult {
		let mut rx = self.events.subscribe();
		let sink = pending.accept().await?;
		loop {
			tokio::select! {
				_ = sink.closed() => break,
				received = rx.recv() => match received {
					Ok(event) => {
						let message = serde_json::value::to_raw_value(&event)?;
						if sink.send(message).await.is_err() {
							break;
						}
					}
					Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
					Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
				},
			}
		}
		Ok(())
	}
}

fn internal_error(message: String) -> ErrorObjectOwned {
	ErrorObjectOwned::owned(
		jsonrpsee::types::error::INTERNAL_ERROR_CODE,
		message,
		None::<()>,
	)
}

/// Maps a structured `QueryError` to a JSON-RPC error, preserving the stable
/// `code` in `data` so clients can branch on it instead of parsing the message.
fn query_error(error: QueryError) -> ErrorObjectOwned {
	let message = error.message.clone();
	ErrorObjectOwned::owned(
		jsonrpsee::types::error::INTERNAL_ERROR_CODE,
		message,
		Some(error),
	)
}

fn generate_token() -> anyhow::Result<String> {
	use std::io::Read;
	let mut file = std::fs::File::open("/dev/urandom")?;
	let mut bytes = [0u8; 16];
	file.read_exact(&mut bytes)?;
	Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub struct WorkspaceDaemon {
	roots: Vec<PathBuf>,
	config_root: PathBuf,
	registry: LocalWorkspaceRegistry,
	notes: WorkspaceNotes,
	live: DaemonLiveState,
}

#[derive(Clone, Copy)]
struct ResponseContext<'a> {
	roots: &'a [PathBuf],
	config_root: &'a Path,
	generation: Option<WorkspaceGeneration>,
}

struct RulesListFilters {
	langs: Vec<String>,
	severities: Vec<String>,
}

struct RulesListEval {
	workspace: Option<String>,
	profile: Option<String>,
	rules: Option<String>,
	filters: RulesListFilters,
	page: Page,
}

struct DaemonLiveState {
	policy: DaemonLiveRefreshPolicy,
	tx: mpsc::Sender<WorkspaceLiveEvent>,
	rx: mpsc::Receiver<WorkspaceLiveEvent>,
	watcher: Option<LiveWorkspaceWatcher>,
	events: Option<tokio::sync::broadcast::Sender<WorkspaceEventDto>>,
}

struct WorkspaceDaemonInit {
	roots: Vec<PathBuf>,
	config_root: PathBuf,
	registry: LocalWorkspaceRegistry,
	live: DaemonLiveState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DaemonLiveRefreshPolicy {
	OnDemand,
	Auto,
}

struct RulesCheckEval {
	workspace: Option<String>,
	profile: Option<String>,
	rules: Option<String>,
	files: Vec<String>,
	report: bool,
	page: Page,
}

struct UsageDtoContext<'a> {
	source_by_id: &'a BTreeMap<SourceId, &'a SourceFileRecord>,
	symbol_by_id: &'a BTreeMap<SymbolId, &'a SymbolRecord>,
	roots: &'a [PathBuf],
	selected_roots: &'a [&'a PathBuf],
	path_filter: &'a FilePathFilter,
	langs: &'a [String],
}

struct NotesResponseInput<'a> {
	snapshot: &'a WorkspaceSnapshot,
	action: NotesAction,
	notes: Vec<Note>,
	deleted: Option<Note>,
	orphan: Option<bool>,
	page: Page,
	generation: Option<WorkspaceGeneration>,
}

impl WorkspaceDaemon {
	pub fn new(roots: Vec<PathBuf>) -> anyhow::Result<Self> {
		Self::new_with_config(config_from_roots(roots)?)
	}

	pub fn new_with_config(config: DaemonWorkspaceConfig) -> anyhow::Result<Self> {
		Self::build(config, None)
	}

	fn with_events(
		config: DaemonWorkspaceConfig,
		events: tokio::sync::broadcast::Sender<WorkspaceEventDto>,
	) -> anyhow::Result<Self> {
		Self::build(config, Some(events))
	}

	fn build(
		config: DaemonWorkspaceConfig,
		events: Option<tokio::sync::broadcast::Sender<WorkspaceEventDto>>,
	) -> anyhow::Result<Self> {
		let init = WorkspaceDaemonInit::new(config)?;
		let mut daemon = Self {
			roots: init.roots,
			config_root: init.config_root,
			registry: init.registry,
			notes: WorkspaceNotes::default(),
			live: init.live,
		};
		daemon.live.events = events;
		Ok(daemon)
	}

	pub fn handle_protocol(&mut self, request: ProtocolRequest) -> ProtocolResponse {
		handle_protocol(self, request)
	}

	fn restart_live_watcher(&mut self) -> anyhow::Result<()> {
		let tx = self.live.tx.clone();
		let events = self.live.events.clone();
		let watcher = LiveWorkspaceWatcher::start(self.registry.watch_roots(), move |event| {
			if let Some(events) = &events {
				let _ = events.send(event_dto(&event));
			}
			let _ = tx.send(event);
		})?;
		self.live.watcher = Some(watcher);
		Ok(())
	}
}

fn event_dto(event: &WorkspaceLiveEvent) -> WorkspaceEventDto {
	let kind = match event {
		WorkspaceLiveEvent::Notes => WorkspaceEventKind::Notes,
		WorkspaceLiveEvent::GitBaseChanged => WorkspaceEventKind::GitBase,
		_ => WorkspaceEventKind::Stale,
	};
	WorkspaceEventDto {
		kind,
		generation: None,
		stale_summary: None,
	}
}

impl WorkspaceDaemonInit {
	fn new(config: DaemonWorkspaceConfig) -> anyhow::Result<Self> {
		let config = canonical_workspace_config(config)?;
		let roots = config_roots(&config);
		Ok(Self {
			config_root: rules_config_root(&roots)?,
			registry: daemon_registry(&config, &roots),
			live: DaemonLiveState::new(DaemonLiveRefreshPolicy::parse(
				config.live_refresh.as_deref(),
			)?),
			roots,
		})
	}
}

impl DaemonLiveState {
	fn new(policy: DaemonLiveRefreshPolicy) -> Self {
		let (tx, rx) = mpsc::channel();
		Self {
			policy,
			tx,
			rx,
			watcher: None,
			events: None,
		}
	}
}

impl DaemonLiveRefreshPolicy {
	fn parse(value: Option<&str>) -> anyhow::Result<Self> {
		match value.unwrap_or("on-demand") {
			"on-demand" => Ok(Self::OnDemand),
			"auto" => Ok(Self::Auto),
			other => anyhow::bail!("unknown daemon live refresh policy `{other}`"),
		}
	}
}

fn daemon_registry(config: &DaemonWorkspaceConfig, roots: &[PathBuf]) -> LocalWorkspaceRegistry {
	LocalWorkspaceRegistry::local(
		LocalWorkspaceOptions::new(roots.to_vec(), config.project.clone())
			.with_cache_dir(config.cache_dir.as_ref().map(PathBuf::from)),
	)
}

fn handle_protocol(daemon: &mut WorkspaceDaemon, request: ProtocolRequest) -> ProtocolResponse {
	match request {
		ProtocolRequest::Query(request) => match handle_query(daemon, *request) {
			Ok(response) => ProtocolResponse::Query(Box::new(response)),
			Err(error) => ProtocolResponse::Error(error),
		},
		ProtocolRequest::Command(request) => match handle_command(daemon, request) {
			Ok(response) => ProtocolResponse::Command(response),
			Err(error) => ProtocolResponse::Error(error),
		},
	}
}

fn handle_command(
	daemon: &mut WorkspaceDaemon,
	request: CommandRequest,
) -> Result<CommandResponse, QueryError> {
	drain_live_events(daemon)?;
	match request.command {
		Command::WorkspaceRefresh => {
			refresh_full(daemon)?;
			restart_live_watcher(daemon)?;
			let status = workspace_status_result(&daemon.roots, &daemon.registry);
			Ok(CommandResponse {
				generation: generation(&daemon.registry),
				message: "workspace refreshed".to_string(),
				status: Some(status),
			})
		}
	}
}

fn handle_query(
	daemon: &mut WorkspaceDaemon,
	request: QueryRequest,
) -> Result<QueryResponse, QueryError> {
	drain_live_events(daemon)?;
	if matches!(&request.query, Query::WorkspaceStatus) {
		return workspace_status(&daemon.roots, &daemon.registry);
	}
	if request.consistency == Consistency::RefreshIfStale
		&& daemon.registry.queries().staleness().is_stale()
	{
		refresh_stale(daemon)?;
	}
	if daemon.registry.queries().snapshot().is_none() {
		return Err(QueryError::new(
			"workspace_loading",
			"workspace snapshot is still loading; retry after workspace.status reports phase ready",
		));
	}
	if request.consistency == Consistency::Current
		&& daemon.registry.queries().staleness().is_stale()
	{
		return Err(QueryError::new(
			"workspace_stale",
			"workspace is stale; request consistency refresh-if-stale or stale-ok",
		));
	}
	let snapshot = snapshot(&daemon.registry)?.clone();
	let current_generation = generation(&daemon.registry);
	let response = ResponseContext {
		roots: &daemon.roots,
		config_root: &daemon.config_root,
		generation: current_generation,
	};
	match request.query {
		Query::WorkspaceStatus => unreachable!("workspace status handled before snapshot load"),
		Query::TreeChildren(query) => tree_children_response(
			&snapshot,
			&daemon.roots,
			query,
			request.page,
			current_generation,
		),
		Query::SymbolSearch(query) => symbol_search_response(
			&snapshot,
			&daemon.roots,
			query,
			request.page,
			current_generation,
		),
		Query::SymbolInsights(query) => {
			symbol_insights_response(&snapshot, &daemon.roots, query, current_generation)
		}
		Query::SymbolDetail(query) => symbol_detail_response(
			&snapshot,
			&daemon.roots,
			query.workspace.as_deref(),
			&query.uri,
			query.context_lines,
			current_generation,
		),
		Query::SymbolUsages(query) => symbol_usages_response(
			&snapshot,
			&daemon.roots,
			query,
			request.page,
			current_generation,
		),
		Query::ViewRead(query) => {
			view_read_response(&snapshot, &daemon.roots, query, current_generation)
		}
		Query::RulesList(query) => rules_list_response(
			&snapshot,
			response,
			RulesListEval {
				workspace: query.workspace,
				profile: query.profile,
				rules: query.rules,
				filters: RulesListFilters {
					langs: query.lang,
					severities: query.severity,
				},
				page: request.page,
			},
		),
		Query::RulesCheck(query) => rules_check_response(
			response,
			RulesCheckEval {
				workspace: query.workspace,
				profile: query.profile,
				rules: query.rules,
				files: query.file,
				report: query.report,
				page: request.page,
			},
		),
		Query::Notes(query) => {
			notes_response(daemon, &snapshot, query, request.page, current_generation)
		}
	}
}

fn workspace_loading_response(request: ProtocolRequest, roots: &[PathBuf]) -> ProtocolResponse {
	match request {
		ProtocolRequest::Query(request) if matches!(&request.query, Query::WorkspaceStatus) => {
			ProtocolResponse::Query(Box::new(workspace_status_loading(roots)))
		}
		_ => ProtocolResponse::Error(QueryError::new(
			"workspace_loading",
			"workspace daemon is busy loading; retry after workspace.status reports phase ready",
		)),
	}
}

fn reject_conflicting_daemons(config: &DaemonWorkspaceConfig) -> anyhow::Result<()> {
	let own_path = registry_path_for_config(config)?;
	for (path, entry) in code_moniker_query::list_registry_files()? {
		if path == own_path {
			continue;
		}
		let shares_root = entry
			.workspace_roots
			.iter()
			.any(|root| config.roots.contains(root));
		if !shares_root {
			continue;
		}
		if code_moniker_query::pid_is_alive(entry.pid) {
			anyhow::bail!(
				"a daemon already serves {} (pid {}, endpoint {}); stop it before starting another",
				entry.workspace_root,
				entry.pid,
				entry.endpoint
			);
		}
		let _ = fs::remove_file(&path);
	}
	Ok(())
}

fn drain_live_events(daemon: &mut WorkspaceDaemon) -> Result<(), QueryError> {
	while let Ok(event) = daemon.live.rx.try_recv() {
		apply_live_event(daemon, event)?;
	}
	if daemon.live.policy == DaemonLiveRefreshPolicy::Auto
		&& daemon.registry.queries().staleness().is_stale()
	{
		refresh_stale(daemon)?;
	}
	Ok(())
}

fn apply_live_event(
	daemon: &mut WorkspaceDaemon,
	event: WorkspaceLiveEvent,
) -> Result<(), QueryError> {
	let plan = WorkspaceLiveRefreshPlan::from_event(event);
	if plan.is_empty() {
		return Ok(());
	}
	match daemon.live.policy {
		DaemonLiveRefreshPolicy::OnDemand => {
			daemon.registry.live_commands().mark_stale(plan);
			Ok(())
		}
		DaemonLiveRefreshPolicy::Auto => apply_live_plan(daemon, plan),
	}
}

fn apply_live_plan(
	daemon: &mut WorkspaceDaemon,
	plan: WorkspaceLiveRefreshPlan,
) -> Result<(), QueryError> {
	let live = daemon
		.registry
		.live_commands()
		.apply_plan(WorkspaceRequest::new("daemon-live-refresh"), plan);
	let replace_watcher = live.replace_watcher();
	workspace_transition_result(live.transition())?;
	if replace_watcher {
		restart_live_watcher(daemon)?;
	}
	Ok(())
}

fn refresh_full(daemon: &mut WorkspaceDaemon) -> Result<(), QueryError> {
	workspace_transition_result(
		daemon
			.registry
			.commands()
			.refresh(WorkspaceRequest::new("daemon-refresh")),
	)
}

fn refresh_stale(daemon: &mut WorkspaceDaemon) -> Result<(), QueryError> {
	let live = daemon
		.registry
		.live_commands()
		.refresh_stale(WorkspaceRequest::new("daemon-refresh-stale"));
	let replace_watcher = live.replace_watcher();
	workspace_transition_result(live.transition())?;
	if replace_watcher {
		restart_live_watcher(daemon)?;
	}
	Ok(())
}

fn workspace_transition_result(transition: WorkspaceTransition) -> Result<(), QueryError> {
	match transition {
		WorkspaceTransition::Ready { .. } => Ok(()),
		WorkspaceTransition::Failed { failure, .. } => {
			Err(QueryError::new("workspace_refresh_failed", failure.message))
		}
	}
}

fn restart_live_watcher(daemon: &mut WorkspaceDaemon) -> Result<(), QueryError> {
	daemon
		.restart_live_watcher()
		.map_err(|err| QueryError::new("live_watcher_failed", err.to_string()))
}

fn generation(registry: &LocalWorkspaceRegistry) -> Option<WorkspaceGeneration> {
	registry
		.queries()
		.snapshot()
		.map(|snapshot| WorkspaceGeneration(snapshot.generation.value()))
}

fn snapshot(registry: &LocalWorkspaceRegistry) -> Result<&WorkspaceSnapshot, QueryError> {
	registry
		.queries()
		.snapshot()
		.ok_or_else(|| QueryError::new("workspace_not_ready", "workspace snapshot is not ready"))
}

fn workspace_status(
	roots: &[PathBuf],
	registry: &LocalWorkspaceRegistry,
) -> Result<QueryResponse, QueryError> {
	let status = workspace_status_result(roots, registry);
	Ok(QueryResponse {
		generation: status.generation,
		result: QueryResult::WorkspaceStatus(status),
		next_cursor: None,
	})
}

fn workspace_status_loading(roots: &[PathBuf]) -> QueryResponse {
	let status = WorkspaceStatus {
		root: workspace_label(roots),
		phase: "loading".to_string(),
		roots: roots
			.iter()
			.map(|root| WorkspaceRootStatus {
				root: root.display().to_string(),
				generation: None,
				files: 0,
				symbols: 0,
				references: 0,
				stale: false,
				stale_summary: "loading".to_string(),
			})
			.collect(),
		generation: None,
		files: 0,
		symbols: 0,
		references: 0,
		stale: false,
		stale_summary: "loading".to_string(),
	};
	QueryResponse {
		generation: None,
		result: QueryResult::WorkspaceStatus(status),
		next_cursor: None,
	}
}

fn workspace_status_result(
	roots: &[PathBuf],
	registry: &LocalWorkspaceRegistry,
) -> WorkspaceStatus {
	let staleness = registry.queries().staleness();
	let generation = registry
		.queries()
		.snapshot()
		.map(|snapshot| WorkspaceGeneration(snapshot.generation.value()));
	let root_statuses = registry
		.queries()
		.snapshot()
		.map(|snapshot| {
			roots
				.iter()
				.map(|root| {
					root_status(
						snapshot,
						roots,
						root,
						staleness.is_stale(),
						&staleness.summary(),
					)
				})
				.collect::<Vec<_>>()
		})
		.unwrap_or_else(|| {
			roots
				.iter()
				.map(|root| WorkspaceRootStatus {
					root: root.display().to_string(),
					generation,
					files: 0,
					symbols: 0,
					references: 0,
					stale: staleness.is_stale(),
					stale_summary: staleness.summary(),
				})
				.collect()
		});
	let files = root_statuses.iter().map(|root| root.files).sum();
	let symbols = root_statuses.iter().map(|root| root.symbols).sum();
	let references = root_statuses.iter().map(|root| root.references).sum();
	WorkspaceStatus {
		root: workspace_label(roots),
		phase: if generation.is_some() {
			"ready".to_string()
		} else {
			"loading".to_string()
		},
		roots: root_statuses,
		generation,
		files,
		symbols,
		references,
		stale: staleness.is_stale(),
		stale_summary: staleness.summary(),
	}
}

fn tree_children_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: TreeChildrenQuery,
	page: Page,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, query.workspace.as_deref())?;
	let path_filter = FilePathFilter::compile(&query.path)
		.map_err(|err| QueryError::new("invalid_path_filter", err.to_string()))?;
	let plain_scope = tree_plain_scope(&query.path);
	let prefix = plain_scope.as_deref().unwrap_or_default();
	let mut map = BTreeMap::<String, TreeNode>::new();
	let mut scoped_sources = Vec::new();
	for source in &snapshot.index.sources {
		let Some(root) = source_root(roots, &selected_roots, source) else {
			continue;
		};
		if !query.lang.is_empty() && !query.lang.iter().any(|lang| lang == &source.language) {
			continue;
		}
		if !path_filter.matches(&source.rel_path) {
			continue;
		}
		scoped_sources.push((root, source));
		let exact_file_scope = plain_scope
			.as_deref()
			.is_some_and(|scope| source.rel_path == scope);
		let remainder = if exact_file_scope {
			source.rel_path.as_str()
		} else {
			source.rel_path[prefix.len()..].trim_start_matches('/')
		};
		if remainder.is_empty() {
			continue;
		}
		let parts = remainder.split('/').collect::<Vec<_>>();
		let depth = query.depth.max(1);
		let take = parts.len().min(depth);
		let row_path = if exact_file_scope || prefix.is_empty() {
			parts[..take].join("/")
		} else {
			format!(
				"{}/{}",
				prefix.trim_end_matches('/'),
				parts[..take].join("/")
			)
		};
		let kind = if take < parts.len() {
			TreeNodeKind::Directory
		} else {
			TreeNodeKind::File
		};
		let root_label = root.display().to_string();
		let entry_key = format!("{root_label}\0{row_path}");
		let entry = map.entry(entry_key).or_insert_with(|| TreeNode {
			root: root_label,
			path: row_path,
			kind,
			language: (kind == TreeNodeKind::File).then(|| source.language.clone()),
			defs: 0,
			refs: 0,
			change_count: 0,
		});
		entry.defs += snapshot
			.index
			.symbols
			.iter()
			.filter(|symbol| symbol.source == source.id && symbol.navigable)
			.count();
		entry.refs += snapshot
			.index
			.references
			.iter()
			.filter(|reference| reference.source == source.id)
			.count();
	}
	let total_files = snapshot
		.index
		.sources
		.iter()
		.filter(|source| source_root(roots, &selected_roots, source).is_some())
		.count();
	let languages = sorted_counts(
		scoped_sources
			.iter()
			.map(|(_, source)| source.language.clone()),
	);
	let prefixes = sorted_counts(
		scoped_sources
			.iter()
			.map(|(_, source)| path_prefix(&source.rel_path)),
	);
	let paged = page_rows(map.into_values().collect(), page, current_generation)?;
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::TreeChildren(TreeChildrenResult {
			root: workspace_label_from_paths(&selected_roots),
			roots: selected_roots
				.iter()
				.map(|root| root.display().to_string())
				.collect(),
			total: paged.total,
			rows: paged.items,
			total_files,
			scoped_files: scoped_sources.len(),
			languages,
			prefixes,
		}),
		next_cursor: paged.next_cursor,
	})
}

fn tree_plain_scope(paths: &[String]) -> Option<String> {
	let [path] = paths else {
		return None;
	};
	if path.contains(['*', '?']) {
		None
	} else {
		Some(normalize_tree_path(path))
	}
}

fn normalize_tree_path(path: &str) -> String {
	path.trim()
		.replace('\\', "/")
		.trim_start_matches("./")
		.trim_start_matches('/')
		.trim_end_matches('/')
		.to_string()
}

fn symbol_search_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: SymbolSearchQuery,
	page: Page,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, query.workspace.as_deref())?;
	let path_filter = FilePathFilter::compile(&query.path)
		.map_err(|err| QueryError::new("invalid_path_filter", err.to_string()))?;
	let name_filter = query
		.name
		.as_ref()
		.map(|pattern| regex::Regex::new(pattern))
		.transpose()
		.map_err(|err| QueryError::new("invalid_name_filter", err.to_string()))?;
	let source_by_id = source_by_id(snapshot);
	let matches_query = |symbol: &SymbolRecord| {
		let Some(source) = source_by_id.get(&symbol.source).copied() else {
			return false;
		};
		source_root(roots, &selected_roots, source).is_some()
			&& path_filter.matches(&source.rel_path)
			&& (query.lang.is_empty() || query.lang.iter().any(|lang| lang == &source.language))
			&& matches_kind_shape(symbol, &query)
			&& name_filter
				.as_ref()
				.is_none_or(|regex| regex.is_match(&symbol.name))
	};
	let mut rows = if let Some(text) = query.text.as_deref().filter(|text| !text.trim().is_empty())
		&& !query.include_non_navigable
	{
		let symbol_by_id = symbol_by_id(snapshot);
		WorkspaceView::new(snapshot)
			.search()
			.search_symbols_matching(text, usize::MAX, matches_query)
			.into_iter()
			.map(|hit| {
				let Some(symbol) = symbol_by_id.get(&hit.symbol).copied() else {
					return Ok(None);
				};
				let Some(source) = source_by_id.get(&symbol.source).copied() else {
					return Ok(None);
				};
				let mut row = symbol_search_dto(symbol, source, roots, hit.score, hit.reason);
				if query.include_code {
					row.source = source_snippet(source, symbol, query.context_lines)?;
				}
				Ok(Some(row))
			})
			.collect::<Result<Vec<_>, QueryError>>()?
			.into_iter()
			.flatten()
			.collect::<Vec<_>>()
	} else {
		snapshot
			.index
			.symbols
			.iter()
			.filter(|symbol| query.include_non_navigable || symbol.navigable)
			.filter(|symbol| matches_query(symbol))
			.filter_map(|symbol| {
				let source = source_by_id.get(&symbol.source).copied()?;
				Some((symbol, source))
			})
			.map(|(symbol, source)| {
				let mut row = symbol_dto(symbol, source, roots);
				if query.include_code {
					row.source = source_snippet(source, symbol, query.context_lines)?;
				}
				Ok(row)
			})
			.collect::<Result<Vec<_>, QueryError>>()?
	};
	if query
		.text
		.as_deref()
		.is_none_or(|text| text.trim().is_empty())
		|| query.include_non_navigable
	{
		rows.sort_by(|a, b| {
			a.file
				.cmp(&b.file)
				.then_with(|| a.line_range.cmp(&b.line_range))
				.then_with(|| a.uri.cmp(&b.uri))
		});
	}
	let paged = page_rows(rows, page, current_generation)?;
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::SymbolList(SymbolListResult {
			total: paged.total,
			rows: paged.items,
		}),
		next_cursor: paged.next_cursor,
	})
}

fn matches_kind_shape(symbol: &SymbolRecord, query: &SymbolSearchQuery) -> bool {
	let kind_matches = query.kind.iter().any(|kind| kind == &symbol.kind);
	let shape_matches = query
		.shape
		.iter()
		.any(|shape| Shape::for_kind(symbol.kind.as_bytes()).as_str() == shape);
	if query
		.text
		.as_deref()
		.is_some_and(|text| !text.trim().is_empty())
		&& !query.kind.is_empty()
		&& !query.shape.is_empty()
	{
		return kind_matches || shape_matches;
	}
	(query.kind.is_empty() || kind_matches) && (query.shape.is_empty() || shape_matches)
}

fn symbol_insights_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: SymbolSearchQuery,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, query.workspace.as_deref())?;
	let path_filter = FilePathFilter::compile(&query.path)
		.map_err(|err| QueryError::new("invalid_path_filter", err.to_string()))?;
	let name_filter = query
		.name
		.as_ref()
		.map(|pattern| regex::Regex::new(pattern))
		.transpose()
		.map_err(|err| QueryError::new("invalid_name_filter", err.to_string()))?;
	let source_by_id = source_by_id(snapshot);
	let scoped_sources = snapshot
		.index
		.sources
		.iter()
		.filter(|source| source_root(roots, &selected_roots, source).is_some())
		.filter(|source| path_filter.matches(&source.rel_path))
		.filter(|source| {
			query.lang.is_empty() || query.lang.iter().any(|lang| lang == &source.language)
		})
		.collect::<Vec<_>>();
	let scoped_source_ids = scoped_sources
		.iter()
		.map(|source| source.id)
		.collect::<BTreeSet<_>>();
	let scoped_symbols = snapshot
		.index
		.symbols
		.iter()
		.filter(|symbol| scoped_source_ids.contains(&symbol.source))
		.filter(|symbol| query.include_non_navigable || symbol.navigable)
		.filter(|symbol| {
			query.kind.is_empty() || query.kind.iter().any(|kind| kind == &symbol.kind)
		})
		.filter(|symbol| {
			query.shape.is_empty()
				|| query
					.shape
					.iter()
					.any(|shape| Shape::for_kind(symbol.kind.as_bytes()).as_str() == shape)
		})
		.filter(|symbol| {
			name_filter
				.as_ref()
				.is_none_or(|regex| regex.is_match(&symbol.name))
		})
		.collect::<Vec<_>>();
	let scoped_refs = snapshot
		.index
		.references
		.iter()
		.filter(|reference| scoped_source_ids.contains(&reference.source))
		.collect::<Vec<_>>();
	let mut symbol_counts = BTreeMap::<String, usize>::new();
	let mut ref_counts = BTreeMap::<String, usize>::new();
	for symbol in &scoped_symbols {
		if let Some(source) = source_by_id.get(&symbol.source) {
			*symbol_counts.entry(source.rel_path.to_owned()).or_default() += 1;
		}
	}
	for reference in &scoped_refs {
		if let Some(source) = source_by_id.get(&reference.source) {
			*ref_counts.entry(source.rel_path.to_owned()).or_default() += 1;
		}
	}
	let result = SymbolInsightsResult {
		files: scoped_sources.len(),
		symbols: scoped_symbols.len(),
		references: scoped_refs.len(),
		navigable_symbols: scoped_symbols
			.iter()
			.filter(|symbol| symbol.navigable)
			.count(),
		non_navigable_symbols: scoped_symbols
			.iter()
			.filter(|symbol| !symbol.navigable)
			.count(),
		languages: sorted_counts(
			scoped_sources
				.iter()
				.map(|source| source.language.to_owned()),
		),
		kinds: sorted_counts(scoped_symbols.iter().map(|symbol| symbol.kind.to_owned())),
		shapes: sorted_counts(
			scoped_symbols
				.iter()
				.map(|symbol| Shape::for_kind(symbol.kind.as_bytes()).as_str().to_string()),
		),
		top_files_by_symbols: count_rows(symbol_counts),
		top_files_by_refs: count_rows(ref_counts),
	};
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::SymbolInsights(result),
		next_cursor: None,
	})
}

fn symbol_detail_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	workspace: Option<&str>,
	uri: &str,
	context_lines: usize,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, workspace)?;
	let source_by_id = source_by_id(snapshot);
	let symbol = find_symbol(snapshot, uri)?;
	let source = source_by_id
		.get(&symbol.source)
		.ok_or_else(|| QueryError::new("source_not_found", "symbol source not found"))?;
	if source_root(roots, &selected_roots, source).is_none() {
		return Err(QueryError::new(
			"symbol_not_in_workspace",
			format!("symbol {uri} is not in the selected workspace"),
		));
	}
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::SymbolDetail(SymbolDetailResult {
			symbol: symbol_dto(symbol, source, roots),
			source: source_snippet(source, symbol, context_lines)?,
		}),
		next_cursor: None,
	})
}

fn symbol_usages_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: SymbolUsagesQuery,
	page: Page,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, query.workspace.as_deref())?;
	let source_by_id = source_by_id(snapshot);
	let symbol_by_id = symbol_by_id(snapshot);
	let reference_by_id = reference_by_id(snapshot);
	let path_filter = FilePathFilter::compile(&query.path)
		.map_err(|err| QueryError::new("invalid_path_filter", err.to_string()))?;
	let target = find_symbol(snapshot, &query.uri)?;
	let target_source = source_by_id
		.get(&target.source)
		.ok_or_else(|| QueryError::new("source_not_found", "target source not found"))?;
	if source_root(roots, &selected_roots, target_source).is_none() {
		return Err(QueryError::new(
			"symbol_not_in_workspace",
			format!("symbol {} is not in the selected workspace", query.uri),
		));
	}
	let mut incoming_rows = Vec::new();
	let mut outgoing_rows = Vec::new();
	let usage_context = UsageDtoContext {
		source_by_id: &source_by_id,
		symbol_by_id: &symbol_by_id,
		roots,
		selected_roots: &selected_roots,
		path_filter: &path_filter,
		langs: &query.lang,
	};
	if matches!(
		query.direction,
		UsageDirection::Incoming | UsageDirection::Both
	) {
		incoming_rows = collect_incoming_usages(
			snapshot,
			target,
			&reference_by_id,
			&symbol_by_id,
			&usage_context,
		);
	}
	if matches!(
		query.direction,
		UsageDirection::Outgoing | UsageDirection::Both
	) {
		for reference in snapshot
			.index
			.references
			.iter()
			.filter(|reference| reference.source_symbol == target.id)
		{
			if let Some(row) = usage_dto(reference, UsageDirection::Outgoing, &usage_context) {
				outgoing_rows.push(row);
			}
		}
	}
	let incoming_summary = matches!(
		query.direction,
		UsageDirection::Incoming | UsageDirection::Both
	)
	.then(|| usage_summary(&incoming_rows, true));
	let outgoing_summary = matches!(
		query.direction,
		UsageDirection::Outgoing | UsageDirection::Both
	)
	.then(|| usage_summary(&outgoing_rows, false));
	let mut rows = Vec::new();
	rows.extend(incoming_rows);
	rows.extend(outgoing_rows);
	rows.sort_by(usage_cmp_for_navigation);
	let paged = page_rows(rows, page, current_generation)?;
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::SymbolUsages(Box::new(SymbolUsagesResult {
			target: symbol_dto(target, target_source, roots),
			direction: query.direction,
			total: paged.total,
			rows: paged.items,
			incoming_summary,
			outgoing_summary,
		})),
		next_cursor: paged.next_cursor,
	})
}

fn view_read_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: ViewReadQuery,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let result = views::read(
		&query.uri,
		roots,
		query.scheme.as_deref().unwrap_or(DEFAULT_SCHEME),
		snapshot,
		query.context_lines,
		query.include_code,
	)
	.map_err(|err| QueryError::new("view_read_failed", err.to_string()))?;
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::ViewRead(result),
		next_cursor: None,
	})
}

fn collect_incoming_usages(
	snapshot: &WorkspaceSnapshot,
	target: &SymbolRecord,
	reference_by_id: &BTreeMap<ReferenceId, &ReferenceRecord>,
	symbol_by_id: &BTreeMap<SymbolId, &SymbolRecord>,
	context: &UsageDtoContext<'_>,
) -> Vec<UsageDto> {
	let mut rows = snapshot
		.linkage
		.resolved
		.iter()
		.filter(|edge| edge.target == target.id)
		.filter_map(|edge| reference_by_id.get(&edge.reference).copied())
		.filter_map(|reference| usage_dto(reference, UsageDirection::Incoming, context))
		.collect::<Vec<_>>();
	let mut seen = rows
		.iter()
		.filter_map(|row| ReferenceId::parse(&row.reference))
		.collect::<BTreeSet<_>>();
	let mut visited = BTreeSet::from([target.id]);
	collect_indirect_incoming_usages(
		snapshot,
		&target.id,
		IndirectUsageContext {
			reference_by_id,
			symbol_by_id,
			usage_context: context,
		},
		IndirectUsageState {
			depth: 0,
			visited: &mut visited,
			seen: &mut seen,
			rows: &mut rows,
		},
	);
	rows
}

struct IndirectUsageContext<'a> {
	reference_by_id: &'a BTreeMap<ReferenceId, &'a ReferenceRecord>,
	symbol_by_id: &'a BTreeMap<SymbolId, &'a SymbolRecord>,
	usage_context: &'a UsageDtoContext<'a>,
}

struct IndirectUsageState<'a> {
	depth: usize,
	visited: &'a mut BTreeSet<SymbolId>,
	seen: &'a mut BTreeSet<ReferenceId>,
	rows: &'a mut Vec<UsageDto>,
}

fn collect_indirect_incoming_usages(
	snapshot: &WorkspaceSnapshot,
	target: &SymbolId,
	context: IndirectUsageContext<'_>,
	state: IndirectUsageState<'_>,
) {
	const MAX_INDIRECT_USAGE_DEPTH: usize = 4;
	if state.depth >= MAX_INDIRECT_USAGE_DEPTH {
		return;
	}
	let aliases = snapshot
		.linkage
		.resolved
		.iter()
		.filter(|edge| &edge.target == target)
		.filter_map(|edge| context.reference_by_id.get(&edge.reference).copied())
		.filter(|reference| reference.kind == "uses_type")
		.filter_map(|reference| context.symbol_by_id.get(&reference.source_symbol))
		.filter(|symbol| symbol.kind == "type")
		.filter(|symbol| state.visited.insert(symbol.id))
		.copied()
		.collect::<Vec<_>>();
	for alias in aliases {
		collect_direct_usages_via(snapshot, alias, &context, state.seen, state.rows);
		collect_indirect_incoming_usages(
			snapshot,
			&alias.id,
			IndirectUsageContext {
				reference_by_id: context.reference_by_id,
				symbol_by_id: context.symbol_by_id,
				usage_context: context.usage_context,
			},
			IndirectUsageState {
				depth: state.depth + 1,
				visited: state.visited,
				seen: state.seen,
				rows: state.rows,
			},
		);
	}
}

fn collect_direct_usages_via(
	snapshot: &WorkspaceSnapshot,
	alias: &SymbolRecord,
	context: &IndirectUsageContext<'_>,
	seen: &mut BTreeSet<ReferenceId>,
	rows: &mut Vec<UsageDto>,
) {
	for edge in snapshot
		.linkage
		.resolved
		.iter()
		.filter(|edge| edge.target == alias.id)
	{
		let Some(reference) = context.reference_by_id.get(&edge.reference).copied() else {
			continue;
		};
		if reference.source_symbol == alias.id || !seen.insert(reference.id) {
			continue;
		}
		let Some(mut row) = usage_dto(reference, UsageDirection::Incoming, context.usage_context)
		else {
			continue;
		};
		row.via = Some(format!("{} ({})", alias.name, alias.identity));
		rows.push(row);
	}
}

fn usage_cmp_for_navigation(left: &UsageDto, right: &UsageDto) -> std::cmp::Ordering {
	usage_kind_priority(&left.kind)
		.cmp(&usage_kind_priority(&right.kind))
		.then_with(|| left.file.cmp(&right.file))
		.then_with(|| left.line_range.cmp(&right.line_range))
		.then_with(|| left.actor.cmp(&right.actor))
		.then_with(|| left.reference.cmp(&right.reference))
}

fn usage_kind_priority(kind: &str) -> u8 {
	match kind {
		"calls" | "constructs" => 10,
		"extends" | "implements" | "inherits" => 20,
		"reads" | "uses_type" | "returns_type" | "annotates" => 30,
		"imports" => 40,
		_ => 50,
	}
}

fn usage_summary(rows: &[UsageDto], shared_signal: bool) -> UsageSummaryDto {
	let mut files = BTreeSet::new();
	let mut contexts = BTreeSet::new();
	let mut prefixes = BTreeMap::<&str, usize>::new();
	let mut kinds = BTreeMap::<&str, usize>::new();
	let mut actors = BTreeMap::<&str, usize>::new();
	for row in rows {
		files.insert(row.file.as_str());
		contexts.insert(row.context.as_str());
		*prefixes.entry(row.prefix.as_str()).or_default() += 1;
		*kinds.entry(row.kind.as_str()).or_default() += 1;
		*actors.entry(row.actor.as_str()).or_default() += 1;
	}
	let top_prefixes = count_rows_borrowed(&prefixes);
	let dominant_prefix = top_prefixes
		.first()
		.map(|row| {
			format!(
				"{} ({} refs, {}%)",
				row.name,
				row.count,
				percent(row.count, rows.len())
			)
		})
		.unwrap_or_default();
	UsageSummaryDto {
		refs: rows.len(),
		files: files.len(),
		contexts: contexts.len(),
		prefixes: prefixes.len(),
		dominant_prefix,
		kinds: count_rows_borrowed(&kinds),
		top_actors: count_rows_borrowed(&actors),
		top_prefixes,
		shared_helper_signal: if shared_signal {
			shared_helper_signal(rows.len(), files.len(), contexts.len(), prefixes)
		} else {
			String::new()
		},
	}
}

fn shared_helper_signal(
	refs: usize,
	files: usize,
	contexts: usize,
	prefixes: BTreeMap<&str, usize>,
) -> String {
	if refs == 0 {
		return "unused_or_unresolved".to_string();
	}
	let prefix_count = prefixes.len();
	let dominant = count_rows_borrowed(&prefixes)
		.first()
		.map(|row| percent(row.count, refs))
		.unwrap_or(0);
	if files >= 3 && contexts >= 3 && prefix_count >= 2 {
		"shared_helper_candidate".to_string()
	} else if files <= 1 || dominant >= 80 {
		"localized_not_shared".to_string()
	} else {
		"mixed_review_needed".to_string()
	}
}

fn count_rows_borrowed(counts: &BTreeMap<&str, usize>) -> Vec<CountDto> {
	let mut rows = counts
		.iter()
		.map(|(name, count)| CountDto {
			name: (*name).to_string(),
			count: *count,
		})
		.collect::<Vec<_>>();
	rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
	rows
}

fn percent(count: usize, total: usize) -> usize {
	count
		.checked_mul(100)
		.and_then(|value| value.checked_div(total))
		.unwrap_or(0)
}

fn rules_list_response(
	snapshot: &WorkspaceSnapshot,
	response: ResponseContext<'_>,
	request: RulesListEval,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(response.roots, request.workspace.as_deref())?;
	let mut rows = Vec::new();
	for root in &selected_roots {
		let requested_langs =
			workspace_langs(snapshot, response.roots, root, &request.filters.langs);
		let rules_path = resolve_rules_path(response.config_root, request.rules.as_deref());
		let specs = RuleSetRequest::with_rules(rules_path, DEFAULT_SCHEME)
			.with_profile(request.profile.clone())
			.compiled_specs_for_langs(requested_langs)
			.map_err(|err| QueryError::new("rules_compile_failed", err.to_string()))?;
		for spec in specs {
			if !request.filters.severities.is_empty()
				&& !request
					.filters
					.severities
					.iter()
					.any(|severity| severity == spec.severity.as_str())
			{
				continue;
			}
			rows.push(rule_dto(root, spec));
		}
	}
	rows.sort_by(|a, b| {
		a.root
			.cmp(&b.root)
			.then_with(|| a.id.cmp(&b.id))
			.then_with(|| a.lang.cmp(&b.lang))
			.then_with(|| a.domain.cmp(&b.domain))
	});
	let paged = page_rows(rows, request.page, response.generation)?;
	Ok(QueryResponse {
		generation: response.generation,
		result: QueryResult::RulesList(RulesListResult {
			roots: selected_roots
				.iter()
				.map(|root| root.display().to_string())
				.collect(),
			total: paged.total,
			rows: paged.items,
		}),
		next_cursor: paged.next_cursor,
	})
}

fn rules_check_response(
	response: ResponseContext<'_>,
	request: RulesCheckEval,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(response.roots, request.workspace.as_deref())?;
	let mut roots = Vec::new();
	for root in &selected_roots {
		roots.push(run_rules_for_root(
			root,
			response.config_root,
			request.profile.clone(),
			request.rules.as_deref(),
			&request.files,
			request.report,
		)?);
	}
	let exit = aggregate_check_exit(&roots);
	let summary = aggregate_check_summary(&roots);
	let rows = rules_check_rows(&roots);
	let paged = page_rows(rows, request.page, response.generation)?;
	let mut violations = Vec::new();
	let mut errors = Vec::new();
	let mut rule_reports = Vec::new();
	let mut skip_reasons = Vec::new();
	for row in paged.items {
		match row {
			RulesCheckRow::Violation(violation) => violations.push(violation),
			RulesCheckRow::Error(error) => errors.push(error),
			RulesCheckRow::RuleReport(report) => rule_reports.push(report),
			RulesCheckRow::SkipReason(reason) => skip_reasons.push(reason),
		}
	}
	let root_summaries = roots
		.into_iter()
		.map(clear_root_payloads)
		.collect::<Vec<_>>();
	Ok(QueryResponse {
		generation: response.generation,
		result: QueryResult::RulesCheck(RulesCheckResult {
			exit,
			summary,
			roots: root_summaries,
			violations,
			errors,
			rule_reports,
			skip_reasons,
		}),
		next_cursor: paged.next_cursor,
	})
}

fn clear_root_payloads(mut root: RulesCheckRootResult) -> RulesCheckRootResult {
	root.violations.clear();
	root.errors.clear();
	root.rule_reports.clear();
	root.skip_reason = None;
	root
}

fn notes_response(
	daemon: &mut WorkspaceDaemon,
	snapshot: &WorkspaceSnapshot,
	request: NotesQuery,
	page: Page,
	generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	daemon
		.notes
		.reload(&daemon.roots)
		.map_err(|err| QueryError::new("notes_load_failed", err.to_string()))?;
	let action = request.action;
	let deleted = match action {
		NotesAction::Create => {
			let note = note_from_create(&daemon.notes.snapshot().map_err(note_error)?, &request)?;
			let id = note.id.clone();
			daemon
				.notes
				.mutate(&daemon.roots, |document| {
					document.insert(note)?;
					Ok(())
				})
				.map_err(note_error)?;
			Some(id)
		}
		NotesAction::Update => {
			if request.status.is_some() {
				return Err(QueryError::new(
					"invalid_note_update",
					"status changes require action=transition",
				));
			}
			let id = required_note_id(&request)?;
			let changes = note_changes(&request)?;
			daemon
				.notes
				.mutate(&daemon.roots, |document| {
					document.update(id, changes, current_timestamp())?;
					Ok(())
				})
				.map_err(note_error)?;
			Some(NoteId::new(id))
		}
		NotesAction::Transition => {
			let id = required_note_id(&request)?;
			let status = request
				.status
				.as_deref()
				.ok_or_else(|| QueryError::new("missing_status", "status is required"))?;
			let status = parse_note_status(status)?;
			daemon
				.notes
				.mutate(&daemon.roots, |document| {
					document.transition(id, status, current_timestamp())?;
					Ok(())
				})
				.map_err(note_error)?;
			Some(NoteId::new(id))
		}
		NotesAction::Delete => {
			let id = required_note_id(&request)?;
			let deleted = daemon
				.notes
				.mutate(&daemon.roots, |document| document.delete(id))
				.map_err(note_error)?;
			return notes_query_response(NotesResponseInput {
				snapshot,
				action,
				notes: Vec::new(),
				deleted: Some(deleted),
				orphan: None,
				page,
				generation,
			});
		}
		NotesAction::List | NotesAction::Get => None,
	};
	daemon.notes.reload(&daemon.roots).map_err(note_error)?;
	let document = daemon.notes.snapshot().map_err(note_error)?;
	let mut notes = document.notes;
	if let Some(id) = deleted {
		notes.retain(|note| note.id == id);
	}
	if action == NotesAction::Get {
		let id = required_note_id(&request)?;
		notes.retain(|note| note.id.as_str() == id);
		if notes.is_empty() {
			return Err(QueryError::new(
				"note_not_found",
				format!("note id `{id}` does not exist"),
			));
		}
	}
	if action == NotesAction::List {
		notes = filter_notes(notes, &request);
	}
	notes_query_response(NotesResponseInput {
		snapshot,
		action,
		notes,
		deleted: None,
		orphan: request.orphan,
		page,
		generation,
	})
}

fn note_changes(request: &NotesQuery) -> Result<NoteChanges, QueryError> {
	Ok(NoteChanges {
		moniker: request.moniker.clone(),
		kind: request.kind.as_deref().map(parse_note_kind).transpose()?,
		title: request.title.clone(),
		body: request.body.clone(),
	})
}

fn notes_query_response(input: NotesResponseInput<'_>) -> Result<QueryResponse, QueryError> {
	let mut resolved = resolve_notes(&input.notes, input.snapshot);
	if let Some(orphan) = input.orphan {
		resolved.retain(|note| note.resolution.is_orphan() == orphan);
	}
	let rows = resolved
		.iter()
		.map(note_dto)
		.collect::<Result<Vec<_>, _>>()?;
	let paged = page_rows(rows, input.page, input.generation)?;
	let deleted = input
		.deleted
		.as_ref()
		.map(|note| note_dto_from_note(note, input.snapshot))
		.transpose()?;
	Ok(QueryResponse {
		generation: input.generation,
		result: QueryResult::Notes(NotesResult {
			action: notes_action_label(input.action).to_string(),
			total: paged.total,
			rows: paged.items,
			deleted,
		}),
		next_cursor: paged.next_cursor,
	})
}

fn note_error(error: anyhow::Error) -> QueryError {
	QueryError::new("notes_failed", error.to_string())
}

fn note_from_create(document: &NotesDocument, request: &NotesQuery) -> Result<Note, QueryError> {
	let moniker = required_note_string(request.moniker.as_deref(), "moniker")?.to_string();
	let title = required_note_string(request.title.as_deref(), "title")?.to_string();
	let now = current_timestamp();
	let id = request
		.id
		.as_deref()
		.map(NoteId::new)
		.unwrap_or_else(|| generated_note_id(document));
	Ok(Note {
		id,
		moniker,
		kind: request
			.kind
			.as_deref()
			.map(parse_note_kind)
			.transpose()?
			.unwrap_or(NoteKind::Note),
		status: request
			.status
			.as_deref()
			.map(parse_note_status)
			.transpose()?
			.unwrap_or(NoteStatus::Pending),
		title,
		body: request.body.clone().unwrap_or_default(),
		created_by: request
			.created_by
			.as_deref()
			.map(parse_note_author)
			.transpose()?
			.unwrap_or(NoteAuthor::Agent),
		created_at: now.clone(),
		updated_at: now,
	})
}

fn generated_note_id(document: &NotesDocument) -> NoteId {
	for attempt in 0..1000_u32 {
		let nanos = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map(|duration| duration.as_nanos())
			.unwrap_or_default();
		let id = NoteId::new(format!("note_{nanos:x}_{attempt:x}"));
		if document.get(id.as_str()).is_none() {
			return id;
		}
	}
	NoteId::new("note_exhausted")
}

fn required_note_id(request: &NotesQuery) -> Result<&str, QueryError> {
	required_note_string(request.id.as_deref(), "id")
}

fn required_note_string<'a>(value: Option<&'a str>, key: &str) -> Result<&'a str, QueryError> {
	value.ok_or_else(|| QueryError::new(format!("missing_{key}"), format!("{key} is required")))
}

fn parse_note_kind(value: &str) -> Result<NoteKind, QueryError> {
	NoteKind::parse(value).map_err(|err| QueryError::new("invalid_note_kind", err.to_string()))
}

fn parse_note_status(value: &str) -> Result<NoteStatus, QueryError> {
	NoteStatus::parse(value).map_err(|err| QueryError::new("invalid_note_status", err.to_string()))
}

fn parse_note_author(value: &str) -> Result<NoteAuthor, QueryError> {
	NoteAuthor::parse(value).map_err(|err| QueryError::new("invalid_note_author", err.to_string()))
}

fn current_timestamp() -> String {
	let seconds = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|duration| duration.as_secs())
		.unwrap_or_default();
	format!("unix:{seconds}")
}

fn filter_notes(notes: Vec<Note>, request: &NotesQuery) -> Vec<Note> {
	notes
		.into_iter()
		.filter(|note| {
			request
				.moniker
				.as_ref()
				.is_none_or(|moniker| note.moniker == *moniker)
		})
		.filter(|note| request.include_done || note.status != NoteStatus::Done)
		.collect()
}

fn note_dto(note: &ResolvedNote) -> Result<NoteDto, QueryError> {
	Ok(NoteDto {
		id: note.note.id.as_str().to_string(),
		moniker: note.note.moniker.to_owned(),
		kind: note.note.kind.as_str().to_string(),
		status: note.note.status.as_str().to_string(),
		title: note.note.title.to_owned(),
		body: note.note.body.to_owned(),
		created_by: note.note.created_by.as_str().to_string(),
		updated_at: note.note.updated_at.to_owned(),
		resolution: note_resolution_dto(&note.resolution),
	})
}

fn note_dto_from_note(note: &Note, snapshot: &WorkspaceSnapshot) -> Result<NoteDto, QueryError> {
	let mut resolved = resolve_notes(std::slice::from_ref(note), snapshot);
	let resolved = resolved
		.pop()
		.ok_or_else(|| QueryError::new("note_resolution_failed", "note did not resolve"))?;
	note_dto(&resolved)
}

fn note_resolution_dto(resolution: &NoteResolution) -> NoteResolutionDto {
	match resolution {
		NoteResolution::Resolved {
			target_label,
			target_file,
			target_slice,
		} => NoteResolutionDto::Resolved {
			target: target_label.clone(),
			file: target_file.clone(),
			slice: *target_slice,
		},
		NoteResolution::Orphan => NoteResolutionDto::Orphan,
	}
}

fn notes_action_label(action: NotesAction) -> &'static str {
	match action {
		NotesAction::List => "list",
		NotesAction::Get => "get",
		NotesAction::Create => "create",
		NotesAction::Update => "update",
		NotesAction::Transition => "transition",
		NotesAction::Delete => "delete",
	}
}

#[derive(Debug)]
struct Paged<T> {
	items: Vec<T>,
	total: usize,
	next_cursor: Option<QueryCursor>,
}

enum RulesCheckRow {
	Violation(ViolationDto),
	Error(FileErrorDto),
	RuleReport(RuleReportDto),
	SkipReason(code_moniker_query::CheckSkipReasonDto),
}

fn rules_check_rows(roots: &[RulesCheckRootResult]) -> Vec<RulesCheckRow> {
	let mut rows = Vec::new();
	for root in roots {
		rows.extend(
			root.violations
				.iter()
				.cloned()
				.map(RulesCheckRow::Violation),
		);
		rows.extend(root.errors.iter().cloned().map(RulesCheckRow::Error));
		rows.extend(
			root.rule_reports
				.iter()
				.cloned()
				.map(RulesCheckRow::RuleReport),
		);
		rows.extend(
			root.skip_reason
				.iter()
				.cloned()
				.map(RulesCheckRow::SkipReason),
		);
	}
	rows
}

fn page_rows<T>(
	rows: Vec<T>,
	page: Page,
	generation: Option<WorkspaceGeneration>,
) -> Result<Paged<T>, QueryError> {
	if let Some(cursor) = page.cursor.as_ref() {
		if cursor.generation != generation {
			return Err(QueryError::new(
				"cursor_generation_mismatch",
				"query cursor belongs to a different workspace generation",
			));
		}
	}
	let total = rows.len();
	let start = page
		.cursor
		.as_ref()
		.map(|cursor| cursor.offset)
		.unwrap_or(0)
		.min(total);
	let end = start.saturating_add(page.limit).min(total);
	let next_cursor = (end < total).then(|| QueryCursor::new(end, generation));
	Ok(Paged {
		items: rows.into_iter().skip(start).take(end - start).collect(),
		total,
		next_cursor,
	})
}

mod helpers {
	use super::*;

	pub(super) fn source_by_id(
		snapshot: &WorkspaceSnapshot,
	) -> BTreeMap<SourceId, &SourceFileRecord> {
		snapshot
			.index
			.sources
			.iter()
			.map(|source| (source.id, source))
			.collect()
	}

	pub(super) fn symbol_by_id(snapshot: &WorkspaceSnapshot) -> BTreeMap<SymbolId, &SymbolRecord> {
		snapshot
			.index
			.symbols
			.iter()
			.map(|symbol| (symbol.id, symbol))
			.collect()
	}

	pub(super) fn reference_by_id(
		snapshot: &WorkspaceSnapshot,
	) -> BTreeMap<ReferenceId, &ReferenceRecord> {
		snapshot
			.index
			.references
			.iter()
			.map(|reference| (reference.id, reference))
			.collect()
	}

	pub(super) fn find_symbol<'a>(
		snapshot: &'a WorkspaceSnapshot,
		uri: &str,
	) -> Result<&'a SymbolRecord, QueryError> {
		snapshot
			.index
			.symbols
			.iter()
			.find(|symbol| symbol.identity.as_ref() == uri || symbol.id.to_string() == uri)
			.ok_or_else(|| QueryError::new("symbol_not_found", format!("symbol not found: {uri}")))
	}

	pub(super) fn symbol_dto(
		symbol: &SymbolRecord,
		source: &SourceFileRecord,
		roots: &[PathBuf],
	) -> SymbolDto {
		SymbolDto {
			root: source_root_label(roots, source),
			uri: symbol.identity.to_string(),
			id: symbol.id.to_string(),
			name: symbol.name.to_string(),
			kind: symbol.kind.to_string(),
			visibility: symbol.visibility.to_string(),
			signature: symbol.signature.to_string(),
			file: source.rel_path.to_string(),
			language: source.language.to_string(),
			line_range: symbol.line_range,
			navigable: symbol.navigable,
			score: None,
			match_reason: None,
			source: None,
		}
	}

	pub(super) fn symbol_search_dto(
		symbol: &SymbolRecord,
		source: &SourceFileRecord,
		roots: &[PathBuf],
		score: u32,
		reason: String,
	) -> SymbolDto {
		let mut dto = symbol_dto(symbol, source, roots);
		dto.score = Some(score);
		dto.match_reason = Some(reason);
		dto
	}

	pub(super) fn usage_dto(
		reference: &ReferenceRecord,
		direction: UsageDirection,
		context: &UsageDtoContext<'_>,
	) -> Option<UsageDto> {
		let source = context.source_by_id.get(&reference.source)?;
		source_root(context.roots, context.selected_roots, source)?;
		if !context.path_filter.matches(&source.rel_path)
			|| (!context.langs.is_empty()
				&& !context.langs.iter().any(|lang| lang == &source.language))
		{
			return None;
		}
		let actor = context
			.symbol_by_id
			.get(&reference.source_symbol)
			.map(|symbol| symbol.name.to_string())
			.unwrap_or_else(|| reference.source_symbol.to_string());
		let source_context = context
			.symbol_by_id
			.get(&reference.source_symbol)
			.map(|symbol| symbol.identity.to_string())
			.unwrap_or_else(|| reference.source_symbol.to_string());
		Some(UsageDto {
			root: source_root_label(context.roots, source),
			direction,
			reference: reference.id.to_string(),
			kind: reference.kind.to_string(),
			actor,
			context: source_context,
			endpoint: reference.target_identity.to_string(),
			file: source.rel_path.to_string(),
			prefix: path_prefix(&source.rel_path),
			location: reference_location(source, reference),
			line_range: reference.line_range,
			via: None,
		})
	}

	pub(super) fn source_snippet(
		source: &SourceFileRecord,
		symbol: &SymbolRecord,
		context_lines: usize,
	) -> Result<Option<SourceSnippet>, QueryError> {
		let Some((start, end)) = symbol.line_range else {
			return Ok(None);
		};
		let first = start.saturating_sub(context_lines as u32).max(1);
		let last = end.saturating_add(context_lines as u32);
		let source_text = if source.text.is_empty() {
			std::fs::read_to_string(&source.path).map_err(|err| {
				QueryError::new(
					"source_read_failed",
					format!("cannot read source {}: {err}", source.path),
				)
			})?
		} else {
			source.text.to_string()
		};
		let lines = source_text
			.lines()
			.enumerate()
			.filter_map(|(idx, text)| {
				let number = idx as u32 + 1;
				(number >= first && number <= last).then(|| SourceLine {
					number,
					text: text.to_string(),
				})
			})
			.collect();
		Ok(Some(SourceSnippet {
			file: source.rel_path.to_owned(),
			first_line: first,
			last_line: last,
			lines,
		}))
	}

	pub(super) fn workspace_langs(
		snapshot: &WorkspaceSnapshot,
		roots: &[PathBuf],
		root: &Path,
		filter: &[String],
	) -> Vec<Lang> {
		let mut langs = snapshot
			.index
			.sources
			.iter()
			.filter(|source| source_in_root(roots, source, root))
			.filter(|source| {
				filter.is_empty() || filter.iter().any(|lang| lang == &source.language)
			})
			.filter_map(|source| Lang::from_tag(&source.language))
			.collect::<Vec<_>>();
		langs.sort_by_key(|lang| lang.tag());
		langs.dedup();
		langs
	}

	pub(super) fn resolve_rules_path(root: &Path, rules: Option<&str>) -> PathBuf {
		let path = rules
			.map(PathBuf::from)
			.unwrap_or_else(|| PathBuf::from(".code-moniker.toml"));
		if path.is_absolute() {
			path
		} else {
			root.join(path)
		}
	}

	pub(super) fn violation_dto(root: &Path, path: &Path, violation: &Violation) -> ViolationDto {
		ViolationDto {
			root: root.display().to_string(),
			path: path.display().to_string(),
			rule_id: violation.rule_id.to_string(),
			severity: violation.severity.as_str().to_string(),
			moniker: violation.moniker.to_string(),
			kind: violation.kind.to_string(),
			lines: violation.lines,
			message: violation.message.to_string(),
		}
	}

	pub(super) fn file_error_dto(root: &Path, path: &Path, error: &str) -> FileErrorDto {
		FileErrorDto {
			root: root.display().to_string(),
			path: path.display().to_string(),
			error: error.to_string(),
		}
	}

	pub(super) fn rule_report_dto(
		root: &Path,
		path: Option<&Path>,
		report: &RuleReport,
	) -> RuleReportDto {
		RuleReportDto {
			root: root.display().to_string(),
			path: path.map(|path| path.display().to_string()),
			rule_id: report.rule_id.to_string(),
			severity: report.severity.as_str().to_string(),
			domain: report.domain.to_string(),
			evaluated: report.evaluated,
			matches: report.matches,
			violations: report.violations,
			antecedent_matches: report.antecedent_matches,
			warning: report.warning.clone(),
		}
	}

	pub(super) fn rule_dto(root: &Path, spec: CompiledRuleSpec) -> RuleDto {
		RuleDto {
			root: root.display().to_string(),
			id: spec.rule_id,
			severity: spec.severity.as_str().to_string(),
			lang: spec.lang,
			domain: spec.domain,
			kind: spec.kind,
			expr: spec.expr,
			expanded_expr: spec.expanded_expr,
			message: spec.message,
			rationale: spec.rationale,
			require_doc_comment: spec.require_doc_comment,
		}
	}

	pub(super) fn run_rules_for_root(
		root: &Path,
		config_root: &Path,
		profile: Option<String>,
		rules: Option<&str>,
		files: &[String],
		report: bool,
	) -> Result<RulesCheckRootResult, QueryError> {
		let rules_path = resolve_rules_path(config_root, rules);
		let rules = RuleSetRequest::with_rules(rules_path, DEFAULT_SCHEME)
			.with_default_rules(DefaultRulesSelection::Config)
			.with_profile(profile);
		let request = CheckRequest::new(root.to_path_buf(), rules)
			.with_report(report)
			.with_files(files.iter().map(PathBuf::from).collect());
		let run = request
			.run()
			.map_err(|err| QueryError::new("rules_check_failed", err.to_string()))?;
		let exit = check_exit(&run);
		let summary = check_summary_dto(&run.summary());
		let violations = run
			.file_violations()
			.map(|(path, violation)| violation_dto(root, path, violation))
			.collect();
		let errors = run
			.error_summaries()
			.map(|(path, error)| file_error_dto(root, path, error))
			.collect();
		let rule_reports = run
			.reports
			.iter()
			.flat_map(|report| {
				report
					.rule_reports
					.iter()
					.map(move |rule| rule_report_dto(root, Some(&report.path), rule))
			})
			.collect();
		let skip_reason = run
			.skip_reason
			.map(|reason| check_skip_reason_dto(root, reason));
		Ok(RulesCheckRootResult {
			root: root.display().to_string(),
			exit,
			summary,
			violations,
			errors,
			rule_reports,
			skip_reason,
		})
	}

	pub(super) fn check_exit(run: &code_moniker_check::CheckRun) -> String {
		if run.any_error() {
			"error"
		} else if run.any_error_violation() {
			"no_match"
		} else {
			"match"
		}
		.to_string()
	}

	pub(super) fn aggregate_check_exit(roots: &[RulesCheckRootResult]) -> String {
		if roots.iter().any(|root| root.exit == "error") {
			"error"
		} else if roots.iter().any(|root| root.exit == "no_match") {
			"no_match"
		} else {
			"match"
		}
		.to_string()
	}

	pub(super) fn aggregate_check_summary(roots: &[RulesCheckRootResult]) -> CheckSummaryDto {
		let mut summary = CheckSummaryDto::default();
		for root in roots {
			summary.files_scanned += root.summary.files_scanned;
			summary.files_with_violations += root.summary.files_with_violations;
			summary.total_violations += root.summary.total_violations;
			summary.total_rule_errors += root.summary.total_rule_errors;
			summary.total_warnings += root.summary.total_warnings;
			summary.files_with_errors += root.summary.files_with_errors;
			summary.total_errors += root.summary.total_errors;
			summary.elapsed_ms += root.summary.elapsed_ms;
			summary
				.failed_rules
				.extend(root.summary.failed_rules.iter().cloned());
		}
		summary.failed_rules.sort_by(|a, b| {
			a.rule_id
				.cmp(&b.rule_id)
				.then_with(|| a.severity.cmp(&b.severity))
		});
		summary
	}

	pub(super) fn check_summary_dto(summary: &CheckSummary) -> CheckSummaryDto {
		CheckSummaryDto {
			files_scanned: summary.files_scanned,
			files_with_violations: summary.files_with_violations,
			total_violations: summary.total_violations,
			total_rule_errors: summary.total_rule_errors,
			total_warnings: summary.total_warnings,
			files_with_errors: summary.files_with_errors,
			total_errors: summary.total_errors,
			elapsed_ms: summary.elapsed_ms,
			failed_rules: summary
				.failed_rules
				.iter()
				.map(|rule| FailedRuleDto {
					rule_id: rule.rule_id.to_string(),
					severity: rule.severity.as_str().to_string(),
					violations: rule.violations,
				})
				.collect(),
		}
	}

	pub(super) fn check_skip_reason_dto(
		root: &Path,
		reason: CheckSkipReason,
	) -> code_moniker_query::CheckSkipReasonDto {
		let reason = match reason {
			CheckSkipReason::ExcludedSingleFile => "excluded_single_file",
			CheckSkipReason::UnsupportedSingleFile => "unsupported_single_file",
			CheckSkipReason::NoMatchingFiles => "no_matching_files",
		};
		code_moniker_query::CheckSkipReasonDto {
			root: root.display().to_string(),
			reason: reason.to_string(),
		}
	}

	pub(super) fn root_status(
		snapshot: &WorkspaceSnapshot,
		roots: &[PathBuf],
		root: &Path,
		stale: bool,
		stale_summary: &str,
	) -> WorkspaceRootStatus {
		let sources = snapshot
			.index
			.sources
			.iter()
			.filter(|source| source_in_root(roots, source, root))
			.collect::<Vec<_>>();
		let source_ids = sources
			.iter()
			.map(|source| source.id)
			.collect::<std::collections::BTreeSet<_>>();
		WorkspaceRootStatus {
			root: root.display().to_string(),
			generation: Some(WorkspaceGeneration(snapshot.generation.value())),
			files: sources.len(),
			symbols: snapshot
				.index
				.symbols
				.iter()
				.filter(|symbol| source_ids.contains(&symbol.source))
				.count(),
			references: snapshot
				.index
				.references
				.iter()
				.filter(|reference| source_ids.contains(&reference.source))
				.count(),
			stale,
			stale_summary: stale_summary.to_string(),
		}
	}

	pub(super) fn selected_roots<'a>(
		roots: &'a [PathBuf],
		selector: Option<&str>,
	) -> Result<Vec<&'a PathBuf>, QueryError> {
		if selector.is_none_or(|selector| selector.trim().is_empty()) {
			return Ok(roots.iter().collect());
		}
		let selected = roots
			.iter()
			.filter(|root| root_matches_selector(root, selector))
			.collect::<Vec<_>>();
		if selected.is_empty() {
			let value = selector.unwrap_or("<all>");
			return Err(QueryError::new(
				"workspace_not_found",
				format!("workspace selector matched no root: {value}"),
			));
		}
		if selected.len() > 1 {
			let value = selector.unwrap_or("<all>");
			return Err(QueryError::new(
				"workspace_selector_ambiguous",
				format!("workspace selector matched multiple roots: {value}"),
			));
		}
		Ok(selected)
	}

	pub(super) fn root_matches_selector(root: &Path, selector: Option<&str>) -> bool {
		let Some(selector) = selector.map(str::trim).filter(|value| !value.is_empty()) else {
			return true;
		};
		root.display().to_string() == selector
			|| root
				.file_name()
				.and_then(|name| name.to_str())
				.is_some_and(|name| name == selector)
	}

	pub(super) fn source_root<'a>(
		roots: &'a [PathBuf],
		selected_roots: &[&PathBuf],
		source: &SourceFileRecord,
	) -> Option<&'a PathBuf> {
		let root = roots.get(source.source_root)?;
		selected_roots
			.iter()
			.any(|selected| selected.as_path() == root.as_path())
			.then_some(root)
	}

	pub(super) fn source_in_root(
		roots: &[PathBuf],
		source: &SourceFileRecord,
		root: &Path,
	) -> bool {
		roots
			.get(source.source_root)
			.is_some_and(|declared_root| declared_root == root)
	}

	pub(super) fn source_root_label(roots: &[PathBuf], source: &SourceFileRecord) -> String {
		roots
			.get(source.source_root)
			.map(|root| root.display().to_string())
			.unwrap_or_default()
	}

	pub(super) fn sorted_counts<I>(values: I) -> Vec<CountDto>
	where
		I: IntoIterator<Item = String>,
	{
		let mut counts = BTreeMap::<String, usize>::new();
		for value in values {
			*counts.entry(value).or_default() += 1;
		}
		count_rows(counts)
	}

	pub(super) fn count_rows(counts: BTreeMap<String, usize>) -> Vec<CountDto> {
		let mut rows = counts
			.into_iter()
			.map(|(name, count)| CountDto { name, count })
			.collect::<Vec<_>>();
		rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
		rows
	}

	pub(super) fn path_prefix(path: &str) -> String {
		path.split('/').next().unwrap_or(path).to_string()
	}

	pub(super) fn reference_location(
		source: &SourceFileRecord,
		reference: &ReferenceRecord,
	) -> String {
		let suffix = reference
			.line_range
			.map(|(start, end)| {
				if start == end {
					format!(":L{start}")
				} else {
					format!(":L{start}-L{end}")
				}
			})
			.unwrap_or_else(|| ":L?".to_string());
		format!("{}{}", source.rel_path, suffix)
	}

	pub(super) fn root_labels(roots: &[PathBuf]) -> Vec<String> {
		roots
			.iter()
			.map(|root| root.display().to_string())
			.collect()
	}

	pub(super) fn common_workspace_root(roots: &[PathBuf]) -> anyhow::Result<PathBuf> {
		let Some(first) = roots.first() else {
			anyhow::bail!("workspace daemon requires at least one root");
		};
		let mut common = first.clone();
		for root in roots.iter().skip(1) {
			while !root.starts_with(&common) {
				if !common.pop() {
					anyhow::bail!("cannot find common root for workspace daemon roots");
				}
			}
		}
		Ok(common)
	}

	pub(super) fn rules_config_root(roots: &[PathBuf]) -> anyhow::Result<PathBuf> {
		let common = common_workspace_root(roots)?;
		let mut cursor = if common.is_file() {
			common
				.parent()
				.map(Path::to_path_buf)
				.unwrap_or_else(|| common.clone())
		} else {
			common.clone()
		};
		loop {
			if cursor.join(".code-moniker.toml").is_file() {
				return Ok(cursor);
			}
			if !cursor.pop() {
				return Ok(common);
			}
		}
	}

	pub(super) fn workspace_label_from_paths(roots: &[&PathBuf]) -> String {
		if roots.len() == 1 {
			roots[0].display().to_string()
		} else {
			roots
				.iter()
				.map(|root| root.display().to_string())
				.collect::<Vec<_>>()
				.join(";")
		}
	}
}

#[allow(dead_code)]
fn _assert_public_boundary_types(_: ReferenceId, _: SourceId, _: SymbolId, _: RuleSeverity) {}

#[cfg(test)]
mod tests {
	use std::fs;

	use code_moniker_query::{
		Page, ProtocolRequest, ProtocolResponse, Query, QueryCursor, QueryRequest, QueryResult,
		WorkspaceGeneration,
	};

	use super::*;

	fn search_symbols(daemon: &mut WorkspaceDaemon, text: &str) -> QueryResult {
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::SymbolSearch(code_moniker_query::SymbolSearchQuery {
				workspace: None,
				text: Some(text.to_string()),
				path: Vec::new(),
				lang: Vec::new(),
				kind: Vec::new(),
				shape: Vec::new(),
				name: None,
				include_non_navigable: false,
				include_code: false,
				context_lines: 0,
				projection: Vec::new(),
			}),
			consistency: code_moniker_query::Consistency::Current,
			page: Page::default(),
		})));
		match response {
			ProtocolResponse::Query(query) => query.result,
			other => panic!("expected query response, got {other:?}"),
		}
	}

	#[test]
	fn auto_policy_applies_live_edits_before_plain_queries() {
		let temp = tempfile::tempdir().expect("tempdir");
		let src = temp.path().join("src");
		fs::create_dir_all(&src).expect("src dir");
		let lib = src.join("lib.rs");
		fs::write(&lib, "pub fn before_auto_edit() {}\n").expect("write lib");
		let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
			roots: vec![temp.path().display().to_string()],
			project: None,
			cache_dir: None,
			live_refresh: Some("auto".to_string()),
		})
		.expect("daemon");
		let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refreshed, ProtocolResponse::Command(_)));

		fs::write(&lib, "pub fn after_auto_edit() {}\n").expect("rewrite lib");
		daemon
			.live
			.tx
			.send(WorkspaceLiveEvent::SourcesChanged(vec![lib.clone()]))
			.expect("send live event");

		match search_symbols(&mut daemon, "after_auto_edit") {
			QueryResult::SymbolList(symbols) => {
				assert_eq!(
					symbols.rows.len(),
					1,
					"auto policy should apply the edit before a plain query"
				);
			}
			other => panic!("expected symbols result, got {other:?}"),
		}

		fs::write(
			src.join("fresh_auto.rs"),
			"pub fn fresh_auto_created() {}\n",
		)
		.expect("create file");
		daemon
			.live
			.tx
			.send(WorkspaceLiveEvent::SourcesChanged(vec![
				src.join("fresh_auto.rs"),
			]))
			.expect("send create event");

		match search_symbols(&mut daemon, "fresh_auto_created") {
			QueryResult::SymbolList(symbols) => {
				assert_eq!(
					symbols.rows.len(),
					1,
					"auto policy should index created files before a plain query"
				);
			}
			other => panic!("expected symbols result, got {other:?}"),
		}
	}

	#[test]
	fn query_error_carries_structured_code_in_data() {
		let error = query_error(QueryError::new("workspace_loading", "still loading"));
		assert_eq!(error.message(), "still loading");
		let data = error.data().expect("error should carry structured data");
		let value: serde_json::Value = serde_json::from_str(data.get()).unwrap();
		assert_eq!(value["code"], "workspace_loading");
		assert_eq!(value["message"], "still loading");
	}

	#[test]
	fn rules_config_root_searches_above_common_multi_root() {
		let temp = tempfile::tempdir().expect("tempdir");
		fs::write(temp.path().join(".code-moniker.toml"), "").expect("rules config");
		let first = temp.path().join("crates").join("first");
		let second = temp.path().join("crates").join("second");
		fs::create_dir_all(&first).expect("first");
		fs::create_dir_all(&second).expect("second");
		let roots = canonical_workspace_roots([&first, &second]).expect("roots");
		let common = temp
			.path()
			.join("crates")
			.canonicalize()
			.expect("canonical common");
		assert_eq!(common_workspace_root(&roots).expect("common root"), common);
		assert_eq!(
			rules_config_root(&roots).expect("rules config root"),
			temp.path().canonicalize().expect("canonical temp")
		);
	}

	#[test]
	fn workspace_selector_rejects_ambiguous_basenames() {
		let temp = tempfile::tempdir().expect("tempdir");
		let first = temp.path().join("a").join("same");
		let second = temp.path().join("b").join("same");
		fs::create_dir_all(&first).expect("first");
		fs::create_dir_all(&second).expect("second");
		let roots = canonical_workspace_roots([&first, &second]).expect("roots");
		let error = selected_roots(&roots, Some("same")).expect_err("ambiguous selector");
		assert_eq!(error.code, "workspace_selector_ambiguous");
	}

	#[test]
	fn source_root_uses_declared_workspace_root() {
		let temp = tempfile::tempdir().expect("tempdir");
		let parent = temp.path().join("workspace");
		let child = parent.join("child");
		fs::create_dir_all(child.join("src")).expect("child src");
		let roots = canonical_workspace_roots([&parent, &child]).expect("roots");
		let canonical_child = child.canonicalize().expect("canonical child");
		let source_owned_by_parent = SourceFileRecord {
			id: SourceId::at(0),
			uri: String::new(),
			source_root: 0,
			path: canonical_child.join("src/lib.rs").display().to_string(),
			rel_path: "child/src/lib.rs".to_string(),
			anchor: String::new(),
			language: "rs".to_string(),
			text: String::new(),
		};
		let selected = roots.iter().collect::<Vec<_>>();
		let root = source_root(&roots, &selected, &source_owned_by_parent).expect("source root");
		assert_eq!(root, &roots[0]);

		let source_owned_by_child = SourceFileRecord {
			source_root: 1,
			..source_owned_by_parent
		};
		let root = source_root(&roots, &selected, &source_owned_by_child).expect("source root");
		assert_eq!(root, &canonical_child);
	}

	#[test]
	fn page_rows_rejects_cursor_from_another_generation() {
		let page = Page {
			cursor: Some(QueryCursor::new(1, Some(WorkspaceGeneration(1)))),
			limit: 1,
		};
		let error = page_rows(vec![1, 2, 3], page, Some(WorkspaceGeneration(2)))
			.expect_err("generation mismatch");
		assert_eq!(error.code, "cursor_generation_mismatch");
	}

	#[test]
	fn page_rows_rejects_offset_only_cursor_for_generated_snapshot() {
		let page = Page {
			cursor: Some(QueryCursor::new(1, None)),
			limit: 1,
		};
		let error = page_rows(vec![1, 2, 3], page, Some(WorkspaceGeneration(2)))
			.expect_err("missing generation");
		assert_eq!(error.code, "cursor_generation_mismatch");
	}

	#[test]
	fn daemon_answers_status_and_symbol_search() {
		let temp = tempfile::tempdir().expect("tempdir");
		fs::write(
			temp.path().join("lib.rs"),
			"pub struct Customer;\nimpl Customer { pub fn id(&self) -> u64 { 42 } }\n",
		)
		.expect("write fixture");
		let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
		let status = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::WorkspaceStatus,
		))));
		match status {
			ProtocolResponse::Query(response) => {
				assert!(matches!(response.result, QueryResult::WorkspaceStatus(_)));
			}
			other => panic!("unexpected response: {other:?}"),
		}
		let refresh = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(
			matches!(refresh, ProtocolResponse::Command(_)),
			"unexpected response: {refresh:?}"
		);
		let search = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::SymbolSearch(code_moniker_query::SymbolSearchQuery {
				text: Some("Customer".to_string()),
				..Default::default()
			}),
		))));
		match search {
			ProtocolResponse::Query(response) => match response.result {
				QueryResult::SymbolList(list) => {
					assert!(list.rows.iter().any(|row| row.name == "Customer"));
				}
				other => panic!("unexpected result: {other:?}"),
			},
			other => panic!("unexpected response: {other:?}"),
		}
	}

	#[tokio::test]
	async fn rpc_server_answers_query_and_streams_events() {
		use code_moniker_query::DaemonRpcClient;
		use code_moniker_query::{WorkspaceEventDto, WorkspaceEventKind};
		use jsonrpsee::ws_client::WsClientBuilder;

		let temp = tempfile::tempdir().expect("tempdir");
		fs::write(temp.path().join("lib.rs"), "pub struct Customer;\n").expect("seed fixture");
		let daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
		let (events, _) = tokio::sync::broadcast::channel(16);
		let service = DaemonRpcService {
			daemon: Arc::new(Mutex::new(daemon)),
			roots: Arc::from(vec![temp.path().to_path_buf()]),
			events: events.clone(),
			shutdown: Arc::new(tokio::sync::Notify::new()),
			handshake: HandshakeResponse {
				protocol_version: code_moniker_query::PROTOCOL_VERSION,
				daemon_version: "test".to_string(),
				workspace_root: "test".to_string(),
				workspace_roots: Vec::new(),
				capabilities: CapabilitySet::default(),
			},
		};
		let server = Server::builder()
			.build("127.0.0.1:0")
			.await
			.expect("server binds");
		let addr = server.local_addr().expect("addr");
		let handle = server.start(service.into_rpc());

		let client = WsClientBuilder::default()
			.build(format!("ws://{addr}"))
			.await
			.expect("client connects");

		let response = client
			.query(QueryRequest::new(Query::WorkspaceStatus))
			.await
			.expect("query");
		assert!(matches!(response.result, QueryResult::WorkspaceStatus(_)));

		let mut subscription = client.subscribe_events().await.expect("subscribe");
		events
			.send(WorkspaceEventDto {
				kind: WorkspaceEventKind::Notes,
				generation: None,
				stale_summary: None,
			})
			.expect("publish event");
		let event = subscription
			.next()
			.await
			.expect("event present")
			.expect("event decoded");
		assert_eq!(event.kind, WorkspaceEventKind::Notes);

		handle.stop().ok();
	}
}
