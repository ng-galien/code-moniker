use std::time::Instant;

use rayon::prelude::*;

use crate::linkage::resolution::CandidateCatalog;
use crate::linkage::resolution::ManifestPolicy;
use crate::linkage::resolution::MethodIndexer;
use crate::linkage::resolution::ReferenceLocations;
use crate::linkage::resolution::ReferenceResolver;
use crate::linkage::resolution::{MethodTable, SemanticLinkage};
use crate::linkage::resolver::{LinkageTimings, LocalLinkage, TimedLinkageSnapshot};
use crate::linkage::storage::LinkageStore;
use crate::snapshot::{CodeIndex, ResourceGeneration, WorkspaceResult};
use crate::source::CodeIndexMaterial;

pub(in crate::linkage) fn run_full_linkage_with_timings(
	linkage: &mut LocalLinkage,
	index: &CodeIndex,
) -> WorkspaceResult<TimedLinkageSnapshot> {
	let total_timer = Instant::now();
	let material = linkage.linkage_material(index)?;
	let generation = linkage.cache.next_generation();
	let candidate_timer = Instant::now();
	let candidates = CandidateCatalog::new(&material);
	let mut candidate_index = candidate_timer.elapsed();
	let method_timer = Instant::now();
	let method_indexer = MethodIndexer::new(&material, &candidates);
	candidate_index += method_timer.elapsed();
	let LinkageResolution { store, mut timings } = resolve_full_linkage(
		&material,
		index,
		generation,
		method_indexer.methods(),
		candidates,
		candidate_index,
	);
	let report_timer = Instant::now();
	let snapshot = store.project_snapshot(&index.references, &material.identity);
	let memory = store.memory_metrics();
	timings.project_snapshot = report_timer.elapsed();
	timings.total = total_timer.elapsed();
	linkage.store = Some(store);
	linkage.method_indexer = Some(method_indexer);
	linkage.memory = memory;
	Ok(TimedLinkageSnapshot {
		snapshot,
		timings,
		memory,
	})
}

fn resolve_full_linkage(
	material: &CodeIndexMaterial,
	index: &CodeIndex,
	generation: ResourceGeneration,
	methods: &MethodTable,
	candidates: CandidateCatalog<'_>,
	candidate_index_elapsed: std::time::Duration,
) -> LinkageResolution {
	let resolver = ReferenceResolver::new(material);
	let mut timings = LinkageTimings {
		candidate_index: candidate_index_elapsed,
		..LinkageTimings::default()
	};
	let manifest_timer = Instant::now();
	let manifests = ManifestPolicy::build(material);
	timings.manifest_policy = manifest_timer.elapsed();
	let resolve_timer = Instant::now();
	let locations = ReferenceLocations::from_material(material);
	let mut decisions = index
		.references
		.par_iter()
		.enumerate()
		.map(|(reference_idx, reference)| {
			resolver.resolve_reference(
				reference_idx,
				reference,
				locations.get(reference_idx),
				&candidates,
				&manifests,
			)
		})
		.collect::<Vec<_>>();
	timings.resolve_references = resolve_timer.elapsed();
	let semantic_timer = Instant::now();
	SemanticLinkage::new(material, methods, &candidates, &locations)
		.enhance(&mut decisions, &index.references);
	timings.semantic_enhance = semantic_timer.elapsed();
	let store_timer = Instant::now();
	let store = LinkageStore::new(
		generation,
		index.generation,
		decisions,
		&index.references,
		material,
		&candidates,
	);
	timings.store_index = store_timer.elapsed();
	LinkageResolution { store, timings }
}

struct LinkageResolution {
	store: LinkageStore,
	timings: LinkageTimings,
}
