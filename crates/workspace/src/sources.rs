use std::collections::{BTreeMap, HashSet};
use std::path::Component;
use std::path::{Path, PathBuf};

use crate::extract;
use crate::lang::path_to_lang;
use crate::tsconfig::{self, TsResolution};
use crate::walk::{self, WalkedFile};

#[derive(Clone, Debug)]
pub struct SourceSet {
	pub roots: Vec<SourceRoot>,
	pub files: Vec<SourceFile>,
	pub multi: bool,
}

#[derive(Clone, Debug)]
pub struct SourceRoot {
	pub input: PathBuf,
	pub path: PathBuf,
	pub label: String,
	pub ctx: extract::Context,
}

#[derive(Clone, Debug)]
pub struct SourceFile {
	pub source: usize,
	pub path: PathBuf,
	pub rel_path: PathBuf,
	pub anchor: PathBuf,
	pub lang: code_moniker_core::lang::Lang,
}

struct SourceScope {
	source: usize,
	root_is_dir: bool,
	root: SourceRoot,
}

impl SourceSet {
	#[allow(dead_code)]
	pub fn display_path(&self) -> String {
		if self.multi {
			self.roots
				.iter()
				.map(|source| source.input.display().to_string())
				.collect::<Vec<_>>()
				.join(", ")
		} else {
			self.roots
				.first()
				.map(|source| source.input.display().to_string())
				.unwrap_or_else(|| "<empty>".to_string())
		}
	}
}

pub fn discover(paths: &[PathBuf], project: Option<String>) -> anyhow::Result<SourceSet> {
	let scopes = discover_scopes(paths, project)?;
	let multi = scopes.len() > 1;
	let mut files = Vec::new();
	for scope in &scopes {
		let walked = if scope.root_is_dir {
			walk::walk_lang_files(&scope.root.input)
		} else {
			let lang = path_to_lang(&scope.root.input)?;
			vec![WalkedFile {
				path: scope.root.input.clone(),
				lang,
			}]
		};
		for walked in walked {
			files.push(source_file_from_walked(scope, walked, multi));
		}
	}
	files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
	Ok(SourceSet {
		roots: scopes.into_iter().map(|scope| scope.root).collect(),
		files,
		multi,
	})
}

pub fn discover_files(
	root: &Path,
	files: &[PathBuf],
	project: Option<String>,
) -> anyhow::Result<SourceSet> {
	let meta = std::fs::metadata(root)
		.map_err(|e| anyhow::anyhow!("cannot stat {}: {e}", root.display()))?;
	if !meta.is_dir() {
		return Err(anyhow::anyhow!(
			"--file requires a directory check path, got {}",
			root.display()
		));
	}
	let scopes = discover_scopes(&[root.to_path_buf()], project)?;
	let scope = scopes
		.first()
		.expect("discover_scopes returns one scope for one root");
	let abs_root = normalize_absolute(&scope.root.path)?;
	let mut source_files = Vec::new();
	let mut seen = HashSet::new();
	for file in files {
		for path in filter_file_candidates(&scope.root.path, file) {
			let abs_path = normalize_absolute(&path)?;
			if !abs_path.starts_with(&abs_root) {
				continue;
			}
			if seen.contains(&abs_path) {
				break;
			}
			if explicit_file_is_ignored(&abs_root, &abs_path) {
				continue;
			}
			let Some(walked) = walk::explicit_lang_file(&path) else {
				continue;
			};
			seen.insert(abs_path);
			source_files.push(source_file_from_walked(scope, walked, false));
			break;
		}
	}
	source_files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
	Ok(SourceSet {
		roots: scopes.into_iter().map(|scope| scope.root).collect(),
		files: source_files,
		multi: false,
	})
}

