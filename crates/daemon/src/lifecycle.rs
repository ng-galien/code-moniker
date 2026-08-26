use std::path::PathBuf;
use std::time::Instant;

use code_moniker_query::{
	BuildIdentity, DaemonWorkspaceConfig, MemorySourceRefreshDto, MemorySourceRefreshModeDto,
	QueryError, QueryResponse, QueryResult, WorkspaceFailureDto, WorkspaceGeneration,
	WorkspaceLifecycle, WorkspacePhase, WorkspaceRootStatus, WorkspaceStatus, WorkspaceTimingsDto,
	current_build_identity, remove_registry_entry_if_own, workspace_label,
};
use code_moniker_workspace::live::WorkspaceLiveRefreshPlan;
use code_moniker_workspace::registry::LocalWorkspaceRegistry;
use code_moniker_workspace::snapshot::{
	MemorySourceRefreshMode, WorkspaceCancellation, WorkspaceRequest, WorkspaceResource,
	WorkspaceSnapshot, WorkspaceTransition,
};

use crate::daemon::{DaemonLiveRefreshPolicy, WorkspaceDaemon};
use crate::helpers::root_status;
use crate::telemetry;

pub(super) fn reject_conflicting_daemons(config: &DaemonWorkspaceConfig) -> anyhow::Result<()> {
	for (path, entry) in code_moniker_query::list_registry_files()? {
		let shares_root = entry
			.workspace_roots
			.iter()
			.any(|root| config.roots.contains(root));
		if !shares_root {
			continue;
		}
		if code_moniker_query::pid_is_alive(entry.pid)
			&& !code_moniker_query::daemon_registry_heartbeat_expired(&entry)
		{
			anyhow::bail!(
				"a daemon already serves {} (pid {}, endpoint {}); stop it before starting another",
				entry.workspace_root,
				entry.pid,
				entry.endpoint
			);
		}
		remove_registry_entry_if_own(&path, &entry);
	}
	Ok(())
}

pub(super) fn drain_live_events(
	daemon: &mut WorkspaceDaemon,
	defer_auto_refresh: bool,
) -> Result<(), QueryError> {
	let watcher_installed = daemon.install_pending_live_watcher()?;
	let mut plan = std::iter::from_fn(|| daemon.live.rx.try_recv().ok())
		.fold(WorkspaceLiveRefreshPlan::default(), |plan, event| {
			plan.coalesce(WorkspaceLiveRefreshPlan::from_event(event))
		});
	if watcher_installed {
		let reconciliation = daemon.live_watcher_reconciliation_plan();
		if defer_auto_refresh {
			plan = plan.coalesce(reconciliation);
		} else {
			refresh_full_cancellable(daemon, WorkspaceCancellation::default())?;
		}
	}
	if !plan.is_empty() {
		if defer_auto_refresh {
			daemon.registry.live_commands().mark_stale(plan);
		} else {
			apply_live_plan_for_policy(daemon, plan)?;
		}
	}
	if !defer_auto_refresh
		&& daemon.live.policy == DaemonLiveRefreshPolicy::Auto
		&& daemon.registry.queries().staleness().is_stale()
	{
		refresh_stale(daemon)?;
	}
	Ok(())
}

fn apply_live_plan_for_policy(
	daemon: &mut WorkspaceDaemon,
	plan: WorkspaceLiveRefreshPlan,
) -> Result<(), QueryError> {
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
	observe_index_operation(daemon, "live", move |daemon| {
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
	})
}

pub(super) fn refresh_full_cancellable(
	daemon: &mut WorkspaceDaemon,
	cancellation: WorkspaceCancellation,
) -> Result<(), QueryError> {
	observe_index_operation(daemon, "full", move |daemon| {
		workspace_transition_result(
			daemon
				.registry
				.commands()
				.refresh(WorkspaceRequest::new("daemon-refresh").with_cancellation(cancellation)),
		)
	})
}

pub(super) fn refresh_stale(daemon: &mut WorkspaceDaemon) -> Result<(), QueryError> {
	observe_index_operation(daemon, "stale", |daemon| {
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
	})
}

fn observe_index_operation<T>(
	daemon: &mut WorkspaceDaemon,
	mode: &'static str,
	operation: impl FnOnce(&mut WorkspaceDaemon) -> Result<T, QueryError>,
) -> Result<T, QueryError> {
	let previous_generation = daemon
		.registry
		.queries()
		.snapshot()
		.map_or(0, |snapshot| snapshot.generation.value());
	let span = telemetry::index_operation_span(mode, previous_generation);
	let started = Instant::now();
	let result = span.in_scope(|| operation(daemon));
	span.in_scope(|| {
		let snapshot = result
			.as_ref()
			.ok()
			.and_then(|_| daemon.registry.queries().snapshot());
		let material =
			snapshot.and_then(|snapshot| daemon.cache.index_material(snapshot.index.generation));
		telemetry::finish_index_operation(
			&span,
			mode,
			previous_generation,
			started.elapsed(),
			result.is_ok(),
			snapshot,
			material.as_deref(),
		);
	});
	result
}

