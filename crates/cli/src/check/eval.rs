use std::collections::HashMap;

use crate::check::config::{Config, ConfigError, KindRules, config_section};
use crate::check::expr::{
	self, Atom, Domain, Lhs, LhsExpr, Node, Op, QuantKind, Rhs, SegmentScope,
};
use crate::lines::line_range;
use crate::render_uri;
use code_moniker_core::core::code_graph::{CodeGraph, DefRecord};
use code_moniker_core::core::kinds::KIND_COMMENT;
use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::core::uri::UriConfig;
use code_moniker_core::lang::Lang;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Violation {
	pub rule_id: String,
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

pub fn evaluate(
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
	let need_doc_anchors = compiled
		.by_kind
		.values()
		.any(|r| r.require_doc_for_vis.is_some());
	let ctx = EvalCtx {
		graph,
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
		let Some(rules) = compiled.for_kind(kind_str) else {
			continue;
		};
		for rule in &rules.rules {
			eval_rule(rule, d, idx, kind_str, &ctx, &mut out);
		}
		check_require_doc_comment(d, kind_str, idx, rules, &ctx, &mut out);
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
	let need_doc_anchors = compiled
		.by_kind
		.values()
		.any(|r| r.require_doc_for_vis.is_some());
	let ctx = EvalCtx {
		graph,
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
	for (kind, rules) in &compiled.by_kind {
		for rule in &rules.rules {
			let mut report = RuleReport::new(rule_id(lang, kind, &rule.id), kind.clone(), rule);
			for (idx, d) in graph.defs().enumerate() {
				if d.kind.as_slice() != kind.as_bytes() {
					continue;
				}
				report.evaluated += 1;
				let premise =
					implication_premise(rule).map(|premise| eval_node(premise, d, idx, &ctx));
				report.record(eval_node(&rule.root, d, idx, &ctx), premise);
			}
			report.finalize_warning();
			out.push(report);
		}
		if rules.require_doc_for_vis.is_some() {
			let mut report = RuleReport::new_require_doc(
				rule_id(lang, kind, "require_doc_comment"),
				kind.clone(),
			);
			for (idx, d) in graph.defs().enumerate() {
				if d.kind.as_slice() != kind.as_bytes() {
					continue;
				}
				report.evaluated += 1;
				report.record(
					eval_require_doc_comment(d, idx, rules, &ctx).map_or(
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
	for rule in &compiled.refs {
		let mut report = RuleReport::new(format!("refs.{}", rule.id), "refs".to_string(), rule);
		for r in graph.refs() {
			report.evaluated += 1;
			let premise = implication_premise(rule).map(|premise| eval_ref_node(premise, r, &ctx));
			report.record(eval_ref_node(&rule.root, r, &ctx), premise);
		}
		report.finalize_warning();
		out.push(report);
	}
	out.sort_by(|a, b| a.rule_id.cmp(&b.rule_id));
	out
}

impl RuleReport {
	fn new(rule_id: String, domain: String, rule: &CompiledRule) -> Self {
		Self {
			rule_id,
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

	fn finalize_warning(&mut self) {
		if self.evaluated > 0 && self.antecedent_matches == Some(0) {
			self.warning = Some("antecedent never matched".to_string());
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
	raw_expr: String,
	root: Node,
	message: Option<String>,
}

#[derive(Default)]
struct CompiledKindRules {
	rules: Vec<CompiledRule>,
	require_doc_for_vis: Option<String>,
}

pub struct CompiledRules {
	by_kind: HashMap<String, CompiledKindRules>,
	refs: Vec<CompiledRule>,
}

impl CompiledRules {
	fn for_lang(cfg: &Config, lang: Lang, scheme: &str) -> Result<Self, ConfigError> {
		let section = config_section(lang);
		let allowed = crate::check::config::allowed_kinds_for(lang);
		let aliases = crate::check::config::resolve_aliases(&cfg.aliases)?;
		let mut by_kind: HashMap<String, CompiledKindRules> = HashMap::new();
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
			if !by_kind.contains_key(kind.as_str()) {
				by_kind.insert(
					kind.clone(),
					compile(rules, "default", kind, scheme, &allowed, &aliases)?,
				);
			}
		}
		let mut refs = Vec::with_capacity(cfg.refs.rules.len() + per_lang_refs.len());
		for (idx, entry) in cfg.refs.rules.iter().enumerate() {
			let id = entry.fallback_id(idx);
			let at = format!("refs.{id}");
			let expanded = crate::check::config::substitute_aliases(&entry.expr, &aliases, &at)?;
			let parsed = expr::parse(&expanded, scheme, &allowed).map_err(|error| {
				ConfigError::InvalidExpr {
					at: at.clone(),
					error,
				}
			})?;
			refs.push(CompiledRule {
				id,
				raw_expr: entry.expr.clone(),
				root: parsed.root,
				message: entry.message.clone(),
			});
		}
		for (idx, entry) in per_lang_refs.iter().enumerate() {
			let id = entry.fallback_id(idx);
			let at = format!("{section}.refs.{id}");
			let expanded = crate::check::config::substitute_aliases(&entry.expr, &aliases, &at)?;
			let parsed = expr::parse(&expanded, scheme, &allowed).map_err(|error| {
				ConfigError::InvalidExpr {
					at: at.clone(),
					error,
				}
			})?;
			refs.push(CompiledRule {
				id,
				raw_expr: entry.expr.clone(),
				root: parsed.root,
				message: entry.message.clone(),
			});
		}
		Ok(Self { by_kind, refs })
	}

	fn for_kind(&self, kind: &str) -> Option<&CompiledKindRules> {
		self.by_kind.get(kind)
	}
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
			raw_expr: entry.expr.clone(),
			root: parsed.root,
			message: entry.message.clone(),
		});
	}
	Ok(CompiledKindRules {
		rules: compiled,
		require_doc_for_vis: rules.require_doc_comment.clone(),
	})
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

fn eval_rule(
	rule: &CompiledRule,
	d: &DefRecord,
	def_idx: usize,
	kind: &str,
	ctx: &EvalCtx<'_, '_>,
	out: &mut Vec<Violation>,
) {
	let Failure {
		atom_raw,
		lhs_label,
		actual,
		expected,
	} = match eval_node(&rule.root, d, def_idx, ctx) {
		NodeOutcome::Pass | NodeOutcome::NotApplicable => return,
		NodeOutcome::Fail(f) => f,
	};
	let name = def_name(d).unwrap_or_default();
	let moniker = render_uri(&d.moniker, &ctx.uri_cfg);
	let (start_line, end_line) = lines_of(d, ctx.source);
	let message = format!(
		"{kind} `{name}` fails `{atom_raw}` ({lhs_label} = {actual}, expected {expected})",
	);
	let explanation = rule.message.as_ref().map(|tpl| {
		render_template(
			tpl,
			&[
				("name", &name),
				("kind", kind),
				("moniker", &moniker),
				("expr", &rule.raw_expr),
				("value", &actual),
				("expected", &expected),
				("pattern", &expected),
				("lines", &actual),
				("limit", &expected),
				("count", &actual),
			],
		)
	});
	out.push(Violation {
		rule_id: rule_id(ctx.lang, kind, &rule.id),
		moniker,
		kind: kind.to_string(),
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
	} = match eval_ref_node(&rule.root, r, ctx) {
		NodeOutcome::Pass | NodeOutcome::NotApplicable => return,
		NodeOutcome::Fail(f) => f,
	};
	let source_def = graph.def_at(r.source);
	let source_uri = render_uri(&source_def.moniker, &ctx.uri_cfg);
	let target_uri = render_uri(&r.target, &ctx.uri_cfg);
	let ref_kind = std::str::from_utf8(&r.kind).unwrap_or_default();
	let (start_line, end_line) = match r.position {
		Some((s, e)) => crate::lines::line_range(ctx.source, s, e),
		None => (0, 0),
	};
	let message = format!(
		"ref {ref_kind} {source_uri} → {target_uri} fails `{atom_raw}` ({lhs_label} = {actual}, expected {expected})"
	);
	out.push(Violation {
		rule_id: format!("refs.{}", rule.id),
		moniker: target_uri,
		kind: ref_kind.to_string(),
		lines: (start_line, end_line),
		message,
		explanation: rule.message.clone(),
	});
}

fn eval_ref_node(
	node: &Node,
	r: &code_moniker_core::core::code_graph::RefRecord,
	ctx: &EvalCtx<'_, '_>,
) -> NodeOutcome {
	walk_node(node, &|a| eval_ref_atom(a, r, ctx), &|_, _, _| {
		NodeOutcome::NotApplicable
	})
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
		LhsExpr::Attr(Lhs::TargetMoniker) => Value::Moniker(r.target.clone()),
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
		_ => return AtomOutcome::NotApplicable,
	};
	if let Rhs::Projection(other) = &atom.rhs {
		let Some(rhs_val) = resolve_ref_lhs(*other, r, ctx) else {
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
		Lhs::Moniker | Lhs::SourceMoniker => Value::Moniker(source_def.moniker.clone()),
		Lhs::TargetMoniker => Value::Moniker(r.target.clone()),
		Lhs::SourceName => Value::Str(name_of(&source_def.moniker)?),
		Lhs::TargetName => Value::Str(name_of(&r.target)?),
		Lhs::SourceKind => Value::Str(last_segment_kind(&source_def.moniker)?),
		Lhs::TargetKind => Value::Str(last_segment_kind(&r.target)?),
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

fn resolve_local_def<'g>(
	graph: &'g CodeGraph,
	m: &code_moniker_core::core::moniker::Moniker,
) -> Option<&'g DefRecord> {
	graph.defs().find(|d| d.moniker == *m)
}

fn describe_lhs(lhs: &LhsExpr) -> &str {
	match lhs {
		LhsExpr::Attr(a) => a.as_str(),
		LhsExpr::Count { .. } => "count",
		LhsExpr::SegmentOf { .. } => "segment",
	}
}

#[derive(Debug)]
struct Failure {
	atom_raw: String,
	lhs_label: String,
	actual: String,
	expected: String,
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
fn walk_node<A, Q>(node: &Node, atom_eval: &A, quant_eval: &Q) -> NodeOutcome
where
	A: Fn(&Atom) -> AtomOutcome,
	Q: Fn(QuantKind, &Domain, &Node) -> NodeOutcome,
{
	match node {
		Node::Atom(atom) => match atom_eval(atom) {
			AtomOutcome::Pass => NodeOutcome::Pass,
			AtomOutcome::Fail { actual, expected } => NodeOutcome::Fail(Failure {
				atom_raw: atom.raw.clone(),
				lhs_label: describe_lhs(&atom.lhs).to_string(),
				actual,
				expected,
			}),
			AtomOutcome::NotApplicable => NodeOutcome::NotApplicable,
		},
		Node::And(children) => {
			let mut na = false;
			for c in children {
				match walk_node(c, atom_eval, quant_eval) {
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
				match walk_node(c, atom_eval, quant_eval) {
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
		Node::Not(inner) => match walk_node(inner, atom_eval, quant_eval) {
			NodeOutcome::Pass => NodeOutcome::Fail(Failure {
				atom_raw: "NOT (...)".to_string(),
				lhs_label: "NOT".to_string(),
				actual: "true".to_string(),
				expected: "false".to_string(),
			}),
			NodeOutcome::Fail(_) => NodeOutcome::Pass,
			NodeOutcome::NotApplicable => NodeOutcome::NotApplicable,
		},
		Node::Implies(prem, cons) => match walk_node(prem, atom_eval, quant_eval) {
			NodeOutcome::Pass => walk_node(cons, atom_eval, quant_eval),
			NodeOutcome::Fail(_) => NodeOutcome::Pass,
			NodeOutcome::NotApplicable => NodeOutcome::NotApplicable,
		},
		Node::Quantifier {
			kind,
			domain,
			filter,
		} => quant_eval(*kind, domain, filter),
	}
}

fn eval_node(node: &Node, d: &DefRecord, def_idx: usize, ctx: &EvalCtx<'_, '_>) -> NodeOutcome {
	walk_node(
		node,
		&|a| eval_atom(a, d, def_idx, ctx),
		&|kind, domain, filter| eval_quantifier_def(kind, domain, filter, d, def_idx, ctx),
	)
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
			Value::Number(el - sl + 1)
		}
		Lhs::Text => {
			let (s, e) = d.position?;
			Value::Str(source.get(s as usize..e as usize).unwrap_or("").to_string())
		}
		Lhs::Moniker => Value::Moniker(d.moniker.clone()),
		Lhs::Depth => Value::Number(d.moniker.as_view().segments().count() as u32),
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
		Lhs::Confidence
		| Lhs::SourceName
		| Lhs::SourceKind
		| Lhs::SourceShape
		| Lhs::SourceVisibility
		| Lhs::SourceMoniker
		| Lhs::TargetName
		| Lhs::TargetKind
		| Lhs::TargetShape
		| Lhs::TargetVisibility
		| Lhs::TargetMoniker
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
	ctx: &EvalCtx<'_, '_>,
) -> u32 {
	match domain {
		Domain::Children(kind) => match filter {
			None => ctx
				.parent_counts
				.get(&(def_idx, kind.as_bytes()))
				.copied()
				.unwrap_or(0),
			Some(node) => count_children_filtered(d, def_idx, kind, node, ctx),
		},
		Domain::Segments => count_segments(d, filter),
		Domain::OutRefs => count_out_refs(d, def_idx, filter, ctx),
		Domain::InRefs => count_in_refs(d, filter, ctx),
	}
}

fn count_children_filtered(
	_d: &DefRecord,
	def_idx: usize,
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
		if cd.kind.as_slice() != kind.as_bytes() {
			continue;
		}
		if let NodeOutcome::Pass = eval_node(filter, cd, ci, ctx) {
			n += 1;
		}
	}
	n
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
	let key = d.moniker.as_bytes();
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
	d: &DefRecord,
	def_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> NodeOutcome {
	let mut total = 0u32;
	let mut passes = 0u32;
	match domain {
		Domain::Children(child_kind) => {
			let empty = Vec::new();
			let child_idxs = ctx.children_by_parent.get(&def_idx).unwrap_or(&empty);
			for &ci in child_idxs {
				let cd = ctx.graph.def_at(ci);
				if cd.kind.as_slice() != child_kind.as_bytes() {
					continue;
				}
				total += 1;
				if matches!(eval_node(filter, cd, ci, ctx), NodeOutcome::Pass) {
					passes += 1;
				}
			}
		}
		Domain::Segments => {
			for seg in d.moniker.as_view().segments() {
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
			let ref_idxs = ctx.out_refs_by_source.get(&def_idx).unwrap_or(&empty);
			for &ri in ref_idxs {
				let r = ctx.graph.ref_at(ri);
				total += 1;
				if matches!(eval_ref_node(filter, r, ctx), NodeOutcome::Pass) {
					passes += 1;
				}
			}
		}
		Domain::InRefs => {
			let key = d.moniker.as_bytes();
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
		})
	}
}

fn eval_node_segment(node: &Node, seg_kind: &[u8], seg_name: &[u8]) -> NodeOutcome {
	walk_node(
		node,
		&|a| eval_atom_segment(a, seg_kind, seg_name),
		&|_, _, _| NodeOutcome::NotApplicable,
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
	apply_op(&value, atom)
}

fn eval_atom(atom: &Atom, d: &DefRecord, def_idx: usize, ctx: &EvalCtx<'_, '_>) -> AtomOutcome {
	let source = ctx.source;
	let value: Value = match &atom.lhs {
		LhsExpr::Attr(Lhs::Name) => match def_name(d) {
			Some(n) => Value::Str(n),
			None => return AtomOutcome::NotApplicable,
		},
		LhsExpr::Attr(Lhs::Kind) => {
			Value::Str(std::str::from_utf8(&d.kind).unwrap_or_default().to_string())
		}
		LhsExpr::Attr(Lhs::Visibility) => Value::Str(
			std::str::from_utf8(&d.visibility)
				.unwrap_or_default()
				.to_string(),
		),
		LhsExpr::Attr(Lhs::Lines) => {
			let Some((s, e)) = d.position else {
				return AtomOutcome::NotApplicable;
			};
			let (sl, el) = line_range(source, s, e);
			Value::Number(el - sl + 1)
		}
		LhsExpr::Attr(Lhs::Text) => {
			let Some((s, e)) = d.position else {
				return AtomOutcome::NotApplicable;
			};
			Value::Str(source.get(s as usize..e as usize).unwrap_or("").to_string())
		}
		LhsExpr::Attr(Lhs::Moniker) => Value::Moniker(d.moniker.clone()),
		LhsExpr::Attr(Lhs::Depth) => Value::Number(d.moniker.as_view().segments().count() as u32),
		LhsExpr::Attr(Lhs::ParentName) => {
			let segs: Vec<_> = d.moniker.as_view().segments().collect();
			let Some(p) = segs.get(segs.len().saturating_sub(2)) else {
				return AtomOutcome::NotApplicable;
			};
			if segs.len() < 2 {
				return AtomOutcome::NotApplicable;
			}
			let bare = bare_callable_name(p.name);
			match std::str::from_utf8(bare) {
				Ok(s) => Value::Str(s.to_string()),
				Err(_) => return AtomOutcome::NotApplicable,
			}
		}
		LhsExpr::Attr(Lhs::ParentKind) => {
			let segs: Vec<_> = d.moniker.as_view().segments().collect();
			if segs.len() < 2 {
				return AtomOutcome::NotApplicable;
			}
			let p = &segs[segs.len() - 2];
			match std::str::from_utf8(p.kind) {
				Ok(s) => Value::Str(s.to_string()),
				Err(_) => return AtomOutcome::NotApplicable,
			}
		}
		LhsExpr::Attr(Lhs::Shape) => match d.shape() {
			Some(s) => Value::Str(s.as_str().to_string()),
			None => return AtomOutcome::NotApplicable,
		},
		LhsExpr::Attr(Lhs::ParentShape) => {
			let segs: Vec<_> = d.moniker.as_view().segments().collect();
			if segs.len() < 2 {
				return AtomOutcome::NotApplicable;
			}
			let parent_kind = segs[segs.len() - 2].kind;
			match code_moniker_core::core::shape::shape_of(parent_kind) {
				Some(s) => Value::Str(s.as_str().to_string()),
				None => return AtomOutcome::NotApplicable,
			}
		}
		LhsExpr::Attr(
			Lhs::Confidence
			| Lhs::SourceName
			| Lhs::SourceKind
			| Lhs::SourceShape
			| Lhs::SourceVisibility
			| Lhs::SourceMoniker
			| Lhs::TargetName
			| Lhs::TargetKind
			| Lhs::TargetShape
			| Lhs::TargetVisibility
			| Lhs::TargetMoniker
			| Lhs::SegmentName
			| Lhs::SegmentKind,
		) => return AtomOutcome::NotApplicable,
		LhsExpr::Count { domain, filter } => {
			let c = eval_count(domain, filter.as_deref(), d, def_idx, ctx);
			Value::Number(c)
		}
		LhsExpr::SegmentOf { scope, kind } => match scope {
			SegmentScope::Def => Value::Str(first_segment_name(&d.moniker, kind.as_bytes())),
			SegmentScope::Source | SegmentScope::Target => {
				return AtomOutcome::NotApplicable;
			}
		},
	};
	if let Rhs::Projection(other) = &atom.rhs {
		let Some(rhs_val) = resolve_def_lhs(*other, d, ctx) else {
			return AtomOutcome::NotApplicable;
		};
		return apply_op_values(&value, atom.op, &rhs_val);
	}
	apply_op(&value, atom)
}

/// Value-vs-Value comparison for the cases where the RHS is itself a
/// projection. Restricted to the ops that pair naturally (equality and
/// numeric ordering); a structural moniker op against a string projection
/// stays `NotApplicable`.
fn apply_op_values(lhs: &Value, op: Op, rhs: &Value) -> AtomOutcome {
	use Op::*;
	let ok = match (lhs, op, rhs) {
		(Value::Str(a), Eq, Value::Str(b)) => a == b,
		(Value::Str(a), Ne, Value::Str(b)) => a != b,
		(Value::Number(a), Eq, Value::Number(b)) => a == b,
		(Value::Number(a), Ne, Value::Number(b)) => a != b,
		(Value::Number(a), Lt, Value::Number(b)) => a < b,
		(Value::Number(a), Le, Value::Number(b)) => a <= b,
		(Value::Number(a), Gt, Value::Number(b)) => a > b,
		(Value::Number(a), Ge, Value::Number(b)) => a >= b,
		(Value::Moniker(a), Eq, Value::Moniker(b)) => a == b,
		(Value::Moniker(a), Ne, Value::Moniker(b)) => a != b,
		(Value::Moniker(a), AncestorOf, Value::Moniker(b)) => a.is_ancestor_of(b),
		(Value::Moniker(a), DescendantOf, Value::Moniker(b)) => b.is_ancestor_of(a),
		(Value::Moniker(a), BindMatch, Value::Moniker(b)) => a.bind_match(b),
		_ => return AtomOutcome::NotApplicable,
	};
	if ok {
		AtomOutcome::Pass
	} else {
		AtomOutcome::Fail {
			actual: render_value(lhs),
			expected: render_value(rhs),
		}
	}
}

enum Value {
	Str(String),
	Number(u32),
	Moniker(code_moniker_core::core::moniker::Moniker),
}

fn apply_op(value: &Value, atom: &Atom) -> AtomOutcome {
	use Op::*;
	let ok = match (value, atom.op, &atom.rhs) {
		(Value::Str(s), RegexMatch, Rhs::RegexStr(_)) => {
			atom.regex.as_ref().is_some_and(|re| re.is_match(s))
		}
		(Value::Str(s), RegexNoMatch, Rhs::RegexStr(_)) => {
			atom.regex.as_ref().is_some_and(|re| !re.is_match(s))
		}
		(Value::Str(s), Eq, Rhs::Str(t)) => s == t,
		(Value::Str(s), Ne, Rhs::Str(t)) => s != t,
		(Value::Number(a), Eq, Rhs::Number(b)) => a == b,
		(Value::Number(a), Ne, Rhs::Number(b)) => a != b,
		(Value::Number(a), Lt, Rhs::Number(b)) => a < b,
		(Value::Number(a), Le, Rhs::Number(b)) => a <= b,
		(Value::Number(a), Gt, Rhs::Number(b)) => a > b,
		(Value::Number(a), Ge, Rhs::Number(b)) => a >= b,
		(Value::Moniker(m), Eq, Rhs::Moniker(t)) => m == t,
		(Value::Moniker(m), Ne, Rhs::Moniker(t)) => m != t,
		(Value::Moniker(m), AncestorOf, Rhs::Moniker(t)) => m.is_ancestor_of(t),
		(Value::Moniker(m), DescendantOf, Rhs::Moniker(t)) => t.is_ancestor_of(m),
		(Value::Moniker(m), BindMatch, Rhs::Moniker(t)) => m.bind_match(t),
		(Value::Moniker(m), PathMatch, Rhs::PathPattern(p)) => crate::check::path::matches(p, m),
		_ => return AtomOutcome::NotApplicable,
	};
	if ok {
		AtomOutcome::Pass
	} else {
		let actual = render_value(value);
		let expected = render_rhs(&atom.rhs);
		AtomOutcome::Fail { actual, expected }
	}
}

fn render_value(v: &Value) -> String {
	match v {
		Value::Str(s) => s.clone(),
		Value::Number(n) => n.to_string(),
		Value::Moniker(m) => format!("{}b moniker", m.as_bytes().len()),
	}
}

fn render_rhs(r: &Rhs) -> String {
	match r {
		Rhs::Str(s) => s.clone(),
		Rhs::Number(n) => n.to_string(),
		Rhs::RegexStr(s) => s.clone(),
		Rhs::Moniker(m) => format!("{}b moniker", m.as_bytes().len()),
		Rhs::PathPattern(p) => format!("path `{}`", p.raw),
		Rhs::Projection(l) => l.as_str().to_string(),
	}
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
		m.entry(r.target.as_bytes().to_vec()).or_default().push(idx);
	}
	m
}

fn parent_counts_by_kind(graph: &CodeGraph) -> HashMap<(usize, &[u8]), u32> {
	let mut m: HashMap<(usize, &[u8]), u32> = HashMap::new();
	for d in graph.defs() {
		if let Some(p) = d.parent {
			*m.entry((p, d.kind.as_slice())).or_insert(0) += 1;
		}
	}
	m
}

fn comment_end_bytes(graph: &CodeGraph) -> Vec<u32> {
	let mut v: Vec<u32> = graph
		.defs()
		.filter(|d| d.kind.as_slice() == KIND_COMMENT)
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
	d: &DefRecord,
	kind: &str,
	def_idx: usize,
	rules: &CompiledKindRules,
	ctx: &EvalCtx<'_, '_>,
	out: &mut Vec<Violation>,
) {
	if eval_require_doc_comment(d, def_idx, rules, ctx) != Some(false) {
		return;
	}

	let moniker = render_uri(&d.moniker, &ctx.uri_cfg);
	let name = def_name(d).unwrap_or_default();
	let (start_line, end_line) = lines_of(d, ctx.source);
	out.push(Violation {
		rule_id: rule_id(ctx.lang, kind, "require_doc_comment"),
		moniker,
		kind: kind.to_string(),
		lines: (start_line, end_line),
		message: format!("{kind} `{name}` is missing a doc comment immediately before it"),
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
mod tests {
	use super::*;
	use code_moniker_core::core::code_graph::DefAttrs;
	use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};

	const SCHEME: &str = "code+moniker://";

	fn cfg_from(s: &str) -> Config {
		toml::from_str(s).expect("test config must parse")
	}

	fn build_module(name: &[u8]) -> Moniker {
		let mut b = MonikerBuilder::new();
		b.project(b".");
		b.segment(b"lang", b"ts");
		b.segment(b"module", name);
		b.build()
	}

	fn child(parent: &Moniker, kind: &[u8], name: &[u8]) -> Moniker {
		let mut b = MonikerBuilder::from_view(parent.as_view());
		b.segment(kind, name);
		b.build()
	}

	#[test]
	fn no_rules_means_no_violations() {
		let cfg: Config = Config::default();
		let module = build_module(b"a");
		let g = CodeGraph::new(module, b"module");
		let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
		assert!(v.is_empty());
	}

	#[test]
	fn name_regex_violation() {
		let cfg = cfg_from(
			r#"
			[[ts.class.where]]
			id   = "name-pascal"
			expr = "name =~ ^[A-Z][A-Za-z0-9]*$"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let bad = child(&module, b"class", b"lower_case_bad");
		g.add_def(bad, b"class", &module, Some((0, 10))).unwrap();
		let v = evaluate(&g, "anything\n", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1);
		assert_eq!(v[0].rule_id, "ts.class.name-pascal");
	}

	#[test]
	fn auto_id_when_user_omits_one() {
		let cfg = cfg_from(
			r#"
			[[ts.class.where]]
			expr = "name =~ ^[A-Z]"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		g.add_def(
			child(&module, b"class", b"lower"),
			b"class",
			&module,
			Some((0, 10)),
		)
		.unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v[0].rule_id, "ts.class.where_0");
	}

	#[test]
	fn lines_le_violation_uses_actual_count() {
		let cfg = cfg_from(
			r#"
			[[ts.function.where]]
			id   = "max-lines"
			expr = "lines <= 2"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let f = child(&module, b"function", b"foo");
		g.add_def(f, b"function", &module, Some((0, 14))).unwrap();
		let v = evaluate(&g, "a\nb\nc\n", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1);
		assert!(v[0].message.contains("3"));
		assert!(v[0].message.contains("expected 2"));
	}

	#[test]
	fn forbid_name_via_regex_no_match() {
		let cfg = cfg_from(
			r#"
			[[ts.function.where]]
			id   = "no-helper-names"
			expr = "name !~ ^(helper|utils|manager)$"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		g.add_def(
			child(&module, b"function", b"helper"),
			b"function",
			&module,
			Some((0, 5)),
		)
		.unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1);
		assert_eq!(v[0].rule_id, "ts.function.no-helper-names");
	}

	#[test]
	fn count_children_groups_by_parent() {
		let cfg = cfg_from(
			r#"
			[[ts.class.where]]
			id   = "max-methods"
			expr = "count(method) <= 2"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let foo = child(&module, b"class", b"Foo");
		g.add_def(foo.clone(), b"class", &module, Some((0, 100)))
			.unwrap();
		g.add_def(child(&foo, b"method", b"a"), b"method", &foo, Some((1, 5)))
			.unwrap();
		g.add_def(child(&foo, b"method", b"b"), b"method", &foo, Some((6, 10)))
			.unwrap();
		g.add_def(
			child(&foo, b"method", b"c"),
			b"method",
			&foo,
			Some((11, 15)),
		)
		.unwrap();
		let bar = child(&module, b"class", b"Bar");
		g.add_def(bar.clone(), b"class", &module, Some((20, 50)))
			.unwrap();
		g.add_def(
			child(&bar, b"method", b"x"),
			b"method",
			&bar,
			Some((21, 25)),
		)
		.unwrap();
		let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "Foo violates, Bar passes: {v:?}");
		assert!(v[0].moniker.contains("class:Foo"));
	}

	#[test]
	fn text_regex_on_comment() {
		let cfg = cfg_from(
			r#"
			[[ts.comment.where]]
			id   = "no-prose"
			expr = '''text =~ ^\s*//\s*TODO'''
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let cmt = child(&module, b"comment", b"0");
		let source = "// random prose\n";
		g.add_def(cmt, b"comment", &module, Some((0, source.len() as u32 - 1)))
			.unwrap();
		let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1);
	}

	#[test]
	fn moniker_descendant_of() {
		let cfg = cfg_from(
			r#"
			[[ts.method.where]]
			id   = "stay-in-foo"
			expr = "moniker <@ code+moniker://./lang:ts/module:a/class:Foo"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let foo = child(&module, b"class", b"Foo");
		g.add_def(foo.clone(), b"class", &module, Some((0, 50)))
			.unwrap();
		g.add_def(child(&foo, b"method", b"a"), b"method", &foo, Some((1, 5)))
			.unwrap();
		let bar = child(&module, b"class", b"Bar");
		g.add_def(bar.clone(), b"class", &module, Some((10, 30)))
			.unwrap();
		g.add_def(
			child(&bar, b"method", b"b"),
			b"method",
			&bar,
			Some((11, 15)),
		)
		.unwrap();
		let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "Bar.b violates, Foo.a passes");
		assert!(v[0].moniker.contains("class:Bar/method:b"));
	}

	#[test]
	fn invalid_expression_surfaces_at_evaluate() {
		let cfg = cfg_from(
			r#"
			[[ts.class.where]]
			expr = "name =~ [unclosed"
			"#,
		);
		let module = build_module(b"a");
		let g = CodeGraph::new(module, b"module");
		match evaluate(&g, "", Lang::Ts, &cfg, SCHEME) {
			Err(ConfigError::InvalidExpr { at, .. }) => {
				assert!(at.contains("ts.class"), "{at}");
			}
			other => panic!("expected InvalidExpr, got {other:?}"),
		}
	}

	#[test]
	fn unknown_kind_section_still_rejected() {
		let r = toml::from_str::<Config>(
			r#"
			[[ts.classs.where]]
			expr = "name =~ ^X"
			"#,
		);
		// parses fine — kind validation happens in config::validate during load
		assert!(r.is_ok());
	}

	#[test]
	fn require_doc_comment_skips_when_annotations_precede_def() {
		let cfg = cfg_from(
			r#"
			[ts.class]
			require_doc_comment = "public"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");

		// Doc comment at lines 1
		let mut b = MonikerBuilder::from_view(module.as_view());
		b.segment(b"comment", b"0");
		let cmt = b.build();
		g.add_def(cmt, b"comment", &module, Some((0, 10))).unwrap();

		// Class def header starts at line 3 (after `@Decorator` on line 2)
		let source = "/** doc */\n@Decorator\nclass Foo {}\n";
		let mut b = MonikerBuilder::from_view(module.as_view());
		b.segment(b"class", b"Foo");
		let foo = b.build();
		let attrs = DefAttrs {
			visibility: b"public",
			..DefAttrs::default()
		};
		// def starts at `class Foo` byte 22, class def is index 2 in graph
		g.add_def_attrs(foo.clone(), b"class", &module, Some((22, 35)), &attrs)
			.unwrap();
		let class_idx = g.defs().position(|d| d.moniker == foo).unwrap();

		// Emit @Decorator as an annotates ref starting at byte 11 (line 2)
		g.add_ref(
			&g.def_at(class_idx).moniker.clone(),
			module.clone(),
			b"annotates",
			Some((11, 21)),
		)
		.unwrap();

		let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).unwrap();
		assert!(
			v.is_empty(),
			"comment line 1 + annotation line 2 + class line 3: doc must attach via annotation anchor: {v:?}"
		);
	}

	// ─── booleans + implication semantics ───────────────────────────────

	#[test]
	fn or_passes_if_one_arm_passes() {
		let cfg = cfg_from(
			r#"
			[[ts.class.where]]
			id   = "any-of"
			expr = "name = 'Foo' OR name = 'Bar'"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		g.add_def(
			child(&module, b"class", b"Foo"),
			b"class",
			&module,
			Some((0, 10)),
		)
		.unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert!(v.is_empty(), "Foo matches first arm: {v:?}");
	}

	#[test]
	fn or_fails_when_all_arms_fail() {
		let cfg = cfg_from(
			r#"
			[[ts.class.where]]
			id   = "any-of"
			expr = "name = 'Foo' OR name = 'Bar'"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		g.add_def(
			child(&module, b"class", b"Baz"),
			b"class",
			&module,
			Some((0, 10)),
		)
		.unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "Baz matches no arm: {v:?}");
	}

	#[test]
	fn not_inverts_pass_and_fail() {
		let cfg = cfg_from(
			r#"
			[[ts.class.where]]
			id   = "not-internal"
			expr = "NOT name = 'Internal'"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		g.add_def(
			child(&module, b"class", b"Internal"),
			b"class",
			&module,
			Some((0, 5)),
		)
		.unwrap();
		g.add_def(
			child(&module, b"class", b"Public"),
			b"class",
			&module,
			Some((6, 10)),
		)
		.unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "only `Internal` violates: {v:?}");
		assert!(v[0].moniker.contains("class:Internal"));
	}

	#[test]
	fn implies_false_premise_is_pass() {
		// `name = 'Entity' => any(...)` should NOT flag classes that aren't Entities.
		// This is the bug that fix-by-implication addresses.
		let cfg = cfg_from(
			r#"
			[[ts.class.where]]
			id   = "entity-implies-x"
			expr = "name =~ Entity$ => kind = 'class'"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		g.add_def(
			child(&module, b"class", b"NotAnEntity"),
			b"class",
			&module,
			Some((0, 10)),
		)
		.unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert!(
			v.is_empty(),
			"premise false (no `Entity` suffix) ⇒ implication trivially true: {v:?}"
		);
	}

	#[test]
	fn implies_true_premise_evaluates_consequent() {
		let cfg = cfg_from(
			r#"
			[[ts.class.where]]
			id   = "entity-must-be-class"
			expr = "name =~ Entity$ => kind = 'class'"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		// kind is 'class', so this should pass
		g.add_def(
			child(&module, b"class", b"UserEntity"),
			b"class",
			&module,
			Some((0, 10)),
		)
		.unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert!(v.is_empty(), "premise true + consequent true: {v:?}");
	}

	// ─── segment(K) projection ──────────────────────────────────────────

	#[test]
	fn segment_of_def_returns_first_match() {
		let cfg = cfg_from(
			r#"
			[[ts.class.where]]
			id   = "must-be-in-domain-module"
			expr = "segment('module') = 'domain'"
			"#,
		);
		let module = build_module(b"app");
		let mut g = CodeGraph::new(module.clone(), b"module");
		g.add_def(
			child(&module, b"class", b"Foo"),
			b"class",
			&module,
			Some((0, 5)),
		)
		.unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(
			v.len(),
			1,
			"class lives in module:app, not module:domain: {v:?}"
		);
	}

	#[test]
	fn source_and_target_segment_in_refs() {
		let cfg = cfg_from(
			r#"
			[[refs.where]]
			id   = "same-module-only"
			expr = "source.segment('module') != target.segment('module') => target.segment('module') = 'std'"
			"#,
		);
		let root = build_root();
		let mut g = CodeGraph::new(root.clone(), b"module");
		let billing = submodule(&root, b"billing");
		g.add_def(billing.clone(), b"module", &root, Some((0, 1)))
			.unwrap();
		let shipping = submodule(&root, b"shipping");
		g.add_def(shipping.clone(), b"module", &root, Some((2, 3)))
			.unwrap();
		let o = child(&billing, b"class", b"Order");
		g.add_def(o.clone(), b"class", &billing, Some((4, 5)))
			.unwrap();
		let p = child(&shipping, b"class", b"Pkg");
		g.add_def(p.clone(), b"class", &shipping, Some((6, 10)))
			.unwrap();
		g.add_ref(&o, p, b"uses_type", Some((4, 5))).unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "billing→shipping violation: {v:?}");
	}

	#[test]
	fn per_lang_refs_section_is_evaluated() {
		let cfg = cfg_from(
			r#"
			[[ts.refs.where]]
			id   = "no-domain-import"
			expr = "source.segment('module') = 'domain' => NOT kind = 'imports'"
			"#,
		);
		let root = build_root();
		let mut g = CodeGraph::new(root.clone(), b"module");
		let domain = submodule(&root, b"domain");
		g.add_def(domain.clone(), b"module", &root, Some((0, 1)))
			.unwrap();
		let other = submodule(&root, b"infra");
		g.add_def(other.clone(), b"module", &root, Some((2, 3)))
			.unwrap();
		let order = child(&domain, b"class", b"Order");
		g.add_def(order.clone(), b"class", &domain, Some((4, 5)))
			.unwrap();
		let infra_cls = child(&other, b"class", b"X");
		g.add_def(infra_cls.clone(), b"class", &other, Some((6, 10)))
			.unwrap();
		g.add_ref(&order, infra_cls, b"imports", Some((4, 5)))
			.unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "per-lang refs rule fires: {v:?}");
		assert_eq!(v[0].rule_id, "refs.no-domain-import");
	}

	// ─── quantifiers ────────────────────────────────────────────────────

	#[test]
	fn count_method_with_filter() {
		let cfg = cfg_from(
			r#"
			[[ts.class.where]]
			id   = "few-getters"
			expr = "count(method, name =~ ^get) <= 1"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let cls = child(&module, b"class", b"Foo");
		g.add_def(cls.clone(), b"class", &module, Some((0, 50)))
			.unwrap();
		for name in [
			b"getFoo".as_slice(),
			b"getBar".as_slice(),
			b"setBaz".as_slice(),
		] {
			let m = child(&cls, b"method", name);
			g.add_def(m, b"method", &cls, Some((1, 5))).unwrap();
		}
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "2 getters > 1 limit: {v:?}");
	}

	#[test]
	fn any_quantifier_children() {
		let cfg = cfg_from(
			r#"
			[[ts.class.where]]
			id   = "must-have-execute"
			expr = "name =~ UseCase$ => any(method, name = 'execute')"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		// MissingUC has no execute → violation
		let uc = child(&module, b"class", b"PayUseCase");
		g.add_def(uc.clone(), b"class", &module, Some((0, 50)))
			.unwrap();
		g.add_def(
			child(&uc, b"method", b"prepare"),
			b"method",
			&uc,
			Some((1, 5)),
		)
		.unwrap();
		// GoodUC has execute → no violation
		let good = child(&module, b"class", b"GoodUseCase");
		g.add_def(good.clone(), b"class", &module, Some((51, 100)))
			.unwrap();
		g.add_def(
			child(&good, b"method", b"execute"),
			b"method",
			&good,
			Some((52, 60)),
		)
		.unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "PayUseCase lacks execute: {v:?}");
		assert!(v[0].moniker.contains("PayUseCase"));
	}

	#[test]
	fn all_quantifier_children() {
		let cfg = cfg_from(
			r#"
			[[ts.class.where]]
			id   = "methods-short"
			expr = "all(method, lines <= 5)"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let cls = child(&module, b"class", b"Foo");
		g.add_def(cls.clone(), b"class", &module, Some((0, 100)))
			.unwrap();
		g.add_def(child(&cls, b"method", b"ok"), b"method", &cls, Some((0, 4)))
			.unwrap();
		g.add_def(
			child(&cls, b"method", b"long"),
			b"method",
			&cls,
			Some((0, 200)),
		)
		.unwrap();
		let source: String = (0..40).map(|_| "a\n").collect();
		let v = evaluate(&g, &source, Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "long method violates: {v:?}");
	}

	#[test]
	fn none_quantifier_segments() {
		// "this def's moniker has no segment whose kind is 'class'"
		let cfg = cfg_from(
			r#"
			[[ts.function.where]]
			id   = "function-not-in-class"
			expr = "none(segment, segment.kind = 'class')"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let cls = child(&module, b"class", b"Foo");
		g.add_def(cls.clone(), b"class", &module, Some((0, 50)))
			.unwrap();
		// function nested inside class → has a class segment → violates
		let f = child(&cls, b"function", b"inner");
		g.add_def(f, b"function", &cls, Some((1, 5))).unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "function inside class violates: {v:?}");
	}

	#[test]
	fn any_out_refs_must_implement_port() {
		let cfg = cfg_from(
			r#"
			[[ts.class.where]]
			id   = "adapter-implements-port"
			expr = "name =~ Adapter$ => any(out_refs, kind = 'implements' AND target.name =~ Port$)"
			"#,
		);
		let root = build_root();
		let mut g = CodeGraph::new(root.clone(), b"module");
		let m = submodule(&root, b"adapters");
		g.add_def(m.clone(), b"module", &root, Some((0, 1)))
			.unwrap();
		let bad = child(&m, b"class", b"OrderAdapter");
		g.add_def(bad.clone(), b"class", &m, Some((2, 10))).unwrap();
		// No implements ref → adapter without port → violation
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "adapter with no implements: {v:?}");
	}

	// ─── projection extensions ──────────────────────────────────────────

	#[test]
	fn depth_projection() {
		let cfg = cfg_from(
			r#"
			[[ts.class.where]]
			id   = "shallow"
			expr = "depth <= 3"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let cls = child(&module, b"class", b"DeepClass");
		g.add_def(cls.clone(), b"class", &module, Some((0, 5)))
			.unwrap();
		// depth = 3 (project segment doesn't count, segments: lang, module, class)
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert!(v.is_empty(), "depth = 3 is within limit: {v:?}");
	}

	#[test]
	fn parent_name_projection() {
		let cfg = cfg_from(
			r#"
			[[ts.method.where]]
			id   = "no-name-clash"
			expr = "name != parent.name"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let cls = child(&module, b"class", b"Foo");
		g.add_def(cls.clone(), b"class", &module, Some((0, 50)))
			.unwrap();
		let m_ok = child(&cls, b"method", b"bar");
		g.add_def(m_ok, b"method", &cls, Some((1, 10))).unwrap();
		let m_bad = child(&cls, b"method", b"Foo");
		g.add_def(m_bad, b"method", &cls, Some((11, 20))).unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "method `Foo` shares parent name: {v:?}");
	}

	#[test]
	fn parent_kind_projection() {
		let cfg = cfg_from(
			r#"
			[[ts.method.where]]
			id   = "method-in-class"
			expr = "parent.kind = 'class'"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		// method directly under module (no class parent) — violates
		let m = child(&module, b"method", b"loose");
		g.add_def(m, b"method", &module, Some((0, 5))).unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "parent is module, not class: {v:?}");
	}

	#[test]
	fn source_and_target_kind_projection() {
		let cfg = cfg_from(
			r#"
			[[refs.where]]
			id   = "no-class-to-function-edge"
			expr = "source.kind = 'class' => NOT target.kind = 'function'"
			"#,
		);
		let root = build_root();
		let mut g = CodeGraph::new(root.clone(), b"module");
		let cls = child(&root, b"class", b"Foo");
		g.add_def(cls.clone(), b"class", &root, Some((0, 5)))
			.unwrap();
		let func = child(&root, b"function", b"bar");
		g.add_def(func.clone(), b"function", &root, Some((6, 10)))
			.unwrap();
		g.add_ref(&cls, func, b"calls", Some((0, 5))).unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "class→function edge flagged: {v:?}");
	}

	// ─── refs pipeline ──────────────────────────────────────────────────

	fn build_root() -> Moniker {
		let mut b = MonikerBuilder::new();
		b.project(b".");
		b.segment(b"lang", b"ts");
		b.build()
	}

	fn submodule(root: &Moniker, name: &[u8]) -> Moniker {
		let mut b = MonikerBuilder::from_view(root.as_view());
		b.segment(b"module", name);
		b.build()
	}

	#[test]
	fn refs_top_level_flags_cross_layer_dep() {
		let cfg = cfg_from(
			r#"
			[[refs.where]]
			id   = "domain-no-infra"
			expr = "source ~ '**/module:domain/**' => NOT target ~ '**/module:infrastructure/**'"
			"#,
		);
		let root = build_root();
		let mut g = CodeGraph::new(root.clone(), b"module");
		let domain = submodule(&root, b"domain");
		g.add_def(domain.clone(), b"module", &root, Some((0, 1)))
			.unwrap();
		let infra = submodule(&root, b"infrastructure");
		g.add_def(infra.clone(), b"module", &root, Some((2, 3)))
			.unwrap();
		let order = child(&domain, b"class", b"Order");
		g.add_def(order.clone(), b"class", &domain, Some((4, 5)))
			.unwrap();
		let repo = child(&infra, b"class", b"OrderRepoImpl");
		g.add_def(repo.clone(), b"class", &infra, Some((6, 10)))
			.unwrap();
		g.add_ref(&order, repo, b"uses_type", Some((4, 5))).unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "cross-layer ref must violate: {v:?}");
		assert_eq!(v[0].rule_id, "refs.domain-no-infra");
	}

	#[test]
	fn refs_implication_skips_unrelated_refs() {
		let cfg = cfg_from(
			r#"
			[[refs.where]]
			id   = "domain-only-self-or-std"
			expr = "source ~ '**/module:domain/**' => target ~ '**/module:domain/**' OR target ~ '**/module:std/**'"
			"#,
		);
		let root = build_root();
		let mut g = CodeGraph::new(root.clone(), b"module");
		let domain = submodule(&root, b"domain");
		g.add_def(domain.clone(), b"module", &root, Some((0, 1)))
			.unwrap();
		let std_mod = submodule(&root, b"std");
		g.add_def(std_mod.clone(), b"module", &root, Some((2, 3)))
			.unwrap();
		let order = child(&domain, b"class", b"Order");
		g.add_def(order.clone(), b"class", &domain, Some((4, 5)))
			.unwrap();
		let vec_class = child(&std_mod, b"class", b"Vec");
		g.add_def(vec_class.clone(), b"class", &std_mod, Some((6, 10)))
			.unwrap();
		g.add_ref(&order, vec_class, b"uses_type", Some((4, 5)))
			.unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert!(v.is_empty(), "domain → std is allowed: {v:?}");
	}

	#[test]
	fn refs_filtered_by_kind() {
		let cfg = cfg_from(
			r#"
			[[refs.where]]
			id   = "no-domain-imports-framework"
			expr = "source ~ '**/module:domain/**' AND kind = 'imports' => NOT target.name =~ ^(express|nestjs)$"
			"#,
		);
		let root = build_root();
		let mut g = CodeGraph::new(root.clone(), b"module");
		let domain = submodule(&root, b"domain");
		g.add_def(domain.clone(), b"module", &root, Some((0, 1)))
			.unwrap();
		let ext = submodule(&root, b"extern");
		g.add_def(ext.clone(), b"module", &root, Some((2, 3)))
			.unwrap();
		let order = child(&domain, b"class", b"Order");
		g.add_def(order.clone(), b"class", &domain, Some((4, 5)))
			.unwrap();
		let express = child(&ext, b"class", b"express");
		g.add_def(express.clone(), b"class", &ext, Some((6, 10)))
			.unwrap();
		g.add_ref(&order, express, b"imports", Some((4, 5)))
			.unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "domain import of express must violate: {v:?}");
	}

	#[test]
	fn alias_expands_in_rule_expr() {
		let cfg = cfg_from(
			r#"
			[aliases]
			domain = "moniker ~ '**/module:domain/**'"

			[[ts.class.where]]
			id   = "no-class-in-domain"
			expr = "NOT $domain"
			"#,
		);
		let module = build_module(b"domain");
		let mut g = CodeGraph::new(module.clone(), b"module");
		g.add_def(
			child(&module, b"class", b"Foo"),
			b"class",
			&module,
			Some((0, 5)),
		)
		.unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "class in module:domain violates: {v:?}");
	}

	#[test]
	fn path_match_subtree_flags_domain_class() {
		let cfg = cfg_from(
			r#"
			[[ts.class.where]]
			id   = "no-class-in-domain"
			expr = "NOT moniker ~ '**/module:domain/**'"
			"#,
		);
		let module = build_module(b"domain");
		let mut g = CodeGraph::new(module.clone(), b"module");
		g.add_def(
			child(&module, b"class", b"User"),
			b"class",
			&module,
			Some((0, 10)),
		)
		.unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "class lives in module:domain: {v:?}");
	}

	#[test]
	fn has_segment_finds_module() {
		let cfg = cfg_from(
			r#"
			[[ts.class.where]]
			id   = "must-be-in-app"
			expr = "has_segment('module', 'application')"
			"#,
		);
		let module = build_module(b"infrastructure");
		let mut g = CodeGraph::new(module.clone(), b"module");
		g.add_def(
			child(&module, b"class", b"Foo"),
			b"class",
			&module,
			Some((0, 5)),
		)
		.unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "Foo lives in infrastructure, not application");
	}

	#[test]
	fn path_regex_step_on_class_name() {
		let cfg = cfg_from(
			r#"
			[[ts.class.where]]
			id   = "ports-only-in-app"
			expr = "moniker ~ '**/class:/Port$/' => has_segment('module', 'application')"
			"#,
		);
		let module = build_module(b"domain");
		let mut g = CodeGraph::new(module.clone(), b"module");
		// A `Port` class living in `domain` (wrong place) — should flag.
		g.add_def(
			child(&module, b"class", b"UserPort"),
			b"class",
			&module,
			Some((0, 5)),
		)
		.unwrap();
		// A non-Port class in domain — premise false, should NOT flag.
		g.add_def(
			child(&module, b"class", b"Order"),
			b"class",
			&module,
			Some((6, 10)),
		)
		.unwrap();
		let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(v.len(), 1, "only `UserPort` violates: {v:?}");
		assert!(v[0].moniker.contains("UserPort"));
	}

	#[test]
	fn implies_true_premise_failed_consequent_violates() {
		let cfg = cfg_from(
			r#"
			[[ts.function.where]]
			id   = "use-case-has-one-method"
			expr = "name =~ UseCase$ => lines <= 5"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		g.add_def(
			child(&module, b"function", b"CreateInvoiceUseCase"),
			b"function",
			&module,
			Some((0, 200)),
		)
		.unwrap();
		// 50 lines of source so lines > 5
		let source: String = (0..50).map(|_| "a\n").collect();
		let v = evaluate(&g, &source, Lang::Ts, &cfg, SCHEME).unwrap();
		assert_eq!(
			v.len(),
			1,
			"premise true, consequent false ⇒ violation: {v:?}"
		);
	}
}
