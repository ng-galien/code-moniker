use code_moniker_core::core::code_graph::{DefRecord, RefRecord};
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder, Segment};
use code_moniker_core::lang::{Lang, kinds};
use rustc_hash::FxHashMap;

use crate::linkage::resolve::ManifestPolicy;
use crate::linkage::resolve::manifest::source_package_roots;
use crate::source::CodeIndexMaterial;

// Imports and reexports introduce binding paths distinct from their canonical
// definitions. This table follows local Rust aliases, language barrels and
// external facade roots before candidate selection.
#[derive(Default)]
pub(in crate::linkage) struct BindingForwards {
	by_root: FxHashMap<Vec<u8>, Vec<u8>>,
	by_barrel: FxHashMap<Moniker, Moniker>,
	rust: RustForwards,
}

#[derive(Default)]
struct RustForwards {
	by_rust_member: FxHashMap<(Vec<u8>, Vec<Vec<u8>>), Moniker>,
	by_rust_alias: FxHashMap<Moniker, Moniker>,
	by_rust_local_alias: FxHashMap<(Vec<u8>, Vec<Vec<u8>>), Moniker>,
	by_rust_prefix: FxHashMap<(Vec<u8>, Vec<Vec<u8>>), Moniker>,
}

struct ForwardSite<'a> {
	manifests: &'a ManifestPolicy,
	file_idx: usize,
	source: &'a DefRecord,
	reference: &'a RefRecord,
}

impl BindingForwards {
	pub(in crate::linkage) fn build(
		material: &CodeIndexMaterial,
		manifests: &ManifestPolicy,
	) -> Self {
		let mut forwards = Self::default();
		for (file_idx, file) in material.files.iter().enumerate() {
			for ref_idx in 0..file.graph.ref_count() {
				let reference = file.graph.ref_at(ref_idx);
				let source = file.graph.def_at(reference.source);
				forwards.record_forward(
					file.lang,
					ForwardSite {
						manifests,
						file_idx,
						source,
						reference,
					},
				);
			}
		}
		forwards
	}

	fn record_forward(&mut self, lang: Lang, site: ForwardSite<'_>) {
		if lang == Lang::Rs {
			self.rust.record_local_binding(&site);
		}
		if site.reference.kind != kinds::REEXPORTS {
			return;
		}
		if let Some(target) = bare_external_root(&site.reference.target) {
			if lang == Lang::Rs && !site.reference.alias.is_empty() {
				self.rust.record_prefix(&site);
			} else {
				self.record_external_root(&site, target);
			}
			return;
		}
		if lang == Lang::Rs {
			self.rust.record_member(&site);
		}
		self.record_barrel(site.source, site.reference);
	}

	fn record_external_root(&mut self, site: &ForwardSite<'_>, target: &[u8]) {
		for root in source_package_roots(site.manifests, site.file_idx) {
			self.by_root.entry(root).or_insert_with(|| target.to_vec());
		}
	}

	fn record_barrel(&mut self, source: &DefRecord, reference: &RefRecord) {
		let Some(barrel) = wildcard_barrel(source, reference) else {
			return;
		};
		self.by_barrel
			.entry(barrel)
			.or_insert_with(|| reference.target.clone());
	}

	pub(in crate::linkage) fn rewrite(&self, target: &Moniker) -> Option<Moniker> {
		self.rust
			.rewrite_named(target)
			.or_else(|| self.rewrite_external_root(target))
			.or_else(|| self.rewrite_barrel_member(target))
	}

