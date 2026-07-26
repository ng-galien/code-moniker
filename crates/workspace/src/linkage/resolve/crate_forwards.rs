use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::kinds;
use rustc_hash::FxHashMap;

use crate::linkage::resolve::ManifestPolicy;
use crate::linkage::resolve::manifest::source_package_roots;
use crate::source::CodeIndexMaterial;

// A facade crate re-exporting another crate wholesale (`pub use inner::*;`)
// makes its own name an alias for the inner crate's surface; the extractor
// records that as a module-level reexport to the bare external root, and the
// resolver retries unmatched external targets under the forwarded root.
#[derive(Default)]
pub(in crate::linkage) struct CrateForwards {
	by_root: FxHashMap<Vec<u8>, Vec<u8>>,
	by_barrel: FxHashMap<Moniker, Moniker>,
}

impl CrateForwards {
	pub(in crate::linkage) fn build(
		material: &CodeIndexMaterial,
		manifests: &ManifestPolicy,
	) -> Self {
		let mut forwards = Self::default();
		for (file_idx, file) in material.files.iter().enumerate() {
			for ref_idx in 0..file.graph.ref_count() {
				forwards.record_reexport(manifests, file_idx, &file.graph, ref_idx);
			}
		}
		forwards
	}

	fn record_reexport(
		&mut self,
		manifests: &ManifestPolicy,
		file_idx: usize,
		graph: &code_moniker_core::core::code_graph::CodeGraph,
		ref_idx: usize,
	) {
		let reference = graph.ref_at(ref_idx);
		if reference.kind != kinds::REEXPORTS {
			return;
		}
		if let Some(target) = bare_external_root(&reference.target) {
			for root in source_package_roots(manifests, file_idx) {
				self.by_root.entry(root).or_insert_with(|| target.to_vec());
			}
			return;
		}
		let Some(barrel) = wildcard_barrel(graph.def_at(reference.source), reference) else {
			return;
		};
		self.by_barrel
			.entry(barrel)
			.or_insert_with(|| reference.target.clone());
	}

	pub(in crate::linkage) fn rewrite(&self, target: &Moniker) -> Option<Moniker> {
		self.rewrite_external_root(target)
			.or_else(|| self.rewrite_barrel_member(target))
	}

	fn rewrite_external_root(&self, target: &Moniker) -> Option<Moniker> {
		let view = target.as_view();
		let mut segments = view.segments();
		let head = segments.next()?;
		if head.kind != kinds::EXTERNAL_PKG {
			return None;
		}
		let forwarded = self.by_root.get(head.name)?;
		let mut builder = MonikerBuilder::new();
		builder.project(view.project());
		builder.segment(kinds::EXTERNAL_PKG, forwarded);
		for segment in segments {
			builder.segment(segment.kind, segment.name);
		}
		Some(builder.build())
	}

	fn rewrite_barrel_member(&self, target: &Moniker) -> Option<Moniker> {
		let parent = target.parent()?;
		let forwarded = self.by_barrel.get(&parent)?;
		let last = target.as_view().segments().last()?;
		let mut builder = MonikerBuilder::from_view(forwarded.as_view());
		builder.segment(last.kind, last.name);
		Some(builder.build())
	}
}

// An `export * from "./inner"` records a module-to-module reexport: the
// barrel module's own members are aliases for the inner module's surface.
fn wildcard_barrel(
	source: &code_moniker_core::core::code_graph::DefRecord,
	reference: &code_moniker_core::core::code_graph::RefRecord,
) -> Option<Moniker> {
	let source_is_module = source
		.moniker
		.as_view()
		.segments()
		.last()
		.is_some_and(|segment| segment.kind == kinds::MODULE);
	let target_is_module = reference
		.target
		.as_view()
		.segments()
		.last()
		.is_some_and(|segment| segment.kind == kinds::MODULE);
	(source_is_module && target_is_module && source.moniker != reference.target)
		.then(|| source.moniker.clone())
}

fn bare_external_root(target: &Moniker) -> Option<&[u8]> {
	let mut segments = target.as_view().segments();
	let head = segments.next()?;
	if head.kind != kinds::EXTERNAL_PKG || segments.next().is_some() {
		return None;
	}
	Some(head.name)
}
