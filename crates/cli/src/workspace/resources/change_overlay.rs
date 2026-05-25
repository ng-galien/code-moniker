use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::core::uri::{UriConfig, to_uri};

use crate::workspace::git::{ChangeFile, ChangeRoot, ChangeScan};
use crate::workspace::resources::material::{LocalResourceCache, symbol_id};
use crate::workspace::session::{
	ChangeId, ChangeOverlay, ChangeOverlayPort, ChangeOverlayReport, ChangeRecord,
	ChangeRecordFields, ChangeResource, ChangeStatus, CodeIndex, LinkageGraph, SourceCatalog,
	WorkspaceFailure, WorkspaceResource, WorkspaceResult,
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
	let changes = change_records(&change_index.entries, material);
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

fn change_records(
	entries: &[crate::workspace::git::ChangeEntry],
	material: &crate::workspace::resources::material::CodeIndexMaterial,
) -> Vec<ChangeRecord> {
	entries
		.iter()
		.enumerate()
		.map(|(idx, entry)| {
			let symbol = entry
				.loc
				.map(|loc| symbol_id(loc.file, loc.def))
				.or_else(|| material.symbols_by_moniker.get(&entry.moniker).cloned());
			let source = entry
				.loc
				.and_then(|loc| material.files.get(loc.file))
				.map(|file| file.source_id.clone());
			ChangeRecord::from_fields(ChangeRecordFields {
				id: ChangeId::new(format!("change:{idx}")),
				status: change_status(entry.status),
				source,
				symbol,
				identity: moniker_identity(&entry.moniker),
				name: entry.name.clone(),
				kind: entry.kind.clone(),
				line_range: entry.line_range,
				hunk_count: entry.hunk_count,
			})
		})
		.collect()
}

fn change_status(status: crate::workspace::git::ChangeStatus) -> ChangeStatus {
	match status {
		crate::workspace::git::ChangeStatus::Added => ChangeStatus::Added,
		crate::workspace::git::ChangeStatus::Modified => ChangeStatus::Modified,
		crate::workspace::git::ChangeStatus::Removed => ChangeStatus::Removed,
	}
}

fn moniker_identity(moniker: &Moniker) -> String {
	to_uri(
		moniker,
		&UriConfig {
			scheme: crate::DEFAULT_SCHEME,
		},
	)
	.unwrap_or_else(|_| String::from_utf8_lossy(moniker.as_bytes()).to_string())
}
