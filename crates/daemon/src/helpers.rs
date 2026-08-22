use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use code_moniker_check::{
	CheckRequest, CheckSkipReason, CheckSummary, CompiledRuleSpec, DefaultRulesSelection,
	RuleCoverage, RulePathReport, RulePathStep, RuleReport, RuleSetRequest, RuleVerdict, Violation,
};
use code_moniker_core::lang::Lang;
use code_moniker_query::{
	CheckSummaryDto, CountDto, FailedRuleDto, FileErrorDto, QueryError, RuleCoverageDto, RuleDto,
	RulePathReportDto, RulePathStepDto, RuleReportDto, RulesCheckRootResult, RulesCheckVerdict,
	SourceLine, SourceSnippet, SymbolDto, UsageDirection, UsageDto, ViolationDto,
	WorkspaceGeneration, WorkspaceRootStatus,
};
use code_moniker_workspace::snapshot::{
	ReferenceRecord, SourceFileRecord, SourceId, SymbolId, SymbolRecord, SymbolSet,
	WorkspaceSnapshot, WorkspaceView,
};
use code_moniker_workspace::source::{
	MEMORY_SOURCE_ROOT, MEMORY_SOURCE_ROOT_LABEL, is_memory_source_path,
};

use crate::query::model::{IndexedRulesCheck, UsageDtoContext};

pub(crate) const DEFAULT_SCHEME: &str = "code+moniker://";

pub(super) fn change_counts_by_source(snapshot: &WorkspaceSnapshot) -> BTreeMap<SourceId, usize> {
	let mut counts = BTreeMap::new();
	for change in &snapshot.changes.changes {
		let Some(source) = change.source else {
			continue;
		};
		*counts.entry(source).or_insert(0) += 1;
	}
	counts
}

pub(super) fn find_symbol<'a>(
	snapshot: &'a WorkspaceSnapshot,
	scope: &SymbolSet,
	uri: &str,
) -> Result<&'a SymbolRecord, QueryError> {
	let inventory = &snapshot.index.inventory;
	if let Some(id) = SymbolId::parse(uri) {
		if !inventory
			.catalog()
			.ordinal(&id)
			.is_some_and(|ordinal| scope.contains(ordinal))
		{
			return Err(QueryError::new(
				"symbol_not_in_workspace",
				format!("symbol {uri} is not in the selected workspace"),
			));
		}
		return symbol_record_by_id(snapshot, id).ok_or_else(|| {
			QueryError::new("symbol_not_found", format!("symbol not found: {uri}"))
		});
	}
	let exact = inventory
		.facets()
		.symbols_by_identity(uri)
		.map(|symbols| symbols.intersection(scope))
		.unwrap_or_default();
	if let Some(ordinal) = exact.single() {
		let id = inventory.catalog().id(ordinal).copied().ok_or_else(|| {
			QueryError::new("symbol_not_found", format!("symbol not found: {uri}"))
		})?;
		return symbol_record_by_id(snapshot, id).ok_or_else(|| {
			QueryError::new("symbol_not_found", format!("symbol not found: {uri}"))
		});
	}
	if exact.len() > 1 {
		return Err(QueryError::new(
			"symbol_ambiguous",
			format!("moniker matches multiple symbols: {uri}"),
		));
	}
	let mut matches = inventory
		.symbol_ids_by_compact_identity(uri)
		.into_iter()
		.filter(|id| {
			inventory
				.catalog()
				.ordinal(id)
				.is_some_and(|ordinal| scope.contains(ordinal))
		})
		.filter_map(|id| symbol_record_by_id(snapshot, id));
	if let Some(symbol) = matches.next() {
		if matches.next().is_some() {
			return Err(QueryError::new(
				"symbol_ambiguous",
				format!("compact moniker matches multiple symbols: {uri}"),
			));
		}
		return Ok(symbol);
	}
	let natural = natural_symbol_candidates(snapshot, scope, uri);
	if natural.len() == 1 {
		return Ok(natural[0]);
	}
	if natural.len() > 1 {
		let candidates = natural
			.iter()
			.take(5)
			.map(|symbol| symbol.identity.as_ref())
			.collect::<Vec<_>>()
			.join(", ");
		return Err(QueryError::new(
			"symbol_ambiguous",
			format!(
				"natural symbol reference `{uri}` matches multiple symbols: {candidates}; next: choose a returned moniker"
			),
		));
	}
	let name = natural_symbol_selector(uri)
		.map(|selector| selector.name)
		.unwrap_or_else(|| uri.to_string());
	Err(QueryError::new(
		"symbol_not_found",
		format!(
			"symbol not found: {uri}; next: use symbol.search name:\"^{name}\" and retry with a returned moniker"
		),
	))
}

