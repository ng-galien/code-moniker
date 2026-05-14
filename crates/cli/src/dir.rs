use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde::Serialize;

use crate::args::{ExtractArgs, OutputFormat, OutputMode};
use crate::cache;
use crate::extract;
use crate::format;
use crate::predicate::{self, MatchSet, Predicate, RefMatch};
use crate::tsconfig;
use crate::walk;
use code_moniker_core::core::code_graph::{DefRecord, RefRecord};
use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;

const TOP_KINDS_DISPLAYED: usize = 3;

pub fn run<W: Write>(
	args: &ExtractArgs,
	stdout: &mut W,
	root: &Path,
	scheme: &str,
) -> anyhow::Result<bool> {
	let files = walk::walk_lang_files(root);
	let ctx = extract::Context {
		ts: tsconfig::load(root),
	};
	let has_filter = !args.kind.is_empty() || !args.shape.is_empty() || !args.where_.is_empty();
	if has_filter {
		run_filter(args, stdout, &files, root, scheme, &ctx)
	} else {
		run_summary(args, stdout, &files, root, &ctx)
	}
}

fn run_summary<W: Write>(
	args: &ExtractArgs,
	stdout: &mut W,
	files: &[walk::WalkedFile],
	root: &Path,
	ctx: &extract::Context,
) -> anyhow::Result<bool> {
	let cache_dir = args.cache.as_deref();
	let summaries: Vec<FileSummary> = files
		.par_iter()
		.filter_map(|f| FileSummary::compute(&f.path, f.lang, root, cache_dir, ctx))
		.collect();
	let total_defs: usize = summaries.iter().map(|s| s.defs).sum();
	let total_refs: usize = summaries.iter().map(|s| s.refs).sum();
	let any = total_defs + total_refs > 0;
	match args.mode() {
		OutputMode::Default => match args.format {
			OutputFormat::Tsv => write_summary_tsv(stdout, &summaries)?,
			OutputFormat::Json => write_summary_json(stdout, &summaries)?,
			#[cfg(feature = "pretty")]
			OutputFormat::Tree => write_summary_tree(stdout, &summaries, args)?,
		},
		OutputMode::Count => writeln!(stdout, "{}", total_defs + total_refs)?,
		OutputMode::Quiet => {}
	}
	Ok(any)
}

fn run_filter<W: Write>(
	args: &ExtractArgs,
	stdout: &mut W,
	files: &[walk::WalkedFile],
	root: &Path,
	scheme: &str,
	ctx: &extract::Context,
) -> anyhow::Result<bool> {
	let predicates = args.compiled_predicates(scheme)?;
	let mut langs: Vec<Lang> = files.iter().map(|f| f.lang).collect();
	langs.sort_by_key(|l| l.tag());
	langs.dedup();
	let known = predicate::known_kinds(langs.iter());
	let unknown = predicate::unknown_kinds(&args.kind, &known);
	if !unknown.is_empty() {
		return Err(crate::unknown_kinds_error(&unknown, &langs, &known));
	}
	let cache_dir = args.cache.as_deref();
	let rows: Vec<FilterRow> = files
		.par_iter()
		.filter_map(|f| {
			FilterRow::compute(
				&f.path,
				f.lang,
				root,
				&predicates,
				&args.kind,
				&args.shape,
				cache_dir,
				ctx,
			)
		})
		.collect();
	let total_defs: usize = rows.iter().map(|r| r.defs.len()).sum();
	let total_refs: usize = rows.iter().map(|r| r.refs.len()).sum();
	let any = total_defs + total_refs > 0;
	match args.mode() {
		OutputMode::Default => match args.format {
			OutputFormat::Tsv => write_filter_tsv(stdout, &rows, args, scheme)?,
			OutputFormat::Json => write_filter_json(stdout, &rows, args, scheme)?,
			#[cfg(feature = "pretty")]
			OutputFormat::Tree => write_filter_tree(stdout, &rows, args, scheme)?,
		},
		OutputMode::Count => writeln!(stdout, "{}", total_defs + total_refs)?,
		OutputMode::Quiet => {}
	}
	Ok(any)
}

#[derive(Serialize)]
struct FileSummary {
	file: String,
	lang: &'static str,
	defs: usize,
	refs: usize,
	by_def_kind: BTreeMap<String, usize>,
	by_ref_kind: BTreeMap<String, usize>,
}

