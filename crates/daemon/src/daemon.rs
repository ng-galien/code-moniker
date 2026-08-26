use std::path::PathBuf;
use std::sync::{Arc, mpsc};

use code_moniker_query::{
	Command, CommandRequest, CommandResponse, Consistency, DaemonWorkspaceConfig, Page,
	ProtocolRequest, ProtocolResponse, Query, QueryError, QueryRequest, QueryResponse, QueryResult,
	WorkspaceEventDto, WorkspaceEventKind, WorkspaceFailureDto, WorkspaceGeneration,
	WorkspacePhase, canonical_workspace_config, config_from_roots, config_roots,
	describe_query_capabilities, validate_daemon_start_config,
};
use code_moniker_workspace::live::{
	LiveWorkspaceWatcher, WorkspaceLiveEvent, WorkspaceLiveRefreshPlan,
};
use code_moniker_workspace::notes::WorkspaceNotes;
use code_moniker_workspace::registry::{LocalWorkspaceOptions, LocalWorkspaceRegistry};
use code_moniker_workspace::snapshot::{
	WorkspaceCancellation, WorkspaceRequest, WorkspaceSnapshot,
};
use code_moniker_workspace::source::LocalResourceCache;

use crate::helpers::rules_config_root;
use crate::lifecycle::{
	drain_live_events, generation, refresh_full_cancellable, refresh_stale, restart_live_watcher,
	workspace_status, workspace_status_result, workspace_transition_result,
};
use crate::query::{
	ResponseContext, RulesCheckEval, RulesListEval, RulesListFilters, change_context_response,
	change_review_response, diff_impact_compare_response, graph_corridor_response,
	graph_path_response, identity_children_response, identity_graph_response,
	metrics_coupling_response, notes_response, resolution_audit_response,
	rules_applicable_response, rules_check_response, rules_list_response, symbol_detail_response,
	symbol_graph_response, symbol_insights_response, symbol_search_response,
	symbol_usages_response, tree_children_response, view_read_response,
};
use crate::runtime::{PublishedSnapshot, SnapshotQueryContext};
use crate::runtime_dependencies::optional_git_change_failure;
use crate::source_sets::{
	MEMORY_SOURCE_LIMITS, parse_memory_source_set, refresh_memory_source_set,
	validate_memory_source_set_limits, validate_srcset,
};
use crate::{syntax, telemetry};

// The daemon aggregate intentionally owns the cohesive workspace services and process provenance dispatched below.
// code-moniker: ignore[smell-god-type-local-metrics]
pub struct WorkspaceDaemon {
	pub(super) roots: Vec<PathBuf>,
	pub(super) config_root: PathBuf,
	pub(super) registry: LocalWorkspaceRegistry,
	pub(super) cache: LocalResourceCache,
	pub(super) notes: WorkspaceNotes,
	pub(super) live: DaemonLiveState,
	pub(super) process_scope: &'static str,
}

pub(super) struct DaemonLiveState {
	pub(super) policy: DaemonLiveRefreshPolicy,
	pub(super) tx: mpsc::Sender<WorkspaceLiveEvent>,
	pub(super) rx: mpsc::Receiver<WorkspaceLiveEvent>,
	pub(super) watcher: Option<LiveWorkspaceWatcher>,
	watcher_epoch: u64,
	watcher_updates_tx: mpsc::Sender<LiveWatcherUpdate>,
	watcher_updates_rx: mpsc::Receiver<LiveWatcherUpdate>,
	watcher_failure: Option<String>,
	pub(super) events: Option<tokio::sync::broadcast::Sender<WorkspaceEventDto>>,
}

pub(super) struct LiveWatcherUpdate {
	pub(super) epoch: u64,
	pub(super) result: Result<LiveWorkspaceWatcher, String>,
}

pub(super) struct LiveWatcherRegistration {
	roots: Vec<code_moniker_workspace::live::WorkspaceWatchRoot>,
	tx: mpsc::Sender<WorkspaceLiveEvent>,
	events: Option<tokio::sync::broadcast::Sender<WorkspaceEventDto>>,
}