struct NaturalSymbolSelector {
	language: Option<String>,
	path: Option<String>,
	kind: Option<String>,
	name: String,
}

fn natural_symbol_selector(value: &str) -> Option<NaturalSymbolSelector> {
	if value.starts_with("code+moniker://") || value.trim().is_empty() {
		return None;
	}
	if let Some((language, rest)) = value.split_once(':')
		&& let Some((path, kind_and_name)) = rest.rsplit_once('.')
		&& let Some((kind, name)) = kind_and_name.split_once(':')
		&& !language.is_empty()
		&& !path.is_empty()
		&& !kind.is_empty()
		&& !name.is_empty()
	{
		return Some(NaturalSymbolSelector {
			language: Some(language.to_string()),
			path: Some(path.to_string()),
			kind: Some(kind.to_string()),
			name: name.to_string(),
		});
	}
	value
		.chars()
		.all(|character| character.is_ascii_alphanumeric() || character == '_')
		.then(|| NaturalSymbolSelector {
			language: None,
			path: None,
			kind: None,
			name: value.to_string(),
		})
}

fn natural_symbol_candidates<'a>(
	snapshot: &'a WorkspaceSnapshot,
	scope: &SymbolSet,
	value: &str,
) -> Vec<&'a SymbolRecord> {
	let Some(selector) = natural_symbol_selector(value) else {
		return Vec::new();
	};
	let inventory = &snapshot.index.inventory;
	let facets = inventory.facets();
	let Some(candidates) = facets.symbols_by_natural_name(&selector.name) else {
		return Vec::new();
	};
	let kind_candidates = match selector.kind.as_deref() {
		Some(kind) => match facets.symbols_by_kind(kind) {
			Some(posting) => Some(posting),
			None => return Vec::new(),
		},
		None => None,
	};
	let language_candidates = match selector.language.as_deref() {
		Some(language) => match facets.symbols_by_language(language) {
			Some(posting) => Some(posting),
			None => return Vec::new(),
		},
		None => None,
	};
	let anchor = [
		Some(candidates),
		Some(scope),
		kind_candidates,
		language_candidates,
	]
	.into_iter()
	.flatten()
	.min_by_key(|posting| posting.len())
	.expect("natural name and workspace scope always provide an anchor");
	let expected_path = selector.path.as_deref().map(std::path::Path::new);
	let mut candidates = anchor
		.iter()
		.filter(|ordinal| {
			candidates.contains(*ordinal)
				&& scope.contains(*ordinal)
				&& kind_candidates.is_none_or(|posting| posting.contains(*ordinal))
				&& language_candidates.is_none_or(|posting| posting.contains(*ordinal))
		})
		.filter_map(|ordinal| inventory.record(ordinal))
		.filter(|record| record.navigable)
		.filter(|record| {
			selector
				.kind
				.as_deref()
				.is_none_or(|kind| record.kind.as_ref() == kind)
				&& selector
					.language
					.as_deref()
					.is_none_or(|language| record.language.as_ref() == language)
				&& expected_path.is_none_or(|expected| {
					let actual = std::path::Path::new(record.source_path.as_ref());
					actual == expected || actual.with_extension("") == expected
				})
		})
		.filter_map(|record| symbol_record_by_id(snapshot, record.id))
		.take(6)
		.collect::<Vec<_>>();
	candidates.sort_by(|left, right| left.identity.cmp(&right.identity));
	candidates
}

pub(super) fn symbol_scope_for_roots<'a>(
	snapshot: &'a WorkspaceSnapshot,
	roots: &[PathBuf],
	selected_roots: &[&PathBuf],
) -> std::borrow::Cow<'a, SymbolSet> {
	if selected_roots.len() == roots.len() {
		return std::borrow::Cow::Borrowed(snapshot.index.inventory.all_symbols());
	}
	let mut scope = SymbolSet::new();
	for (index, root) in roots.iter().enumerate() {
		if selected_roots
			.iter()
			.any(|selected| selected.as_path() == root.as_path())
			&& let Some(posting) = snapshot
				.index
				.inventory
				.facets()
				.symbols_by_source_root(index)
		{
			scope.union_with(posting);
		}
	}
	std::borrow::Cow::Owned(scope)
}

