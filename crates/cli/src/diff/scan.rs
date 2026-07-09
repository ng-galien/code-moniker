use std::collections::HashSet;
use std::path::{Path, PathBuf};

use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_workspace::changes::diff::{ChangeFile, ChangeRoot, ChangeScan};
use code_moniker_workspace::changes::semantic::review::{
	SemanticReview, build_semantic_review_from, collect_review_diffs,
};
use code_moniker_workspace::environment::{self, SourceFile};

use crate::args::DiffArgs;

pub fn semantic_review(args: &DiffArgs) -> anyhow::Result<SemanticReview> {
	let sources =
		environment::discover_sources(std::slice::from_ref(&args.path), args.project.clone())?;
	let diffs = collect_review_diffs(&review_root_keys(&sources));
	let changed: HashSet<PathBuf> = diffs.current_paths().into_iter().collect();
	let mut extracted: Vec<(&SourceFile, CodeGraph, String)> = Vec::new();
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
		extracted.push((file, graph, source));
	}
	let scan = ChangeScan {
		roots: sources
			.roots
			.iter()
			.map(|root| ChangeRoot {
				label: &root.label,
				path: &root.path,
				ctx: &root.ctx,
			})
			.collect(),
		files: extracted
			.iter()
			.enumerate()
			.map(|(file_idx, (file, graph, source))| ChangeFile {
				file_idx,
				source_root: file.source,
				path: &file.path,
				rel_path: &file.rel_path,
				anchor: &file.anchor,
				lang: file.lang,
				graph,
				source,
			})
			.collect(),
	};
	Ok(build_semantic_review_from(&scan, &diffs))
}

fn review_root_keys(sources: &environment::SourceFileSet) -> Vec<(String, PathBuf)> {
	sources
		.roots
		.iter()
		.map(|root| (root.label.clone(), root.path.clone()))
		.collect()
}

fn canonical(path: &Path) -> PathBuf {
	path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
