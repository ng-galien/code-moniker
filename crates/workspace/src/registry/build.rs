// code-moniker: ignore-file[smell-clone-reflex]
// Registry build steps clone source/index records into published workspace snapshots.
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::changes::ChangeOverlayPort;
use crate::code::{CodeIndexGraphDiff, CodeIndexPort};
use crate::linkage::{LinkageGraphDelta, LinkagePort, LinkageRefreshImpact};
use crate::live::WorkspaceLiveRefreshPlan;
use crate::snapshot::{
	ChangeOverlay, CodeIndex, LinkageSnapshot, RecordTable, ResourceGeneration, SourceCatalog,
	SourceFileRecord, SourceId, WorkspaceFailure, WorkspaceRequest, WorkspaceResource,
	WorkspaceResult, WorkspaceSnapshot, WorkspaceTimings,
};
use crate::source::SourceCatalogPort;

pub(crate) fn build_complete_snapshot(
	source_catalog: &mut (impl SourceCatalogPort + ?Sized),
	code_index: &mut (impl CodeIndexPort + ?Sized),
	linkage: &mut (impl LinkagePort + ?Sized),
	change_overlay: &mut (impl ChangeOverlayPort + ?Sized),
	request: WorkspaceRequest,
	generation: ResourceGeneration,
) -> WorkspaceResult<WorkspaceSnapshot> {
	let total_timer = Instant::now();
	let catalog_timer = Instant::now();
	let catalog = source_catalog.load_catalog(&request)?;
	let catalog_elapsed = catalog_timer.elapsed();
	let index_timer = Instant::now();
	let index = code_index.build_index(&catalog)?;
	let index_elapsed = index_timer.elapsed();
	let linkage_timer = Instant::now();
	let linkage = linkage.resolve_linkage(&index)?;
	let linkage_elapsed = linkage_timer.elapsed();
	let changes_timer = Instant::now();
	let changes = change_overlay.build_change_overlay(&catalog, &index)?;
	let changes_elapsed = changes_timer.elapsed();
	let timings = timings(
		catalog_elapsed,
		&index,
		index_elapsed,
		linkage_elapsed,
		changes_elapsed,
		total_timer.elapsed(),
	);
	Ok(WorkspaceSnapshot {
		generation,
		catalog,
		index,
		linkage,
		changes,
		timings,
	})
}

pub(crate) fn build_index_only_snapshot(
	current: Option<&WorkspaceSnapshot>,
	source_catalog: &mut (impl SourceCatalogPort + ?Sized),
	code_index: &mut (impl CodeIndexPort + ?Sized),
	request: WorkspaceRequest,
	generation: ResourceGeneration,
) -> WorkspaceResult<WorkspaceSnapshot> {
	let total_timer = Instant::now();
	let (catalog, catalog_elapsed) = load_catalog_for_index(current, source_catalog, &request)?;
	let index_timer = Instant::now();
	let index = code_index.build_index(&catalog)?;
	let index_elapsed = index_timer.elapsed();
	let linkage = empty_linkage(&catalog, &index);
	let changes = empty_changes(&catalog, &index);
	let timings = timings(
		catalog_elapsed,
		&index,
		index_elapsed,
		Duration::ZERO,
		Duration::ZERO,
		total_timer.elapsed(),
	);
	Ok(WorkspaceSnapshot {
		generation,
		catalog,
		index,
		linkage,
		changes,
		timings,
	})
}