impl FileSummary {
	fn compute(
		path: &Path,
		lang: Lang,
		root: &Path,
		cache_dir: Option<&Path>,
		ctx: &extract::Context,
	) -> Option<Self> {
		let rel = path.strip_prefix(root).unwrap_or(path);
		let (graph, _) = cache::load_or_extract(path, rel, lang, cache_dir, ctx)?;
		let mut by_def_kind: BTreeMap<String, usize> = BTreeMap::new();
		let mut defs = 0usize;
		for d in graph.defs() {
			defs += 1;
			bump_kind(&mut by_def_kind, &d.kind);
		}
		let mut by_ref_kind: BTreeMap<String, usize> = BTreeMap::new();
		let mut refs = 0usize;
		for r in graph.refs() {
			refs += 1;
			bump_kind(&mut by_ref_kind, &r.kind);
		}
		Some(Self {
			file: rel.display().to_string(),
			lang: lang.tag(),
			defs,
			refs,
			by_def_kind,
			by_ref_kind,
		})
	}
}

fn write_summary_tsv<W: Write>(w: &mut W, summaries: &[FileSummary]) -> std::io::Result<()> {
	for s in summaries {
		writeln!(
			w,
			"{file}\t{lang}\t{defs}\t{refs}\t{top}",
			file = s.file,
			lang = s.lang,
			defs = s.defs,
			refs = s.refs,
			top = top_kinds(&s.by_def_kind, TOP_KINDS_DISPLAYED),
		)?;
	}
	Ok(())
}

#[cfg(feature = "pretty")]
fn write_summary_tree<W: Write>(
	w: &mut W,
	summaries: &[FileSummary],
	args: &ExtractArgs,
) -> anyhow::Result<()> {
	let entries: Vec<(String, String)> = summaries
		.iter()
		.map(|s| {
			let label = format!(
				"({lang}) defs:{defs} refs:{refs} [{top}]",
				lang = s.lang,
				defs = s.defs,
				refs = s.refs,
				top = top_kinds(&s.by_def_kind, TOP_KINDS_DISPLAYED),
			);
			(s.file.clone(), label)
		})
		.collect();
	format::tree::render_dir_tree(w, &entries, args)?;
	Ok(())
}

fn write_summary_json<W: Write>(w: &mut W, summaries: &[FileSummary]) -> anyhow::Result<()> {
	#[derive(Serialize)]
	struct Out<'a> {
		total_files: usize,
		total_defs: usize,
		total_refs: usize,
		files: &'a [FileSummary],
	}
	let total_defs = summaries.iter().map(|s| s.defs).sum();
	let total_refs = summaries.iter().map(|s| s.refs).sum();
	let out = Out {
		total_files: summaries.len(),
		total_defs,
		total_refs,
		files: summaries,
	};
	serde_json::to_writer_pretty(&mut *w, &out)?;
	w.write_all(b"\n")?;
	Ok(())
}

fn bump_kind(map: &mut BTreeMap<String, usize>, kind: &[u8]) {
	let key = std::str::from_utf8(kind).unwrap_or("");
	if let Some(c) = map.get_mut(key) {
		*c += 1;
	} else {
		map.insert(key.to_owned(), 1);
	}
}

fn top_kinds(map: &BTreeMap<String, usize>, n: usize) -> String {
	if map.is_empty() {
		return "-".to_string();
	}
	let mut pairs: Vec<(&String, &usize)> = map.iter().collect();
	pairs.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
	pairs
		.into_iter()
		.take(n)
		.map(|(k, v)| format!("{k}:{v}"))
		.collect::<Vec<_>>()
		.join(", ")
}

struct FilterRow {
	rel: PathBuf,
	lang: Lang,
	source: String,
	defs: Vec<DefRecord>,
	refs: Vec<(RefRecord, Moniker)>,
}

impl FilterRow {
	#[allow(clippy::too_many_arguments)]
	fn compute(
		path: &Path,
		lang: Lang,
		root: &Path,
		predicates: &[Predicate],
		kinds: &[String],
		shapes: &[code_moniker_core::core::shape::Shape],
		cache_dir: Option<&Path>,
		ctx: &extract::Context,
	) -> Option<Self> {
		let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
		let (graph, extracted_source) = cache::load_or_extract(path, &rel, lang, cache_dir, ctx)?;
		let matches = predicate::filter(&graph, predicates, kinds, shapes);
		if matches.defs.is_empty() && matches.refs.is_empty() {
			return None;
		}
		let source = match extracted_source {
			Some(s) => s,
			None => std::fs::read_to_string(path).ok()?,
		};
		let defs = matches.defs.into_iter().cloned().collect();
		let refs = matches
			.refs
			.into_iter()
			.map(|rm| (rm.record.clone(), rm.source.clone()))
			.collect();
		Some(Self {
			rel,
			lang,
			source,
			defs,
			refs,
		})
	}

