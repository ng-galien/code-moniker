use std::collections::BTreeMap;

use crate::linkage::decision::ReferenceLinkageDecision;
use crate::snapshot::{
	LinkageSnapshot, ReferenceId, ReferenceRecord, ResourceGeneration, SymbolId,
};
use crate::source::LocalIdentityResolver;

#[derive(Clone)]
pub(super) struct LinkageStore {
	generation: ResourceGeneration,
	index_generation: ResourceGeneration,
	decisions: Vec<ReferenceLinkageDecision>,
	decisions_by_reference: BTreeMap<ReferenceId, usize>,
	references_by_target: BTreeMap<SymbolId, Vec<ReferenceId>>,
}

impl LinkageStore {
	pub(super) fn new(
		generation: ResourceGeneration,
		index_generation: ResourceGeneration,
		decisions: Vec<ReferenceLinkageDecision>,
		references: &[ReferenceRecord],
	) -> Self {
		let indexes = LinkageStoreIndexes::new(&decisions, references);
		Self {
			generation,
			index_generation,
			decisions,
			decisions_by_reference: indexes.decisions_by_reference,
			references_by_target: indexes.references_by_target,
		}
	}

	pub(super) fn project_snapshot(
		&self,
		references: &[ReferenceRecord],
		identity: &LocalIdentityResolver,
	) -> LinkageSnapshot {
		LinkageSnapshot::from_report(
			crate::linkage::decision::project_decisions(
				self.decisions.clone(),
				references,
				identity,
			)
			.into_report(self.generation, self.index_generation),
		)
	}

	pub(super) fn advance_index_generation(&mut self, index_generation: ResourceGeneration) {
		self.index_generation = index_generation;
	}

	#[allow(dead_code)]
	pub(super) fn decision_index(&self, reference: &ReferenceId) -> Option<usize> {
		self.decisions_by_reference.get(reference).copied()
	}

	#[allow(dead_code)]
	pub(super) fn references_resolved_to(&self, target: &SymbolId) -> &[ReferenceId] {
		self.references_by_target
			.get(target)
			.map(Vec::as_slice)
			.unwrap_or(&[])
	}
}

struct LinkageStoreIndexes {
	decisions_by_reference: BTreeMap<ReferenceId, usize>,
	references_by_target: BTreeMap<SymbolId, Vec<ReferenceId>>,
}

impl LinkageStoreIndexes {
	fn new(decisions: &[ReferenceLinkageDecision], references: &[ReferenceRecord]) -> Self {
		let mut decisions_by_reference = BTreeMap::new();
		let mut references_by_target = BTreeMap::<SymbolId, Vec<ReferenceId>>::new();
		for (decision_idx, decision) in decisions.iter().enumerate() {
			let Some(reference) = references.get(decision.reference_idx()) else {
				continue;
			};
			decisions_by_reference.insert(reference.id.clone(), decision_idx);
			if let Some(targets) = decision.resolved_targets() {
				for target in targets {
					references_by_target
						.entry(target.clone())
						.or_default()
						.push(reference.id.clone());
				}
			}
		}
		Self {
			decisions_by_reference,
			references_by_target,
		}
	}
}
