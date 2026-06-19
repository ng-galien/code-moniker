use std::cmp::Ordering;
use std::io::Write;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use regex::Regex;
use serde::Serialize;

use code_moniker_workspace::glob::FilePathFilter;

use crate::args::{ExtractArgs, OutputFormat, OutputMode};
use crate::language_kinds;
use crate::page::{PageInfo, PageSpec};
use code_moniker_core::core::code_graph::{DefRecord, RefRecord};
use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;
use code_moniker_workspace::environment::{self, SourceFile, SourceFileSet};

use super::filter::{self, MatchSet, Predicate, RefMatch};
use super::format;

pub fn run<W1: Write, W2: Write>(
	args: &ExtractArgs,
	stdout: &mut W1,
	stderr: &mut W2,
	root: &Path,
	scheme: &str,
) -> anyhow::Result<bool> {
	let sources = environment::discover_sources(&[root.to_path_buf()], args.project.clone())?;
	run_filter(args, stdout, stderr, &sources, root, scheme)
}

fn run_filter<W1: Write, W2: Write>(
	args: &ExtractArgs,
	stdout: &mut W1,
	stderr: &mut W2,
	sources: &SourceFileSet,
	root: &Path,
	scheme: &str,
) -> anyhow::Result<bool> {
	let predicates = args.compiled_predicates(scheme)?;
	let names = filter::compile_name_filters(&args.name)?;
	let path_filter = FilePathFilter::compile(&args.path_filter)?;
	let scoped_files: Vec<&SourceFile> = sources
		.files
		.iter()
		.filter(|file| path_filter.matches(&file.rel_path))
		.collect();
	let mut langs: Vec<Lang> = scoped_files.iter().map(|f| f.lang).collect();
	langs.sort_by_key(|l| l.tag());
	langs.dedup();
	let known = language_kinds::known_kinds(langs.iter());
	let unknown = language_kinds::unknown_kinds(&args.kind, &known);
	if !scoped_files.is_empty() && !unknown.is_empty() {
		return Err(super::unknown_kinds_error(&unknown, &langs, &known));
	}
	let filter = DirectoryFilter {
		sources,
		root,
		predicates: &predicates,
		kinds: &args.kind,
		names: &names,
		shapes: &args.shape,
		cache_dir: args.cache.as_deref(),
	};
	let mut rows: Vec<FilterRow> = scoped_files
		.par_iter()
		.filter_map(|file| compute_filter_row(&filter, file))
		.collect();
	rows.sort_by(|a, b| a.rel.cmp(&b.rel));
	let total_defs: usize = rows.iter().map(|r| r.defs.len()).sum();
	let total_refs: usize = rows.iter().map(|r| r.refs.len()).sum();
	let any = total_defs + total_refs > 0;
	match args.mode() {
		OutputMode::Default => {
			if super::uses_tree_visibility(args) {
				apply_tree_visibility(&mut rows, args);
			}
			let page = paginate_rows(&mut rows, args, scheme)?;
			match args.format {
				OutputFormat::Text => write_filter_text(stdout, &rows, args, scheme)?,
				OutputFormat::Tsv => write_filter_tsv(stdout, &rows, args, scheme)?,
				OutputFormat::Json => write_filter_json(stdout, &rows, args, scheme, &page)?,
				#[cfg(feature = "pretty")]
				OutputFormat::Tree => write_filter_tree(stdout, &rows, args, scheme)?,
			}
			super::write_page_notice(stderr, args, &page)?;
			Ok(page.emitted > 0)
		}
		OutputMode::Count => {
			writeln!(stdout, "{}", total_defs + total_refs)?;
			Ok(any)
		}
		OutputMode::Quiet => Ok(any),
	}
}

fn apply_tree_visibility(rows: &mut [FilterRow], args: &ExtractArgs) {
	if !args.kind.is_empty() {
		return;
	}
	for row in rows {
		row.defs
			.retain(|def| super::tree_visible_def_kind(&def.kind));
		row.refs.clear();
	}
}

struct FilterRow {
	rel: PathBuf,
	lang: Lang,
	source: String,
	defs: Vec<DefRecord>,
	refs: Vec<(RefRecord, Moniker)>,
}

struct DirectoryFilter<'a> {
	sources: &'a SourceFileSet,
	root: &'a Path,
	predicates: &'a [Predicate],
	kinds: &'a [String],
	names: &'a [Regex],
	shapes: &'a [code_moniker_core::core::shape::Shape],
	cache_dir: Option<&'a Path>,
}

