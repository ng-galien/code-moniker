use std::collections::HashMap;

mod collection;
mod layout;
mod local;
mod metrics;
mod pairs;
mod value;

use crate::check::config::{Config, ConfigError, KindRules, RuleSeverity, config_section};
use crate::check::expr::{
	self, Atom, Domain, Lhs, LhsExpr, Node, NumberExpr, Op, QuantKind, Rhs, SegmentScope,
	VerticalLayout,
};
use code_moniker_core::core::code_graph::{CodeGraph, DefRecord};
use code_moniker_core::core::kinds::{KIND_COMMENT, REF_CALLS, REF_METHOD_CALL};
use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::core::shape::Shape;
use code_moniker_core::core::uri::{UriConfig, to_uri};
use code_moniker_core::lang::Lang;
use code_moniker_workspace::lines::line_range;

use collection::{collection_has_pair_binding, eval_collection_size, eval_collection_subset};
use layout::eval_vertical_layout;
use local::{
	AggregateEval, DomainItem, domain_items, eval_aggregate, eval_entropy, eval_mode,
	project_def_lhs_value,
};
use metrics::eval_metric;
use pairs::{eval_pair_count, eval_pair_quantifier};
use value::{Value, apply_op, apply_op_values, number_expr_label};

fn is_call_ref_kind(kind: &[u8]) -> bool {
	matches!(kind, REF_CALLS | REF_METHOD_CALL)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Violation {
	pub rule_id: String,
	pub severity: RuleSeverity,
	pub moniker: String,
	pub kind: String,
	#[serde(serialize_with = "serialize_lines")]
	pub lines: (u32, u32),
	pub message: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub explanation: Option<String>,
}

fn serialize_lines<S: serde::Serializer>(v: &(u32, u32), s: S) -> Result<S::Ok, S::Error> {
	use serde::ser::SerializeTuple;
	let mut t = s.serialize_tuple(2)?;
	t.serialize_element(&v.0)?;
	t.serialize_element(&v.1)?;
	t.end()
}

#[cfg(test)]
pub(crate) fn evaluate(
	graph: &CodeGraph,
	source: &str,
	lang: Lang,
	cfg: &Config,
	scheme: &str,
) -> Result<Vec<Violation>, ConfigError> {
	let compiled = compile_rules(cfg, lang, scheme)?;
	Ok(evaluate_compiled(graph, source, lang, scheme, &compiled))
}

/// Build the compiled rule set for a single `lang`. Parses every rule
/// expression, resolves aliases. Call once per language and reuse across
/// many files of that language — the eval pipeline is shaped so the heavy
/// work happens here, not per-file.
pub fn compile_rules(cfg: &Config, lang: Lang, scheme: &str) -> Result<CompiledRules, ConfigError> {
	CompiledRules::for_lang(cfg, lang, scheme)
}

pub fn evaluate_compiled(
	graph: &CodeGraph,
	source: &str,
	lang: Lang,
	scheme: &str,
	compiled: &CompiledRules,
) -> Vec<Violation> {
	evaluate_compiled_with_requirements(graph, source, lang, scheme, compiled, None)
}

pub(in crate::check) trait RequirementResolver: Sync {
	fn exists(&self, pattern: &str, source: &DefRecord, scheme: &str) -> bool;
	fn descendant_defs<'a>(&'a self, _owner: &DefRecord, _inner: &Domain) -> Vec<&'a DefRecord> {
		Vec::new()
	}
}