pub(crate) fn build_linkage_snapshot(
	current: Option<&WorkspaceSnapshot>,
	linkage: &mut (impl LinkagePort + ?Sized),
	change_overlay: &mut (impl ChangeOverlayPort + ?Sized),
	request: WorkspaceRequest,
	generation: ResourceGeneration,
) -> WorkspaceResult<WorkspaceSnapshot> {
	let current = current.ok_or_else(|| {
		WorkspaceFailure::new(
			WorkspaceResource::LinkageSnapshot,
			format!("{} requires an indexed workspace snapshot", request.label),
		)
	})?;
	let linkage_timer = Instant::now();
	let linkage = linkage.resolve_linkage(&current.index)?;
	let linkage_elapsed = linkage_timer.elapsed();
	let changes_timer = Instant::now();
	let changes = change_overlay.build_change_overlay(&current.catalog, &current.index)?;
	let changes_elapsed = changes_timer.elapsed();
	let total = current.timings.source_catalog
		+ current.timings.code_index
		+ linkage_elapsed
		+ changes_elapsed;
	let timings = timings(
		current.timings.source_catalog,
		&current.index,
		current.timings.code_index,
		linkage_elapsed,
		changes_elapsed,
		total,
	);
	Ok(WorkspaceSnapshot {
		generation,
		catalog: current.catalog.clone(),
		index: current.index.clone(),
		linkage,
		changes,
		timings,
	})
}

pub(crate) fn build_change_overlay_snapshot(
	current: Option<&WorkspaceSnapshot>,
	change_overlay: &mut (impl ChangeOverlayPort + ?Sized),
	request: WorkspaceRequest,
	generation: ResourceGeneration,
) -> WorkspaceResult<WorkspaceSnapshot> {
	let current = current.ok_or_else(|| {
		WorkspaceFailure::new(
			WorkspaceResource::ChangeOverlay,
			format!("{} requires a workspace snapshot", request.label),
		)
	})?;
	let changes_timer = Instant::now();
	let changes = change_overlay.build_change_overlay(&current.catalog, &current.index)?;
	let changes_elapsed = changes_timer.elapsed();
	let total = current.timings.source_catalog
		+ current.timings.code_index
		+ current.timings.linkage
		+ changes_elapsed;
	let timings = timings(
		current.timings.source_catalog,
		&current.index,
		current.timings.code_index,
		current.timings.linkage,
		changes_elapsed,
		total,
	);
	Ok(WorkspaceSnapshot {
		generation,
		catalog: current.catalog.clone(),
		index: current.index.clone(),
		linkage: current.linkage.clone(),
		changes,
		timings,
	})
}

pub(crate) struct RefreshPorts<'a> {
	pub(crate) source_catalog: &'a mut (dyn SourceCatalogPort + Send),
	pub(crate) code_index: &'a mut (dyn CodeIndexPort + Send),
	pub(crate) linkage: &'a mut (dyn LinkagePort + Send),
}

pub(crate) fn build_incremental_paths_snapshot(
	current: Option<&WorkspaceSnapshot>,
	ports: RefreshPorts<'_>,
	request: WorkspaceRequest,
	paths: &[PathBuf],
	generation: ResourceGeneration,
) -> WorkspaceResult<WorkspaceSnapshot> {
	let RefreshPorts {
		source_catalog,
		code_index,
		linkage,
	} = ports;
	let current = current.ok_or_else(|| {
		WorkspaceFailure::new(
			WorkspaceResource::CodeIndex,
			format!("{} requires an indexed workspace snapshot", request.label),
		)
	})?;
	let total_timer = Instant::now();
	let catalog_timer = Instant::now();
	let extended_catalog = source_catalog.extend_catalog(&current.catalog, paths)?;
	let catalog_elapsed = catalog_timer.elapsed();
	let index_timer = Instant::now();
	let refresh = match &extended_catalog {
		Some(catalog) => code_index.refresh_catalog_paths(&current.index, catalog, paths)?,
		None => code_index.refresh_paths(&current.index, paths)?,
	};
	let changed_sources = refresh.changed_sources;
	let graph_diff = refresh.graph_diff;
	let index = refresh.index;
	let index_elapsed = index_timer.elapsed();
	let linkage_timer = Instant::now();
	let linkage = linkage.refresh_linkage(
		&current.linkage,
		&index,
		linkage_impact(changed_sources.clone(), paths.to_vec(), &graph_diff),
	)?;
	let linkage_elapsed = linkage_timer.elapsed();
	let catalog = extended_catalog.unwrap_or_else(|| current.catalog.clone());
	let changes = ChangeOverlay::new(
		generation,
		catalog.generation,
		index.generation,
		graph_diff.changed_symbols.clone(),
	);
	let timings = timings(
		current.timings.source_catalog + catalog_elapsed,
		&index,
		index_elapsed,
		linkage_elapsed,
		Duration::ZERO,
		total_timer.elapsed(),
	);
	Ok(WorkspaceSnapshot {
		generation,
		catalog,
		index,
		linkage,
		changes,
		timings,
	})
}

