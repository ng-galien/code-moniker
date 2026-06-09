use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use code_moniker_core::lang::Lang;
use code_moniker_workspace::snapshot::{SourceFileRecord, SymbolRecord, WorkspaceSnapshot};

use code_moniker_check as check;

use super::model::RenderOptions;
use crate::DEFAULT_SCHEME;

pub(crate) type SourceSlice = Option<(usize, usize)>;
pub(crate) type CodeExcerpt = Vec<(usize, String)>;

#[derive(Clone, Debug)]
pub(crate) struct SymbolEvidence {
	pub(crate) selector: String,
	pub(crate) label: String,
	pub(crate) moniker: String,
	pub(crate) file: String,
	pub(crate) slice: SourceSlice,
	pub(crate) active_slice: SourceSlice,
	pub(crate) code: CodeExcerpt,
}

#[derive(Clone, Debug)]
pub(crate) struct MissingSymbol {
	pub(crate) selector: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SymbolResolution {
	pub(crate) evidence: Vec<SymbolEvidence>,
	pub(crate) missing: Vec<MissingSymbol>,
}

#[derive(Clone, Debug)]
pub(crate) struct RuleEvidence {
	pub(crate) id: String,
	pub(crate) severity: String,
	pub(crate) domain: String,
	pub(crate) rationale: Option<String>,
}

pub(crate) fn resolve_symbols(
	snapshot: &WorkspaceSnapshot,
	scope_path: &str,
	selectors: &[String],
	options: RenderOptions,
) -> SymbolResolution {
	let source_by_id = source_by_id(snapshot);
	let mut resolution = SymbolResolution::default();
	for selector in selectors {
		let matches = matching_symbols(snapshot, &source_by_id, scope_path, selector);
		if matches.is_empty() {
			resolution.missing.push(MissingSymbol {
				selector: selector.clone(),
			});
			continue;
		}
		let evidence = select_symbol_evidence(selector, matches, options);
		if evidence.is_empty() {
			resolution.missing.push(MissingSymbol {
				selector: selector.clone(),
			});
			continue;
		}
		for item in evidence {
			resolution.evidence.push(item);
		}
	}
	resolution
}

pub(crate) fn resolve_rules(
	roots: &[PathBuf],
	snapshot: &WorkspaceSnapshot,
	rule_ids: &[String],
) -> anyhow::Result<Vec<RuleEvidence>> {
	if rule_ids.is_empty() {
		return Ok(Vec::new());
	}
	let specs = compiled_rule_specs(roots, snapshot)?;
	Ok(rule_ids
		.iter()
		.map(|id| resolve_rule(id, &specs).unwrap_or_else(|| missing_rule(id)))
		.collect())
}

fn source_by_id(snapshot: &WorkspaceSnapshot) -> BTreeMap<&str, &SourceFileRecord> {
	snapshot
		.index
		.sources
		.iter()
		.map(|source| (source.id.as_str(), source))
		.collect()
}

fn matching_symbols<'a>(
	snapshot: &'a WorkspaceSnapshot,
	source_by_id: &'a BTreeMap<&str, &'a SourceFileRecord>,
	scope_path: &str,
	selector: &str,
) -> Vec<(&'a SymbolRecord, &'a SourceFileRecord)> {
	let mut matches = snapshot
		.index
		.symbols
		.iter()
		.filter_map(|symbol| {
			let source = source_by_id.get(symbol.source.as_str())?;
			(source_in_scope(source, scope_path) && selector_matches(symbol, selector))
				.then_some((symbol, *source))
		})
		.collect::<Vec<_>>();
	matches.sort_by(|a, b| a.0.identity.cmp(&b.0.identity));
	matches
}

fn select_symbol_evidence(
	selector: &str,
	matches: Vec<(&SymbolRecord, &SourceFileRecord)>,
	options: RenderOptions,
) -> Vec<SymbolEvidence> {
	const MAX_EVIDENCE_PER_SELECTOR: usize = 3;
	let matches = exact_suffix_matches(selector, &matches).unwrap_or(matches);
	let allow_internal = selector_allows_internal(selector);
	let mut matches = matches
		.into_iter()
		.filter(|(symbol, _)| allow_internal || is_view_evidence_symbol(symbol))
		.collect::<Vec<_>>();
	if matches.is_empty() {
		return Vec::new();
	}
	matches.sort_by(|left, right| {
		symbol_rank(selector, left.0)
			.cmp(&symbol_rank(selector, right.0))
			.then_with(|| left.0.identity.cmp(&right.0.identity))
	});
	let mut selected = Vec::new();
	let mut seen_slice = HashSet::new();
	let mut seen_file = BTreeSet::new();
	let mut seen_kind = BTreeSet::new();
	for (symbol, source) in matches {
		let evidence = symbol_evidence(selector, symbol, source, options);
		if !seen_slice.insert(slice_key(&evidence)) {
			continue;
		}
		let is_diverse =
			seen_file.insert(evidence.file.clone()) || seen_kind.insert(evidence.label.clone());
		if selected.is_empty() || is_diverse || selected.len() + 1 == MAX_EVIDENCE_PER_SELECTOR {
			selected.push(evidence);
		}
		if selected.len() == MAX_EVIDENCE_PER_SELECTOR {
			break;
		}
	}
	selected
}

fn exact_suffix_matches<'a>(
	selector: &str,
	matches: &[(&'a SymbolRecord, &'a SourceFileRecord)],
) -> Option<Vec<(&'a SymbolRecord, &'a SourceFileRecord)>> {
	let exact = matches
		.iter()
		.copied()
		.filter(|(symbol, _)| symbol.identity.ends_with(selector))
		.collect::<Vec<_>>();
	(!exact.is_empty()).then_some(exact)
}