pub(in crate::check) fn evaluate_compiled_with_requirements(
	graph: &CodeGraph,
	source: &str,
	lang: Lang,
	scheme: &str,
	compiled: &CompiledRules,
	requirements: Option<&dyn RequirementResolver>,
) -> Vec<Violation> {
	let need_doc_anchors = compiled
		.by_kind
		.values()
		.any(|r| r.require_doc_for_vis.is_some())
		|| compiled
			.by_shape
			.values()
			.any(|r| r.require_doc_for_vis.is_some());
	let ctx = EvalCtx {
		graph,
		requirements,
		source,
		lang,
		uri_cfg: UriConfig { scheme },
		parent_counts: parent_counts_by_kind(graph),
		children_by_parent: children_by_parent(graph),
		out_refs_by_source: out_refs_by_source(graph),
		in_refs_by_target: in_refs_by_target(graph),
		comment_ends: if need_doc_anchors {
			comment_end_bytes(graph)
		} else {
			Vec::new()
		},
		doc_anchors: if need_doc_anchors {
			doc_anchors_by_def(graph)
		} else {
			HashMap::new()
		},
	};
	let mut out = Vec::new();

	for (idx, d) in graph.defs().enumerate() {
		let Ok(kind_str) = std::str::from_utf8(&d.kind) else {
			continue;
		};
		let kind_rules = compiled.for_kind(kind_str);
		if let Some(rules) = kind_rules {
			let target = RuleTarget {
				scope: DefScope { record: d, idx },
				kind: kind_str,
			};
			for rule in &rules.rules {
				eval_rule(rule, d, idx, kind_str, &ctx, &mut out);
			}
			check_require_doc_comment(target, rules, &ctx, &mut out);
		}
		if let Some(shape) = d.shape()
			&& let Some(rules) = compiled.for_shape(shape)
		{
			for rule in &rules.rules {
				if rule.explicit_id
					&& kind_rules
						.is_some_and(|kind_rules| kind_rules.has_explicit_rule_id(&rule.id))
				{
					continue;
				}
				eval_shape_rule(rule, d, idx, kind_str, &ctx, &mut out);
			}
			if kind_rules.is_none_or(|kind_rules| kind_rules.require_doc_for_vis.is_none()) {
				if let Some(rule_id) = &rules.require_doc_rule_id {
					let target = RuleTarget {
						scope: DefScope { record: d, idx },
						kind: kind_str,
					};
					check_require_doc_comment_with_id(
						target,
						rules,
						rule_id.clone(),
						&ctx,
						&mut out,
					);
				}
			}
		}
	}

	for r in graph.refs() {
		for rule in &compiled.refs {
			eval_ref_rule(rule, r, graph, &ctx, &mut out);
		}
	}

	out
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RuleReport {
	pub rule_id: String,
	pub severity: RuleSeverity,
	pub domain: String,
	pub evaluated: usize,
	pub matches: usize,
	pub violations: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub antecedent_matches: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub warning: Option<String>,
}

pub fn rule_report_compiled(
	graph: &CodeGraph,
	source: &str,
	lang: Lang,
	scheme: &str,
	compiled: &CompiledRules,
) -> Vec<RuleReport> {
	rule_report_compiled_with_requirements(graph, source, lang, scheme, compiled, None)
}

pub(in crate::check) fn rule_report_compiled_with_requirements(
	graph: &CodeGraph,
	source: &str,
	lang: Lang,
	scheme: &str,
	compiled: &CompiledRules,
	requirements: Option<&dyn RequirementResolver>,
) -> Vec<RuleReport> {
	let need_doc_anchors = compiled
		.by_kind
		.values()
		.any(|r| r.require_doc_for_vis.is_some())
		|| compiled
			.by_shape
			.values()
			.any(|r| r.require_doc_for_vis.is_some());
	let ctx = EvalCtx {
		graph,
		requirements,
		source,
		lang,
		uri_cfg: UriConfig { scheme },
		parent_counts: parent_counts_by_kind(graph),
		children_by_parent: children_by_parent(graph),
		out_refs_by_source: out_refs_by_source(graph),
		in_refs_by_target: in_refs_by_target(graph),
		comment_ends: if need_doc_anchors {
			comment_end_bytes(graph)
		} else {
			Vec::new()
		},
		doc_anchors: if need_doc_anchors {
			doc_anchors_by_def(graph)
		} else {
			HashMap::new()
		},
	};
	let mut out = Vec::new();
	push_kind_rule_reports(&mut out, graph, lang, &ctx, compiled);
	push_shape_rule_reports(&mut out, graph, &ctx, compiled);
	push_ref_rule_reports(&mut out, graph, &ctx, compiled);
	out.sort_by(|a, b| a.rule_id.cmp(&b.rule_id));
	out
}

fn push_kind_rule_reports(
	out: &mut Vec<RuleReport>,
	graph: &CodeGraph,
	lang: Lang,
	ctx: &EvalCtx<'_, '_>,
	compiled: &CompiledRules,
) {
	for (kind, rules) in &compiled.by_kind {
		for rule in &rules.rules {
			let mut report = RuleReport::new(rule_id(lang, kind, &rule.id), kind.clone(), rule);
			for (idx, d) in graph.defs().enumerate() {
				if d.kind.as_ref() != kind.as_bytes() {
					continue;
				}
				report.evaluated += 1;
				let premise =
					implication_premise(rule).map(|premise| eval_node(premise, d, idx, ctx));
				report.record(eval_node(&rule.root, d, idx, ctx), premise);
			}
			out.push(report);
		}
		if rules.require_doc_for_vis.is_some() {
			let mut report = RuleReport::new_require_doc(
				rule_id(lang, kind, "require_doc_comment"),
				kind.clone(),
			);
			for (idx, d) in graph.defs().enumerate() {
				if d.kind.as_ref() != kind.as_bytes() {
					continue;
				}
				report.evaluated += 1;
				report.record(
					eval_require_doc_comment(d, idx, rules, ctx).map_or(
						NodeOutcome::NotApplicable,
						|has_doc| {
							if has_doc {
								NodeOutcome::Pass
							} else {
								NodeOutcome::Fail(Failure {
									atom_raw: "require_doc_comment".to_string(),
									lhs_label: "doc_comment".to_string(),
									actual: "missing".to_string(),
									expected: "present".to_string(),
									def_idx: None,
									details: None,
								})
							}
						},
					),
					None,
				);
			}
			out.push(report);
		}
	}
}

fn push_shape_rule_reports(
	out: &mut Vec<RuleReport>,
	graph: &CodeGraph,
	ctx: &EvalCtx<'_, '_>,
	compiled: &CompiledRules,
) {
	for (shape, rules) in &compiled.by_shape {
		for rule in &rules.rules {
			let mut report =
				RuleReport::new(rule.rule_id.clone(), format!("shape:{shape} defs"), rule);
			for (idx, d) in graph.defs().enumerate() {
				if !def_has_shape(d, shape) {
					continue;
				}
				let Ok(kind_str) = std::str::from_utf8(&d.kind) else {
					continue;
				};
				if compiled.for_kind(kind_str).is_some_and(|kind_rules| {
					rule.explicit_id && kind_rules.has_explicit_rule_id(&rule.id)
				}) {
					continue;
				}
				report.evaluated += 1;
				let premise =
					implication_premise(rule).map(|premise| eval_node(premise, d, idx, ctx));
				report.record(eval_node(&rule.root, d, idx, ctx), premise);
			}
			out.push(report);
		}
		if let Some(rule_id) = &rules.require_doc_rule_id {
			let mut report =
				RuleReport::new_require_doc(rule_id.clone(), format!("shape:{shape} defs"));
			for (idx, d) in graph.defs().enumerate() {
				if !def_has_shape(d, shape) {
					continue;
				}
				let Ok(kind_str) = std::str::from_utf8(&d.kind) else {
					continue;
				};
				if compiled
					.for_kind(kind_str)
					.is_some_and(|kind_rules| kind_rules.require_doc_for_vis.is_some())
				{
					continue;
				}
				report.evaluated += 1;
				report.record(
					eval_require_doc_comment(d, idx, rules, ctx).map_or(
						NodeOutcome::NotApplicable,
						|has_doc| {
							if has_doc {
								NodeOutcome::Pass
							} else {
								NodeOutcome::Fail(Failure {
									atom_raw: "require_doc_comment".to_string(),
									lhs_label: "doc_comment".to_string(),
									actual: "missing".to_string(),
									expected: "present".to_string(),
									def_idx: None,
									details: None,
								})
							}
						},
					),
					None,
				);
			}
			out.push(report);
		}
	}
}

fn push_ref_rule_reports(
	out: &mut Vec<RuleReport>,
	graph: &CodeGraph,
	ctx: &EvalCtx<'_, '_>,
	compiled: &CompiledRules,
) {
	for rule in &compiled.refs {
		let mut report = RuleReport::new(rule.rule_id.clone(), "refs".to_string(), rule);
		for r in graph.refs() {
			report.evaluated += 1;
			let premise = implication_premise(rule).map(|premise| eval_ref_node(premise, r, ctx));
			report.record(eval_ref_node(&rule.root, r, ctx), premise);
		}
		out.push(report);
	}
}

impl RuleReport {
	fn new(rule_id: String, domain: String, rule: &CompiledRule) -> Self {
		Self {
			rule_id,
			severity: rule.severity,
			domain,
			evaluated: 0,
			matches: 0,
			violations: 0,
			antecedent_matches: implication_premise(rule).map(|_| 0),
			warning: None,
		}
	}

	fn new_require_doc(rule_id: String, domain: String) -> Self {
		Self {
			rule_id,
			severity: RuleSeverity::Error,
			domain,
			evaluated: 0,
			matches: 0,
			violations: 0,
			antecedent_matches: None,
			warning: None,
		}
	}

	fn record(&mut self, outcome: NodeOutcome, premise: Option<NodeOutcome>) {
		if matches!(premise, Some(NodeOutcome::Pass)) {
			self.antecedent_matches = Some(self.antecedent_matches.unwrap_or(0) + 1);
		}
		match outcome {
			NodeOutcome::Pass => {
				if premise.is_none() || matches!(premise, Some(NodeOutcome::Pass)) {
					self.matches += 1;
				}
			}
			NodeOutcome::Fail(_) => self.violations += 1,
			NodeOutcome::NotApplicable => {}
		}
	}
}

fn implication_premise(rule: &CompiledRule) -> Option<&Node> {
	match &rule.root {
		Node::Implies(premise, _) => Some(premise),
		_ => None,
	}
}

struct EvalCtx<'g, 'src> {
	graph: &'g CodeGraph,
	requirements: Option<&'g dyn RequirementResolver>,
	source: &'src str,
	lang: Lang,
	uri_cfg: UriConfig<'src>,
	parent_counts: HashMap<(usize, &'g [u8]), u32>,
	children_by_parent: HashMap<usize, Vec<usize>>,
	out_refs_by_source: HashMap<usize, Vec<usize>>,
	in_refs_by_target: HashMap<Vec<u8>, Vec<usize>>,
	comment_ends: Vec<u32>,
	doc_anchors: HashMap<usize, u32>,
}

#[derive(Debug)]
struct CompiledRule {
	id: String,
	explicit_id: bool,
	rule_id: String,
	raw_expr: String,
	expanded_expr: String,
	root: Node,
	severity: RuleSeverity,
	message: Option<String>,
	rationale: Option<String>,
}

#[derive(Default)]
struct CompiledKindRules {
	rules: Vec<CompiledRule>,
	require_doc_for_vis: Option<String>,
	require_doc_rule_id: Option<String>,
}

pub struct CompiledRules {
	by_kind: HashMap<String, CompiledKindRules>,
	by_shape: HashMap<String, CompiledKindRules>,
	refs: Vec<CompiledRule>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CompiledRuleSpec {
	pub rule_id: String,
	pub severity: RuleSeverity,
	pub lang: String,
	pub domain: String,
	pub kind: Option<String>,
	pub expr: String,
	pub expanded_expr: String,
	pub message: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub rationale: Option<String>,
	pub require_doc_comment: Option<String>,
}

impl CompiledRules {
	fn for_lang(cfg: &Config, lang: Lang, scheme: &str) -> Result<Self, ConfigError> {
		compile_rules_for_lang(cfg, lang, scheme)
	}

	fn for_kind(&self, kind: &str) -> Option<&CompiledKindRules> {
		self.by_kind.get(kind)
	}

	fn for_shape(&self, shape: Shape) -> Option<&CompiledKindRules> {
		self.by_shape.get(shape.as_str())
	}

	pub fn specs(&self, lang: Lang) -> Vec<CompiledRuleSpec> {
		compiled_rule_specs(self, lang)
	}
}

fn compile_rules_for_lang(
	cfg: &Config,
	lang: Lang,
	scheme: &str,
) -> Result<CompiledRules, ConfigError> {
	let section = config_section(lang);
	let allowed = crate::check::config::allowed_kinds_for(lang);
	let aliases = crate::check::config::resolve_aliases(&cfg.aliases)?;
	let mut by_kind: HashMap<String, CompiledKindRules> = HashMap::new();
	let mut by_shape: HashMap<String, CompiledKindRules> = HashMap::new();
	let mut per_lang_refs: Vec<&crate::check::config::RuleEntry> = Vec::new();
	for (kind, rules) in cfg.for_lang(lang).kinds.iter() {
		if kind == "refs" {
			per_lang_refs.extend(rules.rules.iter());
			continue;
		}
		by_kind.insert(
			kind.clone(),
			compile(rules, section, kind, scheme, &allowed, &aliases)?,
		);
	}
	for (kind, rules) in cfg.default.kinds.iter() {
		if kind == "refs" {
			continue;
		}
		if !allowed.contains(&kind.as_str()) {
			continue;
		}
		if !by_kind.contains_key(kind.as_str()) {
			by_kind.insert(
				kind.clone(),
				compile(rules, "default", kind, scheme, &allowed, &aliases)?,
			);
		}
	}
	compile_shape_rules_into(
		&mut by_shape,
		&cfg.shape,
		"shape",
		scheme,
		&allowed,
		&aliases,
	)?;
	compile_shape_rules_into(
		&mut by_shape,
		&cfg.for_lang(lang).shape,
		&format!("{section}.shape"),
		scheme,
		&allowed,
		&aliases,
	)?;
	let mut refs = Vec::with_capacity(cfg.refs.rules.len() + per_lang_refs.len());
	for (idx, entry) in cfg.refs.rules.iter().enumerate() {
		let id = entry.fallback_id(idx);
		let at = format!("refs.{id}");
		refs.push(compile_rule_entry(
			entry, id, at, scheme, &allowed, &aliases,
		)?);
	}
	for (idx, entry) in per_lang_refs.iter().enumerate() {
		let id = entry.fallback_id(idx);
		let at = format!("{section}.refs.{id}");
		refs.push(compile_rule_entry(
			entry, id, at, scheme, &allowed, &aliases,
		)?);
	}
	Ok(CompiledRules {
		by_kind,
		by_shape,
		refs,
	})
}

fn compiled_rule_specs(rules: &CompiledRules, lang: Lang) -> Vec<CompiledRuleSpec> {
	let mut out = Vec::new();
	for (kind, rules) in &rules.by_kind {
		for rule in &rules.rules {
			out.push(CompiledRuleSpec {
				rule_id: rule_id(lang, kind, &rule.id),
				lang: lang.tag().to_string(),
				domain: format!("{kind} defs"),
				kind: Some(kind.clone()),
				expr: rule.raw_expr.clone(),
				expanded_expr: rule.expanded_expr.clone(),
				message: rule.message.clone(),
				severity: rule.severity,
				rationale: rule.rationale.clone(),
				require_doc_comment: None,
			});
		}
		if let Some(value) = &rules.require_doc_for_vis {
			out.push(CompiledRuleSpec {
				rule_id: rule_id(lang, kind, "require_doc_comment"),
				lang: lang.tag().to_string(),
				domain: format!("{kind} defs"),
				kind: Some(kind.clone()),
				expr: format!("require_doc_comment = \"{value}\""),
				expanded_expr: format!("require_doc_comment = \"{value}\""),
				message: None,
				severity: RuleSeverity::Error,
				rationale: None,
				require_doc_comment: Some(value.clone()),
			});
		}
	}
	for (shape, rules) in &rules.by_shape {
		for rule in &rules.rules {
			out.push(CompiledRuleSpec {
				rule_id: rule.rule_id.clone(),
				lang: lang.tag().to_string(),
				domain: format!("shape:{shape} defs"),
				kind: None,
				expr: rule.raw_expr.clone(),
				expanded_expr: rule.expanded_expr.clone(),
				message: rule.message.clone(),
				severity: rule.severity,
				rationale: rule.rationale.clone(),
				require_doc_comment: None,
			});
		}
		if let (Some(value), Some(rule_id)) =
			(&rules.require_doc_for_vis, &rules.require_doc_rule_id)
		{
			out.push(CompiledRuleSpec {
				rule_id: rule_id.clone(),
				lang: lang.tag().to_string(),
				domain: format!("shape:{shape} defs"),
				kind: None,
				expr: format!("require_doc_comment = \"{value}\""),
				expanded_expr: format!("require_doc_comment = \"{value}\""),
				message: None,
				severity: RuleSeverity::Error,
				rationale: None,
				require_doc_comment: Some(value.clone()),
			});
		}
	}
	for rule in &rules.refs {
		out.push(CompiledRuleSpec {
			rule_id: rule.rule_id.clone(),
			lang: lang.tag().to_string(),
			domain: "refs".to_string(),
			kind: None,
			expr: rule.raw_expr.clone(),
			expanded_expr: rule.expanded_expr.clone(),
			message: rule.message.clone(),
			severity: rule.severity,
			rationale: rule.rationale.clone(),
			require_doc_comment: None,
		});
	}
	out.sort_by(|a, b| a.rule_id.cmp(&b.rule_id));
	out
}

fn compile_rule_entry(
	entry: &crate::check::config::RuleEntry,
	id: String,
	at: String,
	scheme: &str,
	allowed_kinds: &[&str],
	aliases: &HashMap<String, String>,
) -> Result<CompiledRule, ConfigError> {
	let expanded = crate::check::config::substitute_aliases(&entry.expr, aliases, &at)?;
	let parsed = expr::parse(&expanded, scheme, allowed_kinds).map_err(|error| {
		ConfigError::InvalidExpr {
			at: at.clone(),
			error,
		}
	})?;
	Ok(CompiledRule {
		id,
		explicit_id: entry.id.is_some(),
		rule_id: at,
		raw_expr: entry.expr.clone(),
		expanded_expr: expanded,
		root: parsed.root,
		message: entry.message.clone(),
		severity: entry.severity,
		rationale: entry.rationale.clone(),
	})
}

fn compile(
	rules: &KindRules,
	section: &str,
	kind: &str,
	scheme: &str,
	allowed_kinds: &[&str],
	aliases: &HashMap<String, String>,
) -> Result<CompiledKindRules, ConfigError> {
	let mut compiled = Vec::with_capacity(rules.rules.len());
	for (idx, entry) in rules.rules.iter().enumerate() {
		let id = entry.fallback_id(idx);
		let at = format!("{section}.{kind}.{id}");
		let expanded = crate::check::config::substitute_aliases(&entry.expr, aliases, &at)?;
		let parsed = expr::parse(&expanded, scheme, allowed_kinds).map_err(|error| {
			ConfigError::InvalidExpr {
				at: at.clone(),
				error,
			}
		})?;
		compiled.push(CompiledRule {
			id,
			explicit_id: entry.id.is_some(),
			rule_id: at,
			raw_expr: entry.expr.clone(),
			expanded_expr: expanded,
			root: parsed.root,
			message: entry.message.clone(),
			severity: entry.severity,
			rationale: entry.rationale.clone(),
		});
	}
	Ok(CompiledKindRules {
		rules: compiled,
		require_doc_for_vis: rules.require_doc_comment.clone(),
		require_doc_rule_id: rules
			.require_doc_comment
			.as_ref()
			.map(|_| format!("{section}.{kind}.require_doc_comment")),
	})
}

fn compile_shape_rules_into(
	dst: &mut HashMap<String, CompiledKindRules>,
	src: &HashMap<String, KindRules>,
	section: &str,
	scheme: &str,
	allowed_kinds: &[&str],
	aliases: &HashMap<String, String>,
) -> Result<(), ConfigError> {
	for (shape, rules) in src {
		let compiled = compile(rules, section, shape, scheme, allowed_kinds, aliases)?;
		match dst.get_mut(shape) {
			Some(existing) => merge_compiled_kind_rules(existing, compiled),
			None => {
				dst.insert(shape.clone(), compiled);
			}
		}
	}
	Ok(())
}

fn merge_compiled_kind_rules(base: &mut CompiledKindRules, ov: CompiledKindRules) {
	for rule in ov.rules {
		match rule
			.explicit_id
			.then(|| {
				base.rules
					.iter()
					.position(|r| r.explicit_id && r.id == rule.id)
			})
			.flatten()
		{
			Some(idx) => base.rules[idx] = rule,
			None => base.rules.push(rule),
		}
	}
	if ov.require_doc_for_vis.is_some() {
		base.require_doc_for_vis = ov.require_doc_for_vis;
		base.require_doc_rule_id = ov.require_doc_rule_id;
	}
}

impl CompiledKindRules {
	fn has_explicit_rule_id(&self, id: &str) -> bool {
		self.rules
			.iter()
			.any(|rule| rule.explicit_id && rule.id == id)
	}
}

fn rule_id(lang: Lang, kind: &str, rule: &str) -> String {
	format!("{}.{}.{}", config_section(lang), kind, rule)
}

fn lines_of(d: &DefRecord, source: &str) -> (u32, u32) {
	match d.position {
		Some((s, e)) => line_range(source, s, e),
		None => (0, 0),
	}
}

fn def_name(d: &DefRecord) -> Option<String> {
	let last = d.moniker.as_view().segments().last()?;
	let bare = bare_callable_name(last.name);
	std::str::from_utf8(bare).ok().map(|s| s.to_string())
}

fn render_template(tpl: &str, vars: &[(&str, &str)]) -> String {
	let mut out = tpl.to_string();
	for (k, v) in vars {
		let placeholder = format!("{{{k}}}");
		if out.contains(&placeholder) {
			out = out.replace(&placeholder, v);
		}
	}
	out
}

#[derive(Clone, Copy)]
struct DefScope<'a> {
	record: &'a DefRecord,
	idx: usize,
}

#[derive(Clone, Copy)]
struct RuleTarget<'a> {
	scope: DefScope<'a>,
	kind: &'a str,
}

fn eval_rule(
	rule: &CompiledRule,
	d: &DefRecord,
	def_idx: usize,
	kind: &str,
	ctx: &EvalCtx<'_, '_>,
	out: &mut Vec<Violation>,
) {
	let target = RuleTarget {
		scope: DefScope {
			record: d,
			idx: def_idx,
		},
		kind,
	};
	eval_rule_with_id(rule, target, rule_id(ctx.lang, kind, &rule.id), ctx, out);
}

fn eval_shape_rule(
	rule: &CompiledRule,
	d: &DefRecord,
	def_idx: usize,
	kind: &str,
	ctx: &EvalCtx<'_, '_>,
	out: &mut Vec<Violation>,
) {
	let target = RuleTarget {
		scope: DefScope {
			record: d,
			idx: def_idx,
		},
		kind,
	};
	eval_rule_with_id(rule, target, rule.rule_id.clone(), ctx, out);
}

fn eval_rule_with_id(
	rule: &CompiledRule,
	target: RuleTarget<'_>,
	rule_id: String,
	ctx: &EvalCtx<'_, '_>,
	out: &mut Vec<Violation>,
) {
	let Failure {
		atom_raw,
		lhs_label,
		actual,
		expected,
		def_idx,
		details,
	} = match eval_node(&rule.root, target.scope.record, target.scope.idx, ctx) {
		NodeOutcome::Pass | NodeOutcome::NotApplicable => return,
		NodeOutcome::Fail(f) => f,
	};
	let diagnostic = def_idx
		.map(|idx| ctx.graph.def_at(idx))
		.unwrap_or(target.scope.record);
	let diagnostic_kind = std::str::from_utf8(&diagnostic.kind).unwrap_or(target.kind);
	let name = def_name(diagnostic).unwrap_or_default();
	let name_snake = to_snake_case(&name);
	let moniker = to_uri(&diagnostic.moniker, &ctx.uri_cfg);
	let (start_line, end_line) = lines_of(diagnostic, ctx.source);
	let message = format!(
		"{diagnostic_kind} `{name}` fails `{atom_raw}` ({lhs_label} = {actual}, expected {expected})",
	);
	let explanation = rule
		.message
		.as_ref()
		.map(|tpl| {
			let mut rendered = render_template(
				tpl,
				&[
					("name", &name),
					("name.snake", &name_snake),
					("kind", diagnostic_kind),
					("moniker", &moniker),
					("expr", &rule.raw_expr),
					("actual", &actual),
					("value", &actual),
					("expected", &expected),
					("pattern", &expected),
					("lines", &actual),
					("limit", &expected),
					("count", &actual),
				],
			);
			if let Some(details) = &details {
				if !rendered.is_empty() {
					rendered.push('\n');
				}
				rendered.push_str(details);
			}
			rendered
		})
		.or(details);
	out.push(Violation {
		rule_id,
		severity: rule.severity,
		moniker,
		kind: diagnostic_kind.to_string(),
		lines: (start_line, end_line),
		message,
		explanation,
	});
}

fn eval_ref_rule(
	rule: &CompiledRule,
	r: &code_moniker_core::core::code_graph::RefRecord,
	graph: &CodeGraph,
	ctx: &EvalCtx<'_, '_>,
	out: &mut Vec<Violation>,
) {
	let Failure {
		atom_raw,
		lhs_label,
		actual,
		expected,
		def_idx: _,
		details,
	} = match eval_ref_node(&rule.root, r, ctx) {
		NodeOutcome::Pass | NodeOutcome::NotApplicable => return,
		NodeOutcome::Fail(f) => f,
	};
	let source_def = graph.def_at(r.source);
	let source_uri = to_uri(&source_def.moniker, &ctx.uri_cfg);
	let target_uri = to_uri(&r.target, &ctx.uri_cfg);
	let ref_kind = std::str::from_utf8(&r.kind).unwrap_or_default();
	let (start_line, end_line) = match r.position {
		Some((s, e)) => line_range(ctx.source, s, e),
		None => (0, 0),
	};
	let message = format!(
		"ref {ref_kind} {source_uri} → {target_uri} fails `{atom_raw}` ({lhs_label} = {actual}, expected {expected})"
	);
	let source_name = name_of(&source_def.moniker).unwrap_or_default();
	let source_kind = last_segment_kind(&source_def.moniker).unwrap_or_default();
	let source_shape = shape_name_of_last_segment(&source_def.moniker);
	let target_name = name_of(&r.target).unwrap_or_default();
	let target_kind = last_segment_kind(&r.target).unwrap_or_default();
	let target_shape = shape_name_of_last_segment(&r.target);
	let explanation = rule
		.message
		.as_ref()
		.map(|tpl| {
			let mut rendered = render_template(
				tpl,
				&[
					("kind", ref_kind),
					("source.name", &source_name),
					("source.kind", &source_kind),
					("source.shape", &source_shape),
					("source.moniker", &source_uri),
					("target.name", &target_name),
					("target.kind", &target_kind),
					("target.shape", &target_shape),
					("target.moniker", &target_uri),
					("atom", &atom_raw),
					("actual", &actual),
					("expected", &expected),
				],
			);
			if let Some(details) = &details {
				if !rendered.is_empty() {
					rendered.push('\n');
				}
				rendered.push_str(details);
			}
			rendered
		})
		.or(details);
	out.push(Violation {
		rule_id: rule.rule_id.clone(),
		severity: rule.severity,
		moniker: target_uri,
		kind: ref_kind.to_string(),
		lines: (start_line, end_line),
		message,
		explanation,
	});
}

fn eval_ref_node(
	node: &Node,
	r: &code_moniker_core::core::code_graph::RefRecord,
	ctx: &EvalCtx<'_, '_>,
) -> NodeOutcome {
	walk_node(
		node,
		&|a| eval_ref_atom(a, r, ctx),
		&|_, _, _| NodeOutcome::NotApplicable,
		&|_| NodeOutcome::NotApplicable,
		&|_| NodeOutcome::NotApplicable,
	)
}

fn eval_ref_atom(
	atom: &Atom,
	r: &code_moniker_core::core::code_graph::RefRecord,
	ctx: &EvalCtx<'_, '_>,
) -> AtomOutcome {
	let graph = ctx.graph;
	let source_def = graph.def_at(r.source);
	let value: Value = match &atom.lhs {
		LhsExpr::Attr(Lhs::Kind) => {
			Value::Str(std::str::from_utf8(&r.kind).unwrap_or_default().to_string())
		}
		LhsExpr::Attr(Lhs::Confidence) => Value::Str(
			std::str::from_utf8(&r.confidence)
				.unwrap_or_default()
				.to_string(),
		),
		LhsExpr::Attr(Lhs::Moniker) | LhsExpr::Attr(Lhs::SourceMoniker) => {
			Value::Moniker(source_def.moniker.clone())
		}
		LhsExpr::Attr(Lhs::ParentMoniker) | LhsExpr::Attr(Lhs::SourceParentMoniker) => {
			match source_def.moniker.parent() {
				Some(parent) => Value::Moniker(parent),
				None => return AtomOutcome::NotApplicable,
			}
		}
		LhsExpr::Attr(Lhs::TargetMoniker) => Value::Moniker(r.target.clone()),
		LhsExpr::Attr(Lhs::TargetParentMoniker) => match r.target.parent() {
			Some(parent) => Value::Moniker(parent),
			None => return AtomOutcome::NotApplicable,
		},
		LhsExpr::Attr(Lhs::SourceName) => match name_of(&source_def.moniker) {
			Some(n) => Value::Str(n),
			None => return AtomOutcome::NotApplicable,
		},
		LhsExpr::Attr(Lhs::TargetName) => match name_of(&r.target) {
			Some(n) => Value::Str(n),
			None => return AtomOutcome::NotApplicable,
		},
		LhsExpr::Attr(Lhs::SourceKind) => match last_segment_kind(&source_def.moniker) {
			Some(k) => Value::Str(k),
			None => return AtomOutcome::NotApplicable,
		},
		LhsExpr::Attr(Lhs::TargetKind) => match last_segment_kind(&r.target) {
			Some(k) => Value::Str(k),
			None => return AtomOutcome::NotApplicable,
		},
		LhsExpr::Attr(Lhs::Shape) | LhsExpr::Attr(Lhs::SourceShape) => {
			match shape_of_last_segment(&source_def.moniker) {
				Some(s) => Value::Str(s.as_str().to_string()),
				None => return AtomOutcome::NotApplicable,
			}
		}
		LhsExpr::Attr(Lhs::TargetShape) => match shape_of_last_segment(&r.target) {
			Some(s) => Value::Str(s.as_str().to_string()),
			None => return AtomOutcome::NotApplicable,
		},
		LhsExpr::Attr(Lhs::ParentShape) => {
			let segs: Vec<_> = source_def.moniker.as_view().segments().collect();
			if segs.len() < 2 {
				return AtomOutcome::NotApplicable;
			}
			let parent_kind = segs[segs.len() - 2].kind;
			match code_moniker_core::core::shape::shape_of(parent_kind) {
				Some(s) => Value::Str(s.as_str().to_string()),
				None => return AtomOutcome::NotApplicable,
			}
		}
		LhsExpr::Attr(Lhs::SourceVisibility) => Value::Str(
			std::str::from_utf8(&source_def.visibility)
				.unwrap_or_default()
				.to_string(),
		),
		LhsExpr::Attr(Lhs::TargetVisibility) => match resolve_local_def(graph, &r.target) {
			Some(def) => Value::Str(
				std::str::from_utf8(&def.visibility)
					.unwrap_or_default()
					.to_string(),
			),
			None => return AtomOutcome::NotApplicable,
		},
		LhsExpr::SegmentOf { scope, kind } => match scope {
			SegmentScope::Def => {
				return AtomOutcome::NotApplicable;
			}
			SegmentScope::Source => {
				Value::Str(first_segment_name(&source_def.moniker, kind.as_bytes()))
			}
			SegmentScope::Target => Value::Str(first_segment_name(&r.target, kind.as_bytes())),
		},
		LhsExpr::Number(expr) => {
			let Some(n) = eval_number_expr_ref(expr, r, ctx) else {
				return AtomOutcome::NotApplicable;
			};
			Value::Number(n)
		}
		_ => return AtomOutcome::NotApplicable,
	};
	if let Rhs::Projection(other) = &atom.rhs {
		let Some(rhs_val) = resolve_ref_lhs(*other, r, ctx) else {
			return AtomOutcome::NotApplicable;
		};
		return apply_op_values(&value, atom.op, &rhs_val);
	}
	if let Rhs::Number(expr) = &atom.rhs {
		let Some(rhs_val) = eval_number_expr_ref(expr, r, ctx).map(Value::Number) else {
			return AtomOutcome::NotApplicable;
		};
		return apply_op_values(&value, atom.op, &rhs_val);
	}
	apply_op(&value, atom)
}

fn resolve_ref_lhs(
	lhs: Lhs,
	r: &code_moniker_core::core::code_graph::RefRecord,
	ctx: &EvalCtx<'_, '_>,
) -> Option<Value> {
	let graph = ctx.graph;
	let source_def = graph.def_at(r.source);
	Some(match lhs {
		Lhs::Kind => Value::Str(std::str::from_utf8(&r.kind).ok()?.to_string()),
		Lhs::Confidence => Value::Str(std::str::from_utf8(&r.confidence).ok()?.to_string()),
		Lhs::StartLine => {
			let (s, e) = r.position?;
			let (sl, _) = line_range(ctx.source, s, e);
			Value::Number(sl as f64)
		}
		Lhs::EndLine => {
			let (s, e) = r.position?;
			let (_, el) = line_range(ctx.source, s, e);
			Value::Number(el as f64)
		}
		Lhs::StartByte => {
			let (s, _) = r.position?;
			Value::Number(s as f64)
		}
		Lhs::EndByte => {
			let (_, e) = r.position?;
			Value::Number(e as f64)
		}
		Lhs::Moniker | Lhs::SourceMoniker => Value::Moniker(source_def.moniker.clone()),
		Lhs::ParentMoniker => Value::Moniker(source_def.moniker.parent()?),
		Lhs::SourceParentMoniker => Value::Moniker(source_def.moniker.parent()?),
		Lhs::TargetMoniker => Value::Moniker(r.target.clone()),
		Lhs::TargetParentMoniker => Value::Moniker(r.target.parent()?),
		Lhs::SourceName => Value::Str(name_of(&source_def.moniker)?),
		Lhs::TargetName => Value::Str(name_of(&r.target)?),
		Lhs::SourceKind => Value::Str(last_segment_kind(&source_def.moniker)?),
		Lhs::TargetKind => Value::Str(last_segment_kind(&r.target)?),
		Lhs::Shape | Lhs::SourceShape => Value::Str(
			shape_of_last_segment(&source_def.moniker)?
				.as_str()
				.to_string(),
		),
		Lhs::TargetShape => Value::Str(shape_of_last_segment(&r.target)?.as_str().to_string()),
		Lhs::ParentShape => {
			let segs: Vec<_> = source_def.moniker.as_view().segments().collect();
			if segs.len() < 2 {
				return None;
			}
			let parent_kind = segs[segs.len() - 2].kind;
			Value::Str(
				code_moniker_core::core::shape::shape_of(parent_kind)?
					.as_str()
					.to_string(),
			)
		}
		Lhs::SourceVisibility => Value::Str(
			std::str::from_utf8(&source_def.visibility)
				.ok()?
				.to_string(),
		),
		Lhs::TargetVisibility => {
			let def = resolve_local_def(graph, &r.target)?;
			Value::Str(std::str::from_utf8(&def.visibility).ok()?.to_string())
		}
		_ => return None,
	})
}

fn name_of(m: &code_moniker_core::core::moniker::Moniker) -> Option<String> {
	let last = m.as_view().segments().last()?;
	let bare = code_moniker_core::core::moniker::query::bare_callable_name(last.name);
	std::str::from_utf8(bare).ok().map(|s| s.to_string())
}

fn first_segment_name(m: &code_moniker_core::core::moniker::Moniker, kind: &[u8]) -> String {
	for seg in m.as_view().segments() {
		if seg.kind == kind {
			return std::str::from_utf8(seg.name)
				.unwrap_or_default()
				.to_string();
		}
	}
	String::new()
}

fn last_segment_kind(m: &code_moniker_core::core::moniker::Moniker) -> Option<String> {
	let last = m.as_view().segments().last()?;
	std::str::from_utf8(last.kind).ok().map(|s| s.to_string())
}

fn shape_of_last_segment(
	m: &code_moniker_core::core::moniker::Moniker,
) -> Option<code_moniker_core::core::shape::Shape> {
	let last = m.as_view().segments().last()?;
	code_moniker_core::core::shape::shape_of(last.kind)
}

fn shape_name_of_last_segment(m: &code_moniker_core::core::moniker::Moniker) -> String {
	shape_of_last_segment(m)
		.map(|shape| shape.as_str().to_string())
		.unwrap_or_default()
}

fn resolve_local_def<'g>(
	graph: &'g CodeGraph,
	m: &code_moniker_core::core::moniker::Moniker,
) -> Option<&'g DefRecord> {
	graph.defs().find(|d| d.moniker == *m)
}

