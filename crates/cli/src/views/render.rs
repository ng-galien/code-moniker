use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use code_moniker_workspace::snapshot::WorkspaceSnapshot;

use super::config;
use super::model::{BoundarySpec, GotchaSpec, RenderOptions, ViewDocument};
use super::resolve::{self, RuleEvidence, SymbolEvidence, SymbolResolution};

const VIEWS_URI: &str = "workspace/views";

pub(crate) fn is_views_uri(uri: &str, scheme: &str) -> bool {
	view_path(uri, scheme).is_some()
}

pub(crate) fn render_lmnav(
	uri: &str,
	roots: &[PathBuf],
	scheme: &str,
	snapshot: &WorkspaceSnapshot,
	options: RenderOptions,
) -> anyhow::Result<String> {
	let views = config::load(roots)?;
	match view_path(uri, scheme) {
		Some(None) => Ok(render_view_list(scheme, &views)),
		Some(Some(id)) => render_view_detail(scheme, roots, snapshot, options, &views, &id),
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

fn render_view_list(scheme: &str, views: &[ViewDocument]) -> String {
	let mut output = String::new();
	output.push_str(&format!("uri: {scheme}{VIEWS_URI}\n"));
	output.push_str("completeness: full\n");
	output.push_str(&format!("views: {}\n\n", views.len()));
	output.push_str("views:\n");
	if views.is_empty() {
		output.push_str("  <empty>\n");
	} else {
		for view in views {
			output.push_str(&format!("  - {}\n", view.spec.id));
			if let Some(title) = &view.spec.title {
				output.push_str(&format!("    title: {title}\n"));
			}
			output.push_str(&format!("    fragment: {}\n", view.fragment));
			output.push_str(&format!("    anchor: {}\n", view.anchor.display()));
			output.push_str(&format!("    scope: {}\n", scope_label(view)));
		}
	}
	output.push_str("\nnext:\n");
	for view in views.iter().take(12) {
		output.push_str(&format!(
			"  - code_moniker_read uri=\"{scheme}{VIEWS_URI}/{}\"\n",
			view.spec.id
		));
	}
	output
}

fn render_view_detail(
	scheme: &str,
	roots: &[PathBuf],
	snapshot: &WorkspaceSnapshot,
	options: RenderOptions,
	views: &[ViewDocument],
	id: &str,
) -> anyhow::Result<String> {
	let view = views
		.iter()
		.find(|view| view.spec.id == id)
		.ok_or_else(|| anyhow::anyhow!("view `{id}` not found"))?;
	let rules = rule_map(roots, snapshot, view)?;
	let mut output = String::new();
	render_view_header(&mut output, scheme, view);
	render_rule_catalog(&mut output, &rules);
	render_boundaries(&mut output, snapshot, options, view, &rules);
	render_gotchas(&mut output, snapshot, options, view, &rules);
	render_next(&mut output, scheme, view);
	Ok(output)
}

fn render_view_header(output: &mut String, scheme: &str, view: &ViewDocument) {
	output.push_str(&format!("uri: {scheme}{VIEWS_URI}/{}\n", view.spec.id));
	output.push_str("completeness: full\n");
	output.push_str(&format!("view: {}\n", view.spec.id));
	if let Some(title) = &view.spec.title {
		output.push_str(&format!("title: {title}\n"));
	}
	output.push_str(&format!("fragment: {}\n", view.fragment));
	output.push_str(&format!("anchor: {}\n", view.anchor.display()));
	output.push_str(&format!("scope: {}\n", scope_label(view)));
	if let Some(intent) = &view.spec.intent {
		output.push_str(&format!("intent: {intent}\n"));
	}
	if let Some(summary) = &view.spec.summary {
		output.push_str("\nsummary:\n");
		render_text_block(output, summary, "  ");
	}
}

fn render_boundaries(
	output: &mut String,
	snapshot: &WorkspaceSnapshot,
	options: RenderOptions,
	view: &ViewDocument,
	rules: &BTreeMap<String, RuleEvidence>,
) {
	output.push_str("\nboundaries:\n");
	if view.spec.boundaries.is_empty() {
		output.push_str("  <empty>\n");
		return;
	}
	for boundary in &view.spec.boundaries {
		render_boundary(output, snapshot, options, view, boundary, rules);
	}
}

fn render_boundary(
	output: &mut String,
	snapshot: &WorkspaceSnapshot,
	options: RenderOptions,
	view: &ViewDocument,
	boundary: &BoundarySpec,
	rules: &BTreeMap<String, RuleEvidence>,
) {
	output.push_str(&format!("  - {}\n", boundary.id));
	render_list(output, "owns", &boundary.owns, "    ");
	render_forbids(output, boundary, "    ");
	if let Some(rationale) = &boundary.rationale {
		output.push_str("    rationale:\n");
		render_text_block(output, rationale, "      ");
	}
	render_rule_refs(output, "rules", &boundary.rules, rules, "    ");
	render_symbols(output, snapshot, options, view, &boundary.symbols, "    ");
}

fn render_gotchas(
	output: &mut String,
	snapshot: &WorkspaceSnapshot,
	options: RenderOptions,
	view: &ViewDocument,
	rules: &BTreeMap<String, RuleEvidence>,
) {
	output.push_str("\ngotchas:\n");
	if view.spec.gotchas.is_empty() {
		output.push_str("  <empty>\n");
		return;
	}
	for gotcha in &view.spec.gotchas {
		render_gotcha(output, snapshot, options, view, gotcha, rules);
	}
}

fn render_gotcha(
	output: &mut String,
	snapshot: &WorkspaceSnapshot,
	options: RenderOptions,
	view: &ViewDocument,
	gotcha: &GotchaSpec,
	rules: &BTreeMap<String, RuleEvidence>,
) {
	output.push_str(&format!("  - {}\n", gotcha.id));
	output.push_str("    rationale:\n");
	render_text_block(output, &gotcha.rationale, "      ");
	if let Some(check) = &gotcha.check {
		output.push_str(&format!("    check: {check}\n"));
	}
	render_rule_refs(output, "rules", &gotcha.rules, rules, "    ");
	render_symbols(output, snapshot, options, view, &gotcha.symbols, "    ");
}

fn render_symbols(
	output: &mut String,
	snapshot: &WorkspaceSnapshot,
	options: RenderOptions,
	view: &ViewDocument,
	selectors: &[String],
	indent: &str,
) {
	if selectors.is_empty() {
		return;
	}
	let resolution = resolve::resolve_symbols(snapshot, &view.scope_path, selectors, options);
	render_symbol_resolution(output, options, resolution, indent);
}

fn render_symbol_resolution(
	output: &mut String,
	options: RenderOptions,
	resolution: SymbolResolution,
	indent: &str,
) {
	output.push_str(indent);
	output.push_str("evidence:\n");
	for evidence in resolution.evidence {
		render_symbol_evidence(output, options, &evidence, indent);
	}
	for missing in resolution.missing {
		output.push_str(indent);
		output.push_str(&format!("  - selector: {}\n", missing.selector));
		output.push_str(indent);
		output.push_str("    status: missing\n");
	}
}

fn render_symbol_evidence(
	output: &mut String,
	options: RenderOptions,
	evidence: &SymbolEvidence,
	indent: &str,
) {
	output.push_str(indent);
	output.push_str(&format!("  - selector: {}\n", evidence.selector));
	output.push_str(indent);
	output.push_str(&format!("    label: {}\n", evidence.label));
	if let Some(moniker) = options.moniker_display.render(&evidence.moniker) {
		output.push_str(indent);
		output.push_str(&format!("    moniker: {moniker}\n"));
	}
	output.push_str(indent);
	output.push_str(&format!("    file: {}\n", evidence.file));
	if let Some((start, end)) = evidence.slice {
		output.push_str(indent);
		output.push_str(&format!("    slice: L{start}-L{end}\n"));
	}
	if !evidence.code.is_empty() {
		output.push_str(indent);
		output.push_str("    code:\n");
		for (line, text) in &evidence.code {
			output.push_str(indent);
			output.push_str(&format!("      {line:>4} | {text}\n"));
		}
	}
}

fn render_rule_catalog(output: &mut String, rules: &BTreeMap<String, RuleEvidence>) {
	if rules.is_empty() {
		return;
	}
	output.push_str("\nrules:\n");
	for rule in rules.values() {
		output.push_str(&format!(
			"  - {} [{}] domain={}\n",
			rule.id, rule.severity, rule.domain
		));
		if let Some(rationale) = &rule.rationale {
			output.push_str("    rationale:\n");
			render_text_block(output, rationale, "      ");
		}
	}
}

fn render_rule_refs(
	output: &mut String,
	label: &str,
	rule_ids: &[String],
	rules: &BTreeMap<String, RuleEvidence>,
	indent: &str,
) {
	if rule_ids.is_empty() {
		return;
	}
	output.push_str(indent);
	output.push_str(label);
	output.push_str(":\n");
	for rule_id in rule_ids {
		if !rules.contains_key(rule_id) {
			output.push_str(indent);
			output.push_str(&format!("  - {rule_id} [missing]\n"));
			continue;
		}
		output.push_str(indent);
		output.push_str(&format!("  - {rule_id}\n"));
	}
}

fn render_forbids(output: &mut String, boundary: &BoundarySpec, indent: &str) {
	if boundary.forbids.is_empty() {
		return;
	}
	output.push_str(indent);
	output.push_str("forbids:\n");
	for value in &boundary.forbids {
		output.push_str(indent);
		output.push_str(&format!("  - {value}\n"));
	}
	output.push_str(indent);
	if boundary.forbid_rules.is_empty() {
		output.push_str("forbids_status: advisory\n");
	} else {
		output.push_str("forbids_status: enforced_by_rules\n");
		render_list(output, "forbid_rules", &boundary.forbid_rules, indent);
	}
}

fn render_list(output: &mut String, label: &str, values: &[String], indent: &str) {
	if values.is_empty() {
		return;
	}
	output.push_str(indent);
	output.push_str(label);
	output.push_str(":\n");
	for value in values {
		output.push_str(indent);
		output.push_str(&format!("  - {value}\n"));
	}
}

fn render_text_block(output: &mut String, text: &str, indent: &str) {
	for line in text.trim().lines() {
		output.push_str(indent);
		output.push_str(line.trim());
		output.push('\n');
	}
}

fn render_next(output: &mut String, scheme: &str, view: &ViewDocument) {
	output.push_str("\nnext:\n");
	output.push_str(&format!(
		"  - code_moniker_symbols uri=\"{scheme}workspace\" path=\"{}**\" limit=50\n",
		next_scope_path(view)
	));
	output.push_str(&format!(
		"  - code_moniker_rules uri=\"{scheme}workspace\" action=\"list\" limit=50\n"
	));
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

fn scope_label(view: &ViewDocument) -> &str {
	if view.scope_path.is_empty() {
		"."
	} else {
		&view.scope_path
	}
}

fn next_scope_path(view: &ViewDocument) -> String {
	if view.scope_path.is_empty() {
		String::new()
	} else {
		format!("{}/", view.scope_path)
	}
}
