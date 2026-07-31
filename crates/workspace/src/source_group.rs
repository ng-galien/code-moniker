use std::path::{Component, Path, PathBuf};

use anyhow::Context as _;
use serde::Deserialize;

const CONFIG_FILE: &str = ".code-moniker.toml";

/// One connected workspace source space. String roots preserve the original
/// connectivity-only declaration. Mapped roots additionally assign the
/// existing `srcset` identity to non-standard source layouts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceGroupConfig {
	pub roots: Vec<SourceGroupRootConfig>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(untagged)]
pub enum SourceGroupRootConfig {
	Path(String),
	Mapped(SourceGroupMappedRootConfig),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceGroupMappedRootConfig {
	pub path: String,
	pub srcset: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SourceGroupMembership<'a> {
	pub group: usize,
	pub srcset: Option<&'a str>,
}

#[doc(hidden)]
#[derive(Clone, Debug, Default)]
pub struct DeclaredSourceGroups {
	roots: Vec<DeclaredSourceRoot>,
}

#[derive(Clone, Debug)]
struct DeclaredSourceRoot {
	group: usize,
	path: PathBuf,
	alternate_path: Option<PathBuf>,
	srcset: Option<String>,
}

#[derive(Clone, Debug)]
struct SourceGroupWorkspace {
	lexical_root: PathBuf,
	absolute_root: PathBuf,
}

impl DeclaredSourceGroups {
	pub(crate) fn load(workspace_root: &Path) -> anyhow::Result<Self> {
		let config_path = workspace_root.join(CONFIG_FILE);
		let text = match std::fs::read_to_string(&config_path) {
			Ok(text) => text,
			Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
				return Ok(Self::default());
			}
			Err(error) => {
				return Err(error)
					.with_context(|| format!("cannot read {}", config_path.display()));
			}
		};
		let file: ConfigFile = toml::from_str(&text)
			.with_context(|| format!("invalid source groups in {}", config_path.display()))?;
		Self::from_config(workspace_root, file.workspace.source_groups)
			.with_context(|| format!("invalid source groups in {}", config_path.display()))
	}

	fn from_config(workspace_root: &Path, groups: Vec<SourceGroupConfig>) -> anyhow::Result<Self> {
		let workspace = SourceGroupWorkspace::new(workspace_root);
		let mut roots = Vec::new();
		for (group, entry) in groups.into_iter().enumerate() {
			roots.extend(entry.into_declared_roots(group, &workspace)?);
		}
		validate_unique_roots(&roots)?;
		validate_group_boundaries(&roots)?;
		roots.sort_by_key(|root| std::cmp::Reverse(root.path.components().count()));
		Ok(Self { roots })
	}

	pub(crate) fn membership(&self, file_path: &Path) -> Option<SourceGroupMembership<'_>> {
		self.roots
			.iter()
			.find(|root| {
				file_path.starts_with(&root.path)
					|| root
						.alternate_path
						.as_ref()
						.is_some_and(|alternate| file_path.starts_with(alternate))
			})
			.map(|root| SourceGroupMembership {
				group: root.group,
				srcset: root.srcset.as_deref(),
			})
	}
}

impl SourceGroupConfig {
	fn into_declared_roots(
		self,
		group: usize,
		workspace: &SourceGroupWorkspace,
	) -> anyhow::Result<Vec<DeclaredSourceRoot>> {
		if self.roots.is_empty() {
			anyhow::bail!("workspace.source_group[{group}].roots must not be empty");
		}
		self.roots
			.into_iter()
			.map(|root| root.into_declared_root(group, workspace))
			.collect()
	}
}

impl SourceGroupRootConfig {
	fn into_declared_root(
		self,
		group: usize,
		workspace: &SourceGroupWorkspace,
	) -> anyhow::Result<DeclaredSourceRoot> {
		let (path, srcset) = match self {
			Self::Path(path) => (path, None),
			Self::Mapped(mapped) => mapped.into_path_and_srcset(group)?,
		};
		let (path, alternate_path) = workspace.resolve(group, &path)?;
		Ok(DeclaredSourceRoot {
			group,
			path,
			alternate_path,
			srcset,
		})
	}
}

impl SourceGroupMappedRootConfig {
	fn into_path_and_srcset(self, group: usize) -> anyhow::Result<(String, Option<String>)> {
		if self.srcset.trim().is_empty() {
			anyhow::bail!("workspace.source_group[{group}] contains an empty srcset");
		}
		Ok((self.path, Some(self.srcset)))
	}
}

impl SourceGroupWorkspace {
	fn new(workspace_root: &Path) -> Self {
		Self {
			lexical_root: lexical_absolute(workspace_root),
			absolute_root: crate::path_util::absolute_path(workspace_root),
		}
	}

	fn resolve(&self, group: usize, relative: &str) -> anyhow::Result<(PathBuf, Option<PathBuf>)> {
		validate_relative_root(group, relative)?;
		let lexical_path = crate::path_util::lexical_path(&self.lexical_root.join(relative));
		let path = crate::path_util::absolute_path(&self.absolute_root.join(relative));
		if !path.starts_with(&self.absolute_root) {
			anyhow::bail!(
				"workspace.source_group[{group}] root {} escapes workspace root {}",
				path.display(),
				self.absolute_root.display()
			);
		}
		let alternate_path = (lexical_path != path).then_some(lexical_path);
		Ok((path, alternate_path))
	}
}

fn lexical_absolute(path: &Path) -> PathBuf {
	if path.is_absolute() {
		return crate::path_util::lexical_path(path);
	}
	std::env::current_dir()
		.map(|current| crate::path_util::lexical_path(&current.join(path)))
		.unwrap_or_else(|_| crate::path_util::lexical_path(path))
}

