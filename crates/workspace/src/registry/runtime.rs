use std::path::PathBuf;
use std::sync::Arc;

use crate::changes::ChangeOverlayPort;
use crate::code::CodeIndexPort;
use crate::linkage::LinkagePort;
use crate::live::{WorkspaceLiveRefreshPlan, WorkspaceWatchRoot};
use crate::snapshot::{
	ResourceGeneration, WorkspaceFailure, WorkspaceRequest, WorkspaceResource, WorkspaceResult,
	WorkspaceSnapshot, WorkspaceTransition, WorkspaceView,
};
use crate::source::SourceCatalogPort;

use super::build::{
	LivePlanBuild, build_catalog_snapshot, build_change_overlay_snapshot, build_complete_snapshot,
	build_incremental_paths_snapshot, build_index_only_snapshot, build_linkage_snapshot,
};
use super::command::{
	WorkspaceCommand, WorkspaceCommandId, WorkspaceCommandKind, WorkspaceCommandSpec,
	WorkspaceScopeUri, WorkspaceSnapshotPublication,
};
use super::event::{
	WorkspaceEvent, WorkspaceEventContext, WorkspaceEventCursor, WorkspaceEventKind,
	WorkspaceEventLog,
};
use super::ports::{WorkspaceCommandPort, WorkspaceEventPort, WorkspacePorts, WorkspaceQueryPort};
use super::staleness::WorkspaceStaleness;
use super::state::WorkspaceState;

pub struct WorkspaceLivePlanTransition {
	transition: WorkspaceTransition,
	replace_watcher: bool,
}

impl WorkspaceLivePlanTransition {
	pub fn transition(self) -> WorkspaceTransition {
		self.transition
	}

	pub fn replace_watcher(&self) -> bool {
		self.replace_watcher
	}
}

pub struct WorkspaceRegistry<Sources, Index, Linkage, Changes> {
	runtime: WorkspaceRuntime<Sources, Index, Linkage, Changes>,
	events: WorkspaceEventLog,
	next_command_id: u64,
}

pub struct WorkspaceCommands<'a, Sources, Index, Linkage, Changes> {
	runtime: &'a mut WorkspaceRuntime<Sources, Index, Linkage, Changes>,
	events: &'a mut WorkspaceEventLog,
	next_command_id: &'a mut u64,
}

pub struct WorkspaceLiveCommands<'a, Sources, Index, Linkage, Changes> {
	runtime: &'a mut WorkspaceRuntime<Sources, Index, Linkage, Changes>,
	events: &'a mut WorkspaceEventLog,
	next_command_id: &'a mut u64,
}

pub struct WorkspaceQueries<'a, Sources, Index, Linkage, Changes> {
	runtime: &'a WorkspaceRuntime<Sources, Index, Linkage, Changes>,
}

pub struct WorkspaceEvents<'a> {
	events: &'a WorkspaceEventLog,
}

pub struct WorkspaceRuntime<Sources, Index, Linkage, Changes> {
	ports: WorkspacePorts<Sources, Index, Linkage, Changes>,
	state: WorkspaceState,
}

impl<Sources, Index, Linkage, Changes> WorkspaceRegistry<Sources, Index, Linkage, Changes> {
	pub fn new(ports: WorkspacePorts<Sources, Index, Linkage, Changes>) -> Self {
		Self {
			runtime: WorkspaceRuntime::new(ports),
			events: WorkspaceEventLog::default(),
			next_command_id: 1,
		}
	}

