use code_moniker_core::core::code_graph::{DefRecord, RefRecord};
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder, Segment};
use code_moniker_core::lang::{Lang, kinds};
use rustc_hash::{FxHashMap, FxHashSet};

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
	by_crate_root: FxHashMap<Vec<u8>, Moniker>,
	by_macro_scope: FxHashMap<Moniker, Vec<Moniker>>,
	by_rust_member: FxHashMap<RustPublicPathKey, Vec<Moniker>>,
	by_rust_alias: FxHashMap<Moniker, Vec<Moniker>>,
	by_rust_local_alias: FxHashMap<RustLocalAliasKey, Vec<Moniker>>,
	by_rust_local_wildcard: FxHashMap<RustLocalAliasKey, Vec<Moniker>>,
	by_rust_prefix: FxHashMap<RustPublicPathKey, Vec<Moniker>>,
}

type RustPublicPathKey = (Vec<u8>, Vec<Vec<u8>>);
type RustLocalAliasKey = (Vec<u8>, Vec<(Vec<u8>, Vec<u8>)>);

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
			if file.lang == Lang::Rs
				&& let Some(import_root) =
					manifests.rust_library_import_root_for_file(file_idx, &file.path)
				&& let Some(module) = rust_file_module(file)
			{
				forwards
					.rust
					.by_crate_root
					.insert(import_root.as_bytes().to_vec(), module);
			}
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
			.into_iter()
			.next()
			.or_else(|| self.rewrite_external_root(target))
			.or_else(|| self.rewrite_barrel_member(target))
	}

	pub(in crate::linkage) fn rewrite_rust_named(&self, target: &Moniker) -> Vec<Moniker> {
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
		if site.reference.kind == kinds::IMPORTS_MODULE
			&& site.reference.receiver_hint.as_ref() == b"rust_macro_use"
		{
			let scope = rust_macro_scope(&site.source.moniker);
			insert_rust_forward(&mut self.by_macro_scope, scope, &site.reference.target);
		}
		if !matches!(
			site.reference.kind.as_ref(),
			kinds::REEXPORTS | kinds::IMPORTS_MODULE | kinds::IMPORTS_SYMBOL
		) {
			return;
		}
		if site.reference.kind == kinds::REEXPORTS && rust_wildcard_reexport(site.reference) {
			insert_rust_forward(
				&mut self.by_rust_local_wildcard,
				rust_local_alias_key(&site.source.moniker),
				&site.reference.target,
			);
			return;
		}
		if let Some(alias) = rust_reexport_alias_moniker(site.source, site.reference) {
			insert_rust_forward(
				&mut self.by_rust_local_alias,
				rust_local_alias_key(&alias),
				&site.reference.target,
			);
		}
	}

	fn record_member(&mut self, site: &ForwardSite<'_>) {
		if !site
			.manifests
			.authorizes_reexport_target(site.file_idx, &site.reference.target)
		{
			return;
		}
		let Some(path) = rust_public_reexport_path(site.source, site.reference) else {
			return;
		};
		if rust_wildcard_reexport(site.reference) {
			for root in source_package_roots(site.manifests, site.file_idx) {
				insert_rust_forward(
					&mut self.by_rust_prefix,
					(root, path.clone()),
					&site.reference.target,
				);
			}
			return;
		}
		if let Some(alias) = rust_reexport_alias_moniker(site.source, site.reference) {
			insert_rust_forward(&mut self.by_rust_alias, alias, &site.reference.target);
		}
		for root in source_package_roots(site.manifests, site.file_idx) {
			insert_rust_forward(
				&mut self.by_rust_prefix,
				(root.clone(), path.clone()),
				&site.reference.target,
			);
			insert_rust_forward(
				&mut self.by_rust_member,
				(root, path.clone()),
				&site.reference.target,
			);
		}
	}

	fn record_prefix(&mut self, site: &ForwardSite<'_>) {
		let Some(path) = rust_public_reexport_path(site.source, site.reference) else {
			return;
		};
		for root in source_package_roots(site.manifests, site.file_idx) {
			insert_rust_forward(
				&mut self.by_rust_prefix,
				(root, path.clone()),
				&site.reference.target,
			);
		}
	}

	fn rewrite_named(&self, target: &Moniker) -> Vec<Moniker> {
		let mut forwarded = self.rewrite_macro_scope(target);
		if forwarded.is_empty() {
			forwarded = self.rewrite_crate_root(target);
		}
		if forwarded.is_empty() {
			forwarded = self.rewrite_local_alias(target);
		}
		if forwarded.is_empty() {
			forwarded = self.rewrite_external_member(target);
		}
		self.follow_aliases(forwarded)
	}

	fn rewrite_macro_scope(&self, target: &Moniker) -> Vec<Moniker> {
		let Some(macro_name) = target
			.as_view()
			.segments()
			.last()
			.filter(|segment| segment.kind == b"macro")
			.map(|segment| segment.name)
		else {
			return Vec::new();
		};
		let mut current = target.parent();
		while let Some(scope) = current {
			if let Some(modules) = self.by_macro_scope.get(&scope) {
				return modules
					.iter()
					.map(|module| {
						MonikerBuilder::from_view(module.as_view())
							.segment(b"macro", macro_name)
							.build()
					})
					.collect();
			}
			current = scope.parent();
		}
		Vec::new()
	}

	fn rewrite_crate_root(&self, target: &Moniker) -> Vec<Moniker> {
		let mut segments = target.as_view().segments();
		let Some(root) = segments.next() else {
			return Vec::new();
		};
		if root.kind != kinds::EXTERNAL_PKG || segments.next().is_some() {
			return Vec::new();
		}
		self.by_crate_root
			.get(root.name)
			.cloned()
			.into_iter()
			.collect()
	}

	fn rewrite_external_member(&self, target: &Moniker) -> Vec<Moniker> {
		let mut segments = target.as_view().segments();
		let Some(head) = segments.next() else {
			return Vec::new();
		};
		if head.kind != kinds::EXTERNAL_PKG {
			return Vec::new();
		}
		let segments = segments.collect::<Vec<_>>();
		let path = segments
			.iter()
			.map(|segment| segment.name.to_vec())
			.collect::<Vec<_>>();
		self.by_rust_member
			.get(&(head.name.to_vec(), path.clone()))
			.cloned()
			.unwrap_or_else(|| self.rewrite_prefix(head.name, &segments, &path))
	}

	fn rewrite_prefix(
		&self,
		root: &[u8],
		segments: &[Segment<'_>],
		path: &[Vec<u8>],
	) -> Vec<Moniker> {
		for prefix_len in (1..=path.len()).rev() {
			let Some(forwarded) = self
				.by_rust_prefix
				.get(&(root.to_vec(), path[..prefix_len].to_vec()))
			else {
				continue;
			};
			return forwarded
				.iter()
				.map(|forwarded| {
					let mut builder = MonikerBuilder::from_view(forwarded.as_view());
					for segment in &segments[prefix_len..] {
						builder.segment(segment.kind, segment.name);
					}
					builder.build()
				})
				.collect();
		}
		Vec::new()
	}

	fn follow_aliases(&self, targets: Vec<Moniker>) -> Vec<Moniker> {
		let mut pending = targets
			.into_iter()
			.map(|target| (target, FxHashSet::<RustLocalAliasKey>::default()))
			.collect::<Vec<_>>();
		let mut seen = FxHashSet::default();
		let mut leaves = Vec::new();
		while let Some((target, used_local_aliases)) = pending.pop() {
			if !seen.insert(target.clone()) {
				continue;
			}
			let mut forwarded = self.by_rust_alias.get(&target).cloned().unwrap_or_default();
			let mut next_used_local_aliases = used_local_aliases.clone();
			if forwarded.is_empty() {
				if let Some((key, local)) =
					self.rewrite_local_alias_excluding(&target, &used_local_aliases)
				{
					next_used_local_aliases.insert(key);
					forwarded = local;
				}
			}
			if forwarded.is_empty() {
				if let Some((key, local)) = self.rewrite_local_forward_excluding(
					&target,
					&used_local_aliases,
					&self.by_rust_local_wildcard,
					true,
				) {
					next_used_local_aliases.insert(key);
					forwarded = local;
				}
			}
			if forwarded.is_empty() {
				forwarded = self.rewrite_external_member(&target);
			}
			if forwarded.is_empty() {
				leaves.push(target);
			} else {
				pending.extend(
					forwarded
						.into_iter()
						.map(|target| (target, next_used_local_aliases.clone())),
				);
			}
		}
		leaves
	}

	fn rewrite_local_alias(&self, target: &Moniker) -> Vec<Moniker> {
		self.rewrite_local_alias_excluding(target, &FxHashSet::default())
			.map(|(_, targets)| targets)
			.unwrap_or_default()
	}

	fn rewrite_local_alias_excluding(
		&self,
		target: &Moniker,
		used: &FxHashSet<RustLocalAliasKey>,
	) -> Option<(RustLocalAliasKey, Vec<Moniker>)> {
		self.rewrite_local_forward_excluding(target, used, &self.by_rust_local_alias, false)
	}

	fn rewrite_local_forward_excluding(
		&self,
		target: &Moniker,
		used: &FxHashSet<RustLocalAliasKey>,
		forwards: &FxHashMap<RustLocalAliasKey, Vec<Moniker>>,
		require_suffix: bool,
	) -> Option<(RustLocalAliasKey, Vec<Moniker>)> {
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
					.enumerate()
					.map(|(index, segment)| {
						rust_local_alias_segment_key(segment, index + 1 == prefix_len)
					})
					.collect(),
			);
			if require_suffix && prefix_len == segments.len() {
				continue;
			}
			if used.contains(&key) {
				continue;
			}
			let Some(forwarded) = forwards.get(&key) else {
				continue;
			};
			let rewritten = forwarded
				.iter()
				.filter_map(|forwarded| {
					if forwarded.is_ancestor_of(target) {
						return None;
					}
					let mut builder = MonikerBuilder::from_view(forwarded.as_view());
					for segment in &segments[prefix_len..] {
						builder.segment(segment.kind, segment.name);
					}
					let rewritten = builder.build();
					(rewritten != *target).then_some(rewritten)
				})
				.collect::<Vec<_>>();
			if !rewritten.is_empty() {
				return Some((key, rewritten));
			}
		}
		None
	}
}