struct WorkspaceDaemonInit {
	roots: Vec<PathBuf>,
	config_root: PathBuf,
	registry: LocalWorkspaceRegistry,
	cache: LocalResourceCache,
	live: DaemonLiveState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DaemonLiveRefreshPolicy {
	OnDemand,
	Auto,
}

impl WorkspaceDaemon {
	pub fn new(roots: Vec<PathBuf>) -> anyhow::Result<Self> {
		Self::new_with_config(config_from_roots(roots)?)
	}

	pub fn new_with_config(config: DaemonWorkspaceConfig) -> anyhow::Result<Self> {
		Self::build(config, None)
	}

	pub(super) fn with_events(
		config: DaemonWorkspaceConfig,
		events: tokio::sync::broadcast::Sender<WorkspaceEventDto>,
	) -> anyhow::Result<Self> {
		Self::build(config, Some(events))
	}

	fn build(
		config: DaemonWorkspaceConfig,
		events: Option<tokio::sync::broadcast::Sender<WorkspaceEventDto>>,
	) -> anyhow::Result<Self> {
		validate_daemon_start_config(&config)?;
		let init = WorkspaceDaemonInit::new(config)?;
		let mut daemon = Self {
			roots: init.roots,
			config_root: init.config_root,
			registry: init.registry,
			cache: init.cache,
			notes: WorkspaceNotes::default(),
			live: init.live,
			process_scope: "daemon",
		};
		daemon.live.events = events;
		Ok(daemon)
	}

	pub fn with_process_scope(mut self, process_scope: &'static str) -> Self {
		self.process_scope = process_scope;
		self
	}

	pub fn handle_protocol(&mut self, request: ProtocolRequest) -> ProtocolResponse {
		handle_protocol(self, request)
	}

	pub fn refresh_cancellable(
		&mut self,
		cancellation: WorkspaceCancellation,
	) -> Result<CommandResponse, QueryError> {
		refresh_full_cancellable(self, cancellation.clone())?;
		if cancellation.is_cancelled() {
			return Err(QueryError::new(
				"workspace_cancelled",
				"workspace refresh was cancelled",
			));
		}
		let status = workspace_status_result(&self.roots, &self.registry);
		Ok(CommandResponse {
			generation: generation(&self.registry),
			message: "workspace refreshed".to_string(),
			status: Some(Box::new(status)),
		})
	}

	pub(super) fn live_watcher_registration(&self) -> LiveWatcherRegistration {
		LiveWatcherRegistration {
			roots: self.registry.watch_roots(),
			tx: self.live.tx.clone(),
			events: self.live.events.clone(),
		}
	}

	pub(super) fn begin_initial_live_watcher_registration(
		&mut self,
	) -> (u64, LiveWatcherRegistration) {
		let epoch = self.live.watcher_epoch.wrapping_add(1);
		self.live.watcher_epoch = epoch;
		(epoch, self.live_watcher_registration())
	}

	pub(super) fn install_initial_live_watcher(
		&mut self,
		epoch: u64,
		watcher: LiveWorkspaceWatcher,
	) -> bool {
		if epoch > self.live.watcher_epoch || self.live.watcher.is_some() {
			return false;
		}
		self.live.watcher = Some(watcher);
		true
	}

	pub(super) fn request_change_overlay(&mut self) -> Result<(), QueryError> {
		workspace_transition_result(
			self.registry
				.commands()
				.refresh_changes(WorkspaceRequest::new("daemon-explicit-change-overlay")),
		)
	}

	pub(super) fn restart_live_watcher(&mut self) -> anyhow::Result<()> {
		let epoch = self.live.watcher_epoch.wrapping_add(1);
		let registration = self.live_watcher_registration();
		let updates = self.live.watcher_updates_tx.clone();
		std::thread::Builder::new()
			.name(format!("code-moniker-watcher-{epoch}"))
			.spawn(move || {
				let result = registration.start().map_err(|error| format!("{error:#}"));
				let _ = updates.send(LiveWatcherUpdate { epoch, result });
			})?;
		self.live.watcher_epoch = epoch;
		Ok(())
	}

