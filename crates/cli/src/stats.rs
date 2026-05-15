use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[cfg(feature = "pretty")]
use anstyle::{AnsiColor, Style};
use code_moniker_core::core::shape::Shape;
use rayon::prelude::*;
use serde::Serialize;

use crate::Exit;
#[cfg(feature = "pretty")]
use crate::args::{Charset, ColorChoice};
use crate::args::{StatsArgs, StatsFormat};
use crate::cache;
use crate::extract;
use crate::lang::path_to_lang;
use crate::tsconfig;
use crate::walk::{self, WalkedFile};

pub fn run<W1: Write, W2: Write>(args: &StatsArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	match stats_inner(args, stdout) {
		Ok(any) => {
			if any {
				Exit::Match
			} else {
				Exit::NoMatch
			}
		}
		Err(e) => {
			let _ = writeln!(stderr, "code-moniker: {e:#}");
			Exit::UsageError
		}
	}
}

fn stats_inner<W: Write>(args: &StatsArgs, stdout: &mut W) -> anyhow::Result<bool> {
	let started = Instant::now();
	let path = &args.path;
	let meta = std::fs::metadata(path)
		.map_err(|e| anyhow::anyhow!("cannot stat {}: {e}", path.display()))?;
	let scan_started = Instant::now();
	let (root, files) = if meta.is_dir() {
		(path.as_path(), walk::walk_lang_files(path))
	} else {
		let lang = path_to_lang(path)?;
		let root = path.parent().unwrap_or_else(|| Path::new("."));
		(
			root,
			vec![WalkedFile {
				path: path.to_path_buf(),
				lang,
			}],
		)
	};
	let scan_elapsed = scan_started.elapsed();
	let ctx = extract::Context {
		ts: tsconfig::load(root),
		project: args.project.clone(),
	};
	let extract_started = Instant::now();
	let cache_dir = args.cache.as_deref();
	let file_stats: Vec<FileStats> = files
		.par_iter()
		.map(|f| FileStats::compute(f, root, cache_dir, &ctx))
		.collect::<anyhow::Result<Vec<_>>>()?;
	let extract_elapsed = extract_started.elapsed();
	let report = StatsReport::from_files(
		path,
		file_stats,
		scan_elapsed,
		extract_elapsed,
		started.elapsed(),
	);
	match args.format {
		StatsFormat::Tsv => write_tsv(stdout, &report)?,
		StatsFormat::Json => {
			serde_json::to_writer_pretty(&mut *stdout, &report)?;
			stdout.write_all(b"\n")?;
		}
		#[cfg(feature = "pretty")]
		StatsFormat::Tree => write_tree(stdout, &report, args)?,
	}
	Ok(report.total_files > 0)
}

#[derive(Serialize)]
struct StatsReport {
	path: String,
	total_files: usize,
	total_defs: usize,
	total_refs: usize,
	total_records: usize,
	timings: Timings,
	by_lang: BTreeMap<&'static str, LangStats>,
	by_shape: BTreeMap<&'static str, usize>,
	by_kind: KindStats,
}

impl StatsReport {
	fn from_files(
		path: &Path,
		files: Vec<FileStats>,
		scan_elapsed: Duration,
		extract_elapsed: Duration,
		total_elapsed: Duration,
	) -> Self {
		let mut by_lang: BTreeMap<&'static str, LangStats> = BTreeMap::new();
		let mut by_shape: BTreeMap<&'static str, usize> = Shape::ALL
			.iter()
			.map(|shape| (shape.as_str(), 0usize))
			.collect();
		let mut by_kind = KindStats::default();
		let mut total_defs = 0usize;
		let mut total_refs = 0usize;
		for file in &files {
			let lang = by_lang.entry(file.lang).or_default();
			lang.files += 1;
			lang.defs += file.defs;
			lang.refs += file.refs;
			total_defs += file.defs;
			total_refs += file.refs;
			merge_counts(&mut by_shape, &file.by_shape);
			merge_counts(&mut by_kind.defs, &file.by_def_kind);
			merge_counts(&mut by_kind.refs, &file.by_ref_kind);
		}
		Self {
			path: path.display().to_string(),
			total_files: files.len(),
			total_defs,
			total_refs,
			total_records: total_defs + total_refs,
			timings: Timings {
				scan_ms: millis(scan_elapsed),
				extract_ms: millis(extract_elapsed),
				total_ms: millis(total_elapsed),
			},
			by_lang,
			by_shape,
			by_kind,
		}
	}
}

