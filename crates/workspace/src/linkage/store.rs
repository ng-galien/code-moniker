use std::collections::BTreeMap;
use std::hash::Hash;

use crate::linkage::candidate::{CandidateCatalog, query_keys};
use crate::linkage::decision::{ReferenceLinkageDecision, UnknownReason};
use crate::linkage::query::LinkageQuery;
use crate::snapshot::{
	LinkageSnapshot, ReferenceId, ReferenceRecord, ResourceGeneration, SourceId, SymbolId,
};
use crate::source::{CodeIndexMaterial, LocalIdentityResolver};
use code_moniker_core::core::uri::{UriConfig, from_uri};
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Clone)]
pub(super) struct LinkageStore {
	generation: ResourceGeneration,
	index_generation: ResourceGeneration,
	decisions: Vec<ReferenceLinkageDecision>,
	pub(super) indexes: LinkageStoreIndexes,
}

pub(super) struct LinkageStoreRefresh<'a> {
	pub(super) generation: ResourceGeneration,
	pub(super) index_generation: ResourceGeneration,
	pub(super) stale_references: &'a FxHashSet<ReferenceId>,
	pub(super) changed_decisions: Vec<ReferenceLinkageDecision>,
	pub(super) reference_indexes: FxHashMap<ReferenceId, usize>,
	pub(super) references: &'a [ReferenceRecord],
	pub(super) material: &'a CodeIndexMaterial,
	pub(super) candidates: &'a CandidateCatalog<'a>,
}

impl LinkageStore {
	pub(super) fn new(
		generation: ResourceGeneration,
		index_generation: ResourceGeneration,
		decisions: Vec<ReferenceLinkageDecision>,
		references: &[ReferenceRecord],
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog<'_>,
	) -> Self {
		let indexes = LinkageStoreIndexes::new(&decisions, references, material, candidates);
		Self {
			generation,
			index_generation,
			decisions,
			indexes,
		}
	}

	pub(super) fn from_snapshot(
		snapshot: &LinkageSnapshot,
		references: &[ReferenceRecord],
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog<'_>,
	) -> Self {
		Self::new(
			snapshot.generation,
			snapshot.index_generation,
			decisions_from_snapshot(snapshot, references, material),
			references,
			material,
			candidates,
		)
	}

	pub(super) fn project_snapshot(
		&self,
		references: &[ReferenceRecord],
		identity: &LocalIdentityResolver,
	) -> LinkageSnapshot {
		LinkageSnapshot::from_report(
			crate::linkage::decision::project_decisions(&self.decisions, references, identity)
				.into_report(self.generation, self.index_generation),
		)
	}

	pub(super) fn advance_index_generation(&mut self, index_generation: ResourceGeneration) {
		self.index_generation = index_generation;
	}

	pub(super) fn apply_refresh(&mut self, refresh: LinkageStoreRefresh<'_>) {
		self.generation = refresh.generation;
		self.index_generation = refresh.index_generation;
		self.indexes.reference_indexes = refresh.reference_indexes;
		self.remove_stale_references(refresh.stale_references);
		self.add_changed_decisions(
			refresh.changed_decisions,
			refresh.references,
			refresh.material,
			refresh.candidates,
		);
	}

	pub(super) fn missing_resolved_references(
		&self,
		material: &CodeIndexMaterial,
	) -> Vec<ReferenceId> {
		self.decisions
			.iter()
			.filter(|decision| {
				!self
					.indexes
					.reference_indexes
					.contains_key(decision.reference())
					|| decision.resolved_targets().is_some_and(|targets| {
						targets.iter().any(|target| !material.symbol_exists(target))
					})
			})
			.map(|decision| decision.reference().clone())
			.collect()
	}

	fn remove_stale_references(&mut self, stale_references: &FxHashSet<ReferenceId>) {
		if stale_references.is_empty() {
			return;
		}
		let reference_indexes = &self.indexes.reference_indexes;
		self.decisions.retain_mut(|decision| {
			if stale_references.contains(decision.reference()) {
				return false;
			}
			let Some(reference_idx) = reference_indexes.get(decision.reference()) else {
				return false;
			};
			decision.set_reference_idx(*reference_idx);
			true
		});
		self.indexes.remove_stale_references(stale_references);
	}