	pub(super) fn install_pending_live_watcher(&mut self) -> Result<bool, QueryError> {
		let mut installed = false;
		while let Ok(update) = self.live.watcher_updates_rx.try_recv() {
			if update.epoch != self.live.watcher_epoch {
				continue;
			}
			match update.result {
				Ok(watcher) => {
					self.live.watcher = Some(watcher);
					self.live.watcher_failure = None;
					installed = true;
				}
				Err(message) => self.live.watcher_failure = Some(message),
			}
		}
		if let Some(message) = &self.live.watcher_failure {
			return Err(QueryError::new("live_watcher_failed", message.clone()));
		}
		Ok(installed)
	}

	pub(super) fn live_watcher_reconciliation_plan(&self) -> WorkspaceLiveRefreshPlan {
		let summary = "live watcher armed; source reconciliation required".to_string();
		if let Some(events) = &self.live.events {
			let _ = events.send(WorkspaceEventDto {
				kind: WorkspaceEventKind::Stale,
				generation: generation(&self.registry),
				stale_summary: Some(summary),
			});
		}
		WorkspaceLiveRefreshPlan::from_event(WorkspaceLiveEvent::RescanRequired)
	}

	pub(super) fn record_live_watcher_failure(&mut self, epoch: u64, message: String) -> bool {
		if epoch != self.live.watcher_epoch {
			return false;
		}
		self.live.watcher_failure = Some(message);
		true
	}

	#[cfg(test)]
	pub(super) fn queue_live_watcher_update_for_test(&mut self, watcher: LiveWorkspaceWatcher) {
		self.live.watcher_epoch = self.live.watcher_epoch.wrapping_add(1);
		self.live
			.watcher_updates_tx
			.send(LiveWatcherUpdate {
				epoch: self.live.watcher_epoch,
				result: Ok(watcher),
			})
			.expect("queue watcher update");
	}