fn discover_scopes(paths: &[PathBuf], project: Option<String>) -> anyhow::Result<Vec<SourceScope>> {
	if paths.is_empty() {
		return Err(anyhow::anyhow!("at least one source path is required"));
	}
	let multi = paths.len() > 1;
	let labels = unique_labels(paths);
	let mut scopes = Vec::with_capacity(paths.len());
	for (source_idx, path) in paths.iter().enumerate() {
		let meta = std::fs::metadata(path)
			.map_err(|e| anyhow::anyhow!("cannot stat {}: {e}", path.display()))?;
		let root_is_dir = meta.is_dir();
		let root = if root_is_dir {
			path.clone()
		} else {
			path.parent()
				.unwrap_or_else(|| Path::new("."))
				.to_path_buf()
		};
		let label = labels[source_idx].clone();
		let source_project = project.clone().or_else(|| multi.then(|| label.clone()));
		let mut ts = tsconfig::load(&root);
		if multi {
			prefix_ts_aliases(&mut ts, &label);
		}
		scopes.push(SourceScope {
			source: source_idx,
			root_is_dir,
			root: SourceRoot {
				input: path.clone(),
				path: root,
				label,
				ctx: extract::Context {
					ts,
					project: source_project,
				},
			},
		});
	}
	Ok(scopes)
}

fn source_file_from_walked(scope: &SourceScope, walked: WalkedFile, multi: bool) -> SourceFile {
	let root = normalize_absolute(&scope.root.path).unwrap_or_else(|_| scope.root.path.clone());
	let path = normalize_absolute(&walked.path).unwrap_or_else(|_| walked.path.clone());
	let rel = path.strip_prefix(&root).unwrap_or(&path).to_path_buf();
	let rel_path = if multi {
		PathBuf::from(&scope.root.label).join(&rel)
	} else {
		rel.clone()
	};
	let anchor = if multi {
		rel_path.clone()
	} else if scope.root_is_dir {
		anchor_with_source_context(&root, &rel)
	} else {
		walked.path.clone()
	};
	SourceFile {
		source: scope.source,
		path: walked.path,
		rel_path,
		anchor,
		lang: walked.lang,
	}
}

fn normalize_absolute(path: &Path) -> anyhow::Result<PathBuf> {
	let path = if path.is_absolute() {
		path.to_path_buf()
	} else {
		std::env::current_dir()?.join(path)
	};
	let mut out = PathBuf::new();
	for component in path.components() {
		match component {
			Component::CurDir => {}
			Component::ParentDir => {
				out.pop();
			}
			Component::Prefix(prefix) => out.push(prefix.as_os_str()),
			Component::RootDir => out.push(component.as_os_str()),
			Component::Normal(part) => out.push(part),
		}
	}
	Ok(out)
}

fn explicit_file_is_ignored(abs_root: &Path, abs_path: &Path) -> bool {
	let Some(parent) = abs_path.parent() else {
		return false;
	};
	let ignore_root = nearest_git_root(abs_root).unwrap_or_else(|| abs_root.to_path_buf());
	let mut ignored = false;
	for dir in dirs_between(&ignore_root, parent) {
		for name in [".ignore", ".gitignore"] {
			if name == ".gitignore" && nearest_git_root(&dir).is_none() {
				continue;
			}
			let file = dir.join(name);
			if !file.is_file() {
				continue;
			}
			let mut builder = ignore::gitignore::GitignoreBuilder::new(&dir);
			let _ = builder.add(&file);
			let Ok(matcher) = builder.build() else {
				continue;
			};
			let matched = matcher.matched_path_or_any_parents(abs_path, false);
			if matched.is_ignore() {
				ignored = true;
			} else if matched.is_whitelist() {
				ignored = false;
			}
		}
	}
	ignored
}

fn nearest_git_root(path: &Path) -> Option<PathBuf> {
	path.ancestors()
		.find(|ancestor| ancestor.join(".git").exists())
		.map(Path::to_path_buf)
}

fn dirs_between(root: &Path, leaf: &Path) -> Vec<PathBuf> {
	let mut dirs = Vec::new();
	let mut current = Some(leaf);
	while let Some(dir) = current {
		dirs.push(dir.to_path_buf());
		if dir == root {
			break;
		}
		current = dir.parent();
	}
	dirs.reverse();
	dirs
}

