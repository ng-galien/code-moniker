use std::path::{Path, PathBuf};

use crate::lang::path_to_lang;
use code_moniker_core::lang::Lang;

pub struct WalkedFile {
	pub path: PathBuf,
	pub lang: Lang,
}

pub(crate) fn workspace_walk_builder(root: &Path) -> ignore::WalkBuilder {
	let mut overrides = ignore::overrides::OverrideBuilder::new(root);
	overrides
		.add("!**/.git/")
		.expect("the built-in Git metadata exclusion is valid");
	let mut builder = ignore::WalkBuilder::new(root);
	builder
		.parents(false)
		.ignore(false)
		.git_ignore(false)
		.git_exclude(false)
		.git_global(false)
		.add_custom_ignore_filename(".gitignore")
		.add_custom_ignore_filename(".ignore")
		.overrides(
			overrides
				.build()
				.expect("the built-in Git metadata exclusion compiles"),
		);
	builder
}

fn workspace_walk(root: &Path) -> ignore::Walk {
	workspace_walk_builder(root).build()
}

pub(crate) fn workspace_ignore_matcher(root: &Path) -> ignore::IncrementalIgnore {
	workspace_walk_builder(root)
		.build_matchers()
		.pop()
		.expect("one matcher for one workspace root")
}

pub fn walk_lang_files(root: &Path) -> Vec<WalkedFile> {
	walk_lang_files_cancellable(root, || false)
}

pub fn walk_lang_files_cancellable(root: &Path, cancelled: impl Fn() -> bool) -> Vec<WalkedFile> {
	workspace_walk(root)
		.take_while(|_| !cancelled())
		.filter_map(|entry| entry.ok())
		.filter(|e| e.file_type().is_some_and(|t| t.is_file()))
		.filter_map(|e| {
			let p = e.into_path();
			let lang = path_to_lang(&p).ok()?;
			Some(WalkedFile { path: p, lang })
		})
		.collect()
}

pub(crate) fn walk_non_ignored_directories(root: &Path) -> Vec<PathBuf> {
	workspace_walk(root)
		.filter_map(|entry| entry.ok())
		.filter(|entry| entry.file_type().is_some_and(|kind| kind.is_dir()))
		.map(|entry| entry.into_path())
		.collect()
}