	fn add_changed_decisions(
		&mut self,
		changed_decisions: Vec<ReferenceLinkageDecision>,
		references: &[ReferenceRecord],
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog<'_>,
	) {
		for decision in changed_decisions {
			let Some(reference) = references.get(decision.reference_idx()) else {
				continue;
			};
			self.add_reference_indexes(reference, material, candidates);
			self.decisions.push(decision);
		}
	}

	fn add_reference_indexes(
		&mut self,
		reference: &ReferenceRecord,
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog<'_>,
	) {
		if let Some(source_root) = reference_source_root(reference, material) {
			self.indexes
				.references_by_source_root
				.entry(source_root)
				.or_default()
				.push(reference.id.clone());
		}
		let Some(query) = LinkageQuery::new(reference, material) else {
			return;
		};
		for key in query_keys(&query) {
			self.indexes
				.references_by_name
				.entry(key)
				.or_default()
				.push(reference.id.clone());
		}
		for source_file in candidates.matching_candidate_sources(&query) {
			self.indexes
				.references_by_candidate_source
				.entry(source_file)
				.or_default()
				.push(reference.id.clone());
		}
	}

	pub(super) fn decisions_mut(&mut self) -> &mut [ReferenceLinkageDecision] {
		&mut self.decisions
	}

	pub(super) fn refresh_resolved_target_index(
		&mut self,
		references: &FxHashSet<ReferenceId>,
		material: &CodeIndexMaterial,
	) {
		self.indexes.remove_resolved_references(references);
		for decision in &self.decisions {
			if references.contains(decision.reference()) {
				self.indexes.add_resolved_target_indexes(decision, material);
			}
		}
	}
}

fn decisions_from_snapshot(
	snapshot: &LinkageSnapshot,
	references: &[ReferenceRecord],
	material: &CodeIndexMaterial,
) -> Vec<ReferenceLinkageDecision> {
	let reference_indexes = reference_indexes(references);
	let mut decisions = Vec::new();
	decisions.extend(resolved_decisions_from_snapshot(
		snapshot,
		&reference_indexes,
	));
	decisions.extend(external_decisions_from_snapshot(
		snapshot,
		&reference_indexes,
		material,
	));
	decisions.extend(
		snapshot
			.manifest_blocked
			.iter()
			.filter_map(|blocked| reference_indexes.get(&blocked.reference).copied())
			.map(|reference_idx| {
				ReferenceLinkageDecision::manifest_blocked(
					reference_idx,
					references[reference_idx].id.clone(),
				)
			}),
	);
	decisions.extend(
		snapshot
			.unresolved
			.iter()
			.filter_map(|unresolved| reference_indexes.get(&unresolved.reference).copied())
			.map(|reference_idx| {
				ReferenceLinkageDecision::unknown(
					UnknownReason::NoCandidate,
					reference_idx,
					references[reference_idx].id.clone(),
				)
			}),
	);
	decisions
}

fn resolved_decisions_from_snapshot(
	snapshot: &LinkageSnapshot,
	reference_indexes: &FxHashMap<ReferenceId, usize>,
) -> Vec<ReferenceLinkageDecision> {
	let mut targets_by_reference = BTreeMap::<ReferenceId, Vec<SymbolId>>::new();
	for edge in &snapshot.resolved {
		targets_by_reference
			.entry(edge.reference.clone())
			.or_default()
			.push(edge.target.clone());
	}
	targets_by_reference
		.into_iter()
		.filter_map(|(reference, targets)| {
			reference_indexes.get(&reference).map(|reference_idx| {
				ReferenceLinkageDecision::resolved(
					crate::linkage::decision::ResolutionScope::Global,
					*reference_idx,
					reference.clone(),
					targets,
				)
			})
		})
		.collect()
}

fn external_decisions_from_snapshot(
	snapshot: &LinkageSnapshot,
	reference_indexes: &FxHashMap<ReferenceId, usize>,
	material: &CodeIndexMaterial,
) -> Vec<ReferenceLinkageDecision> {
	snapshot
		.external
		.iter()
		.filter_map(|external| {
			let reference_idx = *reference_indexes.get(&external.reference)?;
			let target = from_uri(
				external.target_identity.as_ref(),
				&UriConfig {
					scheme: material.identity.scheme(),
				},
			)
			.ok();
			Some(match target {
				Some(target) => ReferenceLinkageDecision::external_target(
					external.origin,
					reference_idx,
					external.reference.clone(),
					target,
				),
				None => ReferenceLinkageDecision::external(
					external.origin,
					reference_idx,
					external.reference.clone(),
				),
			})
		})
		.collect()
}

