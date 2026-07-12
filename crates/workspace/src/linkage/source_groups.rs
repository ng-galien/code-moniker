use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;
use serde::Deserialize;

use crate::source::CodeIndexMaterial;

const CONFIG_FILE: &str = ".code-moniker.toml";

// The verdict a linkage policy renders for a (source file, target file) pair.
// Declared source groups and manifest detection both speak this language;
// declared groups are consulted first and are authoritative for any pair they
// cover, manifest detection only decides the pairs they stay silent on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::linkage) enum LinkPermission {
	Allowed,
	Blocked,
	Unknown,
}

// Declared connectivity is a workspace-wide collaborator, kept separate from
// ManifestPolicy so manifest detection stays focused on parsing what build
// systems declare, not on resolving what the user declared instead.
#[derive(Default)]
pub(in crate::linkage) struct SourceGroupPolicy {
	by_root: FxHashMap<usize, DeclaredSourceGroups>,
}

impl SourceGroupPolicy {
	pub(in crate::linkage) fn build(material: &CodeIndexMaterial) -> Self {
		let mut by_root = FxHashMap::default();
		for (root_idx, root) in material.source_catalog.sources.roots.iter().enumerate() {
			if let Some(groups) = DeclaredSourceGroups::load(&root.path) {
				by_root.insert(root_idx, groups);
			}
		}
		Self { by_root }
	}

	pub(in crate::linkage) fn link_permission(
		&self,
		material: &CodeIndexMaterial,
		source_file: usize,
		target_file: usize,
	) -> Option<LinkPermission> {
		let source = self.group_of(material, source_file);
		let target = self.group_of(material, target_file);
		match (source, target) {
			(None, None) => None,
			(source, target) if source == target => Some(LinkPermission::Allowed),
			_ => Some(LinkPermission::Blocked),
		}
	}

	fn group_of(&self, material: &CodeIndexMaterial, file_idx: usize) -> Option<(usize, usize)> {
		let file = material.files.get(file_idx)?;
		let groups = self.by_root.get(&file.source_root)?;
		Some((file.source_root, groups.group_for(&file.path)?))
	}
}

// Declared connectivity takes priority over manifest detection: when a build
// system is too complex to parse declaratively (Gradle, Bazel, …), the user
// states which source directories see each other directly, instead of the
// linkage layer guessing.
pub(in crate::linkage) struct DeclaredSourceGroups {
	groups: Vec<Vec<PathBuf>>,
}

impl DeclaredSourceGroups {
	pub(in crate::linkage) fn load(workspace_root: &Path) -> Option<Self> {
		let config_path = workspace_root.join(CONFIG_FILE);
		let text = std::fs::read_to_string(&config_path).ok()?;
		let file: ConfigFile = toml::from_str(&text).ok()?;
		if file.workspace.source_group.is_empty() {
			return None;
		}
		let groups = file
			.workspace
			.source_group
			.into_iter()
			.map(|group| {
				group
					.roots
					.into_iter()
					.map(|root| workspace_root.join(root))
					.collect::<Vec<_>>()
			})
			.collect::<Vec<_>>();
		Some(Self { groups })
	}

	pub(in crate::linkage) fn group_for(&self, file_path: &Path) -> Option<usize> {
		self.groups
			.iter()
			.position(|group| group.iter().any(|root| file_path.starts_with(root)))
	}
}

#[derive(Debug, Default, Deserialize)]
struct ConfigFile {
	#[serde(default)]
	workspace: WorkspaceSection,
}

#[derive(Debug, Default, Deserialize)]
struct WorkspaceSection {
	#[serde(default, rename = "source_group")]
	source_group: Vec<SourceGroupEntry>,
}

#[derive(Debug, Deserialize)]
struct SourceGroupEntry {
	roots: Vec<String>,
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn declared_groups_take_priority_over_absence_of_config() {
		let dir = tempfile::tempdir().expect("tempdir");
		std::fs::write(
			dir.path().join(CONFIG_FILE),
			r#"
[[workspace.source_group]]
roots = ["module-a", "module-b"]

[[workspace.source_group]]
roots = ["module-c"]
"#,
		)
		.expect("write config");

		let groups = DeclaredSourceGroups::load(dir.path()).expect("groups load");
		let a_file = dir.path().join("module-a/src/main/java/A.java");
		let b_file = dir.path().join("module-b/src/main/java/B.java");
		let c_file = dir.path().join("module-c/src/main/java/C.java");
		let unrelated = dir.path().join("module-z/src/main/java/Z.java");

		assert_eq!(groups.group_for(&a_file), Some(0));
		assert_eq!(groups.group_for(&b_file), Some(0));
		assert_eq!(groups.group_for(&c_file), Some(1));
		assert_eq!(groups.group_for(&unrelated), None);
	}

	#[test]
	fn missing_config_yields_no_groups() {
		let dir = tempfile::tempdir().expect("tempdir");
		assert!(DeclaredSourceGroups::load(dir.path()).is_none());
	}

	#[test]
	fn config_without_source_group_yields_no_groups() {
		let dir = tempfile::tempdir().expect("tempdir");
		std::fs::write(dir.path().join(CONFIG_FILE), "default_rules = false\n")
			.expect("write config");
		assert!(DeclaredSourceGroups::load(dir.path()).is_none());
	}
}
