use code_moniker_core::core::code_graph::RefRecord;
use code_moniker_core::core::moniker::{Moniker, Segment};
use code_moniker_core::lang::{
	Lang,
	build_manifest::{Manifest, parse as parse_manifest},
};
use rustc_hash::{FxHashMap, FxHashSet};

use super::index::{DefLocation, RefLocation, SessionIndex};

#[derive(Clone, Debug, Default)]
pub(crate) struct LinkageIndex {
	stats: LinkageStats,
	manifests: ManifestLinkage,
	refs_by_source: FxHashMap<Moniker, Vec<RefLocation>>,
	refs_by_target_key: FxHashMap<LinkKey, Vec<RefLocation>>,
	refs_by_target_projectless_key: FxHashMap<LinkKey, Vec<RefLocation>>,
	refs_by_target_ancestor: FxHashMap<PathKey, Vec<RefLocation>>,
	refs_by_target_projectless_ancestor: FxHashMap<PathKey, Vec<RefLocation>>,
	refs_by_callable_name: FxHashMap<Vec<u8>, Vec<RefLocation>>,
	resolved_defs_by_ref: FxHashMap<RefLocation, Vec<DefLocation>>,
	unresolved_refs: Vec<RefLocation>,
	manifest_blocked_refs: Vec<RefLocation>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct LinkageStats {
	pub(crate) resolved_refs: usize,
	pub(crate) external_refs: usize,
	pub(crate) manifest_blocked_refs: usize,
	pub(crate) unresolved_refs: usize,
	pub(crate) ambiguous_refs: usize,
}

impl LinkageIndex {
	pub(crate) fn build(index: &SessionIndex) -> Self {
		let mut linkage = Self::default();
		let defs_by_key = defs_by_link_key(index, true);
		let projectless_defs_by_key = defs_by_link_key(index, false);
		let manifests = ManifestLinkage::build(index);
		linkage.manifests = manifests.clone();
		for (file_idx, file) in index.files.iter().enumerate() {
			for (ref_idx, reference) in file.graph.refs().enumerate() {
				let loc = RefLocation {
					file: file_idx,
					reference: ref_idx,
				};
				let source = file.graph.def_at(reference.source).moniker.clone();
				linkage.refs_by_source.entry(source).or_default().push(loc);
				linkage.index_target_reference(&reference.target, loc);
				let resolution = resolve_reference(
					index,
					&defs_by_key,
					&projectless_defs_by_key,
					&manifests,
					loc,
					reference,
				);
				let resolved = resolution.defs;
				if !resolved.is_empty() {
					linkage.stats.resolved_refs += 1;
					if resolved.len() > 1 {
						linkage.stats.ambiguous_refs += 1;
					}
					linkage.resolved_defs_by_ref.insert(loc, resolved);
				} else if resolution.manifest_blocked {
					linkage.stats.manifest_blocked_refs += 1;
					linkage.manifest_blocked_refs.push(loc);
				} else if ref_is_external(reference) {
					linkage.stats.external_refs += 1;
				} else {
					linkage.stats.unresolved_refs += 1;
					linkage.unresolved_refs.push(loc);
				}
			}
		}
		linkage
	}

	pub(crate) fn stats(&self) -> &LinkageStats {
		&self.stats
	}

	pub(crate) fn outgoing_refs(&self, source: &Moniker) -> &[RefLocation] {
		self.refs_by_source.get(source).map_or(&[], Vec::as_slice)
	}

	pub(crate) fn unresolved_refs(&self) -> &[RefLocation] {
		&self.unresolved_refs
	}

	pub(crate) fn manifest_blocked_refs(&self) -> &[RefLocation] {
		&self.manifest_blocked_refs
	}

