// code-moniker: ignore-file[smell-feature-envy-local]
// TODO(smell): split single-file extraction, pagination, and tree visibility.
use std::cmp::Ordering;
use std::io::Write;
use std::path::Path;

use code_moniker_core::core::code_graph::{DefRecord, RefRecord};
use code_moniker_core::core::kinds::{KIND_COMMENT, KIND_LOCAL, KIND_PARAM};
use code_moniker_core::core::moniker::Moniker;
use code_moniker_workspace::environment;

use crate::args::{ExtractArgs, OutputFormat, OutputMode};
use crate::{Exit, format, language_kinds, page, path_to_lang};

mod directory;
pub(crate) mod filter;

pub use filter::{MatchSet, Predicate, RefMatch};

fn unknown_kinds_error(
	unknown: &[String],
	langs: &[code_moniker_core::lang::Lang],
	known: &std::collections::BTreeSet<&'static str>,
) -> anyhow::Error {
	let lang_tags: Vec<&str> = langs.iter().map(|l| l.tag()).collect();
	let known_list: Vec<&str> = known.iter().copied().collect();
	anyhow::anyhow!(
		"unknown --kind {} (langs in scope: {}; known kinds: {})",
		unknown.join(", "),
		lang_tags.join(", "),
		known_list.join(", "),
	)
}

pub fn run<W1: Write, W2: Write>(args: &ExtractArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	match extract_inner(args, stdout, stderr) {
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

fn extract_inner<W1: Write, W2: Write>(
	args: &ExtractArgs,
	stdout: &mut W1,
	stderr: &mut W2,
) -> anyhow::Result<bool> {
	let path: &Path = &args.path;
	let scheme = args
		.scheme
		.as_deref()
		.unwrap_or(crate::DEFAULT_SCHEME)
		.to_string();
	let meta = std::fs::metadata(path)
		.map_err(|e| anyhow::anyhow!("cannot stat {}: {e}", path.display()))?;
	if meta.is_dir() {
		return directory::run(args, stdout, stderr, path, &scheme);
	}
	let lang = path_to_lang(path)?;
	let predicates = args.compiled_predicates(&scheme)?;
	let names = filter::compile_name_filters(&args.name)?;
	let known = language_kinds::known_kinds(std::iter::once(&lang));
	let unknown = language_kinds::unknown_kinds(&args.kind, &known);
	if !unknown.is_empty() {
		return Err(unknown_kinds_error(&unknown, &[lang], &known));
	}
	let sources = environment::discover_sources(&[path.to_path_buf()], args.project.clone())?;
	let file = sources
		.files
		.first()
		.ok_or_else(|| anyhow::anyhow!("cannot read {}", path.display()))?;
	let ctx = &sources.roots[file.source].ctx;
	let (graph, extracted_source) = environment::load_or_extract_source(
		&file.path,
		&file.anchor,
		file.lang,
		args.cache.as_deref(),
		ctx,
	)
	.map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
	let source = match extracted_source {
		Some(s) => s,
		None => std::fs::read_to_string(path)
			.map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?,
	};
	let matches = filter::filter(&graph, &predicates, &args.kind, &names, &args.shape);
	match args.mode() {
		OutputMode::Default => {
			let visible;
			let page_source = if uses_tree_visibility(args) {
				visible = tree_visible_match_set(&matches, args);
				&visible
			} else {
				&matches
			};
			let (paged, page) = paginate_match_set(page_source, args, &scheme)?;
			match args.format {
				OutputFormat::Text => format::write_text(stdout, &paged, args, &scheme)?,
				OutputFormat::Tsv => format::write_tsv(stdout, &paged, &source, args, &scheme)?,
				OutputFormat::Json => format::write_json(
					stdout,
					&paged,
					&source,
					args,
					format::JsonContext {
						lang: file.lang,
						path,
						scheme: &scheme,
						page: &page,
					},
				)?,
				#[cfg(feature = "pretty")]
				OutputFormat::Tree => crate::tree::write_tree(stdout, &paged, &source, args, &scheme)?,
			}
			write_page_notice(stderr, args, &page)?;
			Ok(page.emitted > 0)
		}
		OutputMode::Count => {
			let n = matches.defs.len() + matches.refs.len();
			writeln!(stdout, "{n}")?;
			Ok(n > 0)
		}
		OutputMode::Quiet => Ok(!matches.defs.is_empty() || !matches.refs.is_empty()),
	}
}

pub(super) fn uses_tree_visibility(args: &ExtractArgs) -> bool {
	#[cfg(feature = "pretty")]
	{
		args.format == OutputFormat::Tree
	}
	#[cfg(not(feature = "pretty"))]
	{
		let _ = args;
		false
	}
}

fn tree_visible_match_set<'g>(matches: &MatchSet<'g>, args: &ExtractArgs) -> MatchSet<'g> {
	if !args.kind.is_empty() {
		return MatchSet {
			defs: matches.defs.clone(),
			refs: matches
				.refs
				.iter()
				.map(|r| RefMatch {
					record: r.record,
					source: r.source,
				})
				.collect(),
		};
	}
	MatchSet {
		defs: matches
			.defs
			.iter()
			.copied()
			.filter(|def| tree_visible_def_kind(&def.kind))
			.collect(),
		refs: Vec::new(),
	}
}

