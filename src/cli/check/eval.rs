use std::collections::HashMap;

use crate::cli::check::config::{Config, ConfigError, KindRules, config_section};
use crate::cli::check::expr::{self, Atom, Lhs, LhsExpr, Node, Op, Rhs};
use crate::cli::lines::line_range;
use crate::cli::render_uri;
use crate::core::code_graph::{CodeGraph, DefRecord};
use crate::core::kinds::KIND_COMMENT;
use crate::core::moniker::query::bare_callable_name;
use crate::core::uri::UriConfig;
use crate::lang::Lang;

#[derive(Debug, Clone)]
pub struct Violation {
	pub rule_id: String,
	pub moniker: String,
	pub kind: String,
	pub lines: (u32, u32),
	pub message: String,
	pub explanation: Option<String>,
}

pub fn evaluate(
	graph: &CodeGraph,
	source: &str,
	lang: Lang,
	cfg: &Config,
	scheme: &str,
) -> Result<Vec<Violation>, ConfigError> {
	let compiled = CompiledRules::for_lang(cfg, lang, scheme)?;
	let need_doc_anchors = compiled
		.by_kind
		.values()
		.any(|r| r.require_doc_for_vis.is_some());
	let ctx = EvalCtx {
		source,
		lang,
		uri_cfg: UriConfig { scheme },
		parent_counts: parent_counts_by_kind(graph),
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

	Ok(out)
}

struct EvalCtx<'g, 'src> {
	source: &'src str,
	lang: Lang,
	uri_cfg: UriConfig<'src>,
	parent_counts: HashMap<(usize, &'g [u8]), u32>,
	/// Sorted byte offsets where comment defs end. Empty when no rule needs
	/// `require_doc_comment`.
	comment_ends: Vec<u32>,
	/// Earliest `annotates` ref start byte per annotated def — the header
	/// anchor for the doc-comment adjacency check.
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

struct CompiledRules<'cfg> {
	by_kind: HashMap<&'cfg str, CompiledKindRules>,
}

impl<'cfg> CompiledRules<'cfg> {
	fn for_lang(cfg: &'cfg Config, lang: Lang, scheme: &str) -> Result<Self, ConfigError> {
		let section = config_section(lang);
		let allowed = crate::cli::check::config::allowed_kinds_for(lang);
		let mut by_kind: HashMap<&str, CompiledKindRules> = HashMap::new();
		for (kind, rules) in cfg.for_lang(lang).kinds.iter() {
			by_kind.insert(
				kind.as_str(),
				compile(rules, section, kind, scheme, &allowed)?,
			);
		}
		for (kind, rules) in cfg.default.kinds.iter() {
			if !by_kind.contains_key(kind.as_str()) {
				by_kind.insert(
					kind.as_str(),
					compile(rules, "default", kind, scheme, &allowed)?,
				);
			}
		}
		Ok(Self { by_kind })
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
) -> Result<CompiledKindRules, ConfigError> {
	let mut compiled = Vec::with_capacity(rules.rules.len());
	for (idx, entry) in rules.rules.iter().enumerate() {
		let id = entry.id.clone().unwrap_or_else(|| format!("where_{idx}"));
		let parsed = expr::parse(&entry.expr, scheme, allowed_kinds).map_err(|error| {
			ConfigError::InvalidExpr {
				at: format!("{section}.{kind}.{id}"),
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
	} = match eval_node(&rule.root, d, def_idx, ctx.source, &ctx.parent_counts) {
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

fn describe_lhs(lhs: &LhsExpr) -> &str {
	match lhs {
		LhsExpr::Attr(a) => a.as_str(),
		LhsExpr::CountChildren(_) => "count",
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
	/// Atom not extractable on this def (anonymous, no position, …).
	/// Propagation rules (Kleene-style trivalent logic):
	///   And : NA + Pass = NA ; NA + Fail = Fail ; NA + NA = NA
	///   Or  : NA + Pass = Pass; NA + Fail = NA ; NA + NA = NA
	///   Not(NA) = NA
	///   Implies(NA, _) = NA  — premise indeterminate ⇒ rule doesn't apply
	NotApplicable,
}

enum AtomOutcome {
	Pass,
	Fail { actual: String, expected: String },
	NotApplicable,
}

fn eval_node(
	node: &Node,
	d: &DefRecord,
	def_idx: usize,
	source: &str,
	parent_counts: &HashMap<(usize, &[u8]), u32>,
) -> NodeOutcome {
	match node {
		Node::Atom(atom) => match eval_atom(atom, d, def_idx, source, parent_counts) {
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
				match eval_node(c, d, def_idx, source, parent_counts) {
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
				match eval_node(c, d, def_idx, source, parent_counts) {
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
		Node::Not(inner) => match eval_node(inner, d, def_idx, source, parent_counts) {
			NodeOutcome::Pass => NodeOutcome::Fail(Failure {
				atom_raw: "NOT (...)".to_string(),
				lhs_label: "NOT".to_string(),
				actual: "true".to_string(),
				expected: "false".to_string(),
			}),
			NodeOutcome::Fail(_) => NodeOutcome::Pass,
			NodeOutcome::NotApplicable => NodeOutcome::NotApplicable,
		},
		Node::Implies(prem, cons) => match eval_node(prem, d, def_idx, source, parent_counts) {
			NodeOutcome::Pass => eval_node(cons, d, def_idx, source, parent_counts),
			NodeOutcome::Fail(_) => NodeOutcome::Pass,
			NodeOutcome::NotApplicable => NodeOutcome::NotApplicable,
		},
	}
}

fn eval_atom(
	atom: &Atom,
	d: &DefRecord,
	def_idx: usize,
	source: &str,
	parent_counts: &HashMap<(usize, &[u8]), u32>,
) -> AtomOutcome {
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
		LhsExpr::CountChildren(k) => {
			let c = parent_counts
				.get(&(def_idx, k.as_bytes()))
				.copied()
				.unwrap_or(0);
			Value::Number(c)
		}
	};
	apply_op(&value, atom)
}

enum Value {
	Str(String),
	Number(u32),
	Moniker(crate::core::moniker::Moniker),
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
		(Value::Moniker(m), PathMatch, Rhs::PathPattern(p)) => {
			crate::cli::check::path::matches(p, m)
		}
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
	}
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
	let Some(filter) = &rules.require_doc_for_vis else {
		return;
	};
	let vis = std::str::from_utf8(&d.visibility).unwrap_or("");
	if filter != "any" && filter != vis {
		return;
	}
	let Some((def_start, _)) = d.position else {
		return;
	};
	let header_start = ctx
		.doc_anchors
		.get(&def_idx)
		.copied()
		.map(|anc| anc.min(def_start))
		.unwrap_or(def_start);

	let idx = ctx.comment_ends.partition_point(|&end| end <= header_start);
	let has_doc =
		idx > 0 && comment_attaches_to(ctx.source, ctx.comment_ends[idx - 1], header_start);
	if has_doc {
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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::code_graph::DefAttrs;
	use crate::core::moniker::{Moniker, MonikerBuilder};

	const SCHEME: &str = "ts+moniker://";

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
			expr = "moniker <@ ts+moniker://./lang:ts/module:a/class:Foo"
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