fn symbol_record_by_id(snapshot: &WorkspaceSnapshot, id: SymbolId) -> Option<&SymbolRecord> {
	snapshot.index.symbols.file_records(id.file()).get(id.def())
}

pub(super) fn symbol_dto(
	symbol: &SymbolRecord,
	source: &SourceFileRecord,
	roots: &[PathBuf],
) -> SymbolDto {
	SymbolDto {
		root: source_root_label(roots, source),
		uri: symbol.identity.to_string(),
		id: symbol.id.to_string(),
		name: symbol.name.to_string(),
		kind: symbol.kind.to_string(),
		visibility: symbol.visibility.to_string(),
		signature: symbol.signature.to_string(),
		file: source.rel_path.to_string(),
		language: source.language.to_string(),
		line_range: symbol.line_range,
		navigable: symbol.navigable,
		score: None,
		match_reason: None,
		source: None,
	}
}

pub(super) fn symbol_search_dto(
	symbol: &SymbolRecord,
	source: &SourceFileRecord,
	roots: &[PathBuf],
	score: u32,
	reason: String,
) -> SymbolDto {
	let mut dto = symbol_dto(symbol, source, roots);
	dto.score = Some(score);
	dto.match_reason = Some(reason);
	dto
}

pub(super) fn usage_dto(
	reference: &ReferenceRecord,
	direction: UsageDirection,
	context: &UsageDtoContext<'_>,
) -> Option<UsageDto> {
	let source = WorkspaceView::new(context.snapshot)
		.sources()
		.record(&reference.source)?;
	source_root(context.roots, context.selected_roots, source)?;
	if !context.path_filter.matches(&source.rel_path)
		|| (!context.langs.is_empty() && !context.langs.iter().any(|lang| lang == &source.language))
	{
		return None;
	}
	let source_symbol = WorkspaceView::new(context.snapshot)
		.symbols()
		.find(&reference.source_symbol);
	let actor = source_symbol
		.map(|symbol| symbol.name.to_string())
		.unwrap_or_else(|| reference.source_symbol.to_string());
	let source_context = source_symbol
		.map(|symbol| symbol.identity.to_string())
		.unwrap_or_else(|| reference.source_symbol.to_string());
	Some(UsageDto {
		root: source_root_label(context.roots, source),
		direction,
		reference: reference.id.to_string(),
		kind: reference.kind.to_string(),
		actor,
		context: source_context,
		endpoint: reference.target_identity.to_string(),
		file: source.rel_path.to_string(),
		prefix: path_prefix(&source.rel_path),
		location: reference_location(source, reference),
		line_range: reference.line_range,
		via: None,
	})
}

pub(super) fn source_snippet(
	source: &SourceFileRecord,
	symbol: &SymbolRecord,
	context_lines: usize,
) -> Result<Option<SourceSnippet>, QueryError> {
	let Some((start, end)) = symbol.line_range else {
		return Ok(None);
	};
	let first = start.saturating_sub(context_lines as u32).max(1);
	let last = end.saturating_add(context_lines as u32);
	let source_text = load_source_text(source)?;
	let lines = source_text
		.lines()
		.enumerate()
		.filter_map(|(idx, text)| {
			let number = idx as u32 + 1;
			(number >= first && number <= last).then(|| SourceLine {
				number,
				text: text.to_string(),
			})
		})
		.collect();
	Ok(Some(SourceSnippet {
		file: source.rel_path.to_owned(),
		first_line: first,
		last_line: last,
		lines,
	}))
}

pub(super) fn load_source_text(source: &SourceFileRecord) -> Result<String, QueryError> {
	if source.text.is_empty() && !is_memory_source_path(Path::new(&source.path)) {
		std::fs::read_to_string(&source.path).map_err(|err| {
			QueryError::new(
				"source_read_failed",
				format!("cannot read source {}: {err}", source.path),
			)
		})
	} else {
		Ok(source.text.to_string())
	}
}

