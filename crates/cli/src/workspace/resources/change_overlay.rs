use crate::workspace::git::{ChangeFile, ChangeRoot, ChangeScan};
use crate::workspace::resources::change_analyzer::ChangeAnalyzer;
use crate::workspace::resources::material::LocalResourceCache;
use crate::workspace::resources::symbol_provider::CodeIndexSymbolProvider;
use crate::workspace::session::{
	ChangeOverlay, ChangeOverlayPort, ChangeOverlayReport, ChangeResource, CodeIndex, LinkageGraph,
	SourceCatalog, WorkspaceFailure, WorkspaceResource, WorkspaceResult,
};

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
		_linkage: &LinkageGraph,
	) -> WorkspaceResult<ChangeOverlay> {
		let material = self.cache.index_material(index.generation).ok_or_else(|| {
			WorkspaceFailure::new(
				WorkspaceResource::ChangeOverlay,
				"code index material is unavailable",
			)
		})?;
		let generation = self.cache.next_generation();
		let change_index = crate::workspace::git::build_change_index(change_scan(&material));
		Ok(ChangeOverlay::from_report(change_report(
			generation,
			catalog.generation,
			index.generation,
			change_index,
			&material,
		)))
	}
}

fn change_scan(
	material: &crate::workspace::resources::material::CodeIndexMaterial,
) -> ChangeScan<'_> {
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
				graph: &file.graph,
				source: &file.source,
			})
			.collect(),
	}
}

fn change_report(
	generation: crate::workspace::session::ResourceGeneration,
	catalog_generation: crate::workspace::session::ResourceGeneration,
	index_generation: crate::workspace::session::ResourceGeneration,
	change_index: crate::workspace::git::ChangeIndex,
	material: &crate::workspace::resources::material::CodeIndexMaterial,
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