	pub fn commands(&mut self) -> WorkspaceCommands<'_, Sources, Index, Linkage, Changes> {
		WorkspaceCommands {
			runtime: &mut self.runtime,
			events: &mut self.events,
			next_command_id: &mut self.next_command_id,
		}
	}

	pub fn live_commands(&mut self) -> WorkspaceLiveCommands<'_, Sources, Index, Linkage, Changes> {
		WorkspaceLiveCommands {
			runtime: &mut self.runtime,
			events: &mut self.events,
			next_command_id: &mut self.next_command_id,
		}
	}

	pub fn queries(&self) -> WorkspaceQueries<'_, Sources, Index, Linkage, Changes> {
		WorkspaceQueries {
			runtime: &self.runtime,
		}
	}

	pub fn events(&self) -> WorkspaceEvents<'_> {
		WorkspaceEvents {
			events: &self.events,
		}
	}

	pub fn watch_roots(&self) -> Vec<WorkspaceWatchRoot> {
		self.runtime
			.ports
			.live_watch_roots(self.runtime.state.snapshot())
	}
}

impl<Sources, Index, Linkage, Changes> WorkspaceRuntime<Sources, Index, Linkage, Changes> {
	fn new(ports: WorkspacePorts<Sources, Index, Linkage, Changes>) -> Self {
		Self {
			ports,
			state: WorkspaceState::new(),
		}
	}
}

impl<'a, Sources, Index, Linkage, Changes> WorkspaceQueries<'a, Sources, Index, Linkage, Changes> {
	pub fn snapshot(&self) -> Option<&'a WorkspaceSnapshot> {
		self.runtime.state.snapshot()
	}

	pub fn snapshot_arc(&self) -> Option<Arc<WorkspaceSnapshot>> {
		self.runtime.state.snapshot_arc()
	}

	pub fn view(&self) -> Option<WorkspaceView<'a>> {
		self.snapshot().map(WorkspaceView::new)
	}

	pub fn last_failure(&self) -> Option<&'a WorkspaceFailure> {
		self.runtime.state.last_failure()
	}

	pub fn staleness(&self) -> WorkspaceStaleness {
		self.runtime.state.pending.staleness()
	}
}

impl WorkspaceEvents<'_> {
	pub fn event_cursor(&self) -> WorkspaceEventCursor {
		self.events.cursor()
	}

	pub fn events_since(&self, cursor: WorkspaceEventCursor) -> &[WorkspaceEvent] {
		self.events.since(cursor)
	}
}

