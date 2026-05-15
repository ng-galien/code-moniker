use std::collections::BTreeMap;
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

impl SourceSet {
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
	if paths.is_empty() {
		return Err(anyhow::anyhow!("at least one source path is required"));
	}
	let multi = paths.len() > 1;
	let labels = unique_labels(paths);
	let mut roots = Vec::with_capacity(paths.len());
	let mut files = Vec::new();
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
		let ctx = extract::Context {
			ts,
			project: source_project,
		};
		let walked = if root_is_dir {
			walk::walk_lang_files(path)
		} else {
			let lang = path_to_lang(path)?;
			vec![WalkedFile {
				path: path.clone(),
				lang,
			}]
		};
		roots.push(SourceRoot {
			input: path.clone(),
			path: root.clone(),
			label: label.clone(),
			ctx,
		});
		for walked in walked {
			let rel = walked
				.path
				.strip_prefix(&root)
				.unwrap_or(&walked.path)
				.to_path_buf();
			let rel_path = if multi {
				PathBuf::from(&label).join(&rel)
			} else {
				rel.clone()
			};
			let anchor = if multi {
				rel_path.clone()
			} else if root_is_dir {
				rel.clone()
			} else {
				walked.path.clone()
			};
			files.push(SourceFile {
				source: source_idx,
				path: walked.path,
				rel_path,
				anchor,
				lang: walked.lang,
			});
		}
	}
	files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
	Ok(SourceSet {
		roots,
		files,
		multi,
	})
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
}
