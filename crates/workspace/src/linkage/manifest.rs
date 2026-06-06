// code-moniker: ignore-file[smell-vertical-layout]
use std::path::{Path, PathBuf};

use code_moniker_core::lang::build_manifest::{Manifest, parse as parse_manifest};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::linkage::candidate::CandidateCatalog;
use crate::linkage::decision::{ReferenceLinkageDecision, ResolutionScope};
use crate::linkage::language;
use crate::linkage::ordinals::SymbolSet;
use crate::linkage::query::LinkageQuery;
use crate::snapshot::ReferenceRecord;
use crate::source::CodeIndexMaterial;
use crate::sources::SourceRoot;

#[derive(Default)]
pub(super) struct ManifestPolicy {
	entries_by_root: FxHashMap<usize, Vec<ManifestEntry>>,
	entry_by_file: FxHashMap<usize, ManifestEntryLocation>,
}

#[derive(Clone, Debug)]
struct ManifestEntry {
	path: PathBuf,
	manifest: Manifest,
	packages: FxHashSet<String>,
	deps: FxHashSet<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ManifestEntryLocation {
	source_root: usize,
	entry: usize,
}

impl ManifestPolicy {
	pub(super) fn build(material: &CodeIndexMaterial) -> Self {
		let mut policy = Self::default();
		for (root_idx, root) in material.source_catalog.sources.roots.iter().enumerate() {
			let entries = manifest_entries_for_root(root);
			if !entries.is_empty() {
				policy.entries_by_root.insert(root_idx, entries);
			}
		}
		policy.index_files(material);
		policy
	}

	pub(super) fn evaluate_global_targets(
		&self,
		query: &LinkageQuery<'_>,
		candidates: SymbolSet,
		catalog: &CandidateCatalog<'_>,
	) -> GlobalTargetPolicy {
		let mut policy = GlobalTargetPolicy {
			external_dependency: self.can_classify_as_declared_external(query),
			..GlobalTargetPolicy::default()
		};
		for symbol in candidates.iter() {
			let Some(candidate) = catalog.candidate(symbol) else {
				continue;
			};
			let target_file = candidate.source_file;
			if let Some(import_root) = external_import_root(query)
				&& !self.target_file_declares_import_root(query.material, target_file, import_root)
			{
				continue;
			}
			match self.source_can_link_to_file(query.material, query.source_file, target_file) {
				LinkPermission::Allowed => {
					policy.allowed.insert(symbol);
				}
				LinkPermission::Blocked => policy.blocked = true,
				LinkPermission::Unknown => policy.unknown = true,
			}
		}
		policy
	}

	fn can_classify_as_declared_external(&self, query: &LinkageQuery<'_>) -> bool {
		if let Some(import_root) = external_import_root(query) {
			return self.source_builtin_external_root(query, import_root)
				|| self.source_declares_import_root(query, import_root);
		}
		if source_declares_language_package_target(self, query) {
			return true;
		}
		language::proc_macro_annotation(query) && self.source_declares_dependencies(query)
	}

	fn source_declares_dependencies(&self, query: &LinkageQuery<'_>) -> bool {
		self.entry_for_file(query.source_file)
			.is_some_and(|entry| entry.manifest == Manifest::Cargo && !entry.deps.is_empty())
	}

	fn entry_for_file(&self, file_idx: usize) -> Option<&ManifestEntry> {
		let location = self.entry_by_file.get(&file_idx)?;
		self.entries_by_root
			.get(&location.source_root)?
			.get(location.entry)
	}

	fn source_declares_import_root(&self, query: &LinkageQuery<'_>, import_root: &str) -> bool {
		let Some(source_lang) = query
			.material
			.files
			.get(query.source_file)
			.map(|file| file.lang)
		else {
			return false;
		};
		let Some(source_manifest) = language::manifest_for_lang(source_lang) else {
			return false;
		};
		self.entry_for_file(query.source_file).is_some_and(|entry| {
			let package = package_id(source_manifest, import_root);
			entry.deps.contains(&package) || source_root_declares_dependency(self, query, &package)
		})
	}