impl<Sources, Index, Linkage, Changes> WorkspaceCommands<'_, Sources, Index, Linkage, Changes>
where
	Sources: SourceCatalogPort,
	Index: CodeIndexPort,
	Linkage: LinkagePort,
	Changes: ChangeOverlayPort,
{
	pub fn execute(&mut self, spec: WorkspaceCommandSpec) -> WorkspaceTransition {
		let command = WorkspaceCommand::new(
			self.allocate_command_id(),
			spec.scope_uri,
			spec.kind,
			spec.request,
		);
		self.run_command(command)
	}

	fn allocate_command_id(&mut self) -> WorkspaceCommandId {
		let id = WorkspaceCommandId::new(*self.next_command_id);
		*self.next_command_id += 1;
		id
	}

	fn run_command(&mut self, command: WorkspaceCommand) -> WorkspaceTransition {
		let generation = self.runtime.state.allocate_generation();
		let context = WorkspaceEventContext::new(command.scope_uri, generation, command.id);
		publish_command_started(self.events, &context);
		let result = run_workspace_command(self.runtime, command.kind, command.request, generation);
		publish_command_finished(self.runtime, self.events, &context, result)
	}

	pub fn refresh(&mut self, request: WorkspaceRequest) -> WorkspaceTransition {
		self.execute(WorkspaceCommandSpec::new(
			WorkspaceCommandKind::Refresh,
			WorkspaceScopeUri::workspace(),
			request,
		))
	}

	pub fn load_catalog(&mut self, request: WorkspaceRequest) -> WorkspaceTransition {
		self.execute(WorkspaceCommandSpec::new(
			WorkspaceCommandKind::LoadSources,
			WorkspaceScopeUri::workspace(),
			request,
		))
	}

	pub fn load_index(&mut self, request: WorkspaceRequest) -> WorkspaceTransition {
		self.execute(WorkspaceCommandSpec::new(
			WorkspaceCommandKind::BuildIndex,
			WorkspaceScopeUri::workspace(),
			request,
		))
	}

	pub fn resolve_linkage(&mut self, request: WorkspaceRequest) -> WorkspaceTransition {
		self.execute(WorkspaceCommandSpec::new(
			WorkspaceCommandKind::ResolveLinkage,
			WorkspaceScopeUri::workspace(),
			request,
		))
	}

	pub fn refresh_paths(
		&mut self,
		request: WorkspaceRequest,
		paths: Vec<PathBuf>,
	) -> WorkspaceTransition {
		let command = WorkspaceCommand::new(
			self.allocate_command_id(),
			WorkspaceScopeUri::workspace(),
			WorkspaceCommandKind::RefreshPaths,
			request,
		);
		let generation = self.runtime.state.allocate_generation();
		let context = WorkspaceEventContext::new(command.scope_uri, generation, command.id);
		publish_command_started(self.events, &context);
		let result = build_incremental_paths_snapshot(
			self.runtime.state.snapshot(),
			&mut self.runtime.ports.code_index,
			&mut self.runtime.ports.linkage,
			command.request,
			&paths,
			generation,
		);
		publish_command_finished(self.runtime, self.events, &context, result)
	}

	pub fn refresh_changes(&mut self, request: WorkspaceRequest) -> WorkspaceTransition {
		let command = WorkspaceCommand::new(
			self.allocate_command_id(),
			WorkspaceScopeUri::workspace(),
			WorkspaceCommandKind::RefreshChanges,
			request,
		);
		let generation = self.runtime.state.allocate_generation();
		let context = WorkspaceEventContext::new(command.scope_uri, generation, command.id);
		publish_command_started(self.events, &context);
		let result = build_change_overlay_snapshot(
			self.runtime.state.snapshot(),
			&mut self.runtime.ports.change_overlay,
			command.request,
			generation,
		);
		publish_command_finished(self.runtime, self.events, &context, result)
	}

	pub fn publish_snapshot(
		&mut self,
		publication: WorkspaceSnapshotPublication,
	) -> WorkspaceTransition {
		let command = WorkspaceCommand::new(
			self.allocate_command_id(),
			publication.scope_uri,
			WorkspaceCommandKind::PublishSnapshot,
			publication.request,
		);
		let context = WorkspaceEventContext::new(
			command.scope_uri,
			publication.snapshot.generation,
			command.id,
		);
		publish_command_started(self.events, &context);
		let transition = self.runtime.state.adopt_snapshot_arc(publication.snapshot);
		events_for_ready_transition(self.events, &context, &transition);
		transition
	}
}

impl<Sources, Index, Linkage, Changes> WorkspaceLiveCommands<'_, Sources, Index, Linkage, Changes>
where
	Sources: SourceCatalogPort,
	Index: CodeIndexPort,
	Linkage: LinkagePort,
	Changes: ChangeOverlayPort,
{
	pub fn apply_plan(
		&mut self,
		request: WorkspaceRequest,
		plan: WorkspaceLiveRefreshPlan,
	) -> WorkspaceLivePlanTransition {
		let command = WorkspaceCommand::new(
			self.allocate_command_id(),
			WorkspaceScopeUri::workspace(),
			WorkspaceCommandKind::RefreshLivePlan,
			request,
		);
		run_live_plan(self.runtime, self.events, command, &plan)
	}

	pub fn refresh_stale(&mut self, request: WorkspaceRequest) -> WorkspaceLivePlanTransition {
		let plan = self.runtime.state.pending.take();
		if plan.is_empty()
			&& let Some(snapshot) = self.runtime.state.snapshot()
		{
			return WorkspaceLivePlanTransition {
				transition: WorkspaceTransition::Ready {
					generation: snapshot.generation,
				},
				replace_watcher: false,
			};
		}
		let command = WorkspaceCommand::new(
			self.allocate_command_id(),
			WorkspaceScopeUri::workspace(),
			WorkspaceCommandKind::RefreshStale,
			request,
		);
		let live = run_live_plan(self.runtime, self.events, command, &plan);
		if !plan.is_empty() && matches!(live.transition, WorkspaceTransition::Failed { .. }) {
			let current = current_generation(self.runtime);
			self.runtime.state.pending.coalesce(current, plan);
		}
		live
	}

	pub fn mark_stale(&mut self, plan: WorkspaceLiveRefreshPlan) -> WorkspaceStaleness {
		let plan = plan.without_notes();
		if plan.is_empty() {
			return self.runtime.state.pending.staleness();
		}
		let command_id = self.allocate_command_id();
		let current = current_generation(self.runtime);
		let staleness = self.runtime.state.pending.coalesce(current, plan);
		let context = WorkspaceEventContext::new(
			WorkspaceScopeUri::workspace(),
			current.unwrap_or_else(|| ResourceGeneration::new(0)),
			command_id,
		);
		self.events
			.publish(context.event(WorkspaceEventKind::StaleMarked));
		staleness
	}

	fn allocate_command_id(&mut self) -> WorkspaceCommandId {
		let id = WorkspaceCommandId::new(*self.next_command_id);
		*self.next_command_id += 1;
		id
	}
}