fn validate_relative_root(group: usize, path: &str) -> anyhow::Result<()> {
	if path.trim().is_empty() {
		anyhow::bail!("workspace.source_group[{group}] contains an empty root");
	}
	let path = Path::new(path);
	if path.is_absolute()
		|| path
			.components()
			.any(|component| matches!(component, Component::ParentDir | Component::RootDir))
	{
		anyhow::bail!(
			"workspace.source_group[{group}] root {} must stay relative to the workspace",
			path.display()
		);
	}
	Ok(())
}

fn validate_unique_roots(roots: &[DeclaredSourceRoot]) -> anyhow::Result<()> {
	for (idx, root) in roots.iter().enumerate() {
		if let Some(existing) = roots[..idx]
			.iter()
			.find(|existing| existing.path == root.path)
		{
			anyhow::bail!(
				"source root {} is declared more than once (groups {} and {})",
				root.path.display(),
				existing.group,
				root.group
			);
		}
	}
	Ok(())
}

fn validate_group_boundaries(roots: &[DeclaredSourceRoot]) -> anyhow::Result<()> {
	for (idx, left) in roots.iter().enumerate() {
		for right in &roots[idx + 1..] {
			if left.group != right.group
				&& (left.path.starts_with(&right.path) || right.path.starts_with(&left.path))
			{
				anyhow::bail!(
					"source roots {} (group {}) and {} (group {}) overlap",
					left.path.display(),
					left.group,
					right.path.display(),
					right.group
				);
			}
		}
	}
	Ok(())
}

#[derive(Debug, Default, Deserialize)]
struct ConfigFile {
	#[serde(default)]
	workspace: WorkspaceSection,
}

#[derive(Debug, Default, Deserialize)]
struct WorkspaceSection {
	#[serde(default, rename = "source_group")]
	source_groups: Vec<SourceGroupConfig>,
}

#[cfg(test)]
mod tests {
	use super::*;

	fn write_config(root: &Path, text: &str) {
		std::fs::write(root.join(CONFIG_FILE), text).expect("write config");
	}

	#[test]
	fn legacy_roots_keep_connectivity_without_assigning_srcset() {
		let dir = tempfile::tempdir().expect("tempdir");
		write_config(
			dir.path(),
			r#"
[[workspace.source_group]]
roots = ["module-a", "module-b"]

[[workspace.source_group]]
roots = ["module-c"]
"#,
		);

		let groups = DeclaredSourceGroups::load(dir.path()).expect("groups load");
		let a = groups
			.membership(&dir.path().join("module-a/src/main/java/A.java"))
			.expect("module-a membership");
		let b = groups
			.membership(&dir.path().join("module-b/src/test/java/B.java"))
			.expect("module-b membership");
		let c = groups
			.membership(&dir.path().join("module-c/src/main/java/C.java"))
			.expect("module-c membership");

		assert_eq!(
			a,
			SourceGroupMembership {
				group: 0,
				srcset: None
			}
		);
		assert_eq!(
			b,
			SourceGroupMembership {
				group: 0,
				srcset: None
			}
		);
		assert_eq!(
			c,
			SourceGroupMembership {
				group: 1,
				srcset: None
			}
		);
	}

	#[test]
	fn mapped_roots_assign_existing_srcset_identity_inside_one_group() {
		let dir = tempfile::tempdir().expect("tempdir");
		write_config(
			dir.path(),
			r#"
[[workspace.source_group]]
roots = [
  { path = "src/java", srcset = "main" },
  { path = "test", srcset = "test" },
]
"#,
		);

		let groups = DeclaredSourceGroups::load(dir.path()).expect("groups load");
		let production = groups
			.membership(&dir.path().join("src/java/org/acme/Service.java"))
			.expect("production membership");
		let test = groups
			.membership(&dir.path().join("test/unit/org/acme/ServiceTest.java"))
			.expect("test membership");

		assert_eq!(production.group, 0);
		assert_eq!(production.srcset, Some("main"));
		assert_eq!(test.group, 0);
		assert_eq!(test.srcset, Some("test"));
	}

	#[test]
	fn deepest_root_supplies_the_srcset_within_one_group() {
		let dir = tempfile::tempdir().expect("tempdir");
		write_config(
			dir.path(),
			r#"
[[workspace.source_group]]
roots = [
  { path = ".", srcset = "main" },
  { path = "test", srcset = "test" },
]
"#,
		);

		let groups = DeclaredSourceGroups::load(dir.path()).expect("groups load");
		assert_eq!(
			groups
				.membership(&dir.path().join("test/unit/ThingTest.java"))
				.and_then(|membership| membership.srcset),
			Some("test")
		);
	}

	#[test]
	fn overlapping_roots_across_groups_are_rejected() {
		let dir = tempfile::tempdir().expect("tempdir");
		write_config(
			dir.path(),
			r#"
[[workspace.source_group]]
roots = ["src"]

[[workspace.source_group]]
roots = ["src/generated"]
"#,
		);

		let error = DeclaredSourceGroups::load(dir.path()).expect_err("overlap must fail");
		assert!(error.to_string().contains("invalid source groups"));
		assert!(format!("{error:#}").contains("overlap"));
	}

	#[test]
	fn missing_config_yields_no_groups() {
		let dir = tempfile::tempdir().expect("tempdir");
		let groups = DeclaredSourceGroups::load(dir.path()).expect("missing config is valid");
		assert!(groups.membership(&dir.path().join("src/A.java")).is_none());
	}
}
