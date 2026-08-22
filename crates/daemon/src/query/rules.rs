use std::path::PathBuf;
use std::sync::Arc;

use code_moniker_check::{IndexedCheckWorkspace, RuleSetRequest};
use code_moniker_core::core::shape::{Shape, shape_of};
use code_moniker_query::{
	FileErrorDto, Page, QueryError, QueryResponse, QueryResult, RuleApplicabilityDto, RuleDto,
	RuleReportDto, RulesApplicableQuery, RulesApplicableResult, RulesCheckResult,
	RulesCheckRootResult, RulesCheckVerdict, RulesListResult, SymbolGraphFocus, ViolationDto,
};
use code_moniker_workspace::snapshot::WorkspaceSnapshot;
use code_moniker_workspace::source::{LocalResourceCache, MEMORY_SOURCE_ROOT};

use super::graph::resolve_unit_boundary;
use super::model::{
	IndexedRulesCheck, ResponseContext, RulesCheckEval, RulesListEval, RulesListFilters,
};
use crate::helpers::{
	DEFAULT_SCHEME, aggregate_check_exit, aggregate_check_summary, has_memory_sources,
	resolve_rules_path, rule_dto, run_rules_for_root, selected_roots, symbol_scope_for_roots,
	workspace_langs, workspace_selector_is_all,
};
use crate::pagination::page_rows;

enum RulesCheckRow {
	Violation(ViolationDto),
	Error(FileErrorDto),
	RuleReport(Box<RuleReportDto>),
	SkipReason(code_moniker_query::CheckSkipReasonDto),
}

fn rules_check_rows(roots: &[RulesCheckRootResult]) -> Vec<RulesCheckRow> {
	let mut rows = Vec::new();
	for root in roots {
		rows.extend(
			root.violations
				.iter()
				.cloned()
				.map(RulesCheckRow::Violation),
		);
		rows.extend(root.errors.iter().cloned().map(RulesCheckRow::Error));
		rows.extend(
			root.rule_reports
				.iter()
				.cloned()
				.map(Box::new)
				.map(RulesCheckRow::RuleReport),
		);
		rows.extend(
			root.skip_reason
				.iter()
				.cloned()
				.map(RulesCheckRow::SkipReason),
		);
	}
	rows
}

pub(crate) fn rules_list_response(
	snapshot: &WorkspaceSnapshot,
	response: ResponseContext<'_>,
	request: RulesListEval,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(response.roots, request.workspace.as_deref())?;
	let mut rule_roots = selected_roots
		.iter()
		.map(|root| (*root).to_path_buf())
		.collect::<Vec<_>>();
	if workspace_selector_is_all(request.workspace.as_deref())
		&& has_memory_sources(snapshot, response.roots)
	{
		rule_roots.push(PathBuf::from(MEMORY_SOURCE_ROOT));
	}
	let mut rows = Vec::new();
	for root in &rule_roots {
		let requested_langs =
			workspace_langs(snapshot, response.roots, root, &request.filters.langs);
		let rules_path = resolve_rules_path(response.config_root, request.rules.as_deref());
		let specs = RuleSetRequest::with_rules(rules_path, DEFAULT_SCHEME)
			.with_project_root(response.config_root)
			.with_profile(request.profile.clone())
			.compiled_specs_for_langs(requested_langs)
			.map_err(|err| QueryError::new("rules_compile_failed", err.to_string()))?;
		for spec in specs {
			if !request.filters.severities.is_empty()
				&& !request
					.filters
					.severities
					.iter()
					.any(|severity| severity == spec.severity.as_str())
			{
				continue;
			}
			rows.push(rule_dto(root, spec));
		}
	}
	rows.sort_by(|a, b| {
		a.root
			.cmp(&b.root)
			.then_with(|| a.id.cmp(&b.id))
			.then_with(|| a.lang.cmp(&b.lang))
			.then_with(|| a.domain.cmp(&b.domain))
	});
	let paged = page_rows(rows, request.page, response.generation)?;
	Ok(QueryResponse {
		generation: response.generation,
		result: QueryResult::RulesList(RulesListResult {
			roots: rule_roots
				.iter()
				.map(|root| root.display().to_string())
				.collect(),
			total: paged.total,
			rows: paged.items,
		}),
		next_cursor: paged.next_cursor,
	})
}

pub(crate) fn rules_applicable_response(
	snapshot: &WorkspaceSnapshot,
	response: ResponseContext<'_>,
	query: RulesApplicableQuery,
	page: Page,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(response.roots, query.workspace.as_deref())?;
	let symbol_scope = symbol_scope_for_roots(snapshot, response.roots, &selected_roots);
	let (_, focus) = resolve_unit_boundary(
		snapshot,
		response.roots,
		&selected_roots,
		&symbol_scope,
		&query.focus,
	)?;
	let (file, language, symbol_kind) = focus_rule_coordinates(snapshot, &focus)?;
	let listed = rules_list_response(
		snapshot,
		response,
		RulesListEval {
			workspace: query.workspace,
			profile: query.profile,
			rules: query.rules,
			filters: RulesListFilters {
				langs: vec![language.clone()],
				severities: Vec::new(),
			},
			page: Page {
				cursor: None,
				limit: usize::MAX,
			},
		},
	)?;
	let QueryResult::RulesList(listed) = listed.result else {
		return Err(QueryError::new(
			"rules_contract",
			"unexpected rules list response",
		));
	};
	let rows = listed
		.rows
		.into_iter()
		.map(|rule| rule_applicability(rule, &language, symbol_kind.as_deref()))
		.collect::<Vec<_>>();
	let paged = page_rows(rows, page, response.generation)?;
	Ok(QueryResponse {
		generation: response.generation,
		result: QueryResult::RulesApplicable(Box::new(RulesApplicableResult {
			focus,
			file,
			language,
			symbol_kind,
			total: paged.total,
			rows: paged.items,
		})),
		next_cursor: paged.next_cursor,
	})
}