fn rust_local_alias_key(moniker: &Moniker) -> RustLocalAliasKey {
	let view = moniker.as_view();
	let segments = view
		.segments()
		.filter(|segment| {
			segment.kind != kinds::MODULE || !matches!(segment.name, b"lib" | b"main")
		})
		.collect::<Vec<_>>();
	let path = segments
		.iter()
		.enumerate()
		.map(|(index, segment)| rust_local_alias_segment_key(segment, index + 1 == segments.len()))
		.collect();
	(view.project().to_vec(), path)
}

fn rust_local_alias_segment_key(segment: &Segment<'_>, terminal: bool) -> (Vec<u8>, Vec<u8>) {
	let kind = if terminal && !rust_scope_kind(segment.kind) {
		kinds::PATH
	} else {
		segment.kind
	};
	(kind.to_vec(), segment.name.to_vec())
}

fn rust_scope_kind(kind: &[u8]) -> bool {
	matches!(
		kind,
		kinds::LANG
			| kinds::DIR
			| kinds::MODULE
			| kinds::PACKAGE
			| kinds::EXTERNAL_PKG
			| kinds::SDK
	)
}

fn rust_file_module(file: &crate::source::IndexedSourceFile) -> Option<Moniker> {
	let root = file.graph.root();
	root.as_view()
		.segments()
		.last()
		.is_some_and(|segment| segment.kind == kinds::MODULE)
		.then(|| root.clone())
}

