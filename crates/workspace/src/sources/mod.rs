// code-moniker: ignore-file[smell-clone-reflex]
// Source discovery clones paths and labels into durable workspace source records.
use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};
use std::path::Component;
use std::path::{Path, PathBuf};

use code_moniker_core::lang::Lang;

use crate::extract;
use crate::lang::path_to_lang;
use crate::path_util::portable_path_buf;
use crate::snapshot::WorkspaceCancellation;
use crate::source_group::DeclaredSourceGroups;
use crate::tsconfig::{self, TsResolution};
use crate::walk::{self, WalkedFile};

mod c;

pub use c::CBuildContext;

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
	pub source_groups: DeclaredSourceGroups,
}

#[derive(Clone, Debug)]
pub struct SourceFile {
	pub source: usize,
	pub path: PathBuf,
	pub rel_path: PathBuf,
	pub anchor: PathBuf,
	pub lang: code_moniker_core::lang::Lang,
	pub root_moniker: Option<code_moniker_core::core::moniker::Moniker>,
	pub source_group: Option<usize>,
	pub srcset: Option<String>,
	pub retired: bool,
}

struct SourceScope {
	source: usize,
	root_is_dir: bool,
	c_header_provenance_loaded: bool,
	root: SourceRoot,
}

#[derive(Clone, Copy)]
struct SourceContextNeeds {
	ts: bool,
	c: bool,
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

impl SourceFile {
	pub fn extraction_context<'a>(&self, root: &'a SourceRoot) -> Cow<'a, extract::Context> {
		extraction_context_with_srcset(&root.ctx, self.srcset.as_deref())
	}
}

impl SourceRoot {
	pub fn extraction_context_for_path(&self, path: &Path) -> Cow<'_, extract::Context> {
		let srcset = self
			.source_groups
			.membership(path)
			.and_then(|membership| membership.srcset);
		extraction_context_with_srcset(&self.ctx, srcset)
	}
}

pub(crate) fn extraction_context_with_srcset<'a>(
	base: &'a extract::Context,
	srcset: Option<&str>,
) -> Cow<'a, extract::Context> {
	match srcset {
		Some(srcset) => {
			let mut ctx = base.clone();
			ctx.srcset = Some(srcset.to_string());
			Cow::Owned(ctx)
		}
		None => Cow::Borrowed(base),
	}
}

pub fn discover(paths: &[PathBuf], project: Option<String>) -> anyhow::Result<SourceSet> {
	discover_cancellable(paths, project, &WorkspaceCancellation::default())
}

pub fn discover_cancellable(
	paths: &[PathBuf],
	project: Option<String>,
	cancellation: &WorkspaceCancellation,
) -> anyhow::Result<SourceSet> {
	discover_cancellable_with_context(paths, project, cancellation, true)
}

pub fn discover_catalog(root: &Path, project: Option<String>) -> anyhow::Result<SourceSet> {
	discover_cancellable_with_context(
		&[root.to_path_buf()],
		project,
		&WorkspaceCancellation::default(),
		false,
	)
}