pub(super) fn workspace_langs(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	root: &Path,
	filter: &[String],
) -> Vec<Lang> {
	let mut langs = snapshot
		.index
		.sources
		.iter()
		.filter(|source| source_in_root(roots, source, root))
		.filter(|source| filter.is_empty() || filter.iter().any(|lang| lang == &source.language))
		.filter_map(|source| Lang::from_tag(&source.language))
		.collect::<Vec<_>>();
	langs.sort_by_key(|lang| lang.tag());
	langs.dedup();
	langs
}

pub(super) fn resolve_rules_path(root: &Path, rules: Option<&str>) -> PathBuf {
	let path = rules
		.map(PathBuf::from)
		.unwrap_or_else(|| PathBuf::from(".code-moniker.toml"));
	if path.is_absolute() {
		path
	} else {
		root.join(path)
	}
}

pub(super) fn violation_dto(root: &Path, path: &Path, violation: &Violation) -> ViolationDto {
	ViolationDto {
		root: root.display().to_string(),
		path: path.display().to_string(),
		rule_id: violation.rule_id.to_string(),
		severity: violation.severity.as_str().to_string(),
		moniker: violation.moniker.to_string(),
		srcset: violation.srcset.clone(),
		kind: violation.kind.to_string(),
		lines: violation.lines,
		message: violation.message.to_string(),
	}
}

pub(super) fn file_error_dto(root: &Path, path: &Path, error: &str) -> FileErrorDto {
	FileErrorDto {
		root: root.display().to_string(),
		path: path.display().to_string(),
		error: error.to_string(),
	}
}

pub(super) fn rule_report_dto(
	root: &Path,
	path: Option<&Path>,
	report: &RuleReport,
) -> RuleReportDto {
	RuleReportDto {
		root: root.display().to_string(),
		path: path.map(|path| path.display().to_string()),
		rule_id: report.rule_id.to_string(),
		severity: report.severity.as_str().to_string(),
		domain: report.domain.to_string(),
		evaluated: report.evaluated,
		matches: report.matches,
		violations: report.violations,
		antecedent_matches: report.antecedent_matches,
		warning: report.warning.clone(),
		inconclusive: report.inconclusive,
		verdict: report.verdict.map(rule_verdict_label),
		coverage: report.coverage.as_ref().map(rule_coverage_dto),
		path_analysis: report.path.as_ref().map(rule_path_report_dto),
	}
}

fn rule_verdict_label(verdict: RuleVerdict) -> String {
	match verdict {
		RuleVerdict::Pass => "pass",
		RuleVerdict::Fail => "fail",
		RuleVerdict::Inconclusive => "inconclusive",
	}
	.to_string()
}

fn rule_coverage_dto(coverage: &RuleCoverage) -> RuleCoverageDto {
	RuleCoverageDto {
		total: coverage.total,
		decided: coverage.decided,
		resolved: coverage.resolved,
		external: coverage.external,
		candidate: coverage.candidate,
		dynamic: coverage.dynamic,
		blocked: coverage.blocked,
		unresolved: coverage.unresolved,
		percent: coverage.percent,
		min_percent: coverage.min_percent,
	}
}

fn rule_path_report_dto(path: &RulePathReport) -> RulePathReportDto {
	RulePathReportDto {
		expectation: path.expectation.clone(),
		relation: path.relation.clone(),
		max_depth: path.max_depth,
		max_symbols: path.max_symbols,
		max_edges: path.max_edges,
		max_pairs: path.max_pairs,
		min_coverage: path.min_coverage,
		source_symbols: path.source_symbols,
		target_symbols: path.target_symbols,
		via_symbols: path.via_symbols,
		evaluated_pairs: path.evaluated_pairs,
		explored_symbols: path.explored_symbols,
		explored_edges: path.explored_edges,
		depth_limit_reached: path.depth_limit_reached,
		symbol_limit_reached: path.symbol_limit_reached,
		edge_limit_reached: path.edge_limit_reached,
		pair_limit_reached: path.pair_limit_reached,
		reasons: path.reasons.clone(),
		witness: path.witness.iter().map(rule_path_step_dto).collect(),
	}
}