	fn target_file_declares_import_root(
		&self,
		material: &CodeIndexMaterial,
		target_file: usize,
		import_root: &str,
	) -> bool {
		let Some(target_lang) = material.files.get(target_file).map(|file| file.lang) else {
			return false;
		};
		let Some(target_manifest) = language::manifest_for_lang(target_lang) else {
			return false;
		};
		let Some(target_entry) = self.entry_for_file(target_file) else {
			return false;
		};
		target_entry
			.packages
			.contains(&package_id(target_manifest, import_root))
	}

	fn source_can_link_to_file(
		&self,
		material: &CodeIndexMaterial,
		source_file: usize,
		target_file: usize,
	) -> LinkPermission {
		if source_file == target_file {
			return LinkPermission::Allowed;
		}
		let source_entry = self.entry_for_file(source_file);
		let target_entry = self.entry_for_file(target_file);
		if source_entry.map(|entry| &entry.path) == target_entry.map(|entry| &entry.path) {
			return LinkPermission::Allowed;
		}
		let Some(target_lang) = material.files.get(target_file).map(|file| file.lang) else {
			return LinkPermission::Unknown;
		};
		let Some(target_manifest) = language::manifest_for_lang(target_lang) else {
			return LinkPermission::Unknown;
		};
		let Some(target_entry) = target_entry else {
			return LinkPermission::Unknown;
		};
		let package_prefix = package_id_prefix(target_manifest);
		let matching_packages = target_entry
			.packages
			.iter()
			.filter(|package| package.starts_with(&package_prefix))
			.collect::<Vec<_>>();
		if matching_packages.is_empty() {
			return LinkPermission::Unknown;
		}
		let Some(source_entry) = source_entry else {
			return LinkPermission::Blocked;
		};
		if source_entry.manifest != target_manifest {
			return LinkPermission::Blocked;
		}
		if matching_packages
			.iter()
			.any(|package| source_entry.deps.contains(*package))
		{
			LinkPermission::Allowed
		} else {
			LinkPermission::Blocked
		}
	}

	fn source_builtin_external_root(&self, query: &LinkageQuery<'_>, import_root: &str) -> bool {
		query
			.material
			.files
			.get(query.source_file)
			.is_some_and(|file| language::builtin_external_root(file.lang, import_root))
	}

	fn index_files(&mut self, material: &CodeIndexMaterial) {
		for (file_idx, file) in material.files.iter().enumerate() {
			let file_path = absolute_path(&file.path);
			let Some(entries) = self.entries_by_root.get(&file.source_root) else {
				continue;
			};
			let Some((entry_idx, _)) = entries
				.iter()
				.enumerate()
				.filter(|(_, entry)| {
					entry
						.path
						.parent()
						.is_some_and(|dir| file_path.starts_with(dir))
				})
				.max_by_key(|(_, entry)| entry.path.components().count())
			else {
				continue;
			};
			self.entry_by_file.insert(
				file_idx,
				ManifestEntryLocation {
					source_root: file.source_root,
					entry: entry_idx,
				},
			);
		}
	}

