use std::path::{Path, PathBuf};

use code_moniker_core::lang::{
	Lang,
	build_manifest::{Manifest, parse as parse_manifest},
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::workspace::linkage::decision::{ReferenceLinkageDecision, ResolutionScope};
use crate::workspace::linkage::query::LinkageQuery;
use crate::workspace::snapshot::{ReferenceRecord, SymbolId};
use crate::workspace::source::CodeIndexMaterial;

#[derive(Default)]
pub(super) struct ManifestPolicy {
	entries_by_root: FxHashMap<usize, Vec<ManifestEntry>>,
}

#[derive(Clone, Debug)]
struct ManifestEntry {
	path: PathBuf,
	manifest: Manifest,
	packages: FxHashSet<String>,
	deps: FxHashSet<String>,
}

impl ManifestPolicy {
	pub(super) fn build(material: &CodeIndexMaterial) -> Self {
		let mut policy = Self::default();
		for (root_idx, root) in material.source_catalog.sources.roots.iter().enumerate() {
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
				let mut entry = ManifestEntry {
					path: manifest_path,
					manifest,
					packages: FxHashSet::default(),
					deps: FxHashSet::default(),
				};
				for dep in deps {
					if dep.dep_kind == "package" {
						entry
							.packages
							.insert(package_id(manifest, &dep.import_root));
					} else {
						entry.deps.insert(package_id(manifest, &dep.import_root));
					}
				}
				policy
					.entries_by_root
					.entry(root_idx)
					.or_default()
					.push(entry);
			}
		}
		policy
	}

	pub(super) fn evaluate_global_targets(
		&self,
		query: &LinkageQuery<'_>,
		candidates: Vec<SymbolId>,
	) -> GlobalTargetPolicy {
		let mut policy = GlobalTargetPolicy::default();
		for symbol in candidates {
			let Some((target_file, _)) = query.material.identity.symbol_location(&symbol) else {
				continue;
			};
			match self.source_can_link_to_file(query.material, query.source_file, target_file) {
				LinkPermission::Allowed => policy.allowed.push(symbol),
				LinkPermission::Blocked => policy.blocked = true,
				LinkPermission::Unknown => policy.unknown = true,
			}
		}
		policy
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
		let source_entry = self.entry_for_file(material, source_file);
		let target_entry = self.entry_for_file(material, target_file);
		if source_entry.map(|entry| &entry.path) == target_entry.map(|entry| &entry.path) {
			return LinkPermission::Allowed;
		}
		let Some(target_lang) = material.files.get(target_file).map(|file| file.lang) else {
			return LinkPermission::Unknown;
		};
		let Some(target_manifest) = manifest_for_lang(target_lang) else {
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

	fn entry_for_file<'a>(
		&'a self,
		material: &'a CodeIndexMaterial,
		file_idx: usize,
	) -> Option<&'a ManifestEntry> {
		let file = material.files.get(file_idx)?;
		let entries = self.entries_by_root.get(&file.source_root)?;
		entries
			.iter()
			.filter(|entry| {
				entry
					.path
					.parent()
					.is_some_and(|dir| file.path.starts_with(dir))
			})
			.max_by_key(|entry| entry.path.components().count())
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LinkPermission {
	Allowed,
	Blocked,
	Unknown,
}

#[derive(Default)]
pub(super) struct GlobalTargetPolicy {
	allowed: Vec<SymbolId>,
	blocked: bool,
	unknown: bool,
}

impl GlobalTargetPolicy {
	pub(super) fn for_reference(
		self,
		reference: &ReferenceRecord,
	) -> Option<ReferenceLinkageDecision> {
		if !self.allowed.is_empty() {
			return Some(ReferenceLinkageDecision::resolved(
				ResolutionScope::Global,
				reference,
				self.allowed,
			));
		}
		if self.blocked && !self.unknown {
			return Some(ReferenceLinkageDecision::manifest_blocked(reference));
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

fn manifest_candidates(root: &Path) -> Vec<PathBuf> {
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
	out
}