fn rule_path_step_dto(step: &RulePathStep) -> RulePathStepDto {
	RulePathStepDto {
		source: step.source.clone(),
		target: step.target.clone(),
		relation: step.relation.clone(),
		reference: step.reference.clone(),
		file: step.file.clone(),
		line_range: step.line_range,
	}
}

pub(super) fn rule_dto(root: &Path, spec: CompiledRuleSpec) -> RuleDto {
	RuleDto {
		root: root.display().to_string(),
		id: spec.rule_id,
		severity: spec.severity.as_str().to_string(),
		lang: spec.lang,
		rule_root: spec.root,
		subject: spec.subject,
		plan: spec.plan,
		capabilities: spec.capabilities,
		group_by: spec.group_by,
		domain: spec.domain,
		kind: spec.kind,
		expr: spec.expr,
		expanded_expr: spec.expanded_expr,
		message: spec.message,
		rationale: spec.rationale,
		require_doc_comment: spec.require_doc_comment,
	}
}

pub(super) fn run_rules_for_root(
	check: IndexedRulesCheck<'_>,
) -> Result<RulesCheckRootResult, QueryError> {
	let rules_path = resolve_rules_path(check.config_root, check.rules);
	let rules = RuleSetRequest::with_rules(rules_path, DEFAULT_SCHEME)
		.with_default_rules(DefaultRulesSelection::Config)
		.with_profile(check.profile);
	let request = CheckRequest::new(check.root.to_path_buf(), rules)
		.with_report(check.report)
		.with_files(check.files.iter().map(PathBuf::from).collect());
	let run = request
		.run_with_workspace(check.workspace)
		.map_err(|err| QueryError::new("rules_check_failed", err.to_string()))?;
	let exit = check_exit(&run);
	let summary = check_summary_dto(&run.summary());
	let violations = run
		.file_violations()
		.map(|(path, violation)| violation_dto(check.root, path, violation))
		.collect();
	let errors = run
		.error_summaries()
		.map(|(path, error)| file_error_dto(check.root, path, error))
		.collect();
	let rule_reports = run
		.reports
		.iter()
		.flat_map(|report| {
			report
				.rule_reports
				.iter()
				.map(move |rule| rule_report_dto(check.root, Some(&report.path), rule))
		})
		.collect();
	let skip_reason = run
		.skip_reason
		.map(|reason| check_skip_reason_dto(check.root, reason));
	Ok(RulesCheckRootResult {
		root: check.root.display().to_string(),
		verdict: RulesCheckVerdict::from_exit(&exit),
		exit,
		summary,
		violations,
		errors,
		rule_reports,
		skip_reason,
	})
}

pub(super) fn check_exit(run: &code_moniker_check::CheckRun) -> String {
	if run.any_error() {
		"error"
	} else if run.any_error_violation() {
		"no_match"
	} else {
		"match"
	}
	.to_string()
}

pub(super) fn aggregate_check_exit(roots: &[RulesCheckRootResult]) -> String {
	if roots.iter().any(|root| root.exit == "error") {
		"error"
	} else if roots.iter().any(|root| root.exit == "no_match") {
		"no_match"
	} else {
		"match"
	}
	.to_string()
}

pub(super) fn aggregate_check_summary(roots: &[RulesCheckRootResult]) -> CheckSummaryDto {
	let mut summary = CheckSummaryDto::default();
	let mut unspecified_srcset = 0usize;
	for root in roots {
		summary.files_scanned += root.summary.files_scanned;
		summary.files_with_violations += root.summary.files_with_violations;
		summary.total_violations += root.summary.total_violations;
		summary.total_rule_errors += root.summary.total_rule_errors;
		summary.total_warnings += root.summary.total_warnings;
		summary.files_with_errors += root.summary.files_with_errors;
		summary.total_errors += root.summary.total_errors;
		summary.elapsed_ms += root.summary.elapsed_ms;
		summary
			.failed_rules
			.extend(root.summary.failed_rules.iter().cloned());
		unspecified_srcset += root
			.summary
			.total_violations
			.saturating_sub(root.summary.violations_by_srcset.values().sum::<usize>());
		for (srcset, violations) in &root.summary.violations_by_srcset {
			*summary
				.violations_by_srcset
				.entry(srcset.clone())
				.or_default() += violations;
		}
	}
	if !summary.violations_by_srcset.is_empty() && unspecified_srcset > 0 {
		*summary
			.violations_by_srcset
			.entry("unspecified".to_string())
			.or_default() += unspecified_srcset;
	}
	summary.failed_rules.sort_by(|a, b| {
		a.rule_id
			.cmp(&b.rule_id)
			.then_with(|| a.severity.cmp(&b.severity))
	});
	summary
}

