use std::path::{Path, PathBuf};

use crate::lang::path_to_lang;
use code_moniker_core::lang::Lang;

pub struct WalkedFile {
	pub path: PathBuf,
	pub lang: Lang,
}

pub fn walk_lang_files(root: &Path) -> Vec<WalkedFile> {
	ignore::WalkBuilder::new(root)
		.build()
		.filter_map(|entry| entry.ok())
		.filter(|e| e.file_type().is_some_and(|t| t.is_file()))
		.filter_map(|e| {
			let p = e.into_path();
			let lang = path_to_lang(&p).ok()?;
			Some(WalkedFile { path: p, lang })
		})
		.collect()
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
}
