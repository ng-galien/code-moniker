use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use code_moniker_core::lang::build_manifest::Manifest;

use crate::linkage::candidate::CandidateCatalog;
use crate::linkage::query::LinkageQuery;
use crate::linkage::store::LinkageStore;
use crate::path_util::normalize_path;
use crate::snapshot::{ReferenceId, ReferenceRecord, SourceId};
use crate::source::CodeIndexMaterial;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinkageRefreshImpact {
	changed_sources: Vec<SourceId>,
	changed_paths: Vec<PathBuf>,
}

impl LinkageRefreshImpact {
	pub fn new(changed_sources: Vec<SourceId>, changed_paths: Vec<PathBuf>) -> Self {
		Self {
			changed_sources,
			changed_paths,
		}
	}

	pub fn is_empty(&self) -> bool {
		self.changed_sources.is_empty() && self.changed_paths.is_empty()
	}
}

pub(super) struct LinkageGarbageCollector<'a> {
	store: &'a LinkageStore,
	changed_sources: BTreeSet<SourceId>,
	changed_source_files: BTreeSet<usize>,
	changed_source_references: Vec<ReferenceId>,
	changed_candidate_references: Vec<ReferenceId>,
	policy_source_roots: BTreeSet<usize>,
	missing_resolved_references: Vec<ReferenceId>,
}

impl<'a> LinkageGarbageCollector<'a> {
	pub(super) fn new(
		store: &'a LinkageStore,
		references: &'a [ReferenceRecord],
		material: &'a CodeIndexMaterial,
		candidates: &'a CandidateCatalog<'a>,
		impact: &LinkageRefreshImpact,
	) -> Self {
		let changed_sources = changed_sources(impact);
		let changed_source_files = changed_source_files(material, &changed_sources);
		Self {
			store,
			changed_source_references: changed_source_references(references, &changed_sources),
			changed_candidate_references: changed_candidate_references(
				references,
				material,
				candidates,
				&changed_source_files,
			),
			changed_source_files,
			policy_source_roots: policy_source_roots(material, &impact.changed_paths),
			missing_resolved_references: store.missing_resolved_references(),
			changed_sources,
		}
	}

	pub(super) fn collect(&self) -> BTreeSet<ReferenceId> {
		let mut stale = BTreeSet::new();
		self.mark_changed_source_references(&mut stale);
		self.mark_policy_references(&mut stale);
		self.mark_candidate_references(&mut stale);
		self.mark_resolved_target_references(&mut stale);
		stale.extend(self.missing_resolved_references.iter().cloned());
		stale
	}

	fn mark_changed_source_references(&self, stale: &mut BTreeSet<ReferenceId>) {
		stale.extend(self.changed_source_references.iter().cloned());
	}

	fn mark_policy_references(&self, stale: &mut BTreeSet<ReferenceId>) {
		for root in &self.policy_source_roots {
			if let Some(references) = self.store.indexes.references_by_source_root.get(root) {
				stale.extend(references.iter().cloned());
			}
		}
	}

	fn mark_candidate_references(&self, stale: &mut BTreeSet<ReferenceId>) {
		stale.extend(self.changed_candidate_references.iter().cloned());
		for source_file in &self.changed_source_files {
			if let Some(references) = self
				.store
				.indexes
				.references_by_candidate_source
				.get(source_file)
			{
				stale.extend(references.iter().cloned());
			}
		}
	}

	fn mark_resolved_target_references(&self, stale: &mut BTreeSet<ReferenceId>) {
		for source in &self.changed_sources {
			if let Some(references) = self.store.indexes.resolved_by_target_source.get(source) {
				stale.extend(references.iter().cloned());
			}
		}
	}
}

fn changed_sources(impact: &LinkageRefreshImpact) -> BTreeSet<SourceId> {
	impact.changed_sources.iter().cloned().collect()
}

fn changed_source_references(
	references: &[ReferenceRecord],
	changed_sources: &BTreeSet<SourceId>,
) -> Vec<ReferenceId> {
	references
		.iter()
		.filter(|reference| changed_sources.contains(&reference.source))
		.map(|reference| reference.id.clone())
		.collect()
}

fn changed_source_files(
	material: &CodeIndexMaterial,
	changed_sources: &BTreeSet<SourceId>,
) -> BTreeSet<usize> {
	material
		.files
		.iter()
		.enumerate()
		.filter(|(_, file)| changed_sources.contains(&file.source_id))
		.map(|(file_idx, _)| file_idx)
		.collect()
}

fn changed_candidate_references(
	references: &[ReferenceRecord],
	material: &CodeIndexMaterial,
	candidates: &CandidateCatalog<'_>,
	changed_source_files: &BTreeSet<usize>,
) -> Vec<ReferenceId> {
	if changed_source_files.is_empty() {
		return Vec::new();
	}
	references
		.iter()
		.filter(|reference| {
			let Some(query) = LinkageQuery::new(reference, material) else {
				return false;
			};
			candidates.matches_any_source(&query, changed_source_files)
		})
		.map(|reference| reference.id.clone())
		.collect()
}

fn policy_source_roots(material: &CodeIndexMaterial, paths: &[PathBuf]) -> BTreeSet<usize> {
	paths
		.iter()
		.filter(|path| Manifest::for_filename(path).is_some())
		.filter_map(|path| source_root_for_path(material, path))
		.collect()
}

fn source_root_for_path(material: &CodeIndexMaterial, path: &Path) -> Option<usize> {
	let path = normalize_path(path);
	material
		.source_catalog
		.sources
		.roots
		.iter()
		.enumerate()
		.filter_map(|(root_idx, root)| {
			let root_path = normalize_path(&root.path);
			path.starts_with(&root_path)
				.then_some((root_idx, root_path.components().count()))
		})
		.max_by_key(|(_, depth)| *depth)
		.map(|(root_idx, _)| root_idx)
}
