use std::path::{Path, PathBuf};

use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::core::uri::{UriConfig, to_uri};
use code_moniker_core::lang::Lang;

pub type ExtractContext = crate::extract::Context;
pub type IdentityResolver = crate::source::LocalIdentityResolver;
pub type IndexedSourceMaterial = crate::source::CodeIndexMaterial;
pub type ResourceCache = crate::source::LocalResourceCache;
pub type SourceFile = crate::sources::SourceFile;
pub type SourceFileSet = crate::sources::SourceSet;
pub type SourceRoot = crate::sources::SourceRoot;

pub fn discover_sources(
	paths: &[PathBuf],
	project: Option<String>,
) -> anyhow::Result<SourceFileSet> {
	crate::sources::discover(paths, project)
}

pub fn discover_source_files(
	root: &Path,
	files: &[PathBuf],
	project: Option<String>,
) -> anyhow::Result<SourceFileSet> {
	crate::sources::discover_files(root, files, project)
}

pub fn language_for_path(path: &Path) -> anyhow::Result<Lang> {
	Ok(crate::lang::path_to_lang(path)?)
}

pub fn load_or_extract_source(
	path: &Path,
	anchor: &Path,
	lang: Lang,
	cache_dir: Option<&Path>,
	ctx: &ExtractContext,
) -> anyhow::Result<(CodeGraph, Option<String>)> {
	Ok(crate::cache::load_or_extract_result(
		path, anchor, lang, cache_dir, ctx,
	)?)
}

pub fn cached_index_material(
	cache: &ResourceCache,
	generation: crate::snapshot::ResourceGeneration,
) -> Option<IndexedSourceMaterial> {
	cache.index_material(generation)
}

pub fn next_resource_generation(cache: &ResourceCache) -> crate::snapshot::ResourceGeneration {
	cache.next_generation()
}

#[cfg(test)]
pub fn extract_source(lang: Lang, source: &str, path: &Path) -> CodeGraph {
	crate::extract::extract(lang, source, path)
}

pub fn extract_source_with(
	lang: Lang,
	source: &str,
	path: &Path,
	ctx: &ExtractContext,
) -> CodeGraph {
	crate::extract::extract_with(lang, source, path, ctx)
}

pub fn line_range(source: &str, start: u32, end: u32) -> (u32, u32) {
	crate::lines::line_range(source, start, end)
}

pub fn compact_moniker(moniker: &Moniker, scheme: &str) -> String {
	render_compact_moniker(moniker).unwrap_or_else(|| {
		to_uri(moniker, &UriConfig { scheme }).unwrap_or_else(|_| non_utf8(moniker))
	})
}

fn render_compact_moniker(moniker: &Moniker) -> Option<String> {
	let view = moniker.as_view();
	let mut lang: Option<String> = None;
	let mut packages: Vec<String> = Vec::new();
	let mut dirs: Vec<String> = Vec::new();
	let mut modules: Vec<String> = Vec::new();
	let mut rest: Vec<(String, String)> = Vec::new();
	for segment in view.segments() {
		let kind = std::str::from_utf8(segment.kind).ok()?.to_string();
		let name = std::str::from_utf8(segment.name).ok()?.to_string();
		match kind.as_str() {
			"lang" => lang = Some(name),
			"package" => packages.push(name),
			"dir" => dirs.push(name),
			"module" => modules.push(name),
			_ => rest.push((kind, name)),
		}
	}
	let head = lang.unwrap_or_else(|| {
		std::str::from_utf8(view.project())
			.unwrap_or(".")
			.to_string()
	});
	if packages.is_empty() && dirs.is_empty() && modules.is_empty() && rest.is_empty() {
		return Some(head);
	}
	let mut out = String::new();
	out.push_str(&head);
	out.push(':');
	let mut wrote_scope = false;
	if !packages.is_empty() {
		out.push_str(&packages.join("."));
		wrote_scope = true;
	} else if !dirs.is_empty() {
		out.push_str(&dirs.join("/"));
		wrote_scope = true;
	}
	let has_module = !modules.is_empty();
	if has_module {
		if wrote_scope {
			out.push('/');
		}
		out.push_str(&modules.join("."));
	}
	for (idx, (kind, name)) in rest.iter().enumerate() {
		if idx == 0 && has_module {
			out.push('.');
		} else if wrote_scope || has_module || idx > 0 {
			out.push('/');
		}
		out.push_str(kind);
		out.push(':');
		out.push_str(name);
	}
	Some(out)
}

fn non_utf8(moniker: &Moniker) -> String {
	format!("<non-utf8:{}b>", moniker.as_bytes().len())
}
