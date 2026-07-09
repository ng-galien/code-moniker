use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Context;
use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_core::lang::Lang;
use code_moniker_workspace::changes::diff::{
	BaseRev, ChangeFile, ChangeRoot, ChangeScan, DiffScope, HeadSide,
};
use code_moniker_workspace::changes::semantic::review::{
	ReviewDiffs, SemanticReview, build_semantic_review_from, collect_review_diffs_scoped, read_blob,
};
use code_moniker_workspace::environment::{self, SourceFileSet};

use crate::args::DiffArgs;

pub fn semantic_review(args: &DiffArgs) -> anyhow::Result<SemanticReview> {
	let (path, scope) = resolve_target(args)?;
	let sources = environment::discover_sources(std::slice::from_ref(&path), args.project.clone())?;
	let diffs = collect_review_diffs_scoped(&review_root_keys(&sources), &scope);
	if scope != DiffScope::worktree() && !diffs.any_root_resolved() {
		anyhow::bail!(
			"cannot resolve the requested revisions: {:?}",
			diffs.diagnostics
		);
	}
	let review = match diffs.head_rev() {
		None => worktree_review(&sources, &diffs)?,
		Some(rev) => blob_review(&sources, &diffs, rev)?,
	};
	Ok(review)
}

fn resolve_target(args: &DiffArgs) -> anyhow::Result<(PathBuf, DiffScope)> {
	let is_range = args.target.contains("..");
	if let Some(base) = &args.base {
		if is_range {
			anyhow::bail!("--base conflicts with a <base>..<head> range");
		}
		let scope = DiffScope {
			base: BaseRev::Rev(base.clone()),
			head: HeadSide::Worktree,
		};
		return Ok((PathBuf::from(&args.target), scope));
	}
	if is_range {
		let scope = DiffScope::parse_range(&args.target).map_err(anyhow::Error::msg)?;
		let path = args.path.clone().unwrap_or_else(|| PathBuf::from("."));
		return Ok((path, scope));
	}
	if let Some(extra) = &args.path {
		anyhow::bail!(
			"unexpected extra path `{}`: `{}` is not a revision range",
			extra.display(),
			args.target
		);
	}
	Ok((PathBuf::from(&args.target), DiffScope::worktree()))
}

fn worktree_review(sources: &SourceFileSet, diffs: &ReviewDiffs) -> anyhow::Result<SemanticReview> {
	let changed: HashSet<PathBuf> = diffs.current_paths().into_iter().collect();
	let mut extracted: Vec<OwnedFile> = Vec::new();
	for file in &sources.files {
		if !changed.contains(&canonical(&file.path)) {
			continue;
		}
		let ctx = &sources.roots[file.source].ctx;
		let (graph, cached_source) =
			environment::load_or_extract_source(&file.path, &file.anchor, file.lang, None, ctx)?;
		let source = match cached_source {
			Some(text) => text,
			None => std::fs::read_to_string(&file.path)?,
		};
		extracted.push(OwnedFile {
			path: file.path.clone(),
			rel_path: file.rel_path.clone(),
			anchor: file.anchor.clone(),
			lang: file.lang,
			source_root: file.source,
			source,
			graph,
		});
	}
	Ok(build_semantic_review_from(
		&owned_scan(sources, &extracted),
		diffs,
	))
}

struct OwnedFile {
	path: PathBuf,
	rel_path: PathBuf,
	anchor: PathBuf,
	lang: Lang,
	source_root: usize,
	source: String,
	graph: CodeGraph,
}

fn blob_review(
	sources: &SourceFileSet,
	diffs: &ReviewDiffs,
	head_rev: &str,
) -> anyhow::Result<SemanticReview> {
	let mut extracted: Vec<OwnedFile> = Vec::new();
	for (repo_root, repo_rel) in diffs.current_rows() {
		let Some(file) = head_file(sources, repo_root, repo_rel, head_rev)? else {
			continue;
		};
		extracted.push(file);
	}
	Ok(build_semantic_review_from(
		&owned_scan(sources, &extracted),
		diffs,
	))
}

fn head_file(
	sources: &SourceFileSet,
	repo_root: &Path,
	repo_rel: &Path,
	head_rev: &str,
) -> anyhow::Result<Option<OwnedFile>> {
	let abs = repo_root.join(repo_rel);
	let Ok(lang) = environment::language_for_path(&abs) else {
		return Ok(None);
	};
	let Some((source_root, root, rel_path)) = owning_root(sources, &abs) else {
		return Ok(None);
	};
	if has_hidden_component(&rel_path) {
		return Ok(None);
	}
	let source = read_blob(repo_root, head_rev, repo_rel)
		.with_context(|| format!("cannot read {head_rev}:{}", repo_rel.display()))?;
	let anchor = if sources.multi {
		PathBuf::from(&root.label).join(&rel_path)
	} else {
		rel_path.clone()
	};
	let graph = environment::extract_source_with(lang, &source, &anchor, &root.ctx);
	Ok(Some(OwnedFile {
		path: abs,
		rel_path: anchor.clone(),
		anchor,
		lang,
		source_root,
		source,
		graph,
	}))
}

fn owning_root<'s>(
	sources: &'s SourceFileSet,
	abs: &Path,
) -> Option<(usize, &'s environment::SourceRoot, PathBuf)> {
	let abs = canonical(abs);
	sources.roots.iter().enumerate().find_map(|(idx, root)| {
		abs.strip_prefix(canonical(&root.path))
			.ok()
			.map(|rel| (idx, root, rel.to_path_buf()))
	})
}

fn owned_scan<'a>(sources: &'a SourceFileSet, files: &'a [OwnedFile]) -> ChangeScan<'a> {
	ChangeScan {
		roots: sources
			.roots
			.iter()
			.map(|root| ChangeRoot {
				label: &root.label,
				path: &root.path,
				ctx: &root.ctx,
			})
			.collect(),
		files: files
			.iter()
			.enumerate()
			.map(|(file_idx, file)| ChangeFile {
				file_idx,
				source_root: file.source_root,
				path: &file.path,
				rel_path: &file.rel_path,
				anchor: &file.anchor,
				lang: file.lang,
				graph: &file.graph,
				source: &file.source,
			})
			.collect(),
	}
}

fn review_root_keys(sources: &SourceFileSet) -> Vec<(String, PathBuf)> {
	sources
		.roots
		.iter()
		.map(|root| (root.label.clone(), root.path.clone()))
		.collect()
}

fn has_hidden_component(path: &Path) -> bool {
	path.components().any(|component| {
		component
			.as_os_str()
			.as_encoded_bytes()
			.first()
			.is_some_and(|byte| *byte == b'.')
	})
}

fn canonical(path: &Path) -> PathBuf {
	path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