pub(super) fn workspace_transition_result(
	transition: WorkspaceTransition,
) -> Result<(), QueryError> {
	match transition {
		WorkspaceTransition::Ready { .. } => Ok(()),
		WorkspaceTransition::Failed { failure, .. } => {
			Err(QueryError::new("workspace_refresh_failed", failure.message))
		}
	}
}

pub(super) fn restart_live_watcher(daemon: &mut WorkspaceDaemon) -> Result<(), QueryError> {
	daemon
		.restart_live_watcher()
		.map_err(|err| QueryError::new("live_watcher_failed", err.to_string()))
}

pub(super) fn generation(registry: &LocalWorkspaceRegistry) -> Option<WorkspaceGeneration> {
	registry
		.queries()
		.snapshot()
		.map(|snapshot| WorkspaceGeneration(snapshot.generation.value()))
}

pub(super) fn workspace_status(
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

pub(super) fn workspace_status_without_snapshot(
	roots: &[PathBuf],
	lifecycle: WorkspaceLifecycle,
) -> QueryResponse {
	let summary = lifecycle
		.failure
		.as_ref()
		.map(|failure| failure.message.clone())
		.unwrap_or_else(|| lifecycle.phase.to_string());
	let status = WorkspaceStatus {
		producer: producer_identity(),
		root: workspace_label(roots),
		phase: lifecycle.phase,
		failure: lifecycle.failure,
		roots: roots
			.iter()
			.map(|root| WorkspaceRootStatus {
				root: root.display().to_string(),
				generation: None,
				files: 0,
				symbols: 0,
				references: 0,
				stale: false,
				stale_summary: summary.clone(),
			})
			.collect(),
		generation: None,
		files: 0,
		symbols: 0,
		references: 0,
		stale: false,
		stale_summary: summary,
		timings: WorkspaceTimingsDto::default(),
		runtime_dependencies: Vec::new(),
		effective_capabilities: Vec::new(),
	};
	QueryResponse {
		generation: None,
		result: QueryResult::WorkspaceStatus(status),
		next_cursor: None,
	}
}

pub(super) fn workspace_status_result(
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
	let failure = registry.queries().last_failure().map(workspace_failure_dto);
	WorkspaceStatus {
		producer: producer_identity(),
		root: workspace_label(roots),
		phase: if generation.is_some() {
			WorkspacePhase::Ready
		} else if failure.is_some() {
			WorkspacePhase::Failed
		} else {
			WorkspacePhase::Loading
		},
		failure,
		roots: root_statuses,
		generation,
		files,
		symbols,
		references,
		stale: staleness.is_stale(),
		stale_summary: staleness.summary(),
		timings: registry
			.queries()
			.snapshot()
			.map(workspace_timings_dto)
			.unwrap_or_default(),
		runtime_dependencies: Vec::new(),
		effective_capabilities: Vec::new(),
	}
}

pub(super) fn workspace_failure_dto(
	failure: &code_moniker_workspace::snapshot::WorkspaceFailure,
) -> WorkspaceFailureDto {
	WorkspaceFailureDto {
		resource: Some(
			match failure.resource {
				WorkspaceResource::SourceCatalog => "source_catalog",
				WorkspaceResource::CodeIndex => "code_index",
				WorkspaceResource::LinkageSnapshot => "linkage_snapshot",
				WorkspaceResource::ChangeOverlay => "change_overlay",
			}
			.to_string(),
		),
		message: failure.message.clone(),
	}
}

fn workspace_timings_dto(snapshot: &WorkspaceSnapshot) -> WorkspaceTimingsDto {
	let milliseconds =
		|duration: std::time::Duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
	WorkspaceTimingsDto {
		source_catalog_ms: milliseconds(snapshot.timings.source_catalog),
		extract_sources_ms: milliseconds(snapshot.timings.extract_sources),
		semantic_index_ms: milliseconds(snapshot.timings.semantic_index),
		code_index_ms: milliseconds(snapshot.timings.code_index),
		linkage_ms: milliseconds(snapshot.timings.linkage),
		change_overlay_ms: milliseconds(snapshot.timings.change_overlay),
		total_ms: milliseconds(snapshot.timings.total),
		memory_source_refresh: snapshot.timings.memory_source_refresh.map(|refresh| {
			MemorySourceRefreshDto {
				mode: match refresh.mode {
					MemorySourceRefreshMode::Bulk => MemorySourceRefreshModeDto::Bulk,
					MemorySourceRefreshMode::Incremental => MemorySourceRefreshModeDto::Incremental,
				},
				documents_total: refresh.documents_total,
				added: refresh.added,
				modified: refresh.modified,
				removed: refresh.removed,
				unchanged: refresh.unchanged,
				extraction_jobs: refresh.extraction_jobs,
				extraction_workers: refresh.extraction_workers,
				linkage_invocations: refresh.linkage_invocations,
			}
		}),
	}
}

pub(super) fn producer_identity() -> BuildIdentity {
	current_build_identity(env!("CARGO_PKG_VERSION")).unwrap_or_else(|error| BuildIdentity {
		version: env!("CARGO_PKG_VERSION").to_string(),
		fingerprint: format!("unavailable:{error}"),
	})
}