	pub(crate) fn incoming_refs(&self, target: &Moniker, index: &SessionIndex) -> Vec<RefLocation> {
		let mut seen = FxHashSet::default();
		let mut out = Vec::new();
		self.collect_matching_refs(
			self.refs_by_target_key.get(&LinkKey::from_moniker(target)),
			target,
			index,
			&mut seen,
			&mut out,
		);
		self.collect_matching_refs(
			self.refs_by_target_projectless_key
				.get(&LinkKey::projectless(target)),
			target,
			index,
			&mut seen,
			&mut out,
		);
		self.collect_matching_refs(
			self.refs_by_target_ancestor
				.get(&PathKey::from_moniker(target)),
			target,
			index,
			&mut seen,
			&mut out,
		);
		self.collect_matching_refs(
			self.refs_by_target_projectless_ancestor
				.get(&PathKey::projectless(target)),
			target,
			index,
			&mut seen,
			&mut out,
		);
		if is_callable_moniker(target) {
			self.collect_matching_refs(
				self.refs_by_callable_name
					.get(last_bare_name(target).expect("callable moniker has a last segment")),
				target,
				index,
				&mut seen,
				&mut out,
			);
		}
		out.sort_by_key(|loc| (index.files[loc.file].rel_path.clone(), loc.reference));
		out
	}

	pub(crate) fn incoming_refs_for_def(
		&self,
		target: &DefLocation,
		index: &SessionIndex,
	) -> Vec<RefLocation> {
		let target_moniker = &index.def(target).moniker;
		self.incoming_refs(target_moniker, index)
			.into_iter()
			.filter(|loc| {
				let reference = index.reference(loc);
				let source = index.files[loc.file].graph.def_at(reference.source);
				if target_moniker.is_ancestor_of(&reference.target)
					&& !target_moniker.bind_match(&reference.target)
					&& target_moniker.is_ancestor_of(&source.moniker)
				{
					return false;
				}
				loc.file == target.file
					|| self.manifests.source_can_link_to_def(
						index,
						index.files[loc.file].source_root,
						target,
					)
			})
			.collect()
	}

	#[allow(dead_code)]
	pub(crate) fn resolved_defs(&self, loc: &RefLocation) -> &[DefLocation] {
		self.resolved_defs_by_ref
			.get(loc)
			.map_or(&[], Vec::as_slice)
	}

	fn index_target_reference(&mut self, target: &Moniker, loc: RefLocation) {
		self.refs_by_target_key
			.entry(LinkKey::from_moniker(target))
			.or_default()
			.push(loc);
		self.refs_by_target_projectless_key
			.entry(LinkKey::projectless(target))
			.or_default()
			.push(loc);
		for key in PathKey::ancestors(target, true) {
			self.refs_by_target_ancestor
				.entry(key)
				.or_default()
				.push(loc);
		}
		for key in PathKey::ancestors(target, false) {
			self.refs_by_target_projectless_ancestor
				.entry(key)
				.or_default()
				.push(loc);
		}
		if let Some(name) = last_bare_name(target) {
			self.refs_by_callable_name
				.entry(name.to_vec())
				.or_default()
				.push(loc);
		}
	}