fn run_live_plan<Sources, Index, Linkage, Changes>(
	runtime: &mut WorkspaceRuntime<Sources, Index, Linkage, Changes>,
	events: &mut WorkspaceEventLog,
	command: WorkspaceCommand,
	plan: &WorkspaceLiveRefreshPlan,
) -> WorkspaceLivePlanTransition
where
	Sources: SourceCatalogPort,
	Index: CodeIndexPort,
	Linkage: LinkagePort,
	Changes: ChangeOverlayPort,
{
	let generation = runtime.state.allocate_generation();
	let context = WorkspaceEventContext::new(command.scope_uri, generation, command.id);
	publish_command_started(events, &context);
	let result = LivePlanBuild {
		current: runtime.state.snapshot(),
		source_catalog: &mut runtime.ports.source_catalog,
		code_index: &mut runtime.ports.code_index,
		linkage: &mut runtime.ports.linkage,
		change_overlay: &mut runtime.ports.change_overlay,
	}
	.build(command.request, plan, generation);
	let replace_watcher = result
		.as_ref()
		.map(|result| result.replace_watcher)
		.unwrap_or_else(|_| plan.requires_rescan());
	let transition = publish_command_finished(
		runtime,
		events,
		&context,
		result.map(|result| result.snapshot),
	);
	WorkspaceLivePlanTransition {
		transition,
		replace_watcher,
	}
}

fn current_generation<Sources, Index, Linkage, Changes>(
	runtime: &WorkspaceRuntime<Sources, Index, Linkage, Changes>,
) -> Option<ResourceGeneration> {
	runtime.state.snapshot().map(|snapshot| snapshot.generation)
}

fn publish_command_started(events: &mut WorkspaceEventLog, context: &WorkspaceEventContext) {
	events.publish(context.event(WorkspaceEventKind::CommandAccepted));
	events.publish(context.event(WorkspaceEventKind::WorkStarted));
}

fn publish_command_finished<Sources, Index, Linkage, Changes>(
	runtime: &mut WorkspaceRuntime<Sources, Index, Linkage, Changes>,
	events: &mut WorkspaceEventLog,
	context: &WorkspaceEventContext,
	result: WorkspaceResult<WorkspaceSnapshot>,
) -> WorkspaceTransition {
	match runtime.state.publish(result) {
		WorkspaceTransition::Ready { generation } => {
			events.publish(context.event(WorkspaceEventKind::SnapshotPublished));
			events.publish(context.event(WorkspaceEventKind::WorkCompleted));
			WorkspaceTransition::Ready { generation }
		}
		WorkspaceTransition::Failed {
			failure,
			preserved_generation,
		} => {
			events.publish(context.event(WorkspaceEventKind::WorkFailed));
			WorkspaceTransition::Failed {
				failure,
				preserved_generation,
			}
		}
	}
}