#[derive(Default, Serialize)]
struct LangStats {
	files: usize,
	defs: usize,
	refs: usize,
}

#[derive(Default, Serialize)]
struct KindStats {
	defs: BTreeMap<String, usize>,
	refs: BTreeMap<String, usize>,
}

#[derive(Serialize)]
struct Timings {
	scan_ms: u64,
	extract_ms: u64,
	total_ms: u64,
}

struct FileStats {
	lang: &'static str,
	defs: usize,
	refs: usize,
	by_shape: BTreeMap<&'static str, usize>,
	by_def_kind: BTreeMap<String, usize>,
	by_ref_kind: BTreeMap<String, usize>,
}

impl FileStats {
	fn compute(
		file: &WalkedFile,
		root: &Path,
		cache_dir: Option<&Path>,
		ctx: &extract::Context,
	) -> anyhow::Result<Self> {
		let rel: PathBuf = file
			.path
			.strip_prefix(root)
			.unwrap_or(&file.path)
			.to_path_buf();
		let (graph, _) = cache::load_or_extract_result(&file.path, &rel, file.lang, cache_dir, ctx)
			.map_err(|e| anyhow::anyhow!("cannot extract {}: {e}", file.path.display()))?;
		let mut defs = 0usize;
		let mut refs = 0usize;
		let mut by_shape: BTreeMap<&'static str, usize> = Shape::ALL
			.iter()
			.map(|shape| (shape.as_str(), 0usize))
			.collect();
		let mut by_def_kind = BTreeMap::new();
		let mut by_ref_kind = BTreeMap::new();
		for def in graph.defs() {
			defs += 1;
			let shape = Shape::for_kind(&def.kind).as_str();
			*by_shape.entry(shape).or_default() += 1;
			bump_kind(&mut by_def_kind, &def.kind);
		}
		for reference in graph.refs() {
			refs += 1;
			*by_shape.entry(Shape::Ref.as_str()).or_default() += 1;
			bump_kind(&mut by_ref_kind, &reference.kind);
		}
		Ok(Self {
			lang: file.lang.tag(),
			defs,
			refs,
			by_shape,
			by_def_kind,
			by_ref_kind,
		})
	}
}

fn write_tsv<W: Write>(w: &mut W, report: &StatsReport) -> std::io::Result<()> {
	writeln!(w, "path\t{}", report.path)?;
	writeln!(w, "files\t{}", report.total_files)?;
	writeln!(w, "defs\t{}", report.total_defs)?;
	writeln!(w, "refs\t{}", report.total_refs)?;
	writeln!(w, "records\t{}", report.total_records)?;
	writeln!(w, "timing\tscan_ms\t{}", report.timings.scan_ms)?;
	writeln!(w, "timing\textract_ms\t{}", report.timings.extract_ms)?;
	writeln!(w, "timing\ttotal_ms\t{}", report.timings.total_ms)?;
	for (lang, stats) in &report.by_lang {
		writeln!(
			w,
			"lang\t{lang}\tfiles\t{}\tdefs\t{}\trefs\t{}",
			stats.files, stats.defs, stats.refs
		)?;
	}
	for (shape, count) in &report.by_shape {
		writeln!(w, "shape\t{shape}\t{count}")?;
	}
	for (kind, count) in &report.by_kind.defs {
		writeln!(w, "def_kind\t{kind}\t{count}")?;
	}
	for (kind, count) in &report.by_kind.refs {
		writeln!(w, "ref_kind\t{kind}\t{count}")?;
	}
	Ok(())
}