	fn collect_matching_refs(
		&self,
		candidates: Option<&Vec<RefLocation>>,
		target: &Moniker,
		index: &SessionIndex,
		seen: &mut FxHashSet<RefLocation>,
		out: &mut Vec<RefLocation>,
	) {
		let Some(candidates) = candidates else {
			return;
		};
		for loc in candidates {
			if seen.insert(*loc) {
				let reference = index.reference(loc);
				if usage_target_matches(target, &reference.target) {
					out.push(*loc);
				}
			}
		}
	}
}

fn defs_by_link_key(
	index: &SessionIndex,
	include_project: bool,
) -> FxHashMap<LinkKey, Vec<DefLocation>> {
	let mut defs = FxHashMap::default();
	for (file_idx, file) in index.files.iter().enumerate() {
		for (def_idx, def) in file.graph.defs().enumerate() {
			defs.entry(LinkKey::new(&def.moniker, include_project))
				.or_insert_with(Vec::new)
				.push(DefLocation {
					file: file_idx,
					def: def_idx,
				});
		}
	}
	defs
}

struct RefResolution {
	defs: Vec<DefLocation>,
	manifest_blocked: bool,
}

fn resolve_reference(
	index: &SessionIndex,
	defs_by_key: &FxHashMap<LinkKey, Vec<DefLocation>>,
	projectless_defs_by_key: &FxHashMap<LinkKey, Vec<DefLocation>>,
	manifests: &ManifestLinkage,
	loc: RefLocation,
	reference: &RefRecord,
) -> RefResolution {
	let mut out = Vec::new();
	let mut blocked = false;
	collect_resolved_candidates(
		index,
		defs_by_key.get(&LinkKey::from_moniker(&reference.target)),
		reference,
		&mut out,
	);
	collect_resolved_candidates(
		index,
		projectless_defs_by_key.get(&LinkKey::projectless(&reference.target)),
		reference,
		&mut out,
	);
	out.retain(|def| {
		let allowed = loc.file == def.file
			|| manifests.source_can_link_to_def(index, index.files[loc.file].source_root, def);
		if !allowed {
			blocked = true;
		}
		allowed
	});
	out.sort_by_key(|def| (def.file, def.def));
	out.dedup();
	RefResolution {
		defs: out,
		manifest_blocked: blocked,
	}
}

fn collect_resolved_candidates(
	index: &SessionIndex,
	candidates: Option<&Vec<DefLocation>>,
	reference: &RefRecord,
	out: &mut Vec<DefLocation>,
) {
	let Some(candidates) = candidates else {
		return;
	};
	out.extend(candidates.iter().copied().filter(|loc| {
		reference.target.bind_match(&index.def(loc).moniker)
			|| moniker_matches_without_project(&reference.target, &index.def(loc).moniker)
	}));
}

fn ref_is_external(reference: &RefRecord) -> bool {
	reference.confidence == code_moniker_core::lang::kinds::CONF_EXTERNAL
		|| reference
			.target
			.as_view()
			.segments()
			.any(|segment| segment.kind == code_moniker_core::lang::kinds::EXTERNAL_PKG)
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct LinkKey {
	project: Option<Vec<u8>>,
	parents: Vec<(Vec<u8>, Vec<u8>)>,
	bare_last_name: Vec<u8>,
}

impl LinkKey {
	fn from_moniker(moniker: &Moniker) -> Self {
		Self::new(moniker, true)
	}

	fn projectless(moniker: &Moniker) -> Self {
		Self::new(moniker, false)
	}

	fn new(moniker: &Moniker, include_project: bool) -> Self {
		let view = moniker.as_view();
		let segments: Vec<_> = view.segments().collect();
		let parents = segments
			.iter()
			.take(segments.len().saturating_sub(1))
			.map(|segment| (segment.kind.to_vec(), segment.name.to_vec()))
			.collect();
		let bare_last_name = segments
			.last()
			.map(|segment| bare_callable_name(segment.name).to_vec())
			.unwrap_or_default();
		Self {
			project: include_project.then(|| view.project().to_vec()),
			parents,
			bare_last_name,
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct PathKey {
	project: Option<Vec<u8>>,
	segments: Vec<(Vec<u8>, Vec<u8>)>,
}

impl PathKey {
	fn from_moniker(moniker: &Moniker) -> Self {
		Self::new(moniker, true)
	}

	fn projectless(moniker: &Moniker) -> Self {
		Self::new(moniker, false)
	}

	fn ancestors(moniker: &Moniker, include_project: bool) -> Vec<Self> {
		let view = moniker.as_view();
		let segments: Vec<_> = view.segments().collect();
		(1..segments.len())
			.map(|len| Self {
				project: include_project.then(|| view.project().to_vec()),
				segments: segments[..len]
					.iter()
					.map(|segment| (segment.kind.to_vec(), segment.name.to_vec()))
					.collect(),
			})
			.collect()
	}

	fn new(moniker: &Moniker, include_project: bool) -> Self {
		let view = moniker.as_view();
		Self {
			project: include_project.then(|| view.project().to_vec()),
			segments: view
				.segments()
				.map(|segment| (segment.kind.to_vec(), segment.name.to_vec()))
				.collect(),
		}
	}
}

#[derive(Clone, Debug, Default)]
struct ManifestLinkage {
	root_packages: FxHashMap<usize, FxHashSet<String>>,
	deps_by_root: FxHashMap<usize, FxHashSet<String>>,
}

impl ManifestLinkage {
	fn build(index: &SessionIndex) -> Self {
		let mut linkage = Self::default();
		for (root_idx, root) in index.roots.iter().enumerate() {
			for manifest_path in manifest_candidates(&root.path) {
				let Some(manifest) = Manifest::for_filename(&manifest_path) else {
					continue;
				};
				let Ok(content) = std::fs::read_to_string(&manifest_path) else {
					continue;
				};
				let project = root.ctx.project.as_deref().unwrap_or(&root.label);
				let Ok(deps) = parse_manifest(manifest, project.as_bytes(), &content) else {
					continue;
				};
				for dep in deps {
					if dep.dep_kind == "package" {
						linkage
							.root_packages
							.entry(root_idx)
							.or_default()
							.insert(package_id(manifest, &dep.import_root));
					} else {
						linkage
							.deps_by_root
							.entry(root_idx)
							.or_default()
							.insert(package_id(manifest, &dep.import_root));
					}
				}
			}
		}
		linkage
	}

	fn source_can_link_to_def(
		&self,
		index: &SessionIndex,
		source_root: usize,
		def: &DefLocation,
	) -> bool {
		let target_root = index.files[def.file].source_root;
		if source_root == target_root {
			return true;
		}
		let Some(packages) = self.root_packages.get(&target_root) else {
			return true;
		};
		let Some(target_manifest) = manifest_for_lang(index.files[def.file].lang) else {
			return true;
		};
		let package_prefix = package_id_prefix(target_manifest);
		let matching_packages = packages
			.iter()
			.filter(|package| package.starts_with(&package_prefix))
			.collect::<Vec<_>>();
		if matching_packages.is_empty() {
			return true;
		}
		self.deps_by_root.get(&source_root).is_some_and(|deps| {
			matching_packages
				.iter()
				.any(|package| deps.contains(*package))
		})
	}
}

fn package_id(manifest: Manifest, import_root: &str) -> String {
	format!("{}\0{import_root}", manifest.tag())
}

fn package_id_prefix(manifest: Manifest) -> String {
	format!("{}\0", manifest.tag())
}

fn manifest_for_lang(lang: Lang) -> Option<Manifest> {
	match lang {
		Lang::Ts => Some(Manifest::PackageJson),
		Lang::Rs => Some(Manifest::Cargo),
		Lang::Java => Some(Manifest::PomXml),
		Lang::Python => Some(Manifest::Pyproject),
		Lang::Go => Some(Manifest::GoMod),
		Lang::Cs => Some(Manifest::Csproj),
		Lang::Sql => None,
	}
}

fn manifest_candidates(root: &std::path::Path) -> Vec<std::path::PathBuf> {
	let mut out = Vec::new();
	for name in [
		"Cargo.toml",
		"package.json",
		"pom.xml",
		"pyproject.toml",
		"go.mod",
	] {
		let path = root.join(name);
		if path.is_file() {
			out.push(path);
		}
	}
	if let Ok(entries) = std::fs::read_dir(root) {
		for entry in entries.flatten() {
			let path = entry.path();
			if path.is_file()
				&& path
					.file_name()
					.and_then(|name| name.to_str())
					.is_some_and(|name| name.ends_with(".csproj"))
			{
				out.push(path);
			}
		}
	}
	out
}

fn usage_target_matches(selected: &Moniker, reference_target: &Moniker) -> bool {
	selected.bind_match(reference_target)
		|| selected.is_ancestor_of(reference_target)
		|| moniker_matches_without_project(selected, reference_target)
		|| moniker_is_ancestor_without_project(selected, reference_target)
		|| callable_last_segment_matches(selected, reference_target)
}

fn moniker_matches_without_project(left: &Moniker, right: &Moniker) -> bool {
	let left_segments: Vec<_> = left.as_view().segments().collect();
	let right_segments: Vec<_> = right.as_view().segments().collect();
	if left_segments.len() != right_segments.len() || left_segments.is_empty() {
		return false;
	}
	let last_idx = left_segments.len() - 1;
	left_segments[..last_idx] == right_segments[..last_idx]
		&& segment_names_match(left_segments[last_idx], right_segments[last_idx])
}

fn moniker_is_ancestor_without_project(parent: &Moniker, child: &Moniker) -> bool {
	let parent_segments: Vec<_> = parent.as_view().segments().collect();
	let child_segments: Vec<_> = child.as_view().segments().collect();
	if parent_segments.is_empty() || parent_segments.len() >= child_segments.len() {
		return false;
	}
	child_segments.starts_with(&parent_segments)
}

fn segment_names_match(left: Segment<'_>, right: Segment<'_>) -> bool {
	left.name == right.name || bare_callable_name(left.name) == bare_callable_name(right.name)
}

fn callable_last_segment_matches(selected: &Moniker, reference_target: &Moniker) -> bool {
	let Some(selected_segment) = selected.as_view().segments().last() else {
		return false;
	};
	let Some(target_segment) = reference_target.as_view().segments().last() else {
		return false;
	};
	if !is_callable_def_kind(selected_segment.kind) {
		return false;
	}
	bare_callable_name(selected_segment.name) == bare_callable_name(target_segment.name)
}

fn last_bare_name(moniker: &Moniker) -> Option<&[u8]> {
	moniker
		.as_view()
		.segments()
		.last()
		.map(|segment| bare_callable_name(segment.name))
}

fn is_callable_def_kind(kind: &[u8]) -> bool {
	matches!(kind, b"method" | b"function" | b"func" | b"constructor")
}

fn is_callable_moniker(moniker: &Moniker) -> bool {
	moniker
		.as_view()
		.segments()
		.last()
		.is_some_and(|segment| is_callable_def_kind(segment.kind))
}

fn bare_callable_name(name: &[u8]) -> &[u8] {
	name.iter()
		.position(|b| *b == b'(')
		.map_or(name, |idx| &name[..idx])
}

#[cfg(test)]
mod tests {
	use std::path::Path;

	use super::*;
	use crate::workspace::{SessionOptions, symbols::last_name};

	fn write(root: &Path, rel: &str, body: &str) {
		let p = root.join(rel);
		if let Some(parent) = p.parent() {
			std::fs::create_dir_all(parent).unwrap();
		}
		std::fs::write(p, body).unwrap();
	}

	fn pom(group: &str, artifact: &str, deps: &[(&str, &str)]) -> String {
		let dependencies = deps
			.iter()
			.map(|(group, artifact)| {
				format!(
					"<dependency><groupId>{group}</groupId><artifactId>{artifact}</artifactId></dependency>"
				)
			})
			.collect::<String>();
		format!(
			"<project><groupId>{group}</groupId><artifactId>{artifact}</artifactId><dependencies>{dependencies}</dependencies></project>"
		)
	}

	#[test]
	fn incoming_refs_resolve_import_placeholders_to_typed_defs() {
		let tmp = tempfile::tempdir().unwrap();
		write(tmp.path(), "src/lib.ts", "export class Lib {}\n");
		write(
			tmp.path(),
			"src/app.ts",
			"import { Lib } from './lib'; export const value = Lib;\n",
		);
		let index = SessionIndex::load(&SessionOptions {
			paths: vec![tmp.path().to_path_buf()],
			project: Some("app".into()),
			cache_dir: None,
		})
		.unwrap();
		let linkage = LinkageIndex::build(&index);
		let lib = index
			.defs_by_name
			.get("Lib")
			.and_then(|locs| {
				locs.iter()
					.find(|loc| last_name(&index.def(loc).moniker) == "Lib")
			})
			.copied()
			.expect("Lib def");

		let refs = linkage.incoming_refs(&index.def(&lib).moniker, &index);

		assert!(
			refs.iter().any(|loc| index
				.reference(loc)
				.target
				.bind_match(&index.def(&lib).moniker)),
			"expected import placeholder to resolve to typed Lib def"
		);
	}

	#[test]
	fn incoming_refs_include_descendant_targets_for_changed_parent_symbols() {
		let tmp = tempfile::tempdir().unwrap();
		write(
			tmp.path(),
			"src/lib.ts",
			"export class Lib { run() { return 1; } }\n",
		);
		write(
			tmp.path(),
			"src/app.ts",
			"import { Lib } from './lib'; export const value = new Lib().run();\n",
		);
		let index = SessionIndex::load(&SessionOptions {
			paths: vec![tmp.path().to_path_buf()],
			project: Some("app".into()),
			cache_dir: None,
		})
		.unwrap();
		let linkage = LinkageIndex::build(&index);
		let lib = index
			.defs_by_name
			.get("Lib")
			.and_then(|locs| {
				locs.iter()
					.find(|loc| last_name(&index.def(loc).moniker) == "Lib")
			})
			.copied()
			.expect("Lib def");

		let refs = linkage.incoming_refs(&index.def(&lib).moniker, &index);

		assert!(!refs.is_empty());
	}

	#[test]
	fn resolves_cross_project_java_imports_allowed_by_pom_dependency() {
		let tmp = tempfile::tempdir().unwrap();
		let common = tmp.path().join("common-lib");
		let billing = tmp.path().join("billing-service");
		write(&common, "pom.xml", &pom("com.acme", "common-lib", &[]));
		write(
			&common,
			"src/main/java/com/acme/common/customer/CustomerProfile.java",
			"package com.acme.common.customer; public class CustomerProfile {}\n",
		);
		write(
			&billing,
			"pom.xml",
			&pom("com.acme", "billing-service", &[("com.acme", "common-lib")]),
		);
		write(
			&billing,
			"src/main/java/com/acme/billing/BillingApplication.java",
			"package com.acme.billing; import com.acme.common.customer.CustomerProfile; public class BillingApplication { CustomerProfile p; }\n",
		);
		let index = SessionIndex::load(&SessionOptions {
			paths: vec![common, billing],
			project: None,
			cache_dir: None,
		})
		.unwrap();
		let linkage = LinkageIndex::build(&index);
		let profile = index
			.defs_by_name
			.get("CustomerProfile")
			.and_then(|locs| {
				locs.iter()
					.find(|loc| index.files[loc.file].source_root == 0)
			})
			.copied()
			.expect("CustomerProfile def");

		let refs = linkage.incoming_refs_for_def(&profile, &index);

		assert!(
			refs.iter()
				.any(|loc| index.files[loc.file].source_root == 1),
			"billing-service import should resolve to common-lib def through pom dependency"
		);
		assert!(
			linkage.stats().resolved_refs > 0,
			"cross-project import should contribute to resolved linkage stats"
		);
	}

	#[test]
	fn classifies_cross_project_java_imports_blocked_by_missing_pom_dependency() {
		let tmp = tempfile::tempdir().unwrap();
		let common = tmp.path().join("common-lib");
		let billing = tmp.path().join("billing-service");
		write(&common, "pom.xml", &pom("com.acme", "common-lib", &[]));
		write(
			&common,
			"src/main/java/com/acme/common/customer/CustomerProfile.java",
			"package com.acme.common.customer; public class CustomerProfile {}\n",
		);
		write(
			&billing,
			"pom.xml",
			&pom("com.acme", "billing-service", &[]),
		);
		write(
			&billing,
			"src/main/java/com/acme/billing/BillingApplication.java",
			"package com.acme.billing; import com.acme.common.customer.CustomerProfile; public class BillingApplication { CustomerProfile p; }\n",
		);
		let index = SessionIndex::load(&SessionOptions {
			paths: vec![common, billing],
			project: None,
			cache_dir: None,
		})
		.unwrap();
		let linkage = LinkageIndex::build(&index);

		assert!(
			linkage.stats().manifest_blocked_refs > 0,
			"missing pom dependency should classify cross-root Java matches as blocked"
		);
	}

	#[test]
	fn package_json_dependency_does_not_unlock_java_defs_in_mixed_root() {
		let tmp = tempfile::tempdir().unwrap();
		let mixed = tmp.path().join("mixed-lib");
		let app = tmp.path().join("app");
		write(&mixed, "pom.xml", &pom("com.acme", "java-lib", &[]));
		write(
			&mixed,
			"package.json",
			r#"{"name":"web-lib","version":"1.0.0"}"#,
		);
		write(
			&mixed,
			"src/main/java/com/acme/lib/Lib.java",
			"package com.acme.lib; public class Lib {}\n",
		);
		write(&app, "pom.xml", &pom("com.acme", "app", &[]));
		write(
			&app,
			"package.json",
			r#"{"name":"app","version":"1.0.0","dependencies":{"web-lib":"1.0.0"}}"#,
		);
		write(
			&app,
			"src/main/java/com/acme/app/App.java",
			"package com.acme.app; import com.acme.lib.Lib; public class App { Lib lib; }\n",
		);
		let index = SessionIndex::load(&SessionOptions {
			paths: vec![mixed, app],
			project: None,
			cache_dir: None,
		})
		.unwrap();
		let linkage = LinkageIndex::build(&index);
		let lib = index
			.defs_by_name
			.get("Lib")
			.and_then(|locs| {
				locs.iter()
					.find(|loc| index.files[loc.file].source_root == 0)
			})
			.copied()
			.expect("Lib class def");

		let refs = linkage.incoming_refs_for_def(&lib, &index);

		assert!(
			refs.iter()
				.all(|loc| index.files[loc.file].source_root != 1),
			"package.json dependency should not authorize Java linkage without matching pom dependency"
		);
		assert!(
			linkage.stats().manifest_blocked_refs > 0,
			"Java import should be classified as manifest-blocked"
		);
	}

	#[test]
	fn incoming_refs_for_method_excludes_own_descendant_local_reads() {
		let tmp = tempfile::tempdir().unwrap();
		write(
			tmp.path(),
			"src/main/java/com/acme/MoneyFormatter.java",
			"package com.acme; public class MoneyFormatter { public String formatForInvoice(long cents) { return String.valueOf(cents); } }\n",
		);
		let index = SessionIndex::load(&SessionOptions {
			paths: vec![tmp.path().to_path_buf()],
			project: Some("app".into()),
			cache_dir: None,
		})
		.unwrap();
		let linkage = LinkageIndex::build(&index);
		let method = index
			.defs_by_name
			.get("formatForInvoice(cents:long)")
			.and_then(|locs| locs.first())
			.copied()
			.expect("formatForInvoice def");

		let refs = linkage.incoming_refs_for_def(&method, &index);

		assert!(
			refs.iter()
				.all(|loc| index.reference(loc).source != method.def),
			"local reads inside the selected method should not make the method appear in its own usage lens"
		);
	}

	#[test]
	fn incoming_refs_for_class_keeps_external_descendant_member_usages() {
		let tmp = tempfile::tempdir().unwrap();
		let lib = tmp.path().join("lib");
		let app = tmp.path().join("app");
		write(&lib, "pom.xml", &pom("com.acme", "lib", &[]));
		write(
			&lib,
			"src/main/java/com/acme/lib/Lib.java",
			"package com.acme.lib; public class Lib { public void run() {} }\n",
		);
		write(
			&app,
			"pom.xml",
			&pom("com.acme", "app", &[("com.acme", "lib")]),
		);
		write(
			&app,
			"src/main/java/com/acme/app/App.java",
			"package com.acme.app; import com.acme.lib.Lib; public class App { void go() { new Lib().run(); } }\n",
		);
		let index = SessionIndex::load(&SessionOptions {
			paths: vec![lib, app],
			project: None,
			cache_dir: None,
		})
		.unwrap();
		let linkage = LinkageIndex::build(&index);
		let class = index
			.defs_by_name
			.get("Lib")
			.and_then(|locs| {
				locs.iter()
					.find(|loc| index.files[loc.file].source_root == 0)
			})
			.copied()
			.expect("Lib class def");

		let refs = linkage.incoming_refs_for_def(&class, &index);

		assert!(
			refs.iter()
				.any(|loc| index.files[loc.file].source_root == 1),
			"external descendant member usage should remain visible when selecting the parent class"
		);
	}

	#[test]
	fn manifest_linkage_keeps_multiple_root_packages() {
		let tmp = tempfile::tempdir().unwrap();
		write(tmp.path(), "pom.xml", &pom("com.acme", "java-lib", &[]));
		write(
			tmp.path(),
			"package.json",
			r#"{"name":"web-lib","version":"1.0.0"}"#,
		);
		let index = SessionIndex::catalog(crate::sources::SourceSet {
			roots: vec![crate::sources::SourceRoot {
				input: tmp.path().to_path_buf(),
				path: tmp.path().to_path_buf(),
				label: "mixed".into(),
				ctx: crate::extract::Context {
					ts: crate::tsconfig::TsResolution::default(),
					project: Some("mixed".into()),
				},
			}],
			files: Vec::new(),
			multi: false,
		});

		let manifests = ManifestLinkage::build(&index);
		let packages = manifests.root_packages.get(&0).expect("root packages");

		assert!(packages.contains(&package_id(Manifest::PomXml, "com.acme:java-lib")));
		assert!(packages.contains(&package_id(Manifest::PackageJson, "web-lib")));
	}
}