fn events_for_ready_transition(
	events: &mut WorkspaceEventLog,
	context: &WorkspaceEventContext,
	transition: &WorkspaceTransition,
) {
	match transition {
		WorkspaceTransition::Ready { .. } => {
			events.publish(context.event(WorkspaceEventKind::SnapshotPublished));
			events.publish(context.event(WorkspaceEventKind::WorkCompleted));
		}
		WorkspaceTransition::Failed { .. } => {
			events.publish(context.event(WorkspaceEventKind::WorkFailed));
		}
	}
}

impl<Sources, Index, Linkage, Changes> WorkspaceQueryPort
	for WorkspaceQueries<'_, Sources, Index, Linkage, Changes>
{
	fn snapshot(&self) -> Option<&WorkspaceSnapshot> {
		self.runtime.state.snapshot()
	}

	fn snapshot_arc(&self) -> Option<Arc<WorkspaceSnapshot>> {
		self.runtime.state.snapshot_arc()
	}

	fn view(&self) -> Option<WorkspaceView<'_>> {
		self.snapshot().map(WorkspaceView::new)
	}

	fn last_failure(&self) -> Option<&WorkspaceFailure> {
		self.runtime.state.last_failure()
	}
}

impl WorkspaceEventPort for WorkspaceEvents<'_> {
	fn event_cursor(&self) -> WorkspaceEventCursor {
		self.events.cursor()
	}

	fn events_since(&self, cursor: WorkspaceEventCursor) -> &[WorkspaceEvent] {
		self.events.since(cursor)
	}
}

impl<Sources, Index, Linkage, Changes> WorkspaceCommandPort
	for WorkspaceCommands<'_, Sources, Index, Linkage, Changes>
where
	Sources: SourceCatalogPort,
	Index: CodeIndexPort,
	Linkage: LinkagePort,
	Changes: ChangeOverlayPort,
{
	fn execute_command(
		&mut self,
		kind: WorkspaceCommandKind,
		scope_uri: WorkspaceScopeUri,
		request: WorkspaceRequest,
	) -> WorkspaceTransition {
		self.execute(WorkspaceCommandSpec::new(kind, scope_uri, request))
	}

	fn publish_snapshot(
		&mut self,
		publication: WorkspaceSnapshotPublication,
	) -> WorkspaceTransition {
		self.publish_snapshot(publication)
	}
}

fn run_workspace_command<Sources, Index, Linkage, Changes>(
	runtime: &mut WorkspaceRuntime<Sources, Index, Linkage, Changes>,
	kind: WorkspaceCommandKind,
	request: WorkspaceRequest,
	generation: ResourceGeneration,
) -> WorkspaceResult<WorkspaceSnapshot>
where
	Sources: SourceCatalogPort,
	Index: CodeIndexPort,
	Linkage: LinkagePort,
	Changes: ChangeOverlayPort,
{
	match kind {
		WorkspaceCommandKind::Refresh => build_complete_snapshot(
			&mut runtime.ports.source_catalog,
			&mut runtime.ports.code_index,
			&mut runtime.ports.linkage,
			&mut runtime.ports.change_overlay,
			request,
			generation,
		),
		WorkspaceCommandKind::LoadSources => {
			build_catalog_snapshot(&mut runtime.ports.source_catalog, request, generation)
		}
		WorkspaceCommandKind::BuildIndex => run_build_index_command(runtime, request, generation),
		WorkspaceCommandKind::ResolveLinkage => build_linkage_snapshot(
			runtime.state.snapshot(),
			&mut runtime.ports.linkage,
			&mut runtime.ports.change_overlay,
			request,
			generation,
		),
		WorkspaceCommandKind::RefreshPaths => Err(WorkspaceFailure::new(
			WorkspaceResource::CodeIndex,
			"RefreshPaths commands require changed paths",
		)),
		WorkspaceCommandKind::RefreshChanges => build_change_overlay_snapshot(
			runtime.state.snapshot(),
			&mut runtime.ports.change_overlay,
			request,
			generation,
		),
		WorkspaceCommandKind::RefreshLivePlan => Err(WorkspaceFailure::new(
			WorkspaceResource::CodeIndex,
			"RefreshLivePlan commands require a live refresh plan",
		)),
		WorkspaceCommandKind::PublishSnapshot => Err(WorkspaceFailure::new(
			WorkspaceResource::CodeIndex,
			"PublishSnapshot commands require a snapshot payload",
		)),
		WorkspaceCommandKind::MarkStale => Err(WorkspaceFailure::new(
			WorkspaceResource::CodeIndex,
			"MarkStale commands require a live refresh plan",
		)),
		WorkspaceCommandKind::RefreshStale => Err(WorkspaceFailure::new(
			WorkspaceResource::CodeIndex,
			"RefreshStale commands run through live refresh_stale",
		)),
	}
}

