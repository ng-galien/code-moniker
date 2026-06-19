// code-moniker: ignore-file[smell-clone-reflex]
// View exploration builds owned RPC DTOs from borrowed view and workspace state.
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use code_moniker_query::{
	SourceLine, ViewBoundaryDto, ViewDetailResult, ViewEvidenceDto, ViewGotchaDto, ViewListResult,
	ViewReadResult, ViewRuleDto, ViewRuleRefDto, ViewSummaryDto,
};
use code_moniker_workspace::snapshot::WorkspaceSnapshot;

use super::config;
use super::model::{BoundarySpec, GotchaSpec, RenderOptions, ViewDocument};
use super::resolve::{self, RuleEvidence, SymbolEvidence, SymbolResolution};

const VIEWS_URI: &str = "workspace/views";

pub fn is_views_uri(uri: &str, scheme: &str) -> bool {
	view_path(uri, scheme).is_some()
}

pub fn read(
	uri: &str,
	roots: &[PathBuf],
	scheme: &str,
	snapshot: &WorkspaceSnapshot,
	context_lines: usize,
	include_code: bool,
) -> anyhow::Result<ViewReadResult> {
	let views = config::load(roots)?;
	match view_path(uri, scheme) {
		Some(None) => Ok(ViewReadResult::List(build_list(&views))),
		Some(Some(id)) => Ok(ViewReadResult::Detail(Box::new(build_detail(
			roots,
			snapshot,
			&views,
			&id,
			context_lines,
			include_code,
		)?))),
		None => anyhow::bail!("unsupported views URI `{uri}`"),
	}
}

fn view_path(uri: &str, scheme: &str) -> Option<Option<String>> {
	let value = uri.trim();
	let path = value.strip_prefix(scheme).unwrap_or(value);
	let rest = path.strip_prefix(VIEWS_URI)?;
	if rest.is_empty() {
		return Some(None);
	}
	rest.strip_prefix('/')
		.filter(|id| !id.is_empty())
		.map(|id| Some(id.to_string()))
}

fn build_list(views: &[ViewDocument]) -> ViewListResult {
	ViewListResult {
		views: views
			.iter()
			.map(|view| ViewSummaryDto {
				id: view.spec.id.clone(),
				title: view.spec.title.clone(),
				fragment: view.fragment.clone(),
				anchor: view.anchor.display().to_string(),
				scope: view.scope_path.clone(),
			})
			.collect(),
	}
}

fn build_detail(
	roots: &[PathBuf],
	snapshot: &WorkspaceSnapshot,
	views: &[ViewDocument],
	id: &str,
	context_lines: usize,
	include_code: bool,
) -> anyhow::Result<ViewDetailResult> {
	let view = views
		.iter()
		.find(|view| view.spec.id == id)
		.ok_or_else(|| anyhow::anyhow!("view `{id}` not found"))?;
	let rules = rule_map(roots, snapshot, view)?;
	let options = RenderOptions {
		context_lines,
		include_code,
	};
	Ok(ViewDetailResult {
		id: view.spec.id.clone(),
		title: view.spec.title.clone(),
		fragment: view.fragment.clone(),
		anchor: view.anchor.display().to_string(),
		scope: view.scope_path.clone(),
		intent: view.spec.intent.clone(),
		summary: view.spec.summary.clone(),
		rules: rules.values().map(rule_dto).collect(),
		boundaries: view
			.spec
			.boundaries
			.iter()
			.map(|boundary| build_boundary(snapshot, options, view, boundary, &rules))
			.collect(),
		gotchas: view
			.spec
			.gotchas
			.iter()
			.map(|gotcha| build_gotcha(snapshot, options, view, gotcha, &rules))
			.collect(),
	})
}

fn build_boundary(
	snapshot: &WorkspaceSnapshot,
	options: RenderOptions,
	view: &ViewDocument,
	boundary: &BoundarySpec,
	rules: &BTreeMap<String, RuleEvidence>,
) -> ViewBoundaryDto {
	let resolution = resolve_selectors(snapshot, view, &boundary.symbols, options);
	ViewBoundaryDto {
		id: boundary.id.clone(),
		owns: boundary.owns.clone(),
		forbids: boundary.forbids.clone(),
		forbid_rules: boundary.forbid_rules.clone(),
		rationale: boundary.rationale.clone(),
		rule_refs: rule_refs(&boundary.rules, rules),
		evidence: evidence_dtos(resolution.evidence),
		missing: resolution.missing,
	}
}

fn build_gotcha(
	snapshot: &WorkspaceSnapshot,
	options: RenderOptions,
	view: &ViewDocument,
	gotcha: &GotchaSpec,
	rules: &BTreeMap<String, RuleEvidence>,
) -> ViewGotchaDto {
	let resolution = resolve_selectors(snapshot, view, &gotcha.symbols, options);
	ViewGotchaDto {
		id: gotcha.id.clone(),
		rationale: gotcha.rationale.clone(),
		check: gotcha.check.clone(),
		rule_refs: rule_refs(&gotcha.rules, rules),
		evidence: evidence_dtos(resolution.evidence),
		missing: resolution.missing,
	}
}

fn resolve_selectors(
	snapshot: &WorkspaceSnapshot,
	view: &ViewDocument,
	selectors: &[String],
	options: RenderOptions,
) -> SymbolResolution {
	if selectors.is_empty() {
		return SymbolResolution::default();
	}
	resolve::resolve_symbols(snapshot, &view.scope_path, selectors, options)
}

fn rule_refs(ids: &[String], rules: &BTreeMap<String, RuleEvidence>) -> Vec<ViewRuleRefDto> {
	ids.iter()
		.map(|id| ViewRuleRefDto {
			id: id.clone(),
			present: rules.contains_key(id),
		})
		.collect()
}

fn rule_dto(rule: &RuleEvidence) -> ViewRuleDto {
	ViewRuleDto {
		id: rule.id.clone(),
		severity: rule.severity.clone(),
		domain: rule.domain.clone(),
		rationale: rule.rationale.clone(),
	}
}

fn evidence_dtos(evidence: Vec<SymbolEvidence>) -> Vec<ViewEvidenceDto> {
	evidence
		.into_iter()
		.map(|item| ViewEvidenceDto {
			selector: item.selector,
			label: item.label,
			moniker: item.moniker,
			file: item.file,
			slice: item.slice.map(|(start, end)| (start as u32, end as u32)),
			active_slice: item
				.active_slice
				.map(|(start, end)| (start as u32, end as u32)),
			code: item
				.code
				.into_iter()
				.map(|(line, text)| SourceLine {
					number: line as u32,
					text,
				})
				.collect(),
		})
		.collect()
}

fn rule_map(
	roots: &[PathBuf],
	snapshot: &WorkspaceSnapshot,
	view: &ViewDocument,
) -> anyhow::Result<BTreeMap<String, RuleEvidence>> {
	let ids = collect_rule_ids(view);
	let rules = resolve::resolve_rules(roots, snapshot, &ids)?;
	Ok(ids.into_iter().zip(rules).collect())
}

fn collect_rule_ids(view: &ViewDocument) -> Vec<String> {
	let mut ids = BTreeSet::new();
	for boundary in &view.spec.boundaries {
		ids.extend(boundary.rules.iter().cloned());
		ids.extend(boundary.forbid_rules.iter().cloned());
	}
	for gotcha in &view.spec.gotchas {
		ids.extend(gotcha.rules.iter().cloned());
	}
	ids.into_iter().collect()
}
