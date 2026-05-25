use std::collections::BTreeSet;

use super::model::{
	ReferenceDirection, ReferenceSet, ReferenceSetSummary, ReferenceSummary, SymbolReferences,
};
use super::symbols::SymbolView;
use crate::workspace::session::model::{ReferenceId, ReferenceRecord, SymbolId, WorkspaceSnapshot};

pub struct ReferenceView<'a> {
	snapshot: &'a WorkspaceSnapshot,
}

impl<'a> ReferenceView<'a> {
	pub(super) fn new(snapshot: &'a WorkspaceSnapshot) -> Self {
		Self { snapshot }
	}

	pub fn for_symbol(&self, symbol: &SymbolId) -> Option<SymbolReferences> {
		let symbols = SymbolView::new(self.snapshot);
		let record = symbols.find(symbol)?;
		Some(SymbolReferences {
			symbol: symbols.summary(record),
			incoming: self.reference_set(&self.incoming_ids(symbol), ReferenceDirection::Incoming),
			outgoing: self.reference_set(&self.outgoing_ids(symbol), ReferenceDirection::Outgoing),
		})
	}

	pub fn incoming_ids(&self, symbol: &SymbolId) -> Vec<ReferenceId> {
		self.snapshot
			.linkage
			.resolved
			.iter()
			.filter(|edge| &edge.target == symbol)
			.map(|edge| edge.reference.clone())
			.collect()
	}

	pub fn outgoing_ids(&self, symbol: &SymbolId) -> Vec<ReferenceId> {
		self.snapshot
			.index
			.references
			.iter()
			.filter(|reference| &reference.source_symbol == symbol)
			.map(|reference| reference.id.clone())
			.collect()
	}

	pub fn reference_set(
		&self,
		refs: &[ReferenceId],
		direction: ReferenceDirection,
	) -> ReferenceSet {
		let groups = refs
			.iter()
			.filter_map(|reference| self.reference(reference))
			.map(|reference| reference_summary(self.snapshot, reference, direction))
			.collect::<Vec<_>>();
		let files = groups
			.iter()
			.filter_map(|group| group.source.clone())
			.collect::<BTreeSet<_>>()
			.len();
		let contexts = groups
			.iter()
			.filter_map(|group| group.context.clone())
			.collect::<BTreeSet<_>>()
			.len();
		ReferenceSet {
			summary: ReferenceSetSummary {
				refs: groups.len(),
				files,
				contexts,
			},
			groups,
		}
	}

	pub(super) fn reference(&self, id: &ReferenceId) -> Option<&ReferenceRecord> {
		self.snapshot
			.index
			.references
			.iter()
			.find(|reference| &reference.id == id)
	}
}

fn reference_summary(
	snapshot: &WorkspaceSnapshot,
	reference: &ReferenceRecord,
	direction: ReferenceDirection,
) -> ReferenceSummary {
	let symbols = SymbolView::new(snapshot);
	let source_symbol = symbols.find(&reference.source_symbol);
	let resolved_target = resolved_target(snapshot, &reference.id);
	ReferenceSummary {
		reference: reference.id.clone(),
		source: Some(reference.source.clone()),
		context: source_symbol.map(|symbol| symbol.id.clone()),
		actor: source_symbol
			.map(|symbol| symbol.name.clone())
			.unwrap_or_else(|| reference.source_symbol.as_str().to_string()),
		endpoint_label: match direction {
			ReferenceDirection::Incoming => "source",
			ReferenceDirection::Outgoing => "target",
		},
		endpoint: resolved_target
			.as_ref()
			.and_then(|symbol| symbols.find(symbol))
			.map(|symbol| symbol.identity.clone())
			.unwrap_or_else(|| reference.target_identity.clone()),
		kind: reference.kind.clone(),
		line_range: reference.line_range,
	}
}

fn resolved_target(snapshot: &WorkspaceSnapshot, reference: &ReferenceId) -> Option<SymbolId> {
	snapshot
		.linkage
		.resolved
		.iter()
		.find(|edge| &edge.reference == reference)
		.map(|edge| edge.target.clone())
}
