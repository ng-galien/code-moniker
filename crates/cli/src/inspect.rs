use std::collections::{BTreeMap, hash_map::Entry};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use code_moniker_core::core::code_graph::{CodeGraph, DefRecord, RefRecord};
use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::core::shape::Shape;
use code_moniker_core::lang::Lang;
use rayon::prelude::*;
use regex::Regex;
use rustc_hash::FxHashMap;

use crate::cache;
use crate::check;
use crate::lines::line_range;
use crate::sources;

#[derive(Clone, Debug)]
pub struct SessionOptions {
	pub paths: Vec<PathBuf>,
	pub project: Option<String>,
	pub cache_dir: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct DefLocation {
	pub file: usize,
	pub def: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct RefLocation {
	pub file: usize,
	pub reference: usize,
}

#[derive(Clone, Debug)]
pub struct IndexedFile {
	pub path: PathBuf,
	pub rel_path: PathBuf,
	pub lang: Lang,
	pub graph: CodeGraph,
	pub source: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionStats {
	pub files: usize,
	pub defs: usize,
	pub refs: usize,
	pub by_lang: BTreeMap<&'static str, LangTotals>,
	pub by_shape: BTreeMap<&'static str, usize>,
	pub by_def_kind: BTreeMap<String, usize>,
	pub by_ref_kind: BTreeMap<String, usize>,
	pub scan_ms: u64,
	pub extract_ms: u64,
	pub index_ms: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LangTotals {
	pub files: usize,
	pub defs: usize,
	pub refs: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ViewFilter {
	pub kind: Option<String>,
	pub name: Option<String>,
	pub shape: Option<Shape>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CheckSummary {
	pub files_scanned: usize,
	pub files_with_violations: usize,
	pub total_violations: usize,
	pub errors: Vec<CheckError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckError {
	pub path: PathBuf,
	pub error: String,
}

pub struct SessionIndex {
	pub root: String,
	pub files: Vec<IndexedFile>,
	pub stats: SessionStats,
	pub defs_by_moniker: FxHashMap<Moniker, Vec<DefLocation>>,
	pub refs_by_source: FxHashMap<Moniker, Vec<RefLocation>>,
	pub refs_by_target: FxHashMap<Moniker, Vec<RefLocation>>,
	pub children_by_parent: FxHashMap<Moniker, Vec<DefLocation>>,
	pub defs_by_kind: BTreeMap<String, Vec<DefLocation>>,
	pub defs_by_name: BTreeMap<String, Vec<DefLocation>>,
}

impl SessionIndex {
	pub fn load(opts: &SessionOptions) -> anyhow::Result<Self> {
		let started = Instant::now();
		let scan_started = Instant::now();
		let sources = sources::discover(&opts.paths, opts.project.clone())?;
		let scan_ms = millis(scan_started.elapsed());
		let extract_started = Instant::now();
		let mut files: Vec<IndexedFile> = sources
			.files
			.par_iter()
			.map(|f| {
				let ctx = &sources.roots[f.source].ctx;
				let (graph, extracted_source) = cache::load_or_extract_result(
					&f.path,
					&f.anchor,
					f.lang,
					opts.cache_dir.as_deref(),
					ctx,
				)
				.map_err(|e| anyhow::anyhow!("cannot extract {}: {e}", f.path.display()))?;
				let source = match extracted_source {
					Some(source) => source,
					None => std::fs::read_to_string(&f.path)
						.map_err(|e| anyhow::anyhow!("cannot read {}: {e}", f.path.display()))?,
				};
				Ok(IndexedFile {
					path: f.path.clone(),
					rel_path: f.rel_path.clone(),
					lang: f.lang,
					graph,
					source,
				})
			})
			.collect::<anyhow::Result<Vec<_>>>()?;
		files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
		let extract_ms = millis(extract_started.elapsed());
		let index_started = Instant::now();
		let mut idx = Self {
			root: sources.display_path(),
			files,
			stats: SessionStats {
				scan_ms,
				extract_ms,
				..SessionStats::default()
			},
			defs_by_moniker: FxHashMap::default(),
			refs_by_source: FxHashMap::default(),
			refs_by_target: FxHashMap::default(),
			children_by_parent: FxHashMap::default(),
			defs_by_kind: BTreeMap::new(),
			defs_by_name: BTreeMap::new(),
		};
		idx.rebuild_indexes();
		idx.stats.index_ms = millis(index_started.elapsed());
		idx.stats.scan_ms = scan_ms;
		idx.stats.extract_ms = extract_ms;
		if idx.stats.index_ms == 0 && !idx.files.is_empty() {
			idx.stats.index_ms = millis(started.elapsed()).saturating_sub(scan_ms + extract_ms);
		}
		Ok(idx)
	}

	pub fn filtered_defs(&self, filter: &ViewFilter) -> Vec<DefLocation> {
		let name_re = filter.name.as_deref().and_then(|raw| Regex::new(raw).ok());
		let mut out: Vec<DefLocation> = self
			.files
			.iter()
			.enumerate()
			.flat_map(|(file_idx, file)| {
				file.graph
					.defs()
					.enumerate()
					.map(move |(def_idx, _)| DefLocation {
						file: file_idx,
						def: def_idx,
					})
			})
			.filter(|loc| {
				let def = self.def(loc);
				if let Some(kind) = &filter.kind
					&& kind_bytes(def) != kind.as_str()
				{
					return false;
				}
				if let Some(shape) = filter.shape
					&& def.shape() != Some(shape)
				{
					return false;
				}
				if let Some(re) = &name_re
					&& !re.is_match(&last_segment_name(&def.moniker))
				{
					return false;
				}
				true
			})
			.collect();
		out.sort_by(|a, b| self.def(a).moniker.cmp(&self.def(b).moniker));
		out
	}

	pub fn def(&self, loc: &DefLocation) -> &DefRecord {
		self.files[loc.file].graph.def_at(loc.def)
	}

	pub fn reference(&self, loc: &RefLocation) -> &RefRecord {
		self.files[loc.file].graph.ref_at(loc.reference)
	}

	pub fn outgoing_refs(&self, moniker: &Moniker) -> &[RefLocation] {
		self.refs_by_source.get(moniker).map_or(&[], Vec::as_slice)
	}

	pub fn incoming_refs(&self, moniker: &Moniker) -> &[RefLocation] {
		self.refs_by_target.get(moniker).map_or(&[], Vec::as_slice)
	}

	pub fn source_snippet(&self, loc: &DefLocation, context: u32) -> Vec<String> {
		let file = &self.files[loc.file];
		let Some((start, end)) = self.def(loc).position else {
			return Vec::new();
		};
		let (start_line, end_line) = line_range(&file.source, start, end);
		let first = start_line.saturating_sub(context).max(1);
		let last = end_line + context;
		file.source
			.lines()
			.enumerate()
			.filter_map(|(idx, line)| {
				let line_no = idx as u32 + 1;
				(first <= line_no && line_no <= last).then(|| format!("{line_no:>4} {line}"))
			})
			.collect()
	}

	pub fn check_summary(
		&self,
		rules: &Path,
		profile: Option<&str>,
		scheme: &str,
	) -> anyhow::Result<CheckSummary> {
		let mut cfg = check::load_with_overrides(Some(rules))?;
		if let Some(profile) = profile {
			cfg.apply_profile(profile)?;
		}
		let mut compiled: FxHashMap<Lang, check::CompiledRules> = FxHashMap::default();
		for file in &self.files {
			if let Entry::Vacant(entry) = compiled.entry(file.lang) {
				entry.insert(check::compile_rules(&cfg, file.lang, scheme)?);
			}
		}
		let mut summary = CheckSummary {
			files_scanned: self.files.len(),
			..CheckSummary::default()
		};
		for file in &self.files {
			let Some(rules) = compiled.get(&file.lang) else {
				continue;
			};
			let raw = check::evaluate_compiled(&file.graph, &file.source, file.lang, scheme, rules);
			let violations = check::apply_suppressions(&file.graph, &file.source, raw);
			if !violations.is_empty() {
				summary.files_with_violations += 1;
				summary.total_violations += violations.len();
			}
		}
		Ok(summary)
	}

	fn rebuild_indexes(&mut self) {
		self.stats.files = self.files.len();
		self.stats.by_shape = Shape::ALL
			.iter()
			.map(|shape| (shape.as_str(), 0usize))
			.collect();
		for (file_idx, file) in self.files.iter().enumerate() {
			let lang = self.stats.by_lang.entry(file.lang.tag()).or_default();
			lang.files += 1;
			for (def_idx, def) in file.graph.defs().enumerate() {
				let loc = DefLocation {
					file: file_idx,
					def: def_idx,
				};
				self.stats.defs += 1;
				lang.defs += 1;
				let kind = kind_bytes(def);
				*self.stats.by_def_kind.entry(kind.clone()).or_default() += 1;
				let shape = def.shape().unwrap_or(Shape::Value);
				*self.stats.by_shape.entry(shape.as_str()).or_default() += 1;
				self.defs_by_moniker
					.entry(def.moniker.clone())
					.or_default()
					.push(loc);
				self.defs_by_kind.entry(kind).or_default().push(loc);
				self.defs_by_name
					.entry(last_segment_name(&def.moniker))
					.or_default()
					.push(loc);
				if let Some(parent_idx) = def.parent {
					let parent = file.graph.def_at(parent_idx).moniker.clone();
					self.children_by_parent.entry(parent).or_default().push(loc);
				}
			}
			for (ref_idx, reference) in file.graph.refs().enumerate() {
				let loc = RefLocation {
					file: file_idx,
					reference: ref_idx,
				};
				self.stats.refs += 1;
				lang.refs += 1;
				*self
					.stats
					.by_ref_kind
					.entry(
						std::str::from_utf8(&reference.kind)
							.unwrap_or("")
							.to_string(),
					)
					.or_default() += 1;
				*self.stats.by_shape.entry(Shape::Ref.as_str()).or_default() += 1;
				let source = file.graph.def_at(reference.source).moniker.clone();
				self.refs_by_source.entry(source).or_default().push(loc);
				self.refs_by_target
					.entry(reference.target.clone())
					.or_default()
					.push(loc);
			}
		}
		for locs in self.children_by_parent.values_mut() {
			locs.sort_by(|a, b| {
				self.files[a.file]
					.rel_path
					.cmp(&self.files[b.file].rel_path)
			});
		}
	}
}

pub fn last_segment_name(moniker: &Moniker) -> String {
	moniker
		.as_view()
		.segments()
		.last()
		.and_then(|s| std::str::from_utf8(s.name).ok())
		.unwrap_or(".")
		.to_string()
}

fn kind_bytes(def: &DefRecord) -> String {
	std::str::from_utf8(&def.kind).unwrap_or("").to_string()
}

fn millis(d: Duration) -> u64 {
	d.as_millis().try_into().unwrap_or(u64::MAX)
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
	fn session_indexes_defs_refs_and_stats() {
		let tmp = tempfile::tempdir().unwrap();
		write(
			tmp.path(),
			"src/a.ts",
			"export class Foo { bar() { return helper(); } }\nfunction helper() { return 1; }\n",
		);
		let idx = SessionIndex::load(&SessionOptions {
			paths: vec![tmp.path().into()],
			project: Some("app".into()),
			cache_dir: None,
		})
		.unwrap();
		assert_eq!(idx.stats.files, 1);
		assert!(idx.stats.defs >= 3, "{:?}", idx.stats);
		assert!(idx.stats.refs >= 1, "{:?}", idx.stats);
		assert!(idx.defs_by_kind.contains_key("class"));
		assert!(idx.defs_by_name.contains_key("Foo"));
		let foo = idx
			.filtered_defs(&ViewFilter {
				name: Some("^Foo$".into()),
				..ViewFilter::default()
			})
			.pop()
			.expect("Foo def");
		assert_eq!(last_segment_name(&idx.def(&foo).moniker), "Foo");
		assert!(!idx.source_snippet(&foo, 1).is_empty());
	}

	#[test]
	fn check_summary_uses_loaded_graphs() {
		let tmp = tempfile::tempdir().unwrap();
		write(tmp.path(), "src/a.ts", "export class Foo {}\n");
		write(
			tmp.path(),
			".code-moniker.toml",
			r#"
[[ts.class.where]]
id = "max-lines"
expr = "lines <= 0"
message = "class too long"
"#,
		);
		let idx = SessionIndex::load(&SessionOptions {
			paths: vec![tmp.path().join("src")],
			project: Some("app".into()),
			cache_dir: None,
		})
		.unwrap();
		let summary = idx
			.check_summary(
				&tmp.path().join(".code-moniker.toml"),
				None,
				"code+moniker://",
			)
			.unwrap();
		assert_eq!(summary.files_scanned, 1);
		assert_eq!(summary.total_violations, 1);
		assert_eq!(summary.files_with_violations, 1);
	}

	#[test]
	fn session_loads_multiple_roots_with_prefixed_relative_paths() {
		let tmp = tempfile::tempdir().unwrap();
		let service_a = tmp.path().join("service-a");
		let service_b = tmp.path().join("service-b");
		write(&service_a, "src/a.ts", "export class Alpha {}\n");
		write(&service_b, "src/b.ts", "export class Beta {}\n");

		let idx = SessionIndex::load(&SessionOptions {
			paths: vec![service_a.clone(), service_b.clone()],
			project: None,
			cache_dir: None,
		})
		.unwrap();

		assert_eq!(idx.stats.files, 2);
		assert!(idx.root.contains("service-a"), "{}", idx.root);
		assert!(idx.root.contains("service-b"), "{}", idx.root);
		assert!(
			idx.files
				.iter()
				.any(|file| file.rel_path.as_path() == Path::new("service-a/src/a.ts"))
		);
		assert!(
			idx.files
				.iter()
				.any(|file| file.rel_path.as_path() == Path::new("service-b/src/b.ts"))
		);
		let alpha = idx
			.filtered_defs(&ViewFilter {
				name: Some("^Alpha$".into()),
				..ViewFilter::default()
			})
			.pop()
			.expect("Alpha def");
		let project =
			std::str::from_utf8(idx.def(&alpha).moniker.as_view().project()).unwrap_or("");
		assert_eq!(project, "service-a");
	}

	#[test]
	fn session_keeps_ts_path_aliases_inside_their_multi_source_root() {
		let tmp = tempfile::tempdir().unwrap();
		let service_a = tmp.path().join("service-a");
		let service_b = tmp.path().join("service-b");
		write(
			&service_a,
			"tsconfig.json",
			r#"{"compilerOptions": {"paths": {"@/*": ["./src/*"]}}}"#,
		);
		write(
			&service_a,
			"src/router.ts",
			r#"import { Foo } from "@/foo"; export const value = Foo;"#,
		);
		write(&service_a, "src/foo.ts", "export class Foo {}\n");
		write(&service_b, "src/foo.ts", "export class Foo {}\n");

		let idx = SessionIndex::load(&SessionOptions {
			paths: vec![service_a, service_b],
			project: None,
			cache_dir: None,
		})
		.unwrap();

		let expected = code_moniker_core::core::moniker::MonikerBuilder::new()
			.project(b"service-a")
			.segment(b"lang", b"ts")
			.segment(b"dir", b"service-a")
			.segment(b"dir", b"src")
			.segment(b"module", b"foo")
			.segment(b"path", b"Foo")
			.build();
		assert!(
			idx.files
				.iter()
				.flat_map(|file| file.graph.refs())
				.any(
					|reference| reference.kind == b"imports_symbol" && reference.target == expected
				),
			"alias import should target service-a's prefixed module"
		);
	}
}
