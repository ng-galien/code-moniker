use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::core::uri::{UriConfig, from_uri};
use code_moniker_core::lang::build_manifest::Manifest;

use crate::linkage::candidate::CandidateCatalog;
use crate::linkage::decision::{
	ExternalOrigin, ReferenceLinkageDecision, ResolutionScope, UnknownReason,
};
use crate::linkage::query::LinkageQuery;
use crate::snapshot::{
	LinkageEdge, LinkageGraph, ReferenceId, ReferenceRecord, SourceId, SymbolId,
};
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
	current: &'a LinkageGraph,
	material: &'a CodeIndexMaterial,
	index: LinkageGcIndex,
}

struct LinkageGcIndex {
	changed_sources: BTreeSet<SourceId>,
	changed_source_files: BTreeSet<usize>,
	policy_source_roots: BTreeSet<usize>,
	reference_indexes: BTreeMap<ReferenceId, usize>,
	references_by_source: BTreeMap<SourceId, Vec<ReferenceId>>,
	references_by_source_root: BTreeMap<usize, Vec<ReferenceId>>,
	references_by_candidate_source: BTreeMap<usize, Vec<ReferenceId>>,
	symbol_sources: BTreeMap<SymbolId, SourceId>,
	resolved_by_target_source: BTreeMap<SourceId, Vec<ReferenceId>>,
	missing_resolved_references: Vec<ReferenceId>,
}

pub(super) struct LinkageSweep {
	reference_indexes: Vec<usize>,
	preserved_decisions: Vec<ReferenceLinkageDecision>,
}

impl<'a> LinkageGarbageCollector<'a> {
	pub(super) fn new(
		current: &'a LinkageGraph,
		references: &'a [ReferenceRecord],
		material: &'a CodeIndexMaterial,
		candidates: &'a CandidateCatalog<'a>,
		impact: &LinkageRefreshImpact,
	) -> Self {
		Self {
			current,
			material,
			index: LinkageGcIndex::new(current, references, material, candidates, impact),
		}
	}

	pub(super) fn collect(&self) -> LinkageSweep {
		let stale_references = self.mark_stale_references();
		let reference_indexes = stale_references
			.iter()
			.filter_map(|reference| self.index.reference_indexes.get(reference).copied())
			.collect::<Vec<_>>();
		let preserved_decisions = LinkageDecisionPreserver::new(
			self.current,
			self.material,
			&self.index,
			&stale_references,
		)
		.decisions();
		LinkageSweep {
			reference_indexes,
			preserved_decisions,
		}
	}

	fn mark_stale_references(&self) -> BTreeSet<ReferenceId> {
		let mut stale = BTreeSet::new();
		self.mark_changed_source_references(&mut stale);
		self.mark_policy_references(&mut stale);
		self.mark_candidate_references(&mut stale);
		self.mark_resolved_target_references(&mut stale);
		stale.extend(self.index.missing_resolved_references.iter().cloned());
		stale
	}

	fn mark_changed_source_references(&self, stale: &mut BTreeSet<ReferenceId>) {
		for source in &self.index.changed_sources {
			if let Some(references) = self.index.references_by_source.get(source) {
				stale.extend(references.iter().cloned());
			}
		}
	}

	fn mark_policy_references(&self, stale: &mut BTreeSet<ReferenceId>) {
		for root in &self.index.policy_source_roots {
			if let Some(references) = self.index.references_by_source_root.get(root) {
				stale.extend(references.iter().cloned());
			}
		}
	}

	fn mark_candidate_references(&self, stale: &mut BTreeSet<ReferenceId>) {
		for source_file in &self.index.changed_source_files {
			if let Some(references) = self.index.references_by_candidate_source.get(source_file) {
				stale.extend(references.iter().cloned());
			}
		}
	}

	fn mark_resolved_target_references(&self, stale: &mut BTreeSet<ReferenceId>) {
		for source in &self.index.changed_sources {
			if let Some(references) = self.index.resolved_by_target_source.get(source) {
				stale.extend(references.iter().cloned());
			}
		}
	}
}

struct LinkageDecisionPreserver<'a> {
	current: &'a LinkageGraph,
	material: &'a CodeIndexMaterial,
	index: &'a LinkageGcIndex,
	stale_references: &'a BTreeSet<ReferenceId>,
}

impl<'a> LinkageDecisionPreserver<'a> {
	fn new(
		current: &'a LinkageGraph,
		material: &'a CodeIndexMaterial,
		index: &'a LinkageGcIndex,
		stale_references: &'a BTreeSet<ReferenceId>,
	) -> Self {
		Self {
			current,
			material,
			index,
			stale_references,
		}
	}