fn rust_macro_scope(source: &Moniker) -> Moniker {
	let last = source.as_view().segments().last();
	if last.is_some_and(|segment| {
		segment.kind == kinds::MODULE && matches!(segment.name, b"lib" | b"main")
	}) {
		return source.parent().unwrap_or_else(|| source.clone());
	}
	source.clone()
}

fn insert_rust_forward<K>(forwards: &mut FxHashMap<K, Vec<Moniker>>, key: K, target: &Moniker)
where
	K: Eq + Hash,
{
	let targets = forwards.entry(key).or_default();
	if !targets.contains(target) {
		targets.push(target.clone());
	}
}

fn rust_public_reexport_path(source: &DefRecord, reference: &RefRecord) -> Option<Vec<Vec<u8>>> {
	let source_is_module = source
		.moniker
		.as_view()
		.segments()
		.last()
		.is_some_and(|segment| segment.kind == kinds::MODULE);
	let target_member = reference.target.as_view().segments().last()?;
	if !source_is_module {
		return None;
	}
	if target_member.kind == kinds::MODULE {
		return rust_wildcard_reexport(reference).then(|| rust_module_path(source));
	}
	let member_name = if reference.alias.is_empty() {
		target_member.name
	} else {
		reference.alias.as_ref()
	};
	let mut path = rust_module_path(source);
	path.push(member_name.to_vec());
	Some(path)
}

fn rust_module_path(source: &DefRecord) -> Vec<Vec<u8>> {
	source
		.moniker
		.as_view()
		.segments()
		.filter(|segment| {
			segment.kind == kinds::MODULE && !matches!(segment.name, b"lib" | b"main")
		})
		.map(|segment| segment.name.to_vec())
		.collect()
}

fn rust_wildcard_reexport(reference: &RefRecord) -> bool {
	reference.receiver_hint.as_ref() == b"rust_wildcard"
		&& reference.alias.is_empty()
		&& reference
			.target
			.as_view()
			.segments()
			.last()
			.is_some_and(|segment| segment.kind == kinds::MODULE)
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