pub(crate) struct LivePlanBuild<'a> {
	pub(crate) current: Option<&'a WorkspaceSnapshot>,
	pub(crate) source_catalog: &'a mut (dyn SourceCatalogPort + Send),
	pub(crate) code_index: &'a mut (dyn CodeIndexPort + Send),
	pub(crate) linkage: &'a mut (dyn LinkagePort + Send),
	pub(crate) change_overlay: &'a mut (dyn ChangeOverlayPort + Send),
}

pub(crate) struct LivePlanSnapshot {
	pub(crate) snapshot: WorkspaceSnapshot,
	pub(crate) replace_watcher: bool,
}

#[derive(Clone)]
struct SnapshotBuildRequest {
	request: WorkspaceRequest,
	generation: ResourceGeneration,
}

impl LivePlanBuild<'_> {
	pub(crate) fn build(
		mut self,
		request: WorkspaceRequest,
		plan: &WorkspaceLiveRefreshPlan,
		generation: ResourceGeneration,
	) -> WorkspaceResult<LivePlanSnapshot> {
		let build = SnapshotBuildRequest {
			request,
			generation,
		};
		let code = self.build_code_snapshot(build.clone(), plan)?;
		let mut snapshot = code.snapshot;
		if plan.includes_git_base() && !plan.requires_rescan() {
			snapshot = build_change_overlay_snapshot(
				Some(&snapshot),
				self.change_overlay,
				build.request,
				build.generation,
			)?;
		}
		Ok(LivePlanSnapshot {
			snapshot,
			replace_watcher: code.replace_watcher,
		})
	}

	fn build_code_snapshot(
		&mut self,
		build: SnapshotBuildRequest,
		plan: &WorkspaceLiveRefreshPlan,
	) -> WorkspaceResult<LivePlanSnapshot> {
		if plan.requires_rescan() {
			return self.build_complete(build, true);
		}
		if plan.source_paths().is_empty() {
			return clone_current_snapshot(self.current, &build.request, build.generation).map(
				|snapshot| LivePlanSnapshot {
					snapshot,
					replace_watcher: false,
				},
			);
		}
		build_incremental_paths_snapshot(
			self.current,
			RefreshPorts {
				source_catalog: &mut *self.source_catalog,
				code_index: &mut *self.code_index,
				linkage: &mut *self.linkage,
			},
			build.request.clone(),
			plan.source_paths(),
			build.generation,
		)
		.map(|snapshot| LivePlanSnapshot {
			snapshot,
			replace_watcher: false,
		})
		.or_else(|_| self.build_complete(build, true))
	}

	fn build_complete(
		&mut self,
		build: SnapshotBuildRequest,
		replace_watcher: bool,
	) -> WorkspaceResult<LivePlanSnapshot> {
		build_complete_snapshot(
			self.source_catalog,
			self.code_index,
			self.linkage,
			self.change_overlay,
			build.request,
			build.generation,
		)
		.map(|snapshot| LivePlanSnapshot {
			snapshot,
			replace_watcher,
		})
	}
}