	fn workspace_declares_package(&self, package: &str) -> bool {
		self.entries_by_root
			.values()
			.any(|entries| entries.iter().any(|entry| entry.packages.contains(package)))
	}
}

fn source_declares_language_package_target(
	policy: &ManifestPolicy,
	query: &LinkageQuery<'_>,
) -> bool {
	let Some(source_lang) = query
		.material
		.files
		.get(query.source_file)
		.map(|file| file.lang)
	else {
		return false;
	};
	let Some(package_prefix) = language::package_prefix_for_target(source_lang, query.target)
	else {
		return false;
	};
	policy
		.entry_for_file(query.source_file)
		.is_some_and(|entry| {
			language::source_declares_external_package(
				source_lang,
				entry.manifest,
				&entry.deps,
				&package_prefix,
				query.confidence,
				|package| policy.workspace_declares_package(package),
			)
		})
}

fn source_root_declares_dependency(
	policy: &ManifestPolicy,
	query: &LinkageQuery<'_>,
	package: &str,
) -> bool {
	let Some(location) = policy.entry_by_file.get(&query.source_file) else {
		return false;
	};
	let Some(root) = query
		.material
		.source_catalog
		.sources
		.roots
		.get(location.source_root)
	else {
		return false;
	};
	let root_path = absolute_path(&root.path);
	policy
		.entries_by_root
		.get(&location.source_root)
		.into_iter()
		.flatten()
		.any(|entry| {
			entry
				.path
				.parent()
				.is_some_and(|parent| parent == root_path)
				&& entry.deps.contains(package)
		})
}

fn manifest_entries_for_root(root: &SourceRoot) -> Vec<ManifestEntry> {
	manifest_candidates(&root.path)
		.into_iter()
		.filter_map(|path| manifest_entry(root, &path))
		.collect()
}

fn manifest_entry(root: &SourceRoot, manifest_path: &Path) -> Option<ManifestEntry> {
	let manifest = Manifest::for_filename(manifest_path)?;
	let content = std::fs::read_to_string(manifest_path).ok()?;
	let project = root.ctx.project.as_deref().unwrap_or(&root.label);
	let deps = parse_manifest(manifest, project.as_bytes(), &content).ok()?;
	let mut entry = ManifestEntry {
		path: absolute_path(manifest_path),
		manifest,
		packages: FxHashSet::default(),
		deps: FxHashSet::default(),
	};
	for dep in deps {
		if matches!(dep.dep_kind.as_str(), "package" | "workspace_member") {
			entry
				.packages
				.insert(package_id(manifest, &dep.import_root));
		} else {
			entry.deps.insert(package_id(manifest, &dep.import_root));
		}
	}
	Some(entry)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LinkPermission {
	Allowed,
	Blocked,
	Unknown,
}

#[derive(Default)]
pub(super) struct GlobalTargetPolicy {
	allowed: SymbolSet,
	blocked: bool,
	unknown: bool,
	external_dependency: bool,
}

impl GlobalTargetPolicy {
	pub(super) fn for_reference(
		self,
		reference_idx: usize,
		reference: &ReferenceRecord,
	) -> Option<ReferenceLinkageDecision> {
		if !self.allowed.is_empty() {
			return Some(ReferenceLinkageDecision::resolved(
				ResolutionScope::Global,
				reference_idx,
				reference.id.clone(),
				self.allowed,
			));
		}
		if self.blocked && !self.unknown {
			return Some(ReferenceLinkageDecision::manifest_blocked(
				reference_idx,
				reference.id.clone(),
			));
		}
		if self.external_dependency {
			return Some(ReferenceLinkageDecision::external(
				crate::linkage::decision::ExternalOrigin::Dependency,
				reference_idx,
				reference.id.clone(),
			));
		}
		None
	}
}

fn package_id(manifest: Manifest, import_root: &str) -> String {
	format!("{}\0{import_root}", manifest.tag())
}

fn package_id_prefix(manifest: Manifest) -> String {
	format!("{}\0", manifest.tag())
}

fn manifest_candidates(root: &Path) -> Vec<PathBuf> {
	let mut out = ignore::WalkBuilder::new(root)
		.build()
		.filter_map(|entry| entry.ok())
		.filter(|entry| {
			entry
				.file_type()
				.is_some_and(|file_type| file_type.is_file())
		})
		.map(|entry| entry.into_path())
		.filter(|path| Manifest::for_filename(path).is_some())
		.collect::<Vec<_>>();
	out.sort();
	out
}

fn absolute_path(path: &Path) -> PathBuf {
	let path = if path.is_absolute() {
		path.to_path_buf()
	} else {
		std::env::current_dir()
			.map(|cwd| cwd.join(path))
			.unwrap_or_else(|_| path.to_path_buf())
	};
	normalize_path(&path)
}

fn normalize_path(path: &Path) -> PathBuf {
	let mut out = PathBuf::new();
	for component in path.components() {
		match component {
			std::path::Component::CurDir => {}
			std::path::Component::ParentDir => {
				out.pop();
			}
			_ => out.push(component.as_os_str()),
		}
	}
	out
}

fn external_import_root<'a>(query: &'a LinkageQuery<'_>) -> Option<&'a str> {
	let head = query.target.as_view().segments().next()?;
	if head.kind != code_moniker_core::lang::kinds::EXTERNAL_PKG {
		return None;
	}
	std::str::from_utf8(head.name).ok()
}
