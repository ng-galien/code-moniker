//! Canonical runtime estimates for the immutable workspace snapshot.
//!
//! These values are deliberately lower-bound estimates of owned heap storage.
//! Allocator metadata and storage hidden inside third-party collections are not
//! observable here; the contract is stable comparison, not exact accounting.

use std::collections::HashSet;
use std::mem::{size_of, size_of_val};
use std::sync::Arc;

use code_moniker_core::core::code_graph::{DefRecord, RefRecord};

use crate::snapshot::{CodeIndex, LinkageSnapshot, WorkspaceSnapshot};
use crate::source::{CodeIndexMaterial, IndexedSourceFile};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SnapshotMemoryEstimate {
	pub source_bytes: usize,
	pub index_bytes: usize,
	pub graph_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RetainedMaterialMemoryEstimate {
	pub source_bytes: usize,
	pub graph_bytes: usize,
	pub lookup_bytes: usize,
	pub metadata_bytes: usize,
	pub total_bytes: usize,
}

impl RetainedMaterialMemoryEstimate {
	pub fn from_material(material: &CodeIndexMaterial) -> Self {
		let source_bytes = material
			.files
			.iter()
			.map(|file| file.source.capacity())
			.sum();
		let metadata_bytes = material.files.capacity() * size_of::<Arc<IndexedSourceFile>>()
			+ material
				.files
				.iter()
				.map(|file| {
					size_of::<IndexedSourceFile>()
						+ file.source_uri.capacity()
						+ file.path.as_os_str().len()
						+ file.rel_path.as_os_str().len()
						+ file.anchor.as_os_str().len()
				})
				.sum::<usize>();
		let graph_bytes =
			material
				.files
				.iter()
				.map(|file| {
					file.graph.def_count() * size_of::<DefRecord>()
						+ file.graph.ref_count() * size_of::<RefRecord>()
						+ file
							.graph
							.defs()
							.map(definition_payload_bytes)
							.sum::<usize>() + file
						.graph
						.refs()
						.map(reference_payload_bytes)
						.sum::<usize>() + file
						.graph
						.defs()
						.map(|definition| definition.moniker.as_encoded().len())
						.sum::<usize>()
				})
				.sum();
		let lookup_bytes = material
			.symbols_by_moniker
			.iter()
			.map(|(moniker, symbol)| moniker.as_encoded().len() + size_of_val(symbol))
			.sum();
		Self {
			source_bytes,
			graph_bytes,
			lookup_bytes,
			metadata_bytes,
			total_bytes: source_bytes + graph_bytes + lookup_bytes + metadata_bytes,
		}
	}
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

fn definition_payload_bytes(definition: &DefRecord) -> usize {
	definition.moniker.as_encoded().len()
		+ definition.kind.len()
		+ definition.visibility.len()
		+ definition.signature.len()
		+ definition.call_name.len()
		+ definition.binding.len()
		+ definition.origin.len()
}

fn reference_payload_bytes(reference: &RefRecord) -> usize {
	reference.target.as_encoded().len()
		+ reference.kind.len()
		+ reference.receiver_hint.len()
		+ reference.alias.len()
		+ reference.confidence.len()
		+ reference.call_name.len()
		+ reference.binding.len()
}

#[cfg(test)]
mod tests {
	use super::SnapshotMemoryEstimate;
	use crate::snapshot::{
		CodeIndex, LinkageEdge, LinkageReadIndexHandle, LinkageSnapshot, ReferenceId,
		ReferenceRecord, ResourceGeneration, SourceId, SymbolId, SymbolRecord,
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

	#[test]
	fn unique_edges_do_not_allocate_a_per_pair_hash_map_or_bitmap() {
		const EDGE_COUNT: usize = 4_096;
		let generation = ResourceGeneration::new(1);
		let source = SourceId::at(0);
		let mut symbols = Vec::with_capacity(EDGE_COUNT * 2);
		let mut references = Vec::with_capacity(EDGE_COUNT);
		let mut edges = Vec::with_capacity(EDGE_COUNT);
		let mut ordinals = Vec::with_capacity(EDGE_COUNT * 2);
		for index in 0..EDGE_COUNT {
			let from = SymbolId::at(0, index * 2);
			let to = SymbolId::at(0, index * 2 + 1);
			let reference = ReferenceId::at(0, index);
			symbols.push(SymbolRecord::new(
				from,
				source,
				format!("from_{index}"),
				"fn",
			));
			symbols.push(SymbolRecord::new(to, source, format!("to_{index}"), "fn"));
			references.push(ReferenceRecord::new(
				reference,
				source,
				from,
				to.to_string(),
				"calls",
				None,
			));
			edges.push(LinkageEdge::new(reference, to));
			ordinals.push(((index * 2) as u32, from));
			ordinals.push(((index * 2 + 1) as u32, to));
		}
		let index = CodeIndex::with_references(generation, generation, symbols, references);
		let mut linkage = LinkageSnapshot::new(generation, generation, EDGE_COUNT, 0);
		linkage.resolved = edges;
		linkage.read_index = LinkageReadIndexHandle::from_snapshot_with_ordinals(
			&linkage,
			&index.references,
			ordinals,
		);

		let graph_bytes = SnapshotMemoryEstimate::from_linkage(&linkage);
		assert!(
			graph_bytes < EDGE_COUNT * 512,
			"unique-edge linkage index used {graph_bytes} bytes for {EDGE_COUNT} edges"
		);
	}
}