pub(super) fn focus_rule_coordinates(
	snapshot: &WorkspaceSnapshot,
	focus: &SymbolGraphFocus,
) -> Result<(String, String, Option<String>), QueryError> {
	match focus {
		SymbolGraphFocus::Symbol { symbol } => Ok((
			symbol.file.clone(),
			symbol.language.clone(),
			Some(symbol.kind.clone()),
		)),
		SymbolGraphFocus::File { path } => {
			let source = snapshot
				.index
				.sources
				.iter()
				.find(|source| &source.rel_path == path)
				.ok_or_else(|| {
					QueryError::new("source_not_found", format!("source not found: {path}"))
				})?;
			Ok((path.clone(), source.language.clone(), None))
		}
	}
}

fn rule_applicability(
	rule: RuleDto,
	language: &str,
	symbol_kind: Option<&str>,
) -> RuleApplicabilityDto {
	let (status, reason) = if rule.lang != language {
		(
			"ignored",
			format!("rule language {} does not match {language}", rule.lang),
		)
	} else if rule.domain == "refs" {
		(
			"potential",
			"reference rule may evaluate references anchored in this scope".to_string(),
		)
	} else if let (Some(expected), Some(actual)) = (rule.kind.as_deref(), symbol_kind) {
		if expected == actual {
			(
				"applicable",
				format!("language and symbol kind `{actual}` match"),
			)
		} else {
			(
				"ignored",
				format!("rule kind `{expected}` does not match symbol kind `{actual}`"),
			)
		}
	} else if let Some(expected) = rule
		.domain
		.strip_prefix("shape:")
		.and_then(|domain| domain.split_whitespace().next())
	{
		match symbol_kind
			.and_then(|kind| shape_of(kind.as_bytes()))
			.map(Shape::as_str)
		{
			Some(actual) if actual == expected => (
				"applicable",
				format!("language and symbol shape `{actual}` match"),
			),
			Some(actual) => (
				"ignored",
				format!("rule shape `{expected}` does not match symbol shape `{actual}`"),
			),
			None => (
				"potential",
				format!("file scope matches; select a `{expected}` symbol to prove applicability"),
			),
		}
	} else if rule.kind.is_some() && symbol_kind.is_none() {
		(
			"potential",
			"file scope matches the language; select a symbol to prove kind applicability"
				.to_string(),
		)
	} else {
		("applicable", "language and scope match".to_string())
	};
	RuleApplicabilityDto {
		rule,
		status: status.to_string(),
		reason,
	}
}

pub(crate) fn rules_check_response(
	cache: &LocalResourceCache,
	snapshot: Arc<WorkspaceSnapshot>,
	response: ResponseContext<'_>,
	request: RulesCheckEval,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(response.roots, request.workspace.as_deref())?;
	let mut check_roots = selected_roots
		.iter()
		.map(|root| (*root).to_path_buf())
		.collect::<Vec<_>>();
	if workspace_selector_is_all(request.workspace.as_deref())
		&& has_memory_sources(&snapshot, response.roots)
	{
		check_roots.push(PathBuf::from(MEMORY_SOURCE_ROOT));
	}
	let mut roots = Vec::new();
	for root in &check_roots {
		let workspace =
			IndexedCheckWorkspace::from_snapshot(root.clone(), cache, Arc::clone(&snapshot))
				.map_err(|error| {
					QueryError::new("indexed_corpus_unavailable", error.to_string())
				})?;
		roots.push(run_rules_for_root(IndexedRulesCheck {
			root,
			config_root: response.config_root,
			workspace: &workspace,
			profile: request.profile.clone(),
			rules: request.rules.as_deref(),
			files: &request.files,
			report: request.report,
		})?);
	}
	let exit = aggregate_check_exit(&roots);
	let verdict = RulesCheckVerdict::from_exit(&exit);
	let summary = aggregate_check_summary(&roots);
	let rows = rules_check_rows(&roots);
	let paged = page_rows(rows, request.page, response.generation)?;
	let mut violations = Vec::new();
	let mut errors = Vec::new();
	let mut rule_reports = Vec::new();
	let mut skip_reasons = Vec::new();
	for row in paged.items {
		match row {
			RulesCheckRow::Violation(violation) => violations.push(violation),
			RulesCheckRow::Error(error) => errors.push(error),
			RulesCheckRow::RuleReport(report) => rule_reports.push(*report),
			RulesCheckRow::SkipReason(reason) => skip_reasons.push(reason),
		}
	}
	let root_summaries = roots
		.into_iter()
		.map(clear_root_payloads)
		.collect::<Vec<_>>();
	Ok(QueryResponse {
		generation: response.generation,
		result: QueryResult::RulesCheck(RulesCheckResult {
			verdict,
			exit,
			summary,
			roots: root_summaries,
			violations,
			errors,
			rule_reports,
			skip_reasons,
		}),
		next_cursor: paged.next_cursor,
	})
}

fn clear_root_payloads(mut root: RulesCheckRootResult) -> RulesCheckRootResult {
	root.violations.clear();
	root.errors.clear();
	root.rule_reports.clear();
	root.skip_reason = None;
	root
}