fn filter_file_candidates(root: &Path, file: &Path) -> Vec<PathBuf> {
	let mut candidates = Vec::new();
	if file.is_absolute() {
		candidates.push(file.to_path_buf());
		return candidates;
	}
	push_unique_path(&mut candidates, file.to_path_buf());
	if let Some(parent) = root.parent() {
		if file_starts_with_root_name(root, file) {
			push_unique_path(&mut candidates, parent.join(file));
		}
	}
	push_unique_path(&mut candidates, root.join(file));
	if let Some(parent) = root.parent() {
		push_unique_path(&mut candidates, parent.join(file));
	}
	candidates
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
	if !paths.iter().any(|existing| existing == &path) {
		paths.push(path);
	}
}

fn file_starts_with_root_name(root: &Path, file: &Path) -> bool {
	let Some(root_name) = root.file_name() else {
		return false;
	};
	file.components()
		.next()
		.is_some_and(|component| component.as_os_str() == root_name)
}

fn anchor_with_source_context(root: &Path, rel: &Path) -> PathBuf {
	if path_has_source_set(rel) {
		return rel.to_path_buf();
	}
	source_set_suffix_from_scope(root, rel).unwrap_or_else(|| rel.to_path_buf())
}

fn source_set_suffix_from_scope(root: &Path, rel: &Path) -> Option<PathBuf> {
	let root_parts: Vec<_> = root.components().collect();
	let rel_parts: Vec<_> = rel.components().collect();
	let rel_first = rel_parts
		.first()
		.and_then(|component| component.as_os_str().to_str());
	for idx in (0..root_parts.len()).rev() {
		let name = root_parts[idx].as_os_str().to_str()?;
		if name != "src" {
			continue;
		}
		if let Some(next) = root_parts
			.get(idx + 1)
			.and_then(|component| component.as_os_str().to_str())
		{
			if matches!(next, "main" | "test" | "tests") {
				return Some(root_parts[idx..].iter().chain(rel_parts.iter()).collect());
			}
		} else if rel_first.is_some_and(|first| matches!(first, "main" | "test" | "tests")) {
			return Some(root_parts[idx..].iter().chain(rel_parts.iter()).collect());
		}
	}
	None
}

fn path_has_source_set(path: &Path) -> bool {
	path.components()
		.filter_map(|component| component.as_os_str().to_str())
		.collect::<Vec<_>>()
		.windows(2)
		.any(|window| matches!(window, ["src", "main" | "test" | "tests"]))
}

fn unique_labels(paths: &[PathBuf]) -> Vec<String> {
	let base: Vec<String> = paths
		.iter()
		.enumerate()
		.map(|(idx, path)| {
			path.file_stem()
				.or_else(|| path.file_name())
				.and_then(|name| name.to_str())
				.filter(|name| !name.is_empty())
				.map(ToOwned::to_owned)
				.unwrap_or_else(|| format!("source{}", idx + 1))
		})
		.collect();
	let mut seen = BTreeMap::<String, usize>::new();
	base.into_iter()
		.map(|label| {
			let count = seen.entry(label.clone()).or_default();
			*count += 1;
			if *count == 1 {
				label
			} else {
				format!("{label}-{}", *count)
			}
		})
		.collect()
}

fn prefix_ts_aliases(ts: &mut TsResolution, label: &str) {
	for alias in &mut ts.aliases {
		alias.substitution = prefix_project_rooted_substitution(&alias.substitution, label);
	}
}

fn prefix_project_rooted_substitution(substitution: &str, label: &str) -> String {
	let rest = substitution.strip_prefix("./").unwrap_or(substitution);
	format!("./{label}/{rest}")
}

#[cfg(test)]
mod tests {
	use super::*;

	fn write(root: &Path, rel: &str, body: &str) {
		let p = root.join(rel);
		if let Some(parent) = p.parent() {
			std::fs::create_dir_all(parent).unwrap();
		}
		std::fs::write(p, body).unwrap();
	}