	fn decisions(&self) -> Vec<ReferenceLinkageDecision> {
		let mut decisions = Vec::new();
		decisions.extend(self.preserved_resolved_decisions());
		decisions.extend(self.preserved_external_decisions());
		decisions.extend(self.preserved_manifest_blocked_decisions());
		decisions.extend(self.preserved_unresolved_decisions());
		decisions
	}

	fn preserved_resolved_decisions(&self) -> Vec<ReferenceLinkageDecision> {
		let mut targets = BTreeMap::<ReferenceId, Vec<SymbolId>>::new();
		for edge in &self.current.resolved {
			if self.preserve_resolved_edge(edge) {
				targets
					.entry(edge.reference.clone())
					.or_default()
					.push(edge.target.clone());
			}
		}
		targets
			.into_iter()
			.filter_map(|(reference, targets)| {
				self.index
					.reference_indexes
					.get(&reference)
					.map(|reference_idx| {
						ReferenceLinkageDecision::resolved(
							ResolutionScope::Global,
							*reference_idx,
							targets,
						)
					})
			})
			.collect()
	}

	fn preserved_external_decisions(&self) -> Vec<ReferenceLinkageDecision> {
		self.current
			.external
			.iter()
			.filter(|external| self.preserve_reference(&external.reference))
			.filter_map(|external| {
				let reference_idx = *self.index.reference_indexes.get(&external.reference)?;
				let origin = external_origin(&external.origin);
				let target = external_target(&external.target_identity, self.material);
				Some(match target {
					Some(target) => {
						ReferenceLinkageDecision::external_target(origin, reference_idx, target)
					}
					None => ReferenceLinkageDecision::external(origin, reference_idx),
				})
			})
			.collect()
	}

	fn preserved_manifest_blocked_decisions(&self) -> Vec<ReferenceLinkageDecision> {
		self.current
			.manifest_blocked
			.iter()
			.filter(|blocked| self.preserve_reference(&blocked.reference))
			.filter_map(|blocked| {
				self.index
					.reference_indexes
					.get(&blocked.reference)
					.map(|reference_idx| ReferenceLinkageDecision::manifest_blocked(*reference_idx))
			})
			.collect()
	}

	fn preserved_unresolved_decisions(&self) -> Vec<ReferenceLinkageDecision> {
		self.current
			.unresolved
			.iter()
			.filter(|unresolved| self.preserve_reference(&unresolved.reference))
			.filter_map(|unresolved| {
				self.index
					.reference_indexes
					.get(&unresolved.reference)
					.map(|reference_idx| {
						ReferenceLinkageDecision::unknown(
							UnknownReason::NoCandidate,
							*reference_idx,
						)
					})
			})
			.collect()
	}

	fn preserve_resolved_edge(&self, edge: &LinkageEdge) -> bool {
		self.preserve_reference(&edge.reference)
			&& self.index.symbol_sources.contains_key(&edge.target)
	}

	fn preserve_reference(&self, reference: &ReferenceId) -> bool {
		!self.stale_references.contains(reference)
			&& self.index.reference_indexes.contains_key(reference)
	}
}

impl LinkageGcIndex {
	fn new(
		current: &LinkageGraph,
		references: &[ReferenceRecord],
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog<'_>,
		impact: &LinkageRefreshImpact,
	) -> Self {
		let changed_sources = changed_sources(impact);
		let reference_indexes = reference_indexes(references);
		let symbol_sources = symbol_sources(material);
		Self {
			changed_source_files: changed_source_files(material, &changed_sources),
			policy_source_roots: policy_source_roots(material, &impact.changed_paths),
			references_by_source: references_by_source(references),
			references_by_source_root: references_by_source_root(references, material),
			references_by_candidate_source: references_by_candidate_source(
				references, material, candidates,
			),
			resolved_by_target_source: resolved_by_target_source(current, &symbol_sources),
			missing_resolved_references: missing_resolved_references(
				current,
				&reference_indexes,
				&symbol_sources,
			),
			reference_indexes,
			symbol_sources,
			changed_sources,
		}
	}
}

impl LinkageSweep {
	pub(super) fn reference_indexes(&self) -> &[usize] {
		&self.reference_indexes
	}

	pub(super) fn into_decisions(
		self,
		changed: Vec<ReferenceLinkageDecision>,
	) -> Vec<ReferenceLinkageDecision> {
		let mut decisions = self.preserved_decisions;
		decisions.extend(changed);
		decisions
	}
}