fn run_build_index_command<Sources, Index, Linkage, Changes>(
	runtime: &mut WorkspaceRuntime<Sources, Index, Linkage, Changes>,
	request: WorkspaceRequest,
	generation: ResourceGeneration,
) -> WorkspaceResult<WorkspaceSnapshot>
where
	Sources: SourceCatalogPort,
	Index: CodeIndexPort,
{
	let catalog_source = request
		.should_reuse_current_catalog()
		.then_some(runtime.state.snapshot())
		.flatten();
	build_index_only_snapshot(
		catalog_source,
		&mut runtime.ports.source_catalog,
		&mut runtime.ports.code_index,
		request,
		generation,
	)
}

#[cfg(test)]
mod tests {
	use std::fs;

	use crate::LocalWorkspaceOptions;

	use super::*;

	#[test]
	fn refresh_paths_publishes_symbols_from_modified_source() {
		let temp = tempfile::tempdir().expect("tempdir");
		let cache_dir = temp.path().join(".cache");
		let source = temp.path().join("lib.rs");
		fs::write(&source, "pub fn before_live_refresh() {}\n").expect("write source");
		let mut registry = crate::LocalWorkspaceRegistry::local(
			LocalWorkspaceOptions::new(vec![temp.path().to_path_buf()], None)
				.with_cache_dir(Some(cache_dir)),
		);

		assert!(matches!(
			registry
				.commands()
				.load_index(WorkspaceRequest::new("acceptance-index")),
			WorkspaceTransition::Ready { .. }
		));
		assert!(snapshot_has_symbol(&registry, "before_live_refresh"));

		fs::write(&source, "pub fn after_live_refresh() {}\n").expect("rewrite source");

		assert!(matches!(
			registry.commands().refresh_paths(
				WorkspaceRequest::new("acceptance-live-refresh"),
				vec![source]
			),
			WorkspaceTransition::Ready { .. }
		));

		assert!(snapshot_has_symbol(&registry, "after_live_refresh"));
		assert!(!snapshot_has_symbol(&registry, "before_live_refresh"));
	}

	#[test]
	fn mark_stale_records_staleness_without_touching_snapshot() {
		let (temp, source, mut registry) = indexed_registry("pub fn before_stale() {}\n");
		let _ = &temp;
		let cursor = registry.events().event_cursor();

		fs::write(&source, "pub fn after_stale() {}\n").expect("rewrite source");
		let staleness = registry
			.live_commands()
			.mark_stale(WorkspaceLiveRefreshPlan::from_event(
				crate::live::WorkspaceLiveEvent::SourcesChanged(vec![source.clone()]),
			));

		assert!(staleness.is_stale());
		assert_eq!(staleness.stale_paths, vec![source]);
		assert!(staleness.since_generation.is_some());
		assert!(snapshot_has_symbol(&registry, "before_stale"));
		assert_eq!(
			registry
				.events()
				.events_since(cursor)
				.iter()
				.map(|event| event.kind)
				.collect::<Vec<_>>(),
			vec![WorkspaceEventKind::StaleMarked]
		);
	}