pub(super) fn tree_visible_def_kind(kind: &[u8]) -> bool {
	!matches!(kind, KIND_LOCAL | KIND_PARAM | KIND_COMMENT)
}

struct MatchItem<'g> {
	cursor: Moniker,
	record: MatchRecord<'g>,
}

enum MatchRecord<'g> {
	Def(&'g DefRecord),
	Ref {
		record: &'g RefRecord,
		source: &'g Moniker,
	},
}

impl<'g> MatchItem<'g> {
	fn cursor_moniker(&self) -> &Moniker {
		&self.cursor
	}

	fn rank(&self) -> u8 {
		match &self.record {
			MatchRecord::Def(_) => 0,
			MatchRecord::Ref { .. } => 1,
		}
	}

	fn source_bytes(&self) -> &'g [u8] {
		match &self.record {
			MatchRecord::Def(_) => &[],
			MatchRecord::Ref { source, .. } => source.as_bytes(),
		}
	}

	fn kind(&self) -> &'g [u8] {
		match &self.record {
			MatchRecord::Def(def) => &def.kind,
			MatchRecord::Ref { record, .. } => &record.kind,
		}
	}

	fn position(&self) -> Option<(u32, u32)> {
		match &self.record {
			MatchRecord::Def(def) => def.position,
			MatchRecord::Ref { record, .. } => record.position,
		}
	}
}

fn cmp_match_item(a: &MatchItem<'_>, b: &MatchItem<'_>) -> Ordering {
	a.cursor_moniker()
		.as_bytes()
		.cmp(b.cursor_moniker().as_bytes())
		.then_with(|| a.rank().cmp(&b.rank()))
		.then_with(|| a.source_bytes().cmp(b.source_bytes()))
		.then_with(|| a.kind().cmp(b.kind()))
		.then_with(|| a.position().cmp(&b.position()))
}

fn paginate_match_set<'g>(
	matches: &MatchSet<'g>,
	args: &ExtractArgs,
	scheme: &str,
) -> anyhow::Result<(MatchSet<'g>, page::PageInfo)> {
	let spec = page::PageSpec::from_args(args, scheme)?;
	let mut items = Vec::with_capacity(matches.defs.len() + matches.refs.len());
	let mut ordinal = 0usize;
	for &def in &matches.defs {
		let cursor = page::def_cursor_moniker(&def.moniker, &[], &def.kind, def.position, ordinal);
		if spec.allows(&cursor) {
			items.push(MatchItem {
				cursor,
				record: MatchRecord::Def(def),
			});
		}
		ordinal += 1;
	}
	for r in &matches.refs {
		let cursor = page::ref_cursor_moniker(
			&r.record.target,
			&[],
			r.source,
			&r.record.kind,
			r.record.position,
			ordinal,
		);
		if spec.allows(&cursor) {
			items.push(MatchItem {
				cursor,
				record: MatchRecord::Ref {
					record: r.record,
					source: r.source,
				},
			});
		}
		ordinal += 1;
	}
	items.sort_by(cmp_match_item);

	let total = items.len();
	let page_len = spec.page_len(total);
	let last = page_len
		.checked_sub(1)
		.and_then(|idx| items.get(idx))
		.map(MatchItem::cursor_moniker);
	let info = spec.info(total, page_len, last, scheme);
	let mut defs = Vec::new();
	let mut refs = Vec::new();
	for item in items.into_iter().take(page_len) {
		match item.record {
			MatchRecord::Def(def) => defs.push(def),
			MatchRecord::Ref { record, source } => refs.push(RefMatch { record, source }),
		}
	}
	Ok((MatchSet { defs, refs }, info))
}

pub(super) fn write_page_notice<W: Write>(
	stderr: &mut W,
	args: &ExtractArgs,
	page: &page::PageInfo,
) -> std::io::Result<()> {
	if args.format == OutputFormat::Json {
		return Ok(());
	}
	if let Some(cursor) = &page.next_cursor {
		writeln!(
			stderr,
			"code-moniker: ... {} more results, use --after '{}' or --all",
			page.remaining, cursor
		)?;
	}
	Ok(())
}