fn changed_sources(impact: &LinkageRefreshImpact) -> BTreeSet<SourceId> {
	impact.changed_sources.iter().cloned().collect()
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

fn reference_indexes(references: &[ReferenceRecord]) -> BTreeMap<ReferenceId, usize> {
	references
		.iter()
		.enumerate()
		.map(|(idx, reference)| (reference.id.clone(), idx))
		.collect()
}

fn references_by_source(references: &[ReferenceRecord]) -> BTreeMap<SourceId, Vec<ReferenceId>> {
	let mut index = BTreeMap::<SourceId, Vec<ReferenceId>>::new();
	for reference in references {
		index
			.entry(reference.source.clone())
			.or_default()
			.push(reference.id.clone());
	}
	index
}

fn references_by_source_root(
	references: &[ReferenceRecord],
	material: &CodeIndexMaterial,
) -> BTreeMap<usize, Vec<ReferenceId>> {
	let mut index = BTreeMap::<usize, Vec<ReferenceId>>::new();
	for reference in references {
		let Some(source_root) = reference_source_root(reference, material) else {
			continue;
		};
		index
			.entry(source_root)
			.or_default()
			.push(reference.id.clone());
	}
	index
}

fn references_by_candidate_source(
	references: &[ReferenceRecord],
	material: &CodeIndexMaterial,
	candidates: &CandidateCatalog<'_>,
) -> BTreeMap<usize, Vec<ReferenceId>> {
	let mut index = BTreeMap::<usize, Vec<ReferenceId>>::new();
	for reference in references {
		let Some(query) = LinkageQuery::new(reference, material) else {
			continue;
		};
		for source_file in candidates.matching_candidate_sources(&query) {
			index
				.entry(source_file)
				.or_default()
				.push(reference.id.clone());
		}
	}
	index
}

fn symbol_sources(material: &CodeIndexMaterial) -> BTreeMap<SymbolId, SourceId> {
	material
		.files
		.iter()
		.enumerate()
		.flat_map(|(file_idx, file)| {
			file.graph.defs().enumerate().map(move |(def_idx, _)| {
				(
					file.identity.symbol_id(file_idx, def_idx),
					file.source_id.clone(),
				)
			})
		})
		.collect()
}

fn resolved_by_target_source(
	current: &LinkageGraph,
	symbol_sources: &BTreeMap<SymbolId, SourceId>,
) -> BTreeMap<SourceId, Vec<ReferenceId>> {
	let mut index = BTreeMap::<SourceId, Vec<ReferenceId>>::new();
	for edge in &current.resolved {
		let Some(source) = symbol_sources.get(&edge.target) else {
			continue;
		};
		index
			.entry(source.clone())
			.or_default()
			.push(edge.reference.clone());
	}
	index
}

fn missing_resolved_references(
	current: &LinkageGraph,
	reference_indexes: &BTreeMap<ReferenceId, usize>,
	symbol_sources: &BTreeMap<SymbolId, SourceId>,
) -> Vec<ReferenceId> {
	current
		.resolved
		.iter()
		.filter(|edge| {
			!reference_indexes.contains_key(&edge.reference)
				|| !symbol_sources.contains_key(&edge.target)
		})
		.map(|edge| edge.reference.clone())
		.collect()
}

fn reference_source_root(
	reference: &ReferenceRecord,
	material: &CodeIndexMaterial,
) -> Option<usize> {
	let (file_idx, _) = material.identity.reference_location(&reference.id)?;
	material.files.get(file_idx).map(|file| file.source_root)
}

fn policy_source_roots(material: &CodeIndexMaterial, paths: &[PathBuf]) -> BTreeSet<usize> {
	paths
		.iter()
		.filter(|path| Manifest::for_filename(path).is_some())
		.filter_map(|path| source_root_for_path(material, path))
		.collect()
}

fn external_origin(label: &str) -> ExternalOrigin {
	match label {
		"dependency" => ExternalOrigin::Dependency,
		"injected" => ExternalOrigin::Injected,
		"unknown_external" => ExternalOrigin::UnknownExternal,
		_ => ExternalOrigin::UnknownExternal,
	}
}

fn external_target(identity: &str, material: &CodeIndexMaterial) -> Option<Moniker> {
	from_uri(
		identity,
		&UriConfig {
			scheme: material.identity.scheme(),
		},
	)
	.ok()
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

fn normalize_path(path: &Path) -> PathBuf {
	let path = if path.is_absolute() {
		path.to_path_buf()
	} else {
		std::env::current_dir()
			.map(|cwd| cwd.join(path))
			.unwrap_or_else(|_| path.to_path_buf())
	};
	path.canonicalize().unwrap_or_else(|_| lexical_path(&path))
}

fn lexical_path(path: &Path) -> PathBuf {
	let mut out = PathBuf::new();
	for component in path.components() {
		match component {
			Component::CurDir => {}
			Component::ParentDir => {
				out.pop();
			}
			_ => out.push(component.as_os_str()),
		}
	}
	out
}
