//! Canonical runtime estimates for the immutable workspace snapshot.
//!
//! These values are deliberately lower-bound estimates of owned heap storage.
//! Allocator metadata and storage hidden inside third-party collections are not
//! observable here; the contract is stable comparison, not exact accounting.

use std::collections::HashSet;
use std::mem::{size_of, size_of_val};
use std::sync::Arc;

use crate::snapshot::{CodeIndex, LinkageSnapshot, WorkspaceSnapshot};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SnapshotMemoryEstimate {
	pub source_bytes: usize,
	pub index_bytes: usize,
	pub graph_bytes: usize,
}

impl SnapshotMemoryEstimate {
	pub fn from_snapshot(snapshot: &WorkspaceSnapshot) -> Self {
		Self {
			source_bytes: source_bytes(&snapshot.index),
			index_bytes: Self::from_index(&snapshot.index),
			graph_bytes: Self::from_linkage(&snapshot.linkage),
		}
	}

	pub fn from_index(index: &CodeIndex) -> usize {
		let sources = index.sources.iter().fold(
			index.sources.capacity() * size_of::<crate::snapshot::SourceFileRecord>(),
			|total, source| {
				total
					+ source.uri.capacity()
					+ source.path.capacity()
					+ source.rel_path.capacity()
					+ source.anchor.capacity()
					+ source.language.capacity()
					+ source.text.capacity()
			},
		);
		let symbols = index.symbols.estimated_heap_bytes()
			+ index.symbols.iter().fold(0, |total, symbol| {
				total
					+ symbol.name.capacity()
					+ symbol.kind.capacity()
					+ symbol.visibility.capacity()
					+ symbol.signature.capacity()
					+ symbol.call_name.as_ref().map_or(0, String::capacity)
			});
		let references = index.references.estimated_heap_bytes()
			+ index.references.iter().fold(0, |total, reference| {
				total
					+ reference.kind.capacity()
					+ reference.call_name.as_ref().map_or(0, String::capacity)
					+ reference.confidence.as_ref().map_or(0, String::capacity)
					+ reference.receiver.as_ref().map_or(0, String::capacity)
					+ reference.alias.as_ref().map_or(0, String::capacity)
			}) + unique_arc_str_payload(
			index
				.references
				.iter()
				.map(|reference| &reference.target_identity),
		);
		size_of_val(index) + sources + symbols + references + index.inventory.estimated_heap_bytes()
	}

	pub fn from_linkage(linkage: &LinkageSnapshot) -> usize {
		let mut bytes = size_of_val(linkage)
			+ linkage.resolved.capacity() * size_of::<crate::snapshot::LinkageEdge>()
			+ linkage.candidates.capacity() * size_of::<crate::snapshot::CandidateReference>()
			+ linkage.external.capacity() * size_of::<crate::snapshot::ExternalReference>()
			+ linkage.dynamic.capacity() * size_of::<crate::snapshot::DynamicReference>()
			+ linkage.blocked.capacity() * size_of::<crate::snapshot::UnresolvedReference>()
			+ linkage.manifest_blocked.capacity()
				* size_of::<crate::snapshot::UnresolvedReference>()
			+ linkage.unresolved.capacity() * size_of::<crate::snapshot::UnresolvedReference>()
			+ linkage.read_index.estimated_heap_bytes();
		bytes += linkage
			.candidates
			.iter()
			.map(|candidate| candidate.targets.capacity() * size_of::<crate::snapshot::SymbolId>())
			.sum::<usize>();
		bytes += linkage
			.dynamic
			.iter()
			.map(|reference| {
				reference.candidates.capacity() * size_of::<crate::snapshot::SymbolId>()
			})
			.sum::<usize>();
		bytes += unique_arc_str_payload(
			linkage
				.external
				.iter()
				.map(|reference| &reference.target_identity)
				.chain(
					linkage
						.dynamic
						.iter()
						.map(|reference| &reference.target_identity),
				)
				.chain(
					linkage
						.blocked
						.iter()
						.map(|reference| &reference.target_identity),
				)
				.chain(
					linkage
						.manifest_blocked
						.iter()
						.map(|reference| &reference.target_identity),
				)
				.chain(
					linkage
						.unresolved
						.iter()
						.map(|reference| &reference.target_identity),
				),
		);
		bytes
	}
}

fn source_bytes(index: &CodeIndex) -> usize {
	index
		.sources
		.iter()
		.map(|source| {
			if source.text.is_empty() {
				std::fs::metadata(&source.path)
					.ok()
					.and_then(|metadata| usize::try_from(metadata.len()).ok())
					.unwrap_or(0)
			} else {
				source.text.len()
			}
		})
		.sum()
}

fn unique_arc_str_payload<'a>(values: impl Iterator<Item = &'a Arc<str>>) -> usize {
	let mut seen = HashSet::<(usize, usize)>::new();
	values
		.filter(|value| seen.insert((value.as_ptr() as usize, value.len())))
		.map(|value| value.len())
		.sum()
}

#[cfg(test)]
mod tests {
	use super::SnapshotMemoryEstimate;
	use crate::snapshot::{
		CodeIndex, LinkageEdge, LinkageSnapshot, ReferenceId, ResourceGeneration, SymbolId,
		SymbolRecord,
	};

	#[test]
	fn estimates_grow_with_index_and_read_graph_storage() {
		let generation = ResourceGeneration::new(1);
		let empty_index = CodeIndex::new(generation, generation, Vec::new());
		let populated_index = CodeIndex::new(
			generation,
			generation,
			vec![SymbolRecord::new(
				SymbolId::at(0, 0),
				crate::snapshot::SourceId::at(0),
				"main",
				"function",
			)],
		);
		assert!(
			SnapshotMemoryEstimate::from_index(&populated_index)
				> SnapshotMemoryEstimate::from_index(&empty_index)
		);

		let empty_graph = LinkageSnapshot::new(generation, generation, 0, 0);
		let populated_graph = LinkageSnapshot::with_refs(
			generation,
			generation,
			vec![LinkageEdge::new(ReferenceId::at(0, 0), SymbolId::at(0, 0))],
			Vec::new(),
		);
		assert!(
			SnapshotMemoryEstimate::from_linkage(&populated_graph)
				> SnapshotMemoryEstimate::from_linkage(&empty_graph)
		);
	}
}
