use std::time::{Duration, Instant};

use crate::changes::ChangeOverlayPort;
use crate::code::CodeIndexPort;
use crate::linkage::LinkagePort;
use crate::snapshot::{
	ChangeOverlay, CodeIndex, CodeIndexFields, LinkageGraph, ResourceGeneration, SourceCatalog,
	SourceFileRecord, SourceFileRecordFields, WorkspaceFailure, WorkspaceRequest,
	WorkspaceResource, WorkspaceResult, WorkspaceSnapshot, WorkspaceTimings,
};
use crate::source::SourceCatalogPort;

pub(crate) fn build_complete_snapshot(
	source_catalog: &mut impl SourceCatalogPort,
	code_index: &mut impl CodeIndexPort,
	linkage: &mut impl LinkagePort,
	change_overlay: &mut impl ChangeOverlayPort,
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
	let changes = change_overlay.build_change_overlay(&catalog, &index, &linkage)?;
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
	source_catalog: &mut impl SourceCatalogPort,
	code_index: &mut impl CodeIndexPort,
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
	linkage: &mut impl LinkagePort,
	change_overlay: &mut impl ChangeOverlayPort,
	request: WorkspaceRequest,
	generation: ResourceGeneration,
) -> WorkspaceResult<WorkspaceSnapshot> {
	let current = current.ok_or_else(|| {
		WorkspaceFailure::new(
			WorkspaceResource::LinkageGraph,
			format!("{} requires an indexed workspace snapshot", request.label),
		)
	})?;
	let linkage_timer = Instant::now();
	let linkage = linkage.resolve_linkage(&current.index)?;
	let linkage_elapsed = linkage_timer.elapsed();
	let changes_timer = Instant::now();
	let changes =
		change_overlay.build_change_overlay(&current.catalog, &current.index, &linkage)?;
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

pub(crate) fn build_catalog_snapshot(
	source_catalog: &mut impl SourceCatalogPort,
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

fn load_catalog_for_index(
	current: Option<&WorkspaceSnapshot>,
	source_catalog: &mut impl SourceCatalogPort,
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
	CodeIndex::from_fields(CodeIndexFields {
		generation: catalog.generation,
		catalog_generation: catalog.generation,
		identity_scheme: crate::DEFAULT_IDENTITY_SCHEME.to_string(),
		sources: catalog_source_records(catalog),
		symbols: Vec::new(),
		references: Vec::new(),
		timings: Default::default(),
	})
}

fn catalog_source_records(catalog: &SourceCatalog) -> Vec<SourceFileRecord> {
	catalog
		.sources
		.iter()
		.enumerate()
		.map(|(idx, source)| {
			SourceFileRecord::from_fields(SourceFileRecordFields {
				id: source.id.clone(),
				uri: source.id.as_str().to_string(),
				source_root: idx,
				path: source.display_name.clone(),
				rel_path: source.display_name.clone(),
				anchor: source.display_name.clone(),
				language: source.language.clone().unwrap_or_default(),
				text: String::new(),
			})
		})
		.collect()
}

fn empty_linkage(catalog: &SourceCatalog, index: &CodeIndex) -> LinkageGraph {
	LinkageGraph::new(catalog.generation, index.generation, 0, 0)
}

fn empty_changes(catalog: &SourceCatalog, index: &CodeIndex) -> ChangeOverlay {
	ChangeOverlay::new(
		catalog.generation,
		catalog.generation,
		index.generation,
		Vec::new(),
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