pub(super) fn check_summary_dto(summary: &CheckSummary) -> CheckSummaryDto {
	CheckSummaryDto {
		files_scanned: summary.files_scanned,
		files_with_violations: summary.files_with_violations,
		total_violations: summary.total_violations,
		total_rule_errors: summary.total_rule_errors,
		total_warnings: summary.total_warnings,
		files_with_errors: summary.files_with_errors,
		total_errors: summary.total_errors,
		elapsed_ms: summary.elapsed_ms,
		failed_rules: summary
			.failed_rules
			.iter()
			.map(|rule| FailedRuleDto {
				rule_id: rule.rule_id.to_string(),
				severity: rule.severity.as_str().to_string(),
				violations: rule.violations,
			})
			.collect(),
		violations_by_srcset: summary.violations_by_srcset.clone(),
	}
}

pub(super) fn check_skip_reason_dto(
	root: &Path,
	reason: CheckSkipReason,
) -> code_moniker_query::CheckSkipReasonDto {
	let reason = match reason {
		CheckSkipReason::ExcludedSingleFile => "excluded_single_file",
		CheckSkipReason::UnsupportedSingleFile => "unsupported_single_file",
		CheckSkipReason::NoMatchingFiles => "no_matching_files",
	};
	code_moniker_query::CheckSkipReasonDto {
		root: root.display().to_string(),
		reason: reason.to_string(),
	}
}

pub(super) fn root_status(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	root: &Path,
	stale: bool,
	stale_summary: &str,
) -> WorkspaceRootStatus {
	let sources = snapshot
		.index
		.sources
		.iter()
		.filter(|source| source_in_root(roots, source, root))
		.collect::<Vec<_>>();
	let source_ids = sources
		.iter()
		.map(|source| source.id)
		.collect::<std::collections::BTreeSet<_>>();
	WorkspaceRootStatus {
		root: root.display().to_string(),
		generation: Some(WorkspaceGeneration(snapshot.generation.value())),
		files: sources.len(),
		symbols: snapshot
			.index
			.symbols
			.iter()
			.filter(|symbol| source_ids.contains(&symbol.source))
			.count(),
		references: snapshot
			.index
			.references
			.iter()
			.filter(|reference| source_ids.contains(&reference.source))
			.count(),
		stale,
		stale_summary: stale_summary.to_string(),
	}
}

pub(super) fn selected_roots<'a>(
	roots: &'a [PathBuf],
	selector: Option<&str>,
) -> Result<Vec<&'a PathBuf>, QueryError> {
	if selector.is_none_or(|selector| selector.trim().is_empty()) {
		return Ok(roots.iter().collect());
	}
	let selected = roots
		.iter()
		.filter(|root| root_matches_selector(root, selector))
		.collect::<Vec<_>>();
	if selected.is_empty() {
		let value = selector.unwrap_or("<all>");
		return Err(QueryError::new(
			"workspace_not_found",
			format!("workspace selector matched no root: {value}"),
		));
	}
	if selected.len() > 1 {
		let value = selector.unwrap_or("<all>");
		return Err(QueryError::new(
			"workspace_selector_ambiguous",
			format!("workspace selector matched multiple roots: {value}"),
		));
	}
	Ok(selected)
}

pub(super) fn root_matches_selector(root: &Path, selector: Option<&str>) -> bool {
	let Some(selector) = selector.map(str::trim).filter(|value| !value.is_empty()) else {
		return true;
	};
	root.display().to_string() == selector
		|| root
			.file_name()
			.and_then(|name| name.to_str())
			.is_some_and(|name| name == selector)
}

pub(super) fn source_root<'a>(
	roots: &'a [PathBuf],
	selected_roots: &[&PathBuf],
	source: &SourceFileRecord,
) -> Option<&'a Path> {
	let Some(root) = roots.get(source.source_root) else {
		return (source.source_root == roots.len() && selected_roots.len() == roots.len())
			.then(|| Path::new(MEMORY_SOURCE_ROOT));
	};
	selected_roots
		.iter()
		.any(|selected| selected.as_path() == root.as_path())
		.then_some(root.as_path())
}