fn compute_filter_row(filter: &DirectoryFilter<'_>, file: &SourceFile) -> Option<FilterRow> {
	let ctx = &filter.sources.roots[file.source].ctx;
	let rel = file
		.path
		.strip_prefix(filter.root)
		.unwrap_or(&file.path)
		.to_path_buf();
	let (graph, extracted_source) = environment::load_or_extract_source(
		&file.path,
		&file.anchor,
		file.lang,
		filter.cache_dir,
		ctx,
	)
	.ok()?;
	let matches = filter::filter(
		&graph,
		filter.predicates,
		filter.kinds,
		filter.names,
		filter.shapes,
	);
	if matches.defs.is_empty() && matches.refs.is_empty() {
		return None;
	}
	let source = match extracted_source {
		Some(s) => s,
		None => std::fs::read_to_string(&file.path).ok()?,
	};
	let defs = matches.defs.into_iter().cloned().collect();
	let refs = matches
		.refs
		.into_iter()
		.map(|rm| (rm.record.clone(), rm.source.clone()))
		.collect();
	Some(FilterRow {
		rel,
		lang: file.lang,
		source,
		defs,
		refs,
	})
}

impl FilterRow {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RowRecordRef {
	row: usize,
	record: RowRecord,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RowMatchItem {
	cursor: Moniker,
	record_ref: RowRecordRef,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RowRecord {
	Def(usize),
	Ref(usize),
}

fn paginate_rows(
	rows: &mut Vec<FilterRow>,
	args: &ExtractArgs,
	scheme: &str,
) -> anyhow::Result<PageInfo> {
	let spec = PageSpec::from_args(args, scheme)?;
	let mut items = Vec::new();
	let mut ordinal = 0usize;
	for (row_idx, row) in rows.iter().enumerate() {
		for (def_idx, def) in row.defs.iter().enumerate() {
			let cursor = crate::page::def_cursor_moniker(
				&def.moniker,
				row.rel.to_string_lossy().as_bytes(),
				&def.kind,
				def.position,
				ordinal,
			);
			if spec.allows(&cursor) {
				items.push(RowMatchItem {
					cursor,
					record_ref: RowRecordRef {
						row: row_idx,
						record: RowRecord::Def(def_idx),
					},
				});
			}
			ordinal += 1;
		}
		for (ref_idx, (record, source)) in row.refs.iter().enumerate() {
			let cursor = crate::page::ref_cursor_moniker(
				&record.target,
				row.rel.to_string_lossy().as_bytes(),
				source,
				&record.kind,
				record.position,
				ordinal,
			);
			if spec.allows(&cursor) {
				items.push(RowMatchItem {
					cursor,
					record_ref: RowRecordRef {
						row: row_idx,
						record: RowRecord::Ref(ref_idx),
					},
				});
			}
			ordinal += 1;
		}
	}
	items.sort_by(|a, b| cmp_row_match_item(rows, a, b));

	let total = items.len();
	let page_len = spec.page_len(total);
	let last = page_len
		.checked_sub(1)
		.and_then(|idx| items.get(idx))
		.map(|item| &item.cursor);
	let info = spec.info(total, page_len, last, scheme);
	let mut keep_defs: Vec<Vec<bool>> =
		rows.iter().map(|row| vec![false; row.defs.len()]).collect();
	let mut keep_refs: Vec<Vec<bool>> =
		rows.iter().map(|row| vec![false; row.refs.len()]).collect();
	for item in items.iter().take(page_len) {
		match item.record_ref.record {
			RowRecord::Def(idx) => keep_defs[item.record_ref.row][idx] = true,
			RowRecord::Ref(idx) => keep_refs[item.record_ref.row][idx] = true,
		}
	}
	for (row_idx, row) in rows.iter_mut().enumerate() {
		let defs = std::mem::take(&mut row.defs);
		row.defs = defs
			.into_iter()
			.enumerate()
			.filter_map(|(idx, def)| keep_defs[row_idx][idx].then_some(def))
			.collect();
		let refs = std::mem::take(&mut row.refs);
		row.refs = refs
			.into_iter()
			.enumerate()
			.filter_map(|(idx, ref_record)| keep_refs[row_idx][idx].then_some(ref_record))
			.collect();
	}
	rows.retain(|row| !row.defs.is_empty() || !row.refs.is_empty());
	Ok(info)
}

fn cmp_row_match_item(rows: &[FilterRow], a: &RowMatchItem, b: &RowMatchItem) -> Ordering {
	a.cursor
		.as_encoded()
		.cmp(b.cursor.as_encoded())
		.then_with(|| rows[a.record_ref.row].rel.cmp(&rows[b.record_ref.row].rel))
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

fn write_filter_text<W: Write>(
	w: &mut W,
	rows: &[FilterRow],
	args: &ExtractArgs,
	scheme: &str,
) -> std::io::Result<()> {
	for row in rows {
		let matches = row.match_set();
		format::write_text(w, &matches, args, scheme)?;
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
	let entries: Vec<crate::tree::FileEntry<'_>> = rows
		.iter()
		.map(|row| crate::tree::FileEntry {
			rel_path: row.rel.to_string_lossy().into_owned(),
			matches: row.match_set(),
			source: row.source.as_str(),
		})
		.collect();
	crate::tree::write_files_tree(w, &entries, args, scheme)?;
	Ok(())
}

fn write_filter_json<W: Write>(
	w: &mut W,
	rows: &[FilterRow],
	args: &ExtractArgs,
	scheme: &str,
	page: &PageInfo,
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
			Ok(Entry {
				file: row.rel.display().to_string(),
				lang: row.lang.tag(),
				matches: format::build_matches_value(&matches, &row.source, args, scheme)?,
			})
		})
		.collect::<anyhow::Result<_>>()?;
	let total_defs: usize = rows.iter().map(|r| r.defs.len()).sum();
	let total_refs: usize = rows.iter().map(|r| r.refs.len()).sum();
	#[derive(Serialize)]
	struct Out<'a> {
		emitted_files: usize,
		emitted_defs: usize,
		emitted_refs: usize,
		files: Vec<Entry>,
		#[serde(skip_serializing_if = "Option::is_none")]
		next_cursor: Option<&'a str>,
		#[serde(skip_serializing_if = "Option::is_none")]
		remaining: Option<usize>,
	}
	let out = Out {
		emitted_files: entries.len(),
		emitted_defs: total_defs,
		emitted_refs: total_refs,
		files: entries,
		next_cursor: page.next_cursor.as_deref(),
		remaining: (page.remaining > 0).then_some(page.remaining),
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

	fn test_filter<'a>(sources: &'a SourceFileSet, root: &'a Path) -> DirectoryFilter<'a> {
		DirectoryFilter {
			sources,
			root,
			predicates: &[],
			kinds: &[],
			names: &[],
			shapes: &[],
			cache_dir: None,
		}
	}

	#[test]
	fn filter_row_computes_file_matches() {
		let tmp = tempfile::tempdir().unwrap();
		let root = tmp.path();
		write_file(root, "a.ts", "export class Foo {}\nfunction bar() {}\n");
		let sources = environment::discover_sources(&[root.to_path_buf()], None).unwrap();
		let filter = test_filter(&sources, root);
		let f = sources
			.files
			.iter()
			.find(|f| f.path.ends_with("a.ts"))
			.unwrap();
		let row = compute_filter_row(&filter, f).unwrap();
		assert!(row.defs.len() >= 2, "a.ts should have defs");
	}

	#[test]
	fn path_filter_keeps_root_context_while_limiting_files() {
		let tmp = tempfile::tempdir().unwrap();
		let root = tmp.path();
		write_file(root, "pkg/src/a.ts", "export class Alpha {}\n");
		write_file(root, "other/b.ts", "export class Beta {}\n");
		let sources = environment::discover_sources(&[root.to_path_buf()], None).unwrap();
		let mut args = ExtractArgs::for_tests();
		args.path = root.to_path_buf();
		args.format = OutputFormat::Json;
		args.path_filter = vec!["pkg/src/**".to_string()];
		let mut out = Vec::new();
		let mut err = Vec::new();
		let any = run_filter(&args, &mut out, &mut err, &sources, root, "code+moniker://").unwrap();
		assert!(any);
		let text = String::from_utf8(out).unwrap();
		assert!(text.contains("\"file\": \"pkg/src/a.ts\""), "{text}");
		assert!(!text.contains("other/b.ts"), "{text}");
		assert!(text.contains("dir:pkg/dir:src"), "{text}");
	}

	#[test]
	fn pagination_applies_a_global_limit_across_files() {
		let tmp = tempfile::tempdir().unwrap();
		let root = tmp.path();
		write_file(root, "a.ts", "export class Alpha {}\n");
		write_file(root, "b.ts", "export class Beta {}\n");
		let sources = environment::discover_sources(&[root.to_path_buf()], None).unwrap();
		let filter = test_filter(&sources, root);
		let mut rows: Vec<FilterRow> = sources
			.files
			.iter()
			.filter_map(|f| compute_filter_row(&filter, f))
			.collect();
		let mut args = ExtractArgs::for_tests();
		args.limit = 1;
		let page = paginate_rows(&mut rows, &args, "code+moniker://").unwrap();
		let emitted: usize = rows.iter().map(|row| row.defs.len() + row.refs.len()).sum();
		assert_eq!(emitted, 1);
		assert_eq!(page.emitted, 1);
		assert!(page.remaining > 0);
		assert!(page.next_cursor.is_some());
	}
}