pub fn explicit_lang_file(path: &Path) -> Option<WalkedFile> {
	workspace_walk(path)
		.filter_map(|entry| entry.ok())
		.filter(|e| e.file_type().is_some_and(|t| t.is_file()))
		.find_map(|e| {
			let p = e.into_path();
			let lang = path_to_lang(&p).ok()?;
			Some(WalkedFile { path: p, lang })
		})
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::collections::HashSet;
	use std::fs;

	fn write(root: &Path, rel: &str, body: &str) {
		let p = root.join(rel);
		if let Some(parent) = p.parent() {
			fs::create_dir_all(parent).unwrap();
		}
		fs::write(p, body).unwrap();
	}

	#[test]
	fn walks_supported_extensions_only() {
		let tmp = tempfile::tempdir().unwrap();
		let root = tmp.path();
		write(root, "a.ts", "");
		write(root, "b.rs", "");
		write(root, "c.txt", "ignored");
		write(root, "nested/d.py", "");
		let mut files: HashSet<(String, Lang)> = walk_lang_files(root)
			.into_iter()
			.map(|f| {
				let rel = f.path.strip_prefix(root).unwrap().to_string_lossy().into();
				(rel, f.lang)
			})
			.collect();
		assert!(files.remove(&("a.ts".into(), Lang::Ts)));
		assert!(files.remove(&("b.rs".into(), Lang::Rs)));
		assert!(files.remove(&("nested/d.py".into(), Lang::Python)));
		assert!(files.is_empty(), "unexpected files: {files:?}");
	}

	#[test]
	fn respects_gitignore() {
		let tmp = tempfile::tempdir().unwrap();
		let root = tmp.path();
		write(root, ".gitignore", "skip/\n");
		write(root, "kept.ts", "");
		write(root, "skip/dropped.ts", "");
		fs::create_dir_all(root.join(".git")).unwrap();
		let files: Vec<String> = walk_lang_files(root)
			.into_iter()
			.map(|f| f.path.strip_prefix(root).unwrap().to_string_lossy().into())
			.collect();
		assert_eq!(files, vec!["kept.ts".to_string()]);
	}

	#[test]
	fn directory_walk_uses_the_same_nested_gitignore_rules() {
		let tmp = tempfile::tempdir().unwrap();
		let root = tmp.path();
		write(root, ".gitignore", "ignored/\n");
		write(root, "kept/.gitignore", "nested-ignored/\n");
		write(root, "kept/src/lib.rs", "");
		write(root, "kept/nested-ignored/generated.rs", "");
		write(root, "ignored/generated.rs", "");
		fs::create_dir_all(root.join(".git")).unwrap();

		let directories: HashSet<PathBuf> =
			walk_non_ignored_directories(root).into_iter().collect();

		assert!(directories.contains(root));
		assert!(directories.contains(&root.join("kept")));
		assert!(directories.contains(&root.join("kept/src")));
		assert!(!directories.contains(&root.join("ignored")));
		assert!(!directories.contains(&root.join("kept/nested-ignored")));
	}

	#[test]
	fn incremental_matcher_uses_nested_gitignore_negations() {
		let tmp = tempfile::tempdir().unwrap();
		let root = tmp.path();
		write(root, ".gitignore", "*.rs\n");
		write(root, "keep/.gitignore", "!*.rs\n");
		write(root, "keep/lib.rs", "");
		write(root, "dropped.rs", "");
		fs::create_dir_all(root.join(".git")).unwrap();
		let mut matcher = workspace_ignore_matcher(root);

		assert!(matcher.matched("dropped.rs", false).is_ignore());
		assert!(!matcher.matched("keep/lib.rs", false).is_ignore());
	}

	#[test]
	fn project_root_is_the_ignore_boundary() {
		let tmp = tempfile::tempdir().unwrap();
		let parent = tmp.path();
		let root = parent.join("project");
		write(parent, ".gitignore", "parent.rs\n");
		write(parent, ".ignore", "also-parent.rs\n");
		write(&root, "parent.rs", "");
		write(&root, "also-parent.rs", "");
		write(&root, "kept.rs", "");

		let files: HashSet<String> = walk_lang_files(&root)
			.into_iter()
			.map(|file| {
				file.path
					.strip_prefix(&root)
					.unwrap()
					.to_string_lossy()
					.into_owned()
			})
			.collect();

		assert!(files.contains("parent.rs"));
		assert!(files.contains("also-parent.rs"));
		assert!(files.contains("kept.rs"));
	}

	#[test]
	fn deepest_project_ignore_rule_wins_across_supported_filenames() {
		let tmp = tempfile::tempdir().unwrap();
		let root = tmp.path();
		write(root, ".ignore", "nested/from-ignore.rs\n");
		write(root, ".gitignore", "nested/from-gitignore.rs\n");
		write(root, "nested/.gitignore", "!from-ignore.rs\n");
		write(root, "nested/.ignore", "!from-gitignore.rs\n");
		write(root, "nested/from-ignore.rs", "");
		write(root, "nested/from-gitignore.rs", "");

		let files: HashSet<String> = walk_lang_files(root)
			.into_iter()
			.map(|file| {
				file.path
					.strip_prefix(root)
					.unwrap()
					.to_string_lossy()
					.into_owned()
			})
			.collect();

		assert!(files.contains("nested/from-ignore.rs"));
		assert!(files.contains("nested/from-gitignore.rs"));
	}

	#[test]
	fn explicit_file_accepts_supported_file() {
		let tmp = tempfile::tempdir().unwrap();
		let root = tmp.path();
		write(root, "kept.ts", "");

		assert!(explicit_lang_file(&root.join("kept.ts")).is_some());
		assert!(explicit_lang_file(&root.join("kept.txt")).is_none());
	}
}