fn selector_allows_internal(selector: &str) -> bool {
	matches!(
		selector_kind_hint(selector).as_deref(),
		Some("local" | "param" | "comment")
	)
}

fn is_view_evidence_symbol(symbol: &SymbolRecord) -> bool {
	symbol.navigable && !matches!(symbol.kind.as_str(), "local" | "param" | "comment")
}

fn symbol_rank(selector: &str, symbol: &SymbolRecord) -> (u8, u8, u8) {
	let exact = (symbol.identity != selector) as u8;
	let kind_mismatch = selector_kind_hint(selector)
		.as_deref()
		.is_some_and(|kind| kind != symbol.kind.as_str()) as u8;
	let child_penalty = symbol.parent.is_some() as u8;
	(exact, kind_mismatch, child_penalty)
}

fn selector_kind_hint(selector: &str) -> Option<String> {
	let tail = selector.rsplit('/').next().unwrap_or(selector);
	let (kind, _) = tail.split_once(':')?;
	(!kind.is_empty()).then(|| kind.to_string())
}

fn slice_key(evidence: &SymbolEvidence) -> (String, SourceSlice) {
	(evidence.file.clone(), evidence.slice)
}

fn source_in_scope(source: &SourceFileRecord, scope_path: &str) -> bool {
	scope_path.is_empty() || source.rel_path.starts_with(scope_path)
}

fn selector_matches(symbol: &SymbolRecord, selector: &str) -> bool {
	let selector = selector.trim();
	if selector.starts_with("code+moniker://") {
		return symbol.identity == selector;
	}
	symbol.identity.contains(selector)
}

fn symbol_evidence(
	selector: &str,
	symbol: &SymbolRecord,
	source: &SourceFileRecord,
	options: RenderOptions,
) -> SymbolEvidence {
	let source_text = std::fs::read_to_string(&source.path).unwrap_or_default();
	let (slice, active_slice, code) = code_slice(&source_text, symbol.line_range, options);
	SymbolEvidence {
		selector: selector.to_string(),
		label: format!("{} {}", symbol.kind, symbol.name),
		moniker: symbol.identity.clone(),
		file: source.rel_path.clone(),
		slice,
		active_slice,
		code,
	}
}

fn code_slice(
	source_text: &str,
	line_range: Option<(u32, u32)>,
	options: RenderOptions,
) -> (SourceSlice, SourceSlice, CodeExcerpt) {
	let total_lines = source_text.lines().count().max(1);
	let Some((start, end)) = line_range else {
		return (None, None, Vec::new());
	};
	let start = start.max(1) as usize;
	let end = end.max(start as u32) as usize;
	let active_slice = Some((start, end));
	let slice_start = start.saturating_sub(options.context_lines).max(1);
	let slice_end = end.saturating_add(options.context_lines).min(total_lines);
	if !options.include_code {
		return (Some((slice_start, slice_end)), active_slice, Vec::new());
	}
	let lines = source_text
		.lines()
		.enumerate()
		.filter_map(|(idx, line)| {
			let line_number = idx + 1;
			(line_number >= slice_start && line_number <= slice_end)
				.then_some((line_number, line.to_string()))
		})
		.collect();
	(Some((slice_start, slice_end)), active_slice, lines)
}

fn compiled_rule_specs(
	roots: &[PathBuf],
	snapshot: &WorkspaceSnapshot,
) -> anyhow::Result<HashMap<String, RuleEvidence>> {
	let rules_path = workspace_config_root(roots)?.join(".code-moniker.toml");
	let config = check::load_with_cli_default_rules(Some(&rules_path), None)?;
	let mut out = HashMap::new();
	for lang in workspace_languages(snapshot) {
		let compiled = check::compile_rules(&config, lang, DEFAULT_SCHEME)?;
		for spec in compiled.specs(lang) {
			out.insert(
				spec.rule_id.clone(),
				RuleEvidence {
					id: spec.rule_id,
					severity: spec.severity.as_str().to_string(),
					domain: spec.domain,
					rationale: spec.rationale,
				},
			);
		}
	}
	Ok(out)
}

fn resolve_rule(id: &str, specs: &HashMap<String, RuleEvidence>) -> Option<RuleEvidence> {
	if let Some(rule) = specs.get(id) {
		return Some(rule.clone());
	}
	let suffix = format!(".{id}");
	let mut matches = specs
		.values()
		.filter(|rule| rule.id.ends_with(&suffix))
		.cloned();
	let first = matches.next()?;
	matches.next().is_none().then_some(first)
}

fn missing_rule(id: &str) -> RuleEvidence {
	RuleEvidence {
		id: id.to_string(),
		severity: "missing".to_string(),
		domain: "unresolved".to_string(),
		rationale: None,
	}
}

fn workspace_languages(snapshot: &WorkspaceSnapshot) -> Vec<Lang> {
	let mut langs = snapshot
		.index
		.sources
		.iter()
		.filter_map(|source| Lang::from_tag(&source.language))
		.collect::<Vec<_>>();
	langs.sort_by_key(|lang| lang.tag());
	langs.dedup();
	langs
}

fn workspace_config_root(roots: &[PathBuf]) -> anyhow::Result<PathBuf> {
	let Some(first) = roots.first() else {
		anyhow::bail!("views require at least one workspace root");
	};
	let mut common = root_dir(first);
	for root in roots.iter().skip(1) {
		let root = root_dir(root);
		while !root.starts_with(&common) {
			if !common.pop() {
				anyhow::bail!("cannot find common root for views");
			}
		}
	}
	Ok(common)
}

fn root_dir(path: &Path) -> PathBuf {
	if path.is_dir() {
		path.to_path_buf()
	} else {
		path.parent()
			.unwrap_or_else(|| Path::new("."))
			.to_path_buf()
	}
}