	pub(in crate::linkage) fn rewrite_rust_named(&self, target: &Moniker) -> Option<Moniker> {
		self.rust.rewrite_named(target)
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

impl RustForwards {
	fn record_local_binding(&mut self, site: &ForwardSite<'_>) {
		if !matches!(
			site.reference.kind.as_ref(),
			kinds::REEXPORTS | kinds::IMPORTS_MODULE | kinds::IMPORTS_SYMBOL
		) {
			return;
		}
		if let Some(alias) = rust_reexport_alias_moniker(site.source, site.reference) {
			insert_forward(
				&mut self.by_rust_local_alias,
				rust_local_alias_key(&alias),
				&site.reference.target,
			);
		}
	}

	fn record_member(&mut self, site: &ForwardSite<'_>) {
		let Some(path) = rust_named_reexport_path(site.source, site.reference) else {
			return;
		};
		if let Some(alias) = rust_reexport_alias_moniker(site.source, site.reference) {
			insert_forward(&mut self.by_rust_alias, alias, &site.reference.target);
		}
		for root in source_package_roots(site.manifests, site.file_idx) {
			if !site.reference.alias.is_empty() {
				insert_forward(
					&mut self.by_rust_prefix,
					(root.clone(), path.clone()),
					&site.reference.target,
				);
			}
			insert_forward(
				&mut self.by_rust_member,
				(root, path.clone()),
				&site.reference.target,
			);
		}
	}

	fn record_prefix(&mut self, site: &ForwardSite<'_>) {
		let Some(path) = rust_named_reexport_path(site.source, site.reference) else {
			return;
		};
		for root in source_package_roots(site.manifests, site.file_idx) {
			insert_forward(
				&mut self.by_rust_prefix,
				(root, path.clone()),
				&site.reference.target,
			);
		}
	}

	fn rewrite_named(&self, target: &Moniker) -> Option<Moniker> {
		if let Some(forwarded) = self.rewrite_local_alias(target) {
			return self.follow_aliases(forwarded);
		}
		let mut segments = target.as_view().segments();
		let head = segments.next()?;
		if head.kind != kinds::EXTERNAL_PKG {
			return None;
		}
		let segments = segments.collect::<Vec<_>>();
		let path = segments
			.iter()
			.map(|segment| segment.name.to_vec())
			.collect::<Vec<_>>();
		let forwarded = self
			.by_rust_member
			.get(&(head.name.to_vec(), path.clone()))
			.cloned()
			.or_else(|| self.rewrite_prefix(head.name, &segments, &path))?;
		self.follow_aliases(forwarded)
	}

	fn rewrite_prefix(
		&self,
		root: &[u8],
		segments: &[Segment<'_>],
		path: &[Vec<u8>],
	) -> Option<Moniker> {
		for prefix_len in (1..=path.len()).rev() {
			let Some(forwarded) = self
				.by_rust_prefix
				.get(&(root.to_vec(), path[..prefix_len].to_vec()))
			else {
				continue;
			};
			let mut builder = MonikerBuilder::from_view(forwarded.as_view());
			for segment in &segments[prefix_len..] {
				builder.segment(segment.kind, segment.name);
			}
			return Some(builder.build());
		}
		None
	}

	fn follow_aliases(&self, mut target: Moniker) -> Option<Moniker> {
		let limit = self.by_rust_alias.len() + self.by_rust_local_alias.len();
		for _ in 0..=limit {
			let forwarded = self
				.by_rust_alias
				.get(&target)
				.cloned()
				.or_else(|| self.rewrite_local_alias(&target));
			let Some(forwarded) = forwarded else {
				return Some(target);
			};
			target = forwarded;
		}
		None
	}

	fn rewrite_local_alias(&self, target: &Moniker) -> Option<Moniker> {
		let view = target.as_view();
		let segments = view
			.segments()
			.filter(|segment| {
				segment.kind != kinds::MODULE || !matches!(segment.name, b"lib" | b"main")
			})
			.collect::<Vec<_>>();
		for prefix_len in (1..=segments.len()).rev() {
			let key = (
				view.project().to_vec(),
				segments[..prefix_len]
					.iter()
					.map(|segment| segment.name.to_vec())
					.collect(),
			);
			let Some(forwarded) = self.by_rust_local_alias.get(&key) else {
				continue;
			};
			let mut builder = MonikerBuilder::from_view(forwarded.as_view());
			for segment in &segments[prefix_len..] {
				builder.segment(segment.kind, segment.name);
			}
			let rewritten = builder.build();
			if rewritten != *target {
				return Some(rewritten);
			}
		}
		None
	}
}

fn rust_local_alias_key(moniker: &Moniker) -> (Vec<u8>, Vec<Vec<u8>>) {
	let view = moniker.as_view();
	let path = view
		.segments()
		.filter(|segment| {
			segment.kind != kinds::MODULE || !matches!(segment.name, b"lib" | b"main")
		})
		.map(|segment| segment.name.to_vec())
		.collect();
	(view.project().to_vec(), path)
}

fn insert_forward<K>(forwards: &mut FxHashMap<K, Moniker>, key: K, target: &Moniker)
where
	K: Eq + Hash,
{
	forwards.entry(key).or_insert_with(|| target.clone());
}

fn rust_named_reexport_path(source: &DefRecord, reference: &RefRecord) -> Option<Vec<Vec<u8>>> {
	let source_is_module = source
		.moniker
		.as_view()
		.segments()
		.last()
		.is_some_and(|segment| segment.kind == kinds::MODULE);
	let target_member = reference.target.as_view().segments().last()?;
	if !source_is_module || target_member.kind == kinds::MODULE {
		return None;
	}
	let member_name = if reference.alias.is_empty() {
		target_member.name
	} else {
		reference.alias.as_ref()
	};
	let mut path = source
		.moniker
		.as_view()
		.segments()
		.filter(|segment| {
			segment.kind == kinds::MODULE && !matches!(segment.name, b"lib" | b"main")
		})
		.map(|segment| segment.name.to_vec())
		.collect::<Vec<_>>();
	path.push(member_name.to_vec());
	Some(path)
}

fn rust_reexport_alias_moniker(source: &DefRecord, reference: &RefRecord) -> Option<Moniker> {
	let member = rust_reexport_member(reference)?;
	let mut builder = MonikerBuilder::from_view(source.moniker.as_view());
	builder.segment(kinds::PATH, member);
	Some(builder.build())
}

fn rust_reexport_member(reference: &RefRecord) -> Option<&[u8]> {
	if !reference.alias.is_empty() {
		return Some(reference.alias.as_ref());
	}
	reference
		.target
		.as_view()
		.segments()
		.last()
		.map(|segment| segment.name)
}

// An `export * from "./inner"` records a module-to-module reexport: the
// barrel module's own members are aliases for the inner module's surface.
fn wildcard_barrel(source: &DefRecord, reference: &RefRecord) -> Option<Moniker> {
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
use std::hash::Hash;