#[derive(Clone)]
pub(super) struct LinkageStoreIndexes {
	reference_indexes: FxHashMap<ReferenceId, usize>,
	pub(super) references_by_source_root: FxHashMap<usize, Vec<ReferenceId>>,
	pub(super) references_by_candidate_source: FxHashMap<usize, Vec<ReferenceId>>,
	pub(super) references_by_name: FxHashMap<Vec<u8>, Vec<ReferenceId>>,
	pub(super) resolved_by_target_source: FxHashMap<SourceId, Vec<ReferenceId>>,
}

impl LinkageStoreIndexes {
	fn new(
		decisions: &[ReferenceLinkageDecision],
		references: &[ReferenceRecord],
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog<'_>,
	) -> Self {
		let mut indexes = Self::from_references(references, material, candidates);
		indexes.collect_decisions(decisions, material);
		indexes
	}

	fn from_references(
		references: &[ReferenceRecord],
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog<'_>,
	) -> Self {
		Self {
			reference_indexes: reference_indexes(references),
			references_by_source_root: references_by_source_root(references, material),
			references_by_candidate_source: references_by_candidate_source(
				references, material, candidates,
			),
			references_by_name: references_by_name(references, material),
			resolved_by_target_source: FxHashMap::default(),
		}
	}

	fn collect_decisions(
		&mut self,
		decisions: &[ReferenceLinkageDecision],
		material: &CodeIndexMaterial,
	) {
		for decision in decisions {
			self.add_resolved_target_indexes(decision, material);
		}
	}

	fn add_resolved_target_indexes(
		&mut self,
		decision: &ReferenceLinkageDecision,
		material: &CodeIndexMaterial,
	) {
		let Some(targets) = decision.resolved_targets() else {
			return;
		};
		for target in targets {
			let Some(source) = material.symbol_source(target) else {
				continue;
			};
			self.resolved_by_target_source
				.entry(source)
				.or_default()
				.push(decision.reference().clone());
		}
	}

	fn remove_stale_references(&mut self, stale_references: &FxHashSet<ReferenceId>) {
		remove_references(&mut self.references_by_source_root, stale_references);
		remove_references(&mut self.references_by_candidate_source, stale_references);
		remove_references(&mut self.references_by_name, stale_references);
		self.remove_resolved_references(stale_references);
	}

	fn remove_resolved_references(&mut self, stale_references: &FxHashSet<ReferenceId>) {
		remove_references(&mut self.resolved_by_target_source, stale_references);
	}
}

pub(super) fn reference_indexes(references: &[ReferenceRecord]) -> FxHashMap<ReferenceId, usize> {
	references
		.iter()
		.enumerate()
		.map(|(idx, reference)| (reference.id.clone(), idx))
		.collect()
}

fn remove_references<K: Eq + Hash>(
	index: &mut FxHashMap<K, Vec<ReferenceId>>,
	references: &FxHashSet<ReferenceId>,
) {
	index.retain(|_, indexed_references| {
		indexed_references.retain(|reference| !references.contains(reference));
		!indexed_references.is_empty()
	});
}

fn references_by_source_root(
	references: &[ReferenceRecord],
	material: &CodeIndexMaterial,
) -> FxHashMap<usize, Vec<ReferenceId>> {
	let mut index = FxHashMap::<usize, Vec<ReferenceId>>::default();
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
) -> FxHashMap<usize, Vec<ReferenceId>> {
	let mut index = FxHashMap::<usize, Vec<ReferenceId>>::default();
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

fn references_by_name(
	references: &[ReferenceRecord],
	material: &CodeIndexMaterial,
) -> FxHashMap<Vec<u8>, Vec<ReferenceId>> {
	let mut index = FxHashMap::<Vec<u8>, Vec<ReferenceId>>::default();
	for reference in references {
		let Some(query) = LinkageQuery::new(reference, material) else {
			continue;
		};
		for key in query_keys(&query) {
			index.entry(key).or_default().push(reference.id.clone());
		}
	}
	index
}

fn reference_source_root(
	reference: &ReferenceRecord,
	material: &CodeIndexMaterial,
) -> Option<usize> {
	let (file_idx, _) = material.identity.reference_location(&reference.id)?;
	material.files.get(file_idx).map(|file| file.source_root)
}