	#[cfg(test)]
	pub(super) fn inject_live_watcher_failure(&mut self, message: &str) {
		self.live.watcher_epoch = self.live.watcher_epoch.wrapping_add(1);
		self.live
			.watcher_updates_tx
			.send(LiveWatcherUpdate {
				epoch: self.live.watcher_epoch,
				result: Err(message.to_string()),
			})
			.expect("inject watcher failure");
	}
}

impl LiveWatcherRegistration {
	pub(super) fn start(self) -> anyhow::Result<LiveWorkspaceWatcher> {
		let tx = self.tx;
		let events = self.events;
		LiveWorkspaceWatcher::start(self.roots, move |event| {
			if let Some(events) = &events {
				let _ = events.send(event_dto(&event));
			}
			let _ = tx.send(event);
		})
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
		let (registry, cache) = daemon_registry(&config, &roots);
		Ok(Self {
			config_root: rules_config_root(&roots)?,
			registry,
			cache,
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
		let (watcher_updates_tx, watcher_updates_rx) = mpsc::channel();
		Self {
			policy,
			tx,
			rx,
			watcher: None,
			watcher_epoch: 0,
			watcher_updates_tx,
			watcher_updates_rx,
			watcher_failure: None,
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

fn daemon_registry(
	config: &DaemonWorkspaceConfig,
	roots: &[PathBuf],
) -> (LocalWorkspaceRegistry, LocalResourceCache) {
	let cache = LocalResourceCache::default();
	let registry = LocalWorkspaceRegistry::local_with_cache(
		LocalWorkspaceOptions::new(roots.to_vec(), config.project.clone())
			.with_cache_dir(config.cache_dir.as_ref().map(PathBuf::from))
			.with_detailed_telemetry(telemetry::export_enabled()),
		cache.clone(),
	);
	(registry, cache)
}

fn handle_protocol(daemon: &mut WorkspaceDaemon, request: ProtocolRequest) -> ProtocolResponse {
	match request {
		ProtocolRequest::Query(request) => {
			if let Err(error) = request.validate() {
				return ProtocolResponse::Error(error);
			}
			match handle_query(daemon, *request) {
				Ok(response) => ProtocolResponse::Query(Box::new(response)),
				Err(error) => ProtocolResponse::Error(error),
			}
		}
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
	let source_refresh = matches!(&request.command, Command::WorkspaceRefresh);
	let recover_live_watcher = match drain_live_events(daemon, source_refresh) {
		Ok(()) => false,
		Err(error)
			if matches!(&request.command, Command::WorkspaceRefresh)
				&& error.code == "live_watcher_failed" =>
		{
			true
		}
		Err(error) => return Err(error),
	};
	match request.command {
		Command::WorkspaceRefresh => {
			let response = daemon.refresh_cancellable(WorkspaceCancellation::default())?;
			if recover_live_watcher {
				restart_live_watcher(daemon)?;
			}
			Ok(response)
		}
		Command::WorkspaceSourceSetReplace { source_set } => {
			let source_set = parse_memory_source_set(source_set)?;
			validate_memory_source_set_limits(&daemon.cache, &source_set, MEMORY_SOURCE_LIMITS)?;
			let srcset = source_set.srcset.clone();
			let update = daemon.cache.replace_memory_source_set(source_set);
			refresh_memory_source_set(daemon, update, format!("source set `{srcset}` replaced"))
		}
		Command::WorkspaceSourceSetRemove { srcset } => {
			validate_srcset(&srcset)?;
			let update = daemon.cache.remove_memory_source_set(&srcset);
			refresh_memory_source_set(daemon, update, format!("source set `{srcset}` removed"))
		}
	}
}

fn handle_query(
	daemon: &mut WorkspaceDaemon,
	request: QueryRequest,
) -> Result<QueryResponse, QueryError> {
	if let Query::SyntaxParse(query) = &request.query {
		return syntax::syntax_parse_response(query.to_owned());
	}
	if let Query::DiffImpactCompare(query) = &request.query {
		return diff_impact_compare_response(query.to_owned());
	}
	if matches!(&request.query, Query::WorkspaceStatus) {
		return match drain_live_events(daemon, true) {
			Ok(()) => workspace_status(&daemon.roots, &daemon.registry),
			Err(error) if error.code == "live_watcher_failed" => Ok(live_watcher_failed_status(
				&daemon.roots,
				&daemon.registry,
				error.message,
			)),
			Err(error) => Err(error),
		};
	}
	drain_live_events(daemon, request.consistency == Consistency::StaleOk)?;
	if let Query::QueryDescribe(query) = &request.query {
		return query_describe_response(query.verb.as_deref());
	}
	let requires_fresh_change_material = matches!(
		&request.query,
		Query::ChangeReview(_) | Query::ChangeContext(_)
	);
	if (request.consistency == Consistency::RefreshIfStale || requires_fresh_change_material)
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
	let global_change_overlay = match &request.query {
		Query::ChangeReview(query) => query.workspace.is_none() || daemon.roots.len() == 1,
		Query::ChangeContext(query) => query.workspace.is_none() || daemon.roots.len() == 1,
		_ => false,
	};
	let git_change_failure = optional_git_change_failure(&request.query, &daemon.roots)?;
	let change_overlay_failure =
		if matches!(&request.query, Query::ChangeReview(_)) && global_change_overlay {
			daemon.request_change_overlay()?;
			None
		} else if matches!(&request.query, Query::ChangeContext(_))
			&& global_change_overlay
			&& git_change_failure.is_none()
		{
			daemon.request_change_overlay().err()
		} else {
			None
		};
	let change_overlay_failure = git_change_failure.or(change_overlay_failure);
	if request.consistency == Consistency::Current
		&& daemon.registry.queries().staleness().is_stale()
	{
		return Err(QueryError::new(
			"workspace_stale",
			"workspace is stale; request consistency refresh-if-stale or stale-ok",
		));
	}
	let snapshot =
		daemon.registry.queries().snapshot_arc().ok_or_else(|| {
			QueryError::new("workspace_loading", "workspace snapshot is not ready")
		})?;
	let current_generation = Some(WorkspaceGeneration(snapshot.generation.value()));
	let response_roots = daemon.roots.clone();
	let response_config_root = daemon.config_root.clone();
	let response = ResponseContext {
		roots: &response_roots,
		config_root: &response_config_root,
		generation: current_generation,
	};
	dispatch_loaded_query(daemon, snapshot, response, request, change_overlay_failure)
}

fn live_watcher_failed_status(
	roots: &[PathBuf],
	registry: &LocalWorkspaceRegistry,
	message: String,
) -> QueryResponse {
	let mut status = workspace_status_result(roots, registry);
	status.phase = WorkspacePhase::Failed;
	status.failure = Some(WorkspaceFailureDto {
		resource: Some("live_watcher".to_string()),
		message: message.clone(),
	});
	status.stale = true;
	status.stale_summary = message.clone();
	for root in &mut status.roots {
		root.stale = true;
		root.stale_summary = message.clone();
	}
	QueryResponse {
		generation: status.generation,
		result: QueryResult::WorkspaceStatus(status),
		next_cursor: None,
	}
}

pub(super) fn concurrent_snapshot_query(query: &Query) -> bool {
	query.requires_workspace_snapshot()
		&& !matches!(
			query,
			Query::ChangeReview(_) | Query::ChangeContext(_) | Query::Notes(_)
		)
}

pub(super) fn handle_stale_snapshot_query(
	published: Option<PublishedSnapshot>,
	request: QueryRequest,
) -> ProtocolResponse {
	let published = match published {
		Some(published) => published,
		None => {
			return ProtocolResponse::Error(QueryError::new(
				"workspace_loading",
				"workspace snapshot is still loading; retry after workspace.status reports phase ready",
			));
		}
	};
	let generation = Some(WorkspaceGeneration(published.snapshot.generation.value()));
	let response = ResponseContext {
		roots: &published.context.roots,
		config_root: &published.context.config_root,
		generation,
	};
	let QueryRequest { query, page, .. } = request;
	dispatch_snapshot_query(
		&published.context,
		published.snapshot,
		response,
		query,
		page,
	)
	.map_or_else(ProtocolResponse::Error, |response| {
		ProtocolResponse::Query(Box::new(response))
	})
}

fn dispatch_loaded_query(
	daemon: &mut WorkspaceDaemon,
	snapshot: Arc<WorkspaceSnapshot>,
	response: ResponseContext<'_>,
	request: QueryRequest,
	change_overlay_failure: Option<QueryError>,
) -> Result<QueryResponse, QueryError> {
	let QueryRequest { query, page, .. } = request;
	match query {
		Query::ChangeContext(query) => {
			change_context_response(daemon, &snapshot, response, query, change_overlay_failure)
		}
		Query::Notes(query) => notes_response(daemon, &snapshot, query, page, response.generation),
		query => {
			let context = SnapshotQueryContext {
				roots: Arc::from(daemon.roots.clone()),
				config_root: Arc::new(daemon.config_root.clone()),
				cache: daemon.cache.clone(),
			};
			dispatch_snapshot_query(&context, snapshot, response, query, page)
		}
	}
}

fn dispatch_snapshot_query(
	context: &SnapshotQueryContext,
	snapshot: Arc<WorkspaceSnapshot>,
	response: ResponseContext<'_>,
	query: Query,
	page: Page,
) -> Result<QueryResponse, QueryError> {
	let current_generation = response.generation;
	match query {
		Query::QueryDescribe(_) => unreachable!("query describe handled before snapshot load"),
		Query::WorkspaceStatus => unreachable!("workspace status handled before snapshot load"),
		Query::SyntaxParse(_) => {
			unreachable!("stateless syntax parse handled before snapshot load")
		}
		Query::DiffImpactCompare(_) => {
			unreachable!("transactional diff impact handled before snapshot load")
		}
		Query::TreeChildren(query) => {
			tree_children_response(&snapshot, &context.roots, query, page, current_generation)
		}
		Query::SymbolSearch(query) => {
			symbol_search_response(&snapshot, &context.roots, query, page, current_generation)
		}
		Query::SymbolInsights(query) => {
			symbol_insights_response(&snapshot, &context.roots, query, current_generation)
		}
		Query::SymbolDetail(query) => symbol_detail_response(
			&snapshot,
			&context.roots,
			query.workspace.as_deref(),
			&query.uri,
			query.context_lines,
			current_generation,
		),
		Query::SyntaxTree(query) => {
			syntax::syntax_tree_response(&snapshot, &context.roots, query, current_generation)
		}
		Query::SymbolUsages(query) => {
			symbol_usages_response(&snapshot, &context.roots, query, page, current_generation)
		}
		Query::ViewRead(query) => view_read_response(
			&snapshot,
			&context.roots,
			&context.config_root,
			query,
			current_generation,
		),
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
				page,
			},
		),
		Query::RulesCheck(query) => rules_check_response(
			&context.cache,
			Arc::clone(&snapshot),
			response,
			RulesCheckEval {
				workspace: query.workspace,
				profile: query.profile,
				rules: query.rules,
				files: query.file,
				report: query.report,
				page,
			},
		),
		Query::RulesApplicable(query) => {
			rules_applicable_response(&snapshot, response, query, page)
		}
		Query::ChangeReview(query) => change_review_response(
			&context.cache,
			&snapshot,
			&context.roots,
			query,
			current_generation,
		),
		Query::ChangeContext(_) => {
			unreachable!("change context is dispatched with exclusive workspace access")
		}
		Query::SymbolGraph(query) => {
			symbol_graph_response(&snapshot, &context.roots, query, current_generation)
		}
		Query::GraphPath(query) => {
			graph_path_response(&snapshot, &context.roots, query, current_generation)
		}
		Query::GraphCorridor(query) => {
			graph_corridor_response(&snapshot, &context.roots, query, current_generation)
		}
		Query::IdentityChildren(query) => {
			identity_children_response(&snapshot, &context.roots, query, current_generation)
		}
		Query::IdentityGraph(query) => {
			identity_graph_response(&snapshot, &context.roots, query, page, current_generation)
		}
		Query::MetricsCoupling(query) => {
			metrics_coupling_response(&snapshot, &context.roots, query, current_generation)
		}
		Query::ResolutionAudit(query) => {
			resolution_audit_response(&snapshot, &context.roots, query, page, current_generation)
		}
		Query::Notes(_) => {
			unreachable!("notes are dispatched with exclusive workspace access")
		}
	}
}

pub(super) fn query_describe_response(verb: Option<&str>) -> Result<QueryResponse, QueryError> {
	let result = describe_query_capabilities(verb).ok_or_else(|| {
		let available = describe_query_capabilities(None)
			.map(|result| {
				result
					.capabilities
					.into_iter()
					.map(|capability| capability.name)
					.collect::<Vec<_>>()
					.join(", ")
			})
			.unwrap_or_default();
		QueryError::new(
			"unknown_query",
			format!(
				"unknown query operation `{}` for protocol {}; available queries: {available}; next: recycle an older daemon with the current binary when the requested query is absent",
				verb.unwrap_or_default(),
				code_moniker_query::PROTOCOL_VERSION,
			),
		)
	})?;
	Ok(QueryResponse {
		generation: None,
		result: QueryResult::QueryDescribe(result),
		next_cursor: None,
	})
}

pub(super) fn stateless_protocol_response(request: &ProtocolRequest) -> Option<ProtocolResponse> {
	let ProtocolRequest::Query(request) = request else {
		return None;
	};
	let response = match &request.query {
		Query::QueryDescribe(query) => query_describe_response(query.verb.as_deref()),
		Query::SyntaxParse(query) => syntax::syntax_parse_response(query.clone()),
		Query::DiffImpactCompare(query) => diff_impact_compare_response(query.clone()),
		_ => return None,
	};
	Some(response.map_or_else(ProtocolResponse::Error, |response| {
		ProtocolResponse::Query(Box::new(response))
	}))
}