pub(crate) fn build_catalog_snapshot(
	source_catalog: &mut (impl SourceCatalogPort + ?Sized),
	request: WorkspaceRequest,
	generation: ResourceGeneration,
) -> WorkspaceResult<WorkspaceSnapshot> {
	let total_timer = Instant::now();
	let catalog_timer = Instant::now();
	let catalog = source_catalog.load_catalog(&request)?;
	let catalog_elapsed = catalog_timer.elapsed();
	let index = catalog_index(&catalog);
	let linkage = empty_linkage(&catalog, &index);
	let changes = empty_changes(&catalog, &index);
	let timings = timings(
		catalog_elapsed,
		&index,
		Duration::ZERO,
		Duration::ZERO,
		Duration::ZERO,
		total_timer.elapsed(),
	);
	Ok(WorkspaceSnapshot {
		generation,
		catalog,
		index,
		linkage,
		changes,
		timings,
	})
}

fn clone_current_snapshot(
	current: Option<&WorkspaceSnapshot>,
	request: &WorkspaceRequest,
	generation: ResourceGeneration,
) -> WorkspaceResult<WorkspaceSnapshot> {
	let current = current.ok_or_else(|| {
		WorkspaceFailure::new(
			WorkspaceResource::CodeIndex,
			format!("{} requires a workspace snapshot", request.label),
		)
	})?;
	let mut snapshot = current.clone();
	snapshot.generation = generation;
	Ok(snapshot)
}

fn load_catalog_for_index(
	current: Option<&WorkspaceSnapshot>,
	source_catalog: &mut (impl SourceCatalogPort + ?Sized),
	request: &WorkspaceRequest,
) -> WorkspaceResult<(SourceCatalog, Duration)> {
	match current {
		Some(snapshot) => Ok((snapshot.catalog.clone(), Duration::ZERO)),
		None => {
			let catalog_timer = Instant::now();
			let catalog = source_catalog.load_catalog(request)?;
			Ok((catalog, catalog_timer.elapsed()))
		}
	}
}

fn catalog_index(catalog: &SourceCatalog) -> CodeIndex {
	CodeIndex {
		generation: catalog.generation,
		catalog_generation: catalog.generation,
		identity_scheme: crate::DEFAULT_IDENTITY_SCHEME.to_string(),
		sources: catalog_source_records(catalog),
		symbols: RecordTable::from_records(Vec::new()),
		references: RecordTable::from_records(Vec::new()),
		timings: Default::default(),
	}
}

fn catalog_source_records(catalog: &SourceCatalog) -> Vec<SourceFileRecord> {
	catalog
		.sources
		.iter()
		.enumerate()
		.map(|(idx, source)| SourceFileRecord {
			id: source.id.clone(),
			uri: source.id.as_str().to_string(),
			source_root: idx,
			path: source.display_name.clone(),
			rel_path: source.display_name.clone(),
			anchor: source.display_name.clone(),
			language: source.language.clone().unwrap_or_default(),
			text: String::new(),
		})
		.collect()
}

fn empty_linkage(catalog: &SourceCatalog, index: &CodeIndex) -> LinkageSnapshot {
	LinkageSnapshot::new(catalog.generation, index.generation, 0, 0)
}

fn empty_changes(catalog: &SourceCatalog, index: &CodeIndex) -> ChangeOverlay {
	ChangeOverlay::new(
		catalog.generation,
		catalog.generation,
		index.generation,
		Vec::new(),
	)
}

fn linkage_impact(
	changed_sources: Vec<SourceId>,
	changed_paths: Vec<PathBuf>,
	graph_diff: &CodeIndexGraphDiff,
) -> LinkageRefreshImpact {
	LinkageRefreshImpact::with_graph_delta(
		changed_sources,
		changed_paths,
		LinkageGraphDelta::from_code_index(graph_diff.clone()),
	)
}

fn timings(
	source_catalog: Duration,
	index: &CodeIndex,
	code_index: Duration,
	linkage: Duration,
	change_overlay: Duration,
	total: Duration,
) -> WorkspaceTimings {
	WorkspaceTimings {
		source_catalog,
		extract_sources: index.timings.extract_sources,
		semantic_index: index.timings.semantic_index,
		code_index,
		linkage,
		change_overlay,
		total,
	}
}