fn describe_lhs(lhs: &LhsExpr) -> &str {
	match lhs {
		LhsExpr::Attr(a) => a.as_str(),
		LhsExpr::Number(n) => number_expr_label(n),
		LhsExpr::Collection(_) => "collection",
		LhsExpr::Mode(_) => "mode",
		LhsExpr::PairProjection(_) => "pair",
		LhsExpr::SegmentOf { .. } => "segment",
	}
}

#[derive(Debug)]
struct Failure {
	atom_raw: String,
	lhs_label: String,
	actual: String,
	expected: String,
	def_idx: Option<usize>,
	details: Option<String>,
}

#[derive(Debug)]
enum NodeOutcome {
	Pass,
	Fail(Failure),
	NotApplicable,
}

enum AtomOutcome {
	Pass,
	Fail { actual: String, expected: String },
	NotApplicable,
}

/// Trivalent-logic walker shared by def/ref/segment evaluators. `atom_eval`
/// produces the atom leaf outcome; `quant_eval` handles `Node::Quantifier`
/// (scopes that can't iterate, like ref and segment, return NotApplicable).
fn walk_node<A, Q, R, L>(
	node: &Node,
	atom_eval: &A,
	quant_eval: &Q,
	require_eval: &R,
	layout_eval: &L,
) -> NodeOutcome
where
	A: Fn(&Atom) -> AtomOutcome,
	Q: Fn(QuantKind, &Domain, &Node) -> NodeOutcome,
	R: Fn(&str) -> NodeOutcome,
	L: Fn(&VerticalLayout) -> NodeOutcome,
{
	match node {
		Node::Atom(atom) => match atom_eval(atom) {
			AtomOutcome::Pass => NodeOutcome::Pass,
			AtomOutcome::Fail { actual, expected } => NodeOutcome::Fail(Failure {
				atom_raw: atom.raw.clone(),
				lhs_label: describe_lhs(&atom.lhs).to_string(),
				actual,
				expected,
				def_idx: None,
				details: None,
			}),
			AtomOutcome::NotApplicable => NodeOutcome::NotApplicable,
		},
		Node::And(children) => {
			let mut na = false;
			for c in children {
				match walk_node(c, atom_eval, quant_eval, require_eval, layout_eval) {
					NodeOutcome::Pass => {}
					NodeOutcome::Fail(f) => return NodeOutcome::Fail(f),
					NodeOutcome::NotApplicable => na = true,
				}
			}
			if na {
				NodeOutcome::NotApplicable
			} else {
				NodeOutcome::Pass
			}
		}
		Node::Or(children) => {
			let mut last_fail: Option<Failure> = None;
			let mut na = false;
			for c in children {
				match walk_node(c, atom_eval, quant_eval, require_eval, layout_eval) {
					NodeOutcome::Pass => return NodeOutcome::Pass,
					NodeOutcome::Fail(f) => last_fail = Some(f),
					NodeOutcome::NotApplicable => na = true,
				}
			}
			if na {
				NodeOutcome::NotApplicable
			} else if let Some(f) = last_fail {
				NodeOutcome::Fail(f)
			} else {
				NodeOutcome::NotApplicable
			}
		}
		Node::Not(inner) => {
			match walk_node(inner, atom_eval, quant_eval, require_eval, layout_eval) {
				NodeOutcome::Pass => NodeOutcome::Fail(Failure {
					atom_raw: "NOT (...)".to_string(),
					lhs_label: "NOT".to_string(),
					actual: "true".to_string(),
					expected: "false".to_string(),
					def_idx: None,
					details: None,
				}),
				NodeOutcome::Fail(_) => NodeOutcome::Pass,
				NodeOutcome::NotApplicable => NodeOutcome::NotApplicable,
			}
		}
		Node::Implies(prem, cons) => {
			match walk_node(prem, atom_eval, quant_eval, require_eval, layout_eval) {
				NodeOutcome::Pass => {
					walk_node(cons, atom_eval, quant_eval, require_eval, layout_eval)
				}
				NodeOutcome::Fail(_) => NodeOutcome::Pass,
				NodeOutcome::NotApplicable => NodeOutcome::NotApplicable,
			}
		}
		Node::Require(pattern) => require_eval(pattern),
		Node::VerticalLayout(layout) => layout_eval(layout),
		Node::Quantifier {
			kind,
			domain,
			filter,
		} => quant_eval(*kind, domain, filter),
	}
}

