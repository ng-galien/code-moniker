use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use code_moniker_core::lang::Lang;
use code_moniker_workspace::snapshot::{SourceFileRecord, SymbolRecord, WorkspaceSnapshot};

use super::model::RenderOptions;
use crate::DEFAULT_SCHEME;
use crate::check;

pub(crate) type SourceSlice = Option<(usize, usize)>;
pub(crate) type CodeExcerpt = Vec<(usize, String)>;

#[derive(Clone, Debug)]
pub(crate) struct SymbolEvidence {
	pub(crate) selector: String,
	pub(crate) label: String,
	pub(crate) moniker: String,
	pub(crate) file: String,
	pub(crate) slice: SourceSlice,
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
		for (symbol, source) in matches.into_iter().take(3) {
			resolution.evidence.push(symbol_evidence(
				selector,
				symbol,
				source,
				options.context_lines,
			));
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
	context_lines: usize,
) -> SymbolEvidence {
	let source_text = std::fs::read_to_string(&source.path).unwrap_or_default();
	let (slice, code) = code_slice(&source_text, symbol.line_range, context_lines);
	SymbolEvidence {
		selector: selector.to_string(),
		label: format!("{} {}", symbol.kind, symbol.name),
		moniker: symbol.identity.clone(),
		file: source.rel_path.clone(),
		slice,
		code,
	}
}

fn code_slice(
	source_text: &str,
	line_range: Option<(u32, u32)>,
	context_lines: usize,
) -> (SourceSlice, CodeExcerpt) {
	let total_lines = source_text.lines().count().max(1);
	let Some((start, end)) = line_range else {
		return (None, Vec::new());
	};
	let start = start.max(1) as usize;
	let end = end.max(start as u32) as usize;
	let slice_start = start.saturating_sub(context_lines).max(1);
	let slice_end = end.saturating_add(context_lines).min(total_lines);
	let lines = source_text
		.lines()
		.enumerate()
		.filter_map(|(idx, line)| {
			let line_number = idx + 1;
			(line_number >= slice_start && line_number <= slice_end)
				.then_some((line_number, line.to_string()))
		})
		.collect();
	(Some((slice_start, slice_end)), lines)
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