	#[test]
	fn refresh_stale_applies_coalesced_pending_plan() {
		let (temp, source, mut registry) = indexed_registry("pub fn before_stale() {}\n");
		let _ = &temp;

		fs::write(&source, "pub fn after_stale() {}\n").expect("rewrite source");
		registry
			.live_commands()
			.mark_stale(WorkspaceLiveRefreshPlan::from_event(
				crate::live::WorkspaceLiveEvent::SourcesChanged(vec![source.clone()]),
			));
		registry
			.live_commands()
			.mark_stale(WorkspaceLiveRefreshPlan::from_event(
				crate::live::WorkspaceLiveEvent::SourcesChanged(vec![source]),
			));

		let live = registry
			.live_commands()
			.refresh_stale(WorkspaceRequest::new("acceptance-refresh-stale"));
		assert!(!live.replace_watcher());
		assert!(matches!(
			live.transition(),
			WorkspaceTransition::Ready { .. }
		));
		assert!(snapshot_has_symbol(&registry, "after_stale"));
		assert!(!snapshot_has_symbol(&registry, "before_stale"));
		assert!(!registry.queries().staleness().is_stale());
	}

	#[test]
	fn refresh_stale_without_pending_is_a_noop() {
		let (temp, _source, mut registry) = indexed_registry("pub fn untouched() {}\n");
		let _ = &temp;
		let generation = registry.queries().snapshot().expect("snapshot").generation;
		let cursor = registry.events().event_cursor();

		let live = registry
			.live_commands()
			.refresh_stale(WorkspaceRequest::new("acceptance-noop"));

		assert!(matches!(
			live.transition(),
			WorkspaceTransition::Ready { generation: ready } if ready == generation
		));
		assert!(registry.events().events_since(cursor).is_empty());
	}

	#[test]
	fn refresh_stale_with_rescan_runs_complete_build() {
		let (temp, source, mut registry) = indexed_registry("pub fn before_stale() {}\n");
		let _ = &temp;

		fs::write(&source, "pub fn after_stale() {}\n").expect("rewrite source");
		registry
			.live_commands()
			.mark_stale(WorkspaceLiveRefreshPlan::from_event(
				crate::live::WorkspaceLiveEvent::RescanRequired,
			));

		let live = registry
			.live_commands()
			.refresh_stale(WorkspaceRequest::new("acceptance-rescan"));

		assert!(live.replace_watcher());
		assert!(matches!(
			live.transition(),
			WorkspaceTransition::Ready { .. }
		));
		assert!(snapshot_has_symbol(&registry, "after_stale"));
		assert!(!registry.queries().staleness().is_stale());
	}

	fn indexed_registry(
		body: &str,
	) -> (
		tempfile::TempDir,
		std::path::PathBuf,
		crate::LocalWorkspaceRegistry,
	) {
		let temp = tempfile::tempdir().expect("tempdir");
		let cache_dir = temp.path().join(".cache");
		let source = temp.path().join("lib.rs");
		fs::write(&source, body).expect("write source");
		let mut registry = crate::LocalWorkspaceRegistry::local(
			LocalWorkspaceOptions::new(vec![temp.path().to_path_buf()], None)
				.with_cache_dir(Some(cache_dir)),
		);
		assert!(matches!(
			registry
				.commands()
				.load_index(WorkspaceRequest::new("acceptance-index")),
			WorkspaceTransition::Ready { .. }
		));
		(temp, source, registry)
	}

	fn snapshot_has_symbol(registry: &crate::LocalWorkspaceRegistry, name: &str) -> bool {
		registry.queries().snapshot().is_some_and(|snapshot| {
			snapshot
				.index
				.symbols
				.iter()
				.any(|symbol| symbol.name.contains(name))
		})
	}
}
