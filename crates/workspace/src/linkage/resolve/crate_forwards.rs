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
}

impl CrateForwards {
	pub(in crate::linkage) fn build(
		material: &CodeIndexMaterial,
		manifests: &ManifestPolicy,
	) -> Self {
		let mut by_root = FxHashMap::default();
		for (file_idx, file) in material.files.iter().enumerate() {
			for ref_idx in 0..file.graph.ref_count() {
				let reference = file.graph.ref_at(ref_idx);
				if reference.kind != kinds::REEXPORTS {
					continue;
				}
				let Some(target) = bare_external_root(&reference.target) else {
					continue;
				};
				for root in source_package_roots(manifests, file_idx) {
					by_root.entry(root).or_insert_with(|| target.to_vec());
				}
			}
		}
		Self { by_root }
	}

	pub(in crate::linkage) fn rewrite(&self, target: &Moniker) -> Option<Moniker> {
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
}

fn bare_external_root(target: &Moniker) -> Option<&[u8]> {
	let mut segments = target.as_view().segments();
	let head = segments.next()?;
	if head.kind != kinds::EXTERNAL_PKG || segments.next().is_some() {
		return None;
	}
	Some(head.name)
}