#[cfg(feature = "pretty")]
fn write_tree<W: Write>(w: &mut W, report: &StatsReport, args: &StatsArgs) -> std::io::Result<()> {
	let opts = StatsTreeOpts::from_args(args);
	let root = style(&opts.palette.title, "stats");
	let path = style(&opts.palette.dim, &report.path);
	writeln!(w, "{root} {path}")?;
	let rows = [
		(
			"files",
			format!("{}", report.total_files),
			Some(format!("{} supported source files", report.total_files)),
		),
		(
			"records",
			format!("{}", report.total_records),
			Some(format!(
				"{} defs, {} refs",
				report.total_defs, report.total_refs
			)),
		),
		(
			"time",
			format!("{} ms", report.timings.total_ms),
			Some(format!(
				"scan {} ms, extract {} ms",
				report.timings.scan_ms, report.timings.extract_ms
			)),
		),
	];
	for (idx, (label, value, detail)) in rows.iter().enumerate() {
		let last = idx + 1 == rows.len() && report.by_lang.is_empty();
		write_metric_row(w, "", last, label, value, detail.as_deref(), &opts)?;
	}
	write_section(
		w,
		"languages",
		&report
			.by_lang
			.iter()
			.map(|(lang, s)| {
				(
					*lang,
					format!("{} files", s.files),
					Some(format!("{} defs, {} refs", s.defs, s.refs)),
				)
			})
			.collect::<Vec<_>>(),
		false,
		&opts,
	)?;
	write_count_section(w, "shapes", &report.by_shape, false, &opts)?;
	write_count_section(w, "def kinds", &report.by_kind.defs, false, &opts)?;
	write_count_section(w, "ref kinds", &report.by_kind.refs, true, &opts)?;
	Ok(())
}

#[cfg(feature = "pretty")]
fn write_section<W: Write>(
	w: &mut W,
	name: &str,
	rows: &[(&str, String, Option<String>)],
	last_section: bool,
	opts: &StatsTreeOpts,
) -> std::io::Result<()> {
	let (branch, child_prefix) = branch("", last_section, opts);
	writeln!(w, "{branch}{}", style(&opts.palette.section, name))?;
	for (idx, (label, value, detail)) in rows.iter().enumerate() {
		write_metric_row(
			w,
			&child_prefix,
			idx + 1 == rows.len(),
			label,
			value,
			detail.as_deref(),
			opts,
		)?;
	}
	Ok(())
}

#[cfg(feature = "pretty")]
fn write_count_section<W: Write, K: AsRef<str>>(
	w: &mut W,
	name: &str,
	counts: &BTreeMap<K, usize>,
	last_section: bool,
	opts: &StatsTreeOpts,
) -> std::io::Result<()> {
	let rows: Vec<(&str, String, Option<String>)> = counts
		.iter()
		.map(|(k, v)| (k.as_ref(), v.to_string(), None))
		.collect();
	write_section(w, name, &rows, last_section, opts)
}

#[cfg(feature = "pretty")]
fn write_metric_row<W: Write>(
	w: &mut W,
	prefix: &str,
	last: bool,
	label: &str,
	value: &str,
	detail: Option<&str>,
	opts: &StatsTreeOpts,
) -> std::io::Result<()> {
	let (branch, _) = branch(prefix, last, opts);
	write!(
		w,
		"{branch}{} {}",
		style(&opts.palette.label, label),
		style(&opts.palette.value, value)
	)?;
	if let Some(detail) = detail {
		write!(w, " {}", style(&opts.palette.dim, detail))?;
	}
	writeln!(w)
}

#[cfg(feature = "pretty")]
fn branch(prefix: &str, last: bool, opts: &StatsTreeOpts) -> (String, String) {
	if last {
		(
			format!("{prefix}{} ", opts.glyph.last),
			format!("{prefix}{}", opts.glyph.skip_last),
		)
	} else {
		(
			format!("{prefix}{} ", opts.glyph.tee),
			format!("{prefix}{}", opts.glyph.skip_mid),
		)
	}
}