fn eval_node(node: &Node, d: &DefRecord, def_idx: usize, ctx: &EvalCtx<'_, '_>) -> NodeOutcome {
	eval_node_with_self(node, d, def_idx, def_idx, ctx)
}

fn eval_node_with_self(
	node: &Node,
	d: &DefRecord,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> NodeOutcome {
	walk_node(
		node,
		&|a| eval_atom(a, d, def_idx, self_idx, ctx),
		&|kind, domain, filter| {
			eval_quantifier_def(
				kind,
				domain,
				filter,
				DefScope {
					record: d,
					idx: def_idx,
				},
				self_idx,
				ctx,
			)
		},
		&|pattern| eval_require(pattern, d, ctx),
		&|layout| eval_vertical_layout(layout, d, def_idx, ctx),
	)
}

fn eval_require(pattern: &str, d: &DefRecord, ctx: &EvalCtx<'_, '_>) -> NodeOutcome {
	let Some(rendered) = render_requirement_pattern(pattern, d) else {
		return NodeOutcome::NotApplicable;
	};
	if local_requirement_exists(&rendered, ctx)
		|| ctx
			.requirements
			.is_some_and(|resolver| resolver.exists(&rendered, d, ctx.uri_cfg.scheme))
	{
		return NodeOutcome::Pass;
	}
	NodeOutcome::Fail(Failure {
		atom_raw: format!("require(\"{pattern}\")"),
		lhs_label: "require".to_string(),
		actual: "missing".to_string(),
		expected: rendered,
		def_idx: None,
		details: None,
	})
}

fn local_requirement_exists(pattern: &str, ctx: &EvalCtx<'_, '_>) -> bool {
	let Ok(pattern) = crate::check::path::parse(pattern) else {
		return false;
	};
	ctx.graph
		.defs()
		.any(|def| crate::check::path::matches(&pattern, &def.moniker))
}

fn render_requirement_pattern(pattern: &str, d: &DefRecord) -> Option<String> {
	let name = def_name(d)?;
	Some(
		pattern
			.replace("{name}", &name)
			.replace("{name.snake}", &to_snake_case(&name)),
	)
}

fn to_snake_case(name: &str) -> String {
	let mut out = String::new();
	for (idx, ch) in name.chars().enumerate() {
		if ch.is_ascii_uppercase() {
			if idx > 0 {
				out.push('_');
			}
			out.push(ch.to_ascii_lowercase());
		} else {
			out.push(ch);
		}
	}
	out
}

fn resolve_def_lhs(lhs: Lhs, d: &DefRecord, ctx: &EvalCtx<'_, '_>) -> Option<Value> {
	let source = ctx.source;
	let value = match lhs {
		Lhs::Name => Value::Str(def_name(d)?),
		Lhs::Kind => Value::Str(std::str::from_utf8(&d.kind).ok()?.to_string()),
		Lhs::Visibility => Value::Str(std::str::from_utf8(&d.visibility).ok()?.to_string()),
		Lhs::Lines => {
			let (s, e) = d.position?;
			let (sl, el) = line_range(source, s, e);
			Value::Number((el - sl + 1) as f64)
		}
		Lhs::StartLine => {
			let (s, e) = d.position?;
			let (sl, _) = line_range(source, s, e);
			Value::Number(sl as f64)
		}
		Lhs::EndLine => {
			let (s, e) = d.position?;
			let (_, el) = line_range(source, s, e);
			Value::Number(el as f64)
		}
		Lhs::StartByte => {
			let (s, _) = d.position?;
			Value::Number(s as f64)
		}
		Lhs::EndByte => {
			let (_, e) = d.position?;
			Value::Number(e as f64)
		}
		Lhs::Text => {
			let (s, e) = d.position?;
			Value::Str(source.get(s as usize..e as usize).unwrap_or("").to_string())
		}
		Lhs::Moniker => Value::Moniker(d.moniker.clone()),
		Lhs::ParentMoniker => Value::Moniker(d.moniker.parent()?),
		Lhs::Depth => Value::Number(d.moniker.as_view().segments().count() as f64),
		Lhs::ParentName => {
			let segs: Vec<_> = d.moniker.as_view().segments().collect();
			if segs.len() < 2 {
				return None;
			}
			let p = &segs[segs.len() - 2];
			let bare = bare_callable_name(p.name);
			Value::Str(std::str::from_utf8(bare).ok()?.to_string())
		}
		Lhs::ParentKind => {
			let segs: Vec<_> = d.moniker.as_view().segments().collect();
			if segs.len() < 2 {
				return None;
			}
			let p = &segs[segs.len() - 2];
			Value::Str(std::str::from_utf8(p.kind).ok()?.to_string())
		}
		Lhs::Shape => Value::Str(d.shape()?.as_str().to_string()),
		Lhs::ParentShape => {
			let segs: Vec<_> = d.moniker.as_view().segments().collect();
			if segs.len() < 2 {
				return None;
			}
			let parent_kind = segs[segs.len() - 2].kind;
			Value::Str(
				code_moniker_core::core::shape::shape_of(parent_kind)?
					.as_str()
					.to_string(),
			)
		}
		Lhs::SourceName => Value::Str(def_name(d)?),
		Lhs::SourceKind => Value::Str(std::str::from_utf8(&d.kind).ok()?.to_string()),
		Lhs::SourceShape => Value::Str(d.shape()?.as_str().to_string()),
		Lhs::SourceVisibility => Value::Str(std::str::from_utf8(&d.visibility).ok()?.to_string()),
		Lhs::SourceMoniker => Value::Moniker(d.moniker.clone()),
		Lhs::SourceParentMoniker => Value::Moniker(d.moniker.parent()?),
		Lhs::Confidence
		| Lhs::TargetName
		| Lhs::TargetKind
		| Lhs::TargetShape
		| Lhs::TargetVisibility
		| Lhs::TargetMoniker
		| Lhs::TargetParentMoniker
		| Lhs::SegmentName
		| Lhs::SegmentKind => return None,
	};
	Some(value)
}

/// `count(<domain>, <filter>?)` evaluated in def scope. Counts items in
/// the domain for which `filter` evaluates to Pass.
fn eval_count(
	domain: &Domain,
	filter: Option<&Node>,
	d: &DefRecord,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> u32 {
	match domain {
		Domain::Children(kind) => match filter {
			None => ctx
				.parent_counts
				.get(&(def_idx, kind.as_bytes()))
				.copied()
				.unwrap_or(0),
			Some(node) => count_children_filtered(d, def_idx, self_idx, kind, node, ctx),
		},
		Domain::ChildrenByShape(shape) => {
			count_children_by_shape(def_idx, self_idx, shape, filter, ctx)
		}
		Domain::Descendants(_) => count_domain_items(domain, filter, def_idx, self_idx, ctx),
		Domain::Pairs(inner) => eval_pair_count(inner, filter, def_idx, self_idx, ctx),
		Domain::Segments => count_segments(d, filter),
		Domain::OutRefs => count_out_refs(d, def_idx, filter, ctx),
		Domain::InRefs => count_in_refs(d, filter, ctx),
	}
}

fn count_domain_items(
	domain: &Domain,
	filter: Option<&Node>,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> u32 {
	let items = domain_items(domain, def_idx, ctx);
	let Some(node) = filter else {
		return items.len() as u32;
	};
	items
		.into_iter()
		.filter(|item| match item {
			DomainItem::Def {
				idx: Some(idx),
				def,
			} => {
				matches!(
					eval_node_with_self(node, def, *idx, self_idx, ctx),
					NodeOutcome::Pass
				)
			}
			DomainItem::Def { idx: None, def } => {
				matches!(eval_external_def_node(node, def, ctx), NodeOutcome::Pass)
			}
			DomainItem::Ref { record } => {
				matches!(eval_ref_node(node, record, ctx), NodeOutcome::Pass)
			}
			DomainItem::Segment { kind, name } => {
				matches!(eval_node_segment(node, kind, name), NodeOutcome::Pass)
			}
		})
		.count() as u32
}

fn eval_number_expr_def(
	expr: &NumberExpr,
	d: &DefRecord,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> Option<f64> {
	match expr {
		NumberExpr::Literal(n) => Some(*n),
		NumberExpr::Projection(lhs) => match resolve_def_lhs(*lhs, d, ctx)? {
			Value::Number(n) => Some(n),
			_ => None,
		},
		NumberExpr::Count { domain, filter } => {
			Some(eval_count(domain, filter.as_deref(), d, def_idx, self_idx, ctx) as f64)
		}
		NumberExpr::Aggregate {
			kind,
			domain,
			expr,
			percentile,
		} => eval_aggregate(
			AggregateEval {
				kind: *kind,
				domain,
				expr,
				percentile: *percentile,
				def_idx,
				self_idx,
			},
			ctx,
		),
		NumberExpr::Metric { kind, binding } => {
			eval_metric(*kind, *binding, def_idx, self_idx, ctx)
		}
		NumberExpr::Entropy(collection) => eval_entropy(collection, def_idx, self_idx, ctx),
		NumberExpr::Size(collection) => {
			if collection_has_pair_binding(collection) {
				return None;
			}
			Some(eval_collection_size(collection, def_idx, self_idx, ctx) as f64)
		}
	}
}

fn eval_number_expr_ref(
	expr: &NumberExpr,
	r: &code_moniker_core::core::code_graph::RefRecord,
	ctx: &EvalCtx<'_, '_>,
) -> Option<f64> {
	match expr {
		NumberExpr::Literal(n) => Some(*n),
		NumberExpr::Projection(lhs) => match resolve_ref_lhs(*lhs, r, ctx)? {
			Value::Number(n) => Some(n),
			_ => None,
		},
		NumberExpr::Count { .. }
		| NumberExpr::Aggregate { .. }
		| NumberExpr::Metric { .. }
		| NumberExpr::Entropy(_)
		| NumberExpr::Size(_) => None,
	}
}

fn eval_number_expr_segment(expr: &NumberExpr) -> Option<f64> {
	match expr {
		NumberExpr::Literal(n) => Some(*n),
		NumberExpr::Projection(_)
		| NumberExpr::Count { .. }
		| NumberExpr::Aggregate { .. }
		| NumberExpr::Metric { .. }
		| NumberExpr::Entropy(_)
		| NumberExpr::Size(_) => None,
	}
}

fn count_children_filtered(
	_d: &DefRecord,
	def_idx: usize,
	self_idx: usize,
	kind: &str,
	filter: &Node,
	ctx: &EvalCtx<'_, '_>,
) -> u32 {
	let Some(child_idxs) = ctx.children_by_parent.get(&def_idx) else {
		return 0;
	};
	let mut n = 0;
	for &ci in child_idxs {
		let cd = ctx.graph.def_at(ci);
		if cd.kind.as_ref() != kind.as_bytes() {
			continue;
		}
		if let NodeOutcome::Pass = eval_node_with_self(filter, cd, ci, self_idx, ctx) {
			n += 1;
		}
	}
	n
}

fn count_children_by_shape(
	def_idx: usize,
	self_idx: usize,
	shape: &str,
	filter: Option<&Node>,
	ctx: &EvalCtx<'_, '_>,
) -> u32 {
	let Some(child_idxs) = ctx.children_by_parent.get(&def_idx) else {
		return 0;
	};
	let mut n = 0;
	for &ci in child_idxs {
		let cd = ctx.graph.def_at(ci);
		if !def_has_shape(cd, shape) {
			continue;
		}
		match filter {
			None => n += 1,
			Some(node) => {
				if let NodeOutcome::Pass = eval_node_with_self(node, cd, ci, self_idx, ctx) {
					n += 1;
				}
			}
		}
	}
	n
}

fn def_has_shape(d: &DefRecord, shape: &str) -> bool {
	d.shape().is_some_and(|actual| actual.as_str() == shape)
}

fn count_segments(d: &DefRecord, filter: Option<&Node>) -> u32 {
	let mut n = 0;
	for seg in d.moniker.as_view().segments() {
		match filter {
			None => n += 1,
			Some(node) => {
				if let NodeOutcome::Pass = eval_node_segment(node, seg.kind, seg.name) {
					n += 1;
				}
			}
		}
	}
	n
}

fn count_out_refs(
	_d: &DefRecord,
	def_idx: usize,
	filter: Option<&Node>,
	ctx: &EvalCtx<'_, '_>,
) -> u32 {
	let Some(ref_idxs) = ctx.out_refs_by_source.get(&def_idx) else {
		return 0;
	};
	let mut n = 0;
	for &ri in ref_idxs {
		let r = ctx.graph.ref_at(ri);
		match filter {
			None => n += 1,
			Some(node) => {
				if let NodeOutcome::Pass = eval_ref_node(node, r, ctx) {
					n += 1;
				}
			}
		}
	}
	n
}

fn count_in_refs(d: &DefRecord, filter: Option<&Node>, ctx: &EvalCtx<'_, '_>) -> u32 {
	let key = d.moniker.as_encoded();
	let Some(ref_idxs) = ctx.in_refs_by_target.get(key) else {
		return 0;
	};
	let mut n = 0;
	for &ri in ref_idxs {
		let r = ctx.graph.ref_at(ri);
		match filter {
			None => n += 1,
			Some(node) => {
				if let NodeOutcome::Pass = eval_ref_node(node, r, ctx) {
					n += 1;
				}
			}
		}
	}
	n
}

/// Quantifier evaluation in def scope.
fn eval_quantifier_def(
	kind: QuantKind,
	domain: &Domain,
	filter: &Node,
	scope: DefScope<'_>,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> NodeOutcome {
	let mut total = 0u32;
	let mut passes = 0u32;
	match domain {
		Domain::Children(_) | Domain::ChildrenByShape(_) | Domain::Descendants(_) => {
			match eval_def_domain_quantifier(kind, domain, filter, scope.idx, self_idx, ctx) {
				Ok((domain_total, domain_passes)) => {
					total = domain_total;
					passes = domain_passes;
				}
				Err(outcome) => return *outcome,
			}
		}
		Domain::Pairs(inner) => {
			return eval_pair_quantifier(kind, inner, filter, scope.idx, self_idx, ctx);
		}
		Domain::Segments => {
			for seg in scope.record.moniker.as_view().segments() {
				total += 1;
				if matches!(
					eval_node_segment(filter, seg.kind, seg.name),
					NodeOutcome::Pass
				) {
					passes += 1;
				}
			}
		}
		Domain::OutRefs => {
			let empty = Vec::new();
			let ref_idxs = ctx.out_refs_by_source.get(&scope.idx).unwrap_or(&empty);
			for &ri in ref_idxs {
				let r = ctx.graph.ref_at(ri);
				total += 1;
				if matches!(eval_ref_node(filter, r, ctx), NodeOutcome::Pass) {
					passes += 1;
				}
			}
		}
		Domain::InRefs => {
			let key = scope.record.moniker.as_encoded();
			let empty = Vec::new();
			let ref_idxs = ctx.in_refs_by_target.get(key).unwrap_or(&empty);
			for &ri in ref_idxs {
				let r = ctx.graph.ref_at(ri);
				total += 1;
				if matches!(eval_ref_node(filter, r, ctx), NodeOutcome::Pass) {
					passes += 1;
				}
			}
		}
	}
	let label = match kind {
		QuantKind::Any => "any",
		QuantKind::All => "all",
		QuantKind::None => "none",
	};
	let ok = match kind {
		QuantKind::Any => passes > 0,
		QuantKind::All => total == 0 || passes == total,
		QuantKind::None => passes == 0,
	};
	if ok {
		NodeOutcome::Pass
	} else {
		NodeOutcome::Fail(Failure {
			atom_raw: format!("{label}(...)"),
			lhs_label: label.to_string(),
			actual: format!("{passes}/{total}"),
			expected: match kind {
				QuantKind::Any => "≥ 1 match".to_string(),
				QuantKind::All => "all match".to_string(),
				QuantKind::None => "zero matches".to_string(),
			},
			def_idx: None,
			details: None,
		})
	}
}

fn eval_def_domain_quantifier(
	kind: QuantKind,
	domain: &Domain,
	filter: &Node,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> Result<(u32, u32), Box<NodeOutcome>> {
	let mut total = 0u32;
	let mut passes = 0u32;
	for item in domain_items(domain, def_idx, ctx) {
		let DomainItem::Def { idx, def } = item else {
			continue;
		};
		total += 1;
		let outcome = match idx {
			Some(idx) => eval_node_with_self(filter, def, idx, self_idx, ctx),
			None => eval_external_def_node(filter, def, ctx),
		};
		match outcome {
			NodeOutcome::Pass => passes += 1,
			NodeOutcome::Fail(mut failure) if kind == QuantKind::All && idx.is_some() => {
				let idx = idx.expect("checked by guard");
				failure.def_idx.get_or_insert(idx);
				return Err(Box::new(NodeOutcome::Fail(failure)));
			}
			NodeOutcome::Fail(failure) if kind == QuantKind::All => {
				return Err(Box::new(NodeOutcome::Fail(failure)));
			}
			NodeOutcome::Fail(_) | NodeOutcome::NotApplicable => {}
		}
	}
	Ok((total, passes))
}

fn eval_external_def_node(node: &Node, def: &DefRecord, ctx: &EvalCtx<'_, '_>) -> NodeOutcome {
	walk_node(
		node,
		&|atom| eval_external_def_atom(atom, def, ctx),
		&|_, _, _| NodeOutcome::NotApplicable,
		&|_| NodeOutcome::NotApplicable,
		&|_| NodeOutcome::NotApplicable,
	)
}

fn eval_external_def_atom(atom: &Atom, def: &DefRecord, ctx: &EvalCtx<'_, '_>) -> AtomOutcome {
	let LhsExpr::Attr(lhs) = &atom.lhs else {
		return AtomOutcome::NotApplicable;
	};
	let Some(value) = project_def_lhs_value(None, def, *lhs, ctx) else {
		return AtomOutcome::NotApplicable;
	};
	if let Rhs::Projection(rhs) = &atom.rhs
		&& let Some(rhs_value) = project_def_lhs_value(None, def, *rhs, ctx)
	{
		return apply_op_values(&value, atom.op, &rhs_value);
	}
	apply_op(&value, atom)
}

fn eval_node_segment(node: &Node, seg_kind: &[u8], seg_name: &[u8]) -> NodeOutcome {
	walk_node(
		node,
		&|a| eval_atom_segment(a, seg_kind, seg_name),
		&|_, _, _| NodeOutcome::NotApplicable,
		&|_| NodeOutcome::NotApplicable,
		&|_| NodeOutcome::NotApplicable,
	)
}

fn eval_atom_segment(atom: &Atom, seg_kind: &[u8], seg_name: &[u8]) -> AtomOutcome {
	let value: Value = match &atom.lhs {
		LhsExpr::Attr(Lhs::SegmentKind) => Value::Str(
			std::str::from_utf8(seg_kind)
				.unwrap_or_default()
				.to_string(),
		),
		LhsExpr::Attr(Lhs::SegmentName) => Value::Str(
			std::str::from_utf8(seg_name)
				.unwrap_or_default()
				.to_string(),
		),
		_ => return AtomOutcome::NotApplicable,
	};
	if let Rhs::Projection(other) = &atom.rhs {
		let rhs_val = match other {
			Lhs::SegmentKind => Value::Str(
				std::str::from_utf8(seg_kind)
					.unwrap_or_default()
					.to_string(),
			),
			Lhs::SegmentName => Value::Str(
				std::str::from_utf8(seg_name)
					.unwrap_or_default()
					.to_string(),
			),
			_ => return AtomOutcome::NotApplicable,
		};
		return apply_op_values(&value, atom.op, &rhs_val);
	}
	if let Rhs::Number(expr) = &atom.rhs {
		let Some(rhs_val) = eval_number_expr_segment(expr).map(Value::Number) else {
			return AtomOutcome::NotApplicable;
		};
		return apply_op_values(&value, atom.op, &rhs_val);
	}
	apply_op(&value, atom)
}

fn eval_atom(
	atom: &Atom,
	d: &DefRecord,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> AtomOutcome {
	if let (LhsExpr::Collection(left), Op::Subset, Rhs::Collection(right)) =
		(&atom.lhs, atom.op, &atom.rhs)
	{
		if collection_has_pair_binding(left) || collection_has_pair_binding(right) {
			return AtomOutcome::NotApplicable;
		}
		return if eval_collection_subset(left, right, def_idx, self_idx, ctx) {
			AtomOutcome::Pass
		} else {
			AtomOutcome::Fail {
				actual: "not subset".to_string(),
				expected: "subset".to_string(),
			}
		};
	}
	let value: Value = match &atom.lhs {
		LhsExpr::Attr(lhs) => {
			let Some(value) = resolve_def_lhs(*lhs, d, ctx) else {
				return AtomOutcome::NotApplicable;
			};
			value
		}
		LhsExpr::Number(expr) => {
			let Some(n) = eval_number_expr_def(expr, d, def_idx, self_idx, ctx) else {
				return AtomOutcome::NotApplicable;
			};
			Value::Number(n)
		}
		LhsExpr::Mode(collection) => {
			let Some(value) = eval_mode(collection, def_idx, self_idx, ctx) else {
				return AtomOutcome::NotApplicable;
			};
			value
		}
		LhsExpr::PairProjection(_) => return AtomOutcome::NotApplicable,
		LhsExpr::SegmentOf { scope, kind } => match scope {
			SegmentScope::Def => Value::Str(first_segment_name(&d.moniker, kind.as_bytes())),
			SegmentScope::Source | SegmentScope::Target => {
				return AtomOutcome::NotApplicable;
			}
		},
		LhsExpr::Collection(_) => return AtomOutcome::NotApplicable,
	};
	if let Rhs::Projection(other) = &atom.rhs {
		let Some(rhs_val) = resolve_def_lhs(*other, d, ctx) else {
			return AtomOutcome::NotApplicable;
		};
		return apply_op_values(&value, atom.op, &rhs_val);
	}
	if let Rhs::Number(expr) = &atom.rhs {
		let Some(rhs_val) =
			eval_number_expr_def(expr, d, def_idx, self_idx, ctx).map(Value::Number)
		else {
			return AtomOutcome::NotApplicable;
		};
		return apply_op_values(&value, atom.op, &rhs_val);
	}
	apply_op(&value, atom)
}

fn children_by_parent(graph: &CodeGraph) -> HashMap<usize, Vec<usize>> {
	let mut m: HashMap<usize, Vec<usize>> = HashMap::new();
	for (idx, d) in graph.defs().enumerate() {
		if let Some(p) = d.parent {
			m.entry(p).or_default().push(idx);
		}
	}
	m
}

fn out_refs_by_source(graph: &CodeGraph) -> HashMap<usize, Vec<usize>> {
	let mut m: HashMap<usize, Vec<usize>> = HashMap::new();
	for (idx, r) in graph.refs().enumerate() {
		m.entry(r.source).or_default().push(idx);
	}
	m
}

fn in_refs_by_target(graph: &CodeGraph) -> HashMap<Vec<u8>, Vec<usize>> {
	let mut m: HashMap<Vec<u8>, Vec<usize>> = HashMap::new();
	for (idx, r) in graph.refs().enumerate() {
		m.entry(r.target.as_encoded().to_vec())
			.or_default()
			.push(idx);
	}
	m
}

fn parent_counts_by_kind(graph: &CodeGraph) -> HashMap<(usize, &[u8]), u32> {
	let mut m: HashMap<(usize, &[u8]), u32> = HashMap::new();
	for d in graph.defs() {
		if let Some(p) = d.parent {
			*m.entry((p, d.kind.as_ref())).or_insert(0) += 1;
		}
	}
	m
}

fn comment_end_bytes(graph: &CodeGraph) -> Vec<u32> {
	let mut v: Vec<u32> = graph
		.defs()
		.filter(|d| d.kind.as_ref() == KIND_COMMENT)
		.filter_map(|d| d.position.map(|(_, e)| e))
		.collect();
	v.sort_unstable();
	v
}

/// One pass over `graph.refs()` to bucket the earliest `annotates`-ref start
/// per annotated def. Saves the O(D × R) per-def filter that the old
/// `doc_anchor_byte` performed.
fn doc_anchors_by_def(graph: &CodeGraph) -> HashMap<usize, u32> {
	let mut m: HashMap<usize, u32> = HashMap::new();
	for r in graph.refs() {
		if r.kind != b"annotates" {
			continue;
		}
		let Some((start, _)) = r.position else {
			continue;
		};
		m.entry(r.source)
			.and_modify(|cur| {
				if start < *cur {
					*cur = start;
				}
			})
			.or_insert(start);
	}
	m
}

fn comment_attaches_to(source: &str, comment_end: u32, header_start: u32) -> bool {
	if comment_end > header_start {
		return false;
	}
	let last_comment_byte = comment_end.saturating_sub(1);
	let (cl, _) = line_range(source, last_comment_byte, last_comment_byte + 1);
	let (hl, _) = line_range(source, header_start, header_start + 1);
	hl == cl || hl == cl + 1
}

fn check_require_doc_comment(
	target: RuleTarget<'_>,
	rules: &CompiledKindRules,
	ctx: &EvalCtx<'_, '_>,
	out: &mut Vec<Violation>,
) {
	check_require_doc_comment_with_id(
		target,
		rules,
		rule_id(ctx.lang, target.kind, "require_doc_comment"),
		ctx,
		out,
	);
}

fn check_require_doc_comment_with_id(
	target: RuleTarget<'_>,
	rules: &CompiledKindRules,
	rule_id: String,
	ctx: &EvalCtx<'_, '_>,
	out: &mut Vec<Violation>,
) {
	if eval_require_doc_comment(target.scope.record, target.scope.idx, rules, ctx) != Some(false) {
		return;
	}

	let moniker = to_uri(&target.scope.record.moniker, &ctx.uri_cfg);
	let name = def_name(target.scope.record).unwrap_or_default();
	let (start_line, end_line) = lines_of(target.scope.record, ctx.source);
	out.push(Violation {
		rule_id,
		severity: RuleSeverity::Error,
		moniker,
		kind: target.kind.to_string(),
		lines: (start_line, end_line),
		message: format!(
			"{} `{name}` is missing a doc comment immediately before it",
			target.kind
		),
		explanation: None,
	});
}

fn eval_require_doc_comment(
	d: &DefRecord,
	def_idx: usize,
	rules: &CompiledKindRules,
	ctx: &EvalCtx<'_, '_>,
) -> Option<bool> {
	let filter = rules.require_doc_for_vis.as_ref()?;
	let vis = std::str::from_utf8(&d.visibility).unwrap_or("");
	if filter != "any" && filter != vis {
		return None;
	}
	let (def_start, _) = d.position?;
	let header_start = ctx
		.doc_anchors
		.get(&def_idx)
		.copied()
		.map(|anc| anc.min(def_start))
		.unwrap_or(def_start);

	let idx = ctx.comment_ends.partition_point(|&end| end <= header_start);
	let has_doc =
		idx > 0 && comment_attaches_to(ctx.source, ctx.comment_ends[idx - 1], header_start);
	Some(has_doc)
}

#[cfg(test)]
mod tests;