	fn match_set(&self) -> MatchSet<'_> {
		MatchSet {
			defs: self.defs.iter().collect(),
			refs: self
				.refs
				.iter()
				.map(|(rec, src)| RefMatch {
					record: rec,
					source: src,
				})
				.collect(),
		}
	}
}

fn write_filter_tsv<W: Write>(
	w: &mut W,
	rows: &[FilterRow],
	args: &ExtractArgs,
	scheme: &str,
) -> std::io::Result<()> {
	for row in rows {
		let matches = row.match_set();
		let mut buf: Vec<u8> = Vec::new();
		format::write_tsv(&mut buf, &matches, &row.source, args, scheme)?;
		let prefix = row.rel.display().to_string();
		for line in std::str::from_utf8(&buf).unwrap_or("").lines() {
			writeln!(w, "{prefix}\t{line}")?;
		}
	}
	Ok(())
}

#[cfg(feature = "pretty")]
fn write_filter_tree<W: Write>(
	w: &mut W,
	rows: &[FilterRow],
	args: &ExtractArgs,
	scheme: &str,
) -> anyhow::Result<()> {
	let entries: Vec<format::tree::FileEntry<'_>> = rows
		.iter()
		.map(|row| format::tree::FileEntry {
			rel_path: row.rel.to_string_lossy().into_owned(),
			matches: row.match_set(),
			source: row.source.as_str(),
		})
		.collect();
	format::tree::write_files_tree(w, &entries, args, scheme)?;
	Ok(())
}

fn write_filter_json<W: Write>(
	w: &mut W,
	rows: &[FilterRow],
	args: &ExtractArgs,
	scheme: &str,
) -> anyhow::Result<()> {
	#[derive(Serialize)]
	struct Entry {
		file: String,
		lang: &'static str,
		matches: serde_json::Value,
	}
	let entries: Vec<Entry> = rows
		.iter()
		.map(|row| {
			let matches = row.match_set();
			Entry {
				file: row.rel.display().to_string(),
				lang: row.lang.tag(),
				matches: format::build_matches_value(&matches, &row.source, args, scheme),
			}
		})
		.collect();
	let total_defs: usize = rows.iter().map(|r| r.defs.len()).sum();
	let total_refs: usize = rows.iter().map(|r| r.refs.len()).sum();
	#[derive(Serialize)]
	struct Out {
		total_files: usize,
		total_defs: usize,
		total_refs: usize,
		files: Vec<Entry>,
	}
	let out = Out {
		total_files: entries.len(),
		total_defs,
		total_refs,
		files: entries,
	};
	serde_json::to_writer_pretty(&mut *w, &out)?;
	w.write_all(b"\n")?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;

	fn write_file(root: &Path, rel: &str, body: &str) {
		let p = root.join(rel);
		if let Some(parent) = p.parent() {
			fs::create_dir_all(parent).unwrap();
		}
		fs::write(p, body).unwrap();
	}

	#[test]
	fn summary_aggregates_per_file_counts() {
		let tmp = tempfile::tempdir().unwrap();
		let root = tmp.path();
		write_file(root, "a.ts", "export class Foo {}\nfunction bar() {}\n");
		write_file(root, "b.ts", "import { x } from 'y';\n");
		let files = walk::walk_lang_files(root);
		let summaries: Vec<FileSummary> = files
			.iter()
			.filter_map(|f| {
				FileSummary::compute(&f.path, f.lang, root, None, &extract::Context::default())
			})
			.collect();
		assert_eq!(summaries.len(), 2);
		let a = summaries.iter().find(|s| s.file.ends_with("a.ts")).unwrap();
		assert!(a.defs >= 2, "a.ts should have at least 2 defs: {a:?}");
		let b = summaries.iter().find(|s| s.file.ends_with("b.ts")).unwrap();
		assert!(b.refs >= 1, "b.ts should have at least 1 ref: {b:?}");
	}

	#[test]
	fn top_kinds_sorted_by_count_desc_then_name() {
		let mut m = BTreeMap::new();
		m.insert("function".to_string(), 5);
		m.insert("class".to_string(), 5);
		m.insert("comment".to_string(), 10);
		assert_eq!(top_kinds(&m, 3), "comment:10, class:5, function:5");
	}

	#[test]
	fn top_kinds_empty_renders_dash() {
		assert_eq!(top_kinds(&BTreeMap::new(), 3), "-");
	}

	impl std::fmt::Debug for FileSummary {
		fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
			f.debug_struct("FileSummary")
				.field("file", &self.file)
				.field("defs", &self.defs)
				.field("refs", &self.refs)
				.finish()
		}
	}
}