#[cfg(feature = "pretty")]
struct StatsTreeOpts {
	glyph: StatsGlyphs,
	palette: StatsPalette,
}

#[cfg(feature = "pretty")]
impl StatsTreeOpts {
	fn from_args(args: &StatsArgs) -> Self {
		let glyph = match args.charset {
			Charset::Utf8 => StatsGlyphs::utf8(),
			Charset::Ascii => StatsGlyphs::ascii(),
		};
		let palette = if resolve_color(args.color) {
			StatsPalette::ansi()
		} else {
			StatsPalette::none()
		};
		Self { glyph, palette }
	}
}

#[cfg(feature = "pretty")]
struct StatsGlyphs {
	tee: &'static str,
	last: &'static str,
	skip_mid: &'static str,
	skip_last: &'static str,
}

#[cfg(feature = "pretty")]
impl StatsGlyphs {
	fn utf8() -> Self {
		Self {
			tee: "├──",
			last: "└──",
			skip_mid: "│   ",
			skip_last: "    ",
		}
	}

	fn ascii() -> Self {
		Self {
			tee: "+--",
			last: "+--",
			skip_mid: "|   ",
			skip_last: "    ",
		}
	}
}

#[cfg(feature = "pretty")]
struct StatsPalette {
	title: Style,
	section: Style,
	label: Style,
	value: Style,
	dim: Style,
}

#[cfg(feature = "pretty")]
impl StatsPalette {
	fn none() -> Self {
		Self {
			title: Style::new(),
			section: Style::new(),
			label: Style::new(),
			value: Style::new(),
			dim: Style::new(),
		}
	}

	fn ansi() -> Self {
		Self {
			title: Style::new().bold().fg_color(Some(AnsiColor::Cyan.into())),
			section: Style::new().bold().fg_color(Some(AnsiColor::Blue.into())),
			label: Style::new().fg_color(Some(AnsiColor::Magenta.into())),
			value: Style::new().bold().fg_color(Some(AnsiColor::Green.into())),
			dim: Style::new()
				.fg_color(Some(AnsiColor::BrightBlack.into()))
				.dimmed(),
		}
	}
}

#[cfg(feature = "pretty")]
fn resolve_color(arg: ColorChoice) -> bool {
	use std::io::IsTerminal;
	if std::env::var_os("NO_COLOR").is_some() {
		return false;
	}
	if std::env::var_os("CLICOLOR_FORCE").is_some_and(|v| v != "0") {
		return true;
	}
	match arg {
		ColorChoice::Always => true,
		ColorChoice::Never => false,
		ColorChoice::Auto => {
			if std::env::var("TERM").is_ok_and(|t| t == "dumb") {
				return false;
			}
			if std::env::var("CLICOLOR").is_ok_and(|v| v == "0") {
				return false;
			}
			std::io::stdout().is_terminal()
		}
	}
}

#[cfg(feature = "pretty")]
fn style(style: &Style, text: &str) -> String {
	let start = style.render().to_string();
	let end = style.render_reset().to_string();
	if start.is_empty() && end.is_empty() {
		text.to_string()
	} else {
		format!("{start}{text}{end}")
	}
}

fn bump_kind(map: &mut BTreeMap<String, usize>, kind: &[u8]) {
	let key = std::str::from_utf8(kind).unwrap_or("").to_owned();
	*map.entry(key).or_default() += 1;
}

fn merge_counts<K: Ord + Clone>(target: &mut BTreeMap<K, usize>, source: &BTreeMap<K, usize>) {
	for (key, count) in source {
		*target.entry(key.clone()).or_default() += count;
	}
}

fn millis(duration: Duration) -> u64 {
	duration.as_millis().try_into().unwrap_or(u64::MAX)
}