	#[test]
	fn discovers_multiple_roots_with_labels_and_prefixed_anchors() {
		let tmp = tempfile::tempdir().unwrap();
		let service_a = tmp.path().join("service-a");
		let service_b = tmp.path().join("service-b");
		write(&service_a, "src/A.java", "class A {}\n");
		write(&service_b, "src/B.java", "class B {}\n");

		let set = discover(&[service_a.clone(), service_b.clone()], None).unwrap();

		assert!(set.multi);
		assert_eq!(set.roots[0].label, "service-a");
		assert_eq!(set.roots[0].ctx.project.as_deref(), Some("service-a"));
		assert_eq!(set.roots[1].ctx.project.as_deref(), Some("service-b"));
		assert!(set.display_path().contains("service-a"));
		assert!(set.display_path().contains("service-b"));
		assert!(
			set.files
				.iter()
				.any(|file| file.rel_path.as_path() == Path::new("service-a/src/A.java"))
		);
		assert!(
			set.files
				.iter()
				.any(|file| file.anchor.as_path() == Path::new("service-b/src/B.java"))
		);
	}

	#[test]
	fn keeps_single_root_paths_compatible() {
		let tmp = tempfile::tempdir().unwrap();
		write(tmp.path(), "src/A.java", "class A {}\n");

		let set = discover(&[tmp.path().to_path_buf()], None).unwrap();

		assert!(!set.multi);
		assert_eq!(set.roots[0].ctx.project, None);
		assert_eq!(set.display_path(), tmp.path().display().to_string());
		assert_eq!(set.files[0].rel_path, PathBuf::from("src/A.java"));
		assert_eq!(set.files[0].anchor, PathBuf::from("src/A.java"));
	}

	#[test]
	fn prefixes_ts_path_aliases_in_multi_source_mode() {
		let tmp = tempfile::tempdir().unwrap();
		let service_a = tmp.path().join("service-a");
		let service_b = tmp.path().join("service-b");
		write(
			&service_a,
			"tsconfig.json",
			r#"{"compilerOptions": {"paths": {"@/*": ["./src/*"]}}}"#,
		);
		write(&service_a, "src/A.ts", "export class A {}\n");
		write(&service_b, "src/B.ts", "export class B {}\n");

		let set = discover(&[service_a, service_b], None).unwrap();

		assert!(
			set.roots[0]
				.ctx
				.ts
				.aliases
				.iter()
				.any(|alias| alias.pattern == "@/*" && alias.substitution == "./service-a/src/*"),
			"{:?}",
			set.roots[0].ctx.ts.aliases,
		);
	}

	#[test]
	fn keeps_single_file_display_path_compatible() {
		let tmp = tempfile::tempdir().unwrap();
		write(tmp.path(), "A.java", "class A {}\n");
		let path = tmp.path().join("A.java");

		let set = discover(std::slice::from_ref(&path), None).unwrap();

		assert!(!set.multi);
		assert_eq!(set.display_path(), path.display().to_string());
		assert_eq!(set.files[0].rel_path, PathBuf::from("A.java"));
		assert_eq!(set.files[0].anchor, path);
	}

	#[test]
	fn source_set_context_uses_scope_suffix_not_parent_directories() {
		let tmp = tempfile::tempdir().unwrap();
		let root = tmp.path().join("outer/src/test/project/src");
		write(
			&root,
			"main/java/com/acme/Foo.java",
			"package com.acme;\nclass Foo {}\n",
		);

		let set = discover_files(
			&root,
			&[PathBuf::from("src/main/java/com/acme/Foo.java")],
			None,
		)
		.unwrap();

		assert_eq!(set.files.len(), 1);
		assert_eq!(
			set.files[0].anchor,
			PathBuf::from("src/main/java/com/acme/Foo.java")
		);
	}

	#[test]
	fn filter_candidates_try_project_relative_scope_prefixed_paths_before_scope_join() {
		let tmp = tempfile::tempdir().unwrap();
		let root = tmp.path().join("project/src");
		write(&root, "order.ts", "class Bad {}\n");
		write(&root, "src/order.ts", "class Duplicate {}\n");

		let candidates = filter_file_candidates(&root, Path::new("src/order.ts"));

		assert_eq!(candidates[0], PathBuf::from("src/order.ts"));
		assert_eq!(candidates[1], tmp.path().join("project/src/order.ts"));
		assert_eq!(candidates[2], tmp.path().join("project/src/src/order.ts"));
	}
}