pub(super) fn source_in_root(roots: &[PathBuf], source: &SourceFileRecord, root: &Path) -> bool {
	if source.source_root == roots.len() {
		return root == Path::new(MEMORY_SOURCE_ROOT);
	}
	roots
		.get(source.source_root)
		.is_some_and(|declared_root| declared_root == root)
}

pub(super) fn source_root_label(roots: &[PathBuf], source: &SourceFileRecord) -> String {
	if source.source_root == roots.len() {
		return MEMORY_SOURCE_ROOT_LABEL.to_string();
	}
	roots
		.get(source.source_root)
		.map(|root| root.display().to_string())
		.unwrap_or_default()
}

pub(super) fn has_memory_sources(snapshot: &WorkspaceSnapshot, roots: &[PathBuf]) -> bool {
	let active_sources = snapshot
		.catalog
		.sources
		.iter()
		.map(|source| source.id)
		.collect::<HashSet<_>>();
	snapshot
		.index
		.sources
		.iter()
		.any(|source| source.source_root == roots.len() && active_sources.contains(&source.id))
}

pub(super) fn workspace_selector_is_all(selector: Option<&str>) -> bool {
	selector.is_none_or(|selector| selector.trim().is_empty())
}

pub(super) fn sorted_counts<I>(values: I) -> Vec<CountDto>
where
	I: IntoIterator<Item = String>,
{
	let mut counts = BTreeMap::<String, usize>::new();
	for value in values {
		*counts.entry(value).or_default() += 1;
	}
	count_rows(counts)
}

pub(super) fn count_rows(counts: BTreeMap<String, usize>) -> Vec<CountDto> {
	let mut rows = counts
		.into_iter()
		.map(|(name, count)| CountDto { name, count })
		.collect::<Vec<_>>();
	rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
	rows
}

pub(super) fn path_prefix(path: &str) -> String {
	let parts = Path::new(path)
		.parent()
		.unwrap_or_else(|| Path::new(""))
		.components()
		.filter_map(|component| component.as_os_str().to_str())
		.take(2)
		.collect::<Vec<_>>();
	if parts.is_empty() {
		"<root>".to_string()
	} else {
		parts.join("/")
	}
}

pub(super) fn reference_location(source: &SourceFileRecord, reference: &ReferenceRecord) -> String {
	let suffix = reference
		.line_range
		.map(|(start, end)| {
			if start == end {
				format!(":L{start}")
			} else {
				format!(":L{start}-L{end}")
			}
		})
		.unwrap_or_else(|| ":L?".to_string());
	format!("{}{}", source.rel_path, suffix)
}

pub(super) fn root_labels(roots: &[PathBuf]) -> Vec<String> {
	roots
		.iter()
		.map(|root| root.display().to_string())
		.collect()
}

pub(super) fn common_workspace_root(roots: &[PathBuf]) -> anyhow::Result<PathBuf> {
	let Some(first) = roots.first() else {
		anyhow::bail!("workspace daemon requires at least one root");
	};
	let mut common = first.clone();
	for root in roots.iter().skip(1) {
		while !root.starts_with(&common) {
			if !common.pop() {
				anyhow::bail!("cannot find common root for workspace daemon roots");
			}
		}
	}
	Ok(common)
}

pub(super) fn rules_config_root(roots: &[PathBuf]) -> anyhow::Result<PathBuf> {
	let common = common_workspace_root(roots)?;
	let mut cursor = if common.is_file() {
		common
			.parent()
			.map(Path::to_path_buf)
			.unwrap_or_else(|| common.clone())
	} else {
		common.clone()
	};
	loop {
		if cursor.join(".code-moniker.toml").is_file() {
			return Ok(cursor);
		}
		if !cursor.pop() {
			return Ok(common);
		}
	}
}

pub(super) fn workspace_label_from_paths(roots: &[&PathBuf]) -> String {
	if roots.len() == 1 {
		roots[0].display().to_string()
	} else {
		roots
			.iter()
			.map(|root| root.display().to_string())
			.collect::<Vec<_>>()
			.join(";")
	}
}
