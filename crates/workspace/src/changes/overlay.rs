use crate::code::CodeIndexSymbolProvider;
use crate::snapshot::{
	ChangeOverlay, ChangeOverlayReport, ChangeResource, CodeIndex, SourceCatalog, WorkspaceFailure,
	WorkspaceResource, WorkspaceResult,
};
use crate::source::{CodeIndexMaterial, LocalResourceCache};

use super::analyzer::ChangeAnalyzer;
use super::diff::{self, ChangeFile, ChangeRoot, ChangeScan};

pub trait ChangeOverlayPort {
	fn build_change_overlay(
		&mut self,
		catalog: &SourceCatalog,
		index: &CodeIndex,
	) -> WorkspaceResult<ChangeOverlay>;
}

pub struct LocalChangeOverlay {
	cache: LocalResourceCache,
}

impl LocalChangeOverlay {
	pub fn new(cache: LocalResourceCache) -> Self {
		Self { cache }
	}
}

impl ChangeOverlayPort for LocalChangeOverlay {
	fn build_change_overlay(
		&mut self,
		catalog: &SourceCatalog,
		index: &CodeIndex,
	) -> WorkspaceResult<ChangeOverlay> {
		let material = self.cache.index_material(index.generation).ok_or_else(|| {
			WorkspaceFailure::new(
				WorkspaceResource::ChangeOverlay,
				"code index material is unavailable",
			)
		})?;
		let generation = self.cache.next_generation();
		let change_index_span = tracing::info_span!("workspace.change_overlay.change_index");
		let change_index =
			change_index_span.in_scope(|| diff::build_change_index(change_scan(&material)));
		let report_span = tracing::info_span!("workspace.change_overlay.report");
		let overlay = report_span.in_scope(|| {
			ChangeOverlay::from_report(change_report(
				generation,
				catalog.generation,
				index.generation,
				change_index,
				&material,
			))
		});
		Ok(overlay)
	}
}

pub fn build_semantic_review(
	material: &CodeIndexMaterial,
) -> super::semantic::review::SemanticReview {
	super::semantic::review::build_semantic_review(&change_scan(material))
}

pub fn build_semantic_review_for_roots(
	material: &CodeIndexMaterial,
	selected_roots: &[usize],
) -> Result<super::semantic::review::SemanticReview, crate::git_runtime::GitRuntimeError> {
	let scan = change_scan(material);
	let roots = selected_roots
		.iter()
		.filter_map(|index| scan.roots.get(*index))
		.map(|root| (root.label.to_string(), root.path.to_path_buf()))
		.collect::<Vec<_>>();
	let diffs = super::semantic::review::collect_review_diffs(&roots);
	if !diffs.any_root_resolved() {
		let failure = diffs.acquisition_failure().cloned().unwrap_or_else(|| {
			crate::git_runtime::GitRuntimeError {
				category: "command_failed".to_string(),
				message: "Git change acquisition failed without a typed cause".to_string(),
			}
		});
		return Err(crate::git_runtime::GitRuntimeError {
			category: failure.category,
			message: format!(
				"cannot collect Git changes for any selected root: {}; {}",
				failure.message,
				diffs.diagnostics.join("; ")
			),
		});
	}
	Ok(super::semantic::review::build_semantic_review_from(
		&scan, &diffs,
	))
}

fn change_scan(material: &CodeIndexMaterial) -> ChangeScan<'_> {
	ChangeScan {
		roots: material
			.source_catalog
			.sources
			.roots
			.iter()
			.map(|root| ChangeRoot {
				label: &root.label,
				path: &root.path,
				ctx: &root.ctx,
				source_groups: &root.source_groups,
			})
			.collect(),
		files: material
			.files
			.iter()
			.enumerate()
			.map(|(file_idx, file)| ChangeFile {
				file_idx,
				source_root: file.source_root,
				path: &file.path,
				rel_path: &file.rel_path,
				anchor: &file.anchor,
				lang: file.lang,
				srcset: material
					.source_catalog
					.sources
					.files
					.get(file_idx)
					.and_then(|source| source.srcset.as_deref()),
				graph: &file.graph,
				source: &file.source,
			})
			.collect(),
	}
}

fn change_report(
	generation: crate::snapshot::ResourceGeneration,
	catalog_generation: crate::snapshot::ResourceGeneration,
	index_generation: crate::snapshot::ResourceGeneration,
	change_index: diff::ChangeIndex,
	material: &CodeIndexMaterial,
) -> ChangeOverlayReport {
	let provider = CodeIndexSymbolProvider::new(material);
	let changes = ChangeAnalyzer::new(&provider).analyze(&change_index.entries);
	ChangeOverlayReport {
		generation,
		catalog_generation,
		index_generation,
		scope: change_index.scope,
		resources: change_index
			.resources
			.into_iter()
			.map(|resource| ChangeResource {
				available: resource.available(),
				label: resource.label,
				message: resource.message,
			})
			.collect(),
		diagnostics: change_index.diagnostics,
		changes,
	}
}