fn discover_cancellable_with_context(
	paths: &[PathBuf],
	project: Option<String>,
	cancellation: &WorkspaceCancellation,
	load_c_context: bool,
) -> anyhow::Result<SourceSet> {
	ensure_not_cancelled(cancellation)?;
	let scopes = discover_scopes(
		paths,
		project,
		SourceContextNeeds {
			ts: true,
			c: load_c_context,
		},
	)?;
	let multi = scopes.len() > 1;
	let mut files = Vec::new();
	for scope in &scopes {
		ensure_not_cancelled(cancellation)?;
		let walked = if scope.root_is_dir {
			walk::walk_lang_files_cancellable(&scope.root.input, || cancellation.is_cancelled())
		} else {
			let lang = path_to_lang(&scope.root.input)?;
			vec![WalkedFile {
				path: scope.root.input.clone(),
				lang,
			}]
		};
		for walked in walked {
			ensure_not_cancelled(cancellation)?;
			if !scope_accepts_file(scope, &walked) {
				continue;
			}
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

fn ensure_not_cancelled(cancellation: &WorkspaceCancellation) -> anyhow::Result<()> {
	if cancellation.is_cancelled() {
		anyhow::bail!("workspace build cancelled");
	}
	Ok(())
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
	let needs = files
		.iter()
		.filter_map(|file| path_to_lang(file).ok())
		.fold(
			SourceContextNeeds {
				ts: false,
				c: false,
			},
			|mut needs, lang| {
				needs.ts |= matches!(lang, Lang::Ts | Lang::Tsx | Lang::Js | Lang::Jsx);
				needs.c |= lang == Lang::C;
				needs
			},
		);
	let scopes = discover_scopes(&[root.to_path_buf()], project, needs)?;
	let Some(scope) = scopes.first() else {
		return Err(anyhow::anyhow!(
			"discover_scopes returned no scope for {}",
			root.display()
		));
	};
	let abs_root = normalize_absolute(&scope.root.path)?;
	let mut ignore = walk::workspace_ignore_matcher(&abs_root);
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
			let relative = abs_path
				.strip_prefix(&abs_root)
				.expect("candidate was checked under source root");
			if ignore.matched(relative, false).is_ignore() {
				continue;
			}
			let Some(walked) = walk::explicit_lang_file(&path) else {
				continue;
			};
			if !scope_accepts_file(scope, &walked) {
				continue;
			}
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

fn discover_scopes(
	paths: &[PathBuf],
	project: Option<String>,
	needs: SourceContextNeeds,
) -> anyhow::Result<Vec<SourceScope>> {
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
		let source_project = project.clone();
		let source_groups = DeclaredSourceGroups::load(&root)?;
		let mut ts = if needs.ts {
			tsconfig::load(&root)
		} else {
			TsResolution::default()
		};
		let c = if needs.c {
			CBuildContext::load(&root)
		} else {
			CBuildContext::default()
		};
		if multi {
			prefix_ts_aliases(&mut ts, &label);
		}
		scopes.push(SourceScope {
			source: source_idx,
			root_is_dir,
			c_header_provenance_loaded: needs.c,
			root: SourceRoot {
				input: path.clone(),
				path: root,
				label,
				ctx: extract::Context {
					c,
					ts,
					project: source_project,
					srcset: None,
				},
				source_groups,
			},
		});
	}
	Ok(scopes)
}

pub(crate) fn source_file_for_new_path(sources: &SourceSet, path: &Path) -> Option<SourceFile> {
	let lang = path_to_lang(path).ok()?;
	let abs = path
		.canonicalize()
		.or_else(|_| normalize_absolute(path))
		.ok()?;
	let (source, root) = sources
		.roots
		.iter()
		.enumerate()
		.filter_map(|(idx, root)| {
			let root_path = canonical_root_path(&root.path)?;
			abs.starts_with(&root_path)
				.then(|| (idx, root, root_path.components().count()))
		})
		.max_by_key(|(_, _, depth)| *depth)
		.map(|(idx, root, _)| (idx, root))?;
	let root_path = canonical_root_path(&root.path)?;
	if lang == code_moniker_core::lang::Lang::C && !root.ctx.c.should_index_as_c(&abs) {
		return None;
	}
	let rel = abs.strip_prefix(&root_path).ok()?.to_path_buf();
	let rel_path = portable_path_buf(&if sources.multi {
		PathBuf::from(&root.label).join(&rel)
	} else {
		rel.clone()
	});
	let anchor = portable_path_buf(&if sources.multi {
		rel_path.clone()
	} else if root_path.is_dir() {
		anchor_with_source_context(&root_path, &rel)
	} else {
		abs.clone()
	});
	let (source_group, srcset) = configured_source_membership(root, &abs);
	let ctx = extraction_context_with_srcset(&root.ctx, srcset.as_deref());
	let root_moniker = extract::source_root(lang, &anchor, &ctx);
	Some(SourceFile {
		source,
		path: abs,
		rel_path,
		anchor,
		lang,
		root_moniker,
		source_group,
		srcset,
		retired: false,
	})
}

fn scope_accepts_file(scope: &SourceScope, walked: &WalkedFile) -> bool {
	if walked.lang != code_moniker_core::lang::Lang::C {
		return true;
	}
	if walked
		.path
		.extension()
		.and_then(|extension| extension.to_str())
		.is_some_and(|extension| extension.eq_ignore_ascii_case("h"))
		&& !scope.c_header_provenance_loaded
	{
		return false;
	}
	scope.root.ctx.c.should_index_as_c(&walked.path)
}

fn canonical_root_path(root: &Path) -> Option<PathBuf> {
	root.canonicalize()
		.or_else(|_| normalize_absolute(root))
		.ok()
}

fn source_file_from_walked(scope: &SourceScope, walked: WalkedFile, multi: bool) -> SourceFile {
	let root = normalize_absolute(&scope.root.path).unwrap_or_else(|_| scope.root.path.clone());
	let path = normalize_absolute(&walked.path).unwrap_or_else(|_| walked.path.clone());
	let rel = path.strip_prefix(&root).unwrap_or(&path).to_path_buf();
	let rel_path = portable_path_buf(&if multi {
		PathBuf::from(&scope.root.label).join(&rel)
	} else {
		rel.clone()
	});
	let anchor = portable_path_buf(&if multi {
		rel_path.clone()
	} else if scope.root_is_dir {
		anchor_with_source_context(&root, &rel)
	} else {
		walked.path.clone()
	});
	let (source_group, srcset) = configured_source_membership(&scope.root, &path);
	let ctx = extraction_context_with_srcset(&scope.root.ctx, srcset.as_deref());
	let root_moniker = extract::source_root(walked.lang, &anchor, &ctx);
	SourceFile {
		source: scope.source,
		path: walked.path,
		rel_path,
		anchor,
		retired: false,
		lang: walked.lang,
		root_moniker,
		source_group,
		srcset,
	}
}

fn configured_source_membership(root: &SourceRoot, path: &Path) -> (Option<usize>, Option<String>) {
	root.source_groups
		.membership(path)
		.map(|membership| {
			(
				Some(membership.group),
				membership.srcset.map(ToOwned::to_owned),
			)
		})
		.unwrap_or_default()
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
		assert_eq!(set.roots[0].ctx.project, None);
		assert_eq!(set.roots[1].ctx.project, None);
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
	fn explicit_rust_file_does_not_load_typescript_resolution() {
		let tmp = tempfile::tempdir().unwrap();
		write(
			tmp.path(),
			"tsconfig.json",
			r#"{"compilerOptions": {"paths": {"@app/*": ["./web/*"]}}}"#,
		);
		write(tmp.path(), "src/lib.rs", "pub fn answer() -> u8 { 42 }\n");

		let set = discover_files(tmp.path(), &[PathBuf::from("src/lib.rs")], None).unwrap();

		assert!(set.roots[0].ctx.ts.aliases.is_empty());
		assert_eq!(set.files.len(), 1);
		assert_eq!(set.files[0].lang, Lang::Rs);
	}

	#[test]
	fn explicit_typescript_file_loads_typescript_resolution() {
		let tmp = tempfile::tempdir().unwrap();
		write(
			tmp.path(),
			"tsconfig.json",
			r#"{"compilerOptions": {"paths": {"@app/*": ["./web/*"]}}}"#,
		);
		write(tmp.path(), "web/app.ts", "export const answer = 42;\n");

		let set = discover_files(tmp.path(), &[PathBuf::from("web/app.ts")], None).unwrap();

		assert!(
			set.roots[0]
				.ctx
				.ts
				.aliases
				.iter()
				.any(|alias| alias.pattern == "@app/*")
		);
		assert_eq!(set.files.len(), 1);
		assert_eq!(set.files[0].lang, Lang::Ts);
	}

	#[test]
	fn explicit_mixed_files_load_union_of_language_context_needs() {
		let tmp = tempfile::tempdir().unwrap();
		write(
			tmp.path(),
			"tsconfig.json",
			r#"{"compilerOptions": {"paths": {"@app/*": ["./web/*"]}}}"#,
		);
		write(tmp.path(), "src/lib.rs", "pub fn answer() -> u8 { 42 }\n");
		write(tmp.path(), "web/app.ts", "export const answer = 42;\n");

		let set = discover_files(
			tmp.path(),
			&[PathBuf::from("src/lib.rs"), PathBuf::from("web/app.ts")],
			None,
		)
		.unwrap();

		assert!(
			set.roots[0]
				.ctx
				.ts
				.aliases
				.iter()
				.any(|alias| alias.pattern == "@app/*")
		);
		assert_eq!(set.files.len(), 2);
		assert!(set.files.iter().any(|file| file.lang == Lang::Rs));
		assert!(set.files.iter().any(|file| file.lang == Lang::Ts));
	}

	#[test]
	fn excludes_headers_reached_only_from_cpp_translation_units() {
		let tmp = tempfile::tempdir().unwrap();
		write(
			tmp.path(),
			"generated/model.pb.cc",
			"#include \"model.pb.h\"\n",
		);
		write(
			tmp.path(),
			"generated/model.pb.h",
			"namespace generated {}\n",
		);
		write(tmp.path(), "src/main.c", "int main(void) { return 0; }\n");
		write(tmp.path(), "include/api.h", "int api(void);\n");

		let set = discover(&[tmp.path().to_path_buf()], None).unwrap();

		assert!(
			set.files
				.iter()
				.any(|file| file.rel_path == Path::new("include/api.h"))
		);
		assert!(
			!set.files
				.iter()
				.any(|file| file.rel_path == Path::new("generated/model.pb.h"))
		);
	}

	#[test]
	fn catalog_keeps_c_translation_units_without_guessing_header_language() {
		let tmp = tempfile::tempdir().unwrap();
		write(
			tmp.path(),
			"generated/model.pb.cc",
			"#include \"model.pb.h\"\n",
		);
		write(
			tmp.path(),
			"generated/model.pb.h",
			"namespace generated {}\n",
		);
		write(tmp.path(), "src/main.c", "int main(void) { return 0; }\n");
		write(tmp.path(), "include/api.h", "int api(void);\n");

		let set = discover_catalog(tmp.path(), None).unwrap();

		assert_eq!(
			set.files
				.iter()
				.map(|file| file.rel_path.as_path())
				.collect::<Vec<_>>(),
			vec![Path::new("src/main.c")]
		);
	}

	#[test]
	fn explicit_c_headers_use_loaded_build_provenance() {
		let tmp = tempfile::tempdir().unwrap();
		write(
			tmp.path(),
			"generated/model.pb.cc",
			"#include \"model.pb.h\"\n",
		);
		write(
			tmp.path(),
			"generated/model.pb.h",
			"namespace generated {}\n",
		);
		write(
			tmp.path(),
			"src/main.c",
			"#include \"../include/api.h\"\nint main(void) { return api(); }\n",
		);
		write(tmp.path(), "include/api.h", "int api(void);\n");

		let set = discover_files(
			tmp.path(),
			&[
				PathBuf::from("include/api.h"),
				PathBuf::from("generated/model.pb.h"),
			],
			None,
		)
		.unwrap();

		assert_eq!(
			set.files
				.iter()
				.map(|file| file.rel_path.as_path())
				.collect::<Vec<_>>(),
			vec![Path::new("include/api.h")]
		);
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
