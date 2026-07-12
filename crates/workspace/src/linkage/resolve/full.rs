use std::time::Instant;

use rayon::prelude::*;

use crate::linkage::binding::LinkageStore;
use crate::linkage::catalog::CandidateCatalog;
use crate::linkage::catalog::ReferenceLocations;
use crate::linkage::resolve::ManifestPolicy;
use crate::linkage::resolve::MethodIndexer;
use crate::linkage::resolve::{LinkagePolicies, ReferenceResolver};
use crate::linkage::resolve::{MethodTable, SemanticLinkage, WorkspacePackageIndex};
use crate::linkage::source_groups::SourceGroupPolicy;
use crate::linkage::{LinkageTimings, LocalLinkage, TimedLinkageSnapshot};
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
		&candidates,
		candidate_index,
	);
	let report_timer = Instant::now();
	let snapshot =
		store.project_snapshot(&index.references, &material.identity, candidates.symbols());
	let memory = store.memory_metrics(candidates.symbols());
	timings.project_snapshot = report_timer.elapsed();
	timings.total = total_timer.elapsed();
	linkage.store = Some(store);
	linkage.candidates = Some(candidates);
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
	candidates: &CandidateCatalog,
	candidate_index_elapsed: std::time::Duration,
) -> LinkageResolution {
	let resolver = ReferenceResolver::new(material);
	let mut timings = LinkageTimings {
		candidate_index: candidate_index_elapsed,
		..LinkageTimings::default()
	};
	let manifest_timer = Instant::now();
	let manifests = ManifestPolicy::build(material);
	let source_groups = SourceGroupPolicy::build(material);
	let packages = WorkspacePackageIndex::build(material);
	timings.manifest_policy = manifest_timer.elapsed();
	let policies = LinkagePolicies {
		candidates,
		manifests: &manifests,
		source_groups: &source_groups,
		packages: &packages,
	};
	let resolve_timer = Instant::now();
	let locations = ReferenceLocations::from_material(material);
	let mut decisions = (0..index.references.len())
		.into_par_iter()
		.map(|reference_idx| {
			resolver.resolve_reference(
				reference_idx,
				&index.references[reference_idx],
				locations.get(reference_idx),
				&policies,
			)
		})
		.collect::<Vec<_>>();
	timings.resolve_references = resolve_timer.elapsed();
	let semantic_timer = Instant::now();
	SemanticLinkage::new(material, methods, candidates, &locations, &source_groups)
		.enhance(&mut decisions, &index.references);
	timings.semantic_enhance = semantic_timer.elapsed();
	let store_timer = Instant::now();
	let store = LinkageStore::new(
		generation,
		index.generation,
		decisions,
		&index.references,
		material,
		candidates,
	);
	timings.store_index = store_timer.elapsed();
	LinkageResolution { store, timings }
}

struct LinkageResolution {
	store: LinkageStore,
	timings: LinkageTimings,
}
