use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::test_support::{KINDS, TS};
use super::*;

#[test]
fn valid_corpus_snapshots_public_ast() {
	for case in corpus_cases("valid") {
		let input = read_case(&case);
		let expr = parse(input.trim(), TS, KINDS).expect("valid corpus expression parses");
		insta::assert_json_snapshot!(case_name(&case), snapshot_expr(&expr));
	}
}

#[test]
fn invalid_corpus_snapshots_public_errors() {
	for case in corpus_cases("invalid") {
		let input = read_case(&case);
		let err = parse(input.trim(), TS, KINDS).expect_err("invalid corpus expression fails");
		insta::assert_json_snapshot!(
			case_name(&case),
			json!({
				"input": input.trim(),
				"error": err.to_string(),
			})
		);
	}
}

fn corpus_cases(kind: &str) -> Vec<PathBuf> {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("src/check/expr/corpus")
		.join(kind);
	let mut cases = fs::read_dir(&root)
		.unwrap_or_else(|err| panic!("read corpus directory {}: {err}", root.display()))
		.map(|entry| entry.expect("read corpus entry").path())
		.filter(|path| path.extension().is_some_and(|ext| ext == "expr"))
		.collect::<Vec<_>>();
	cases.sort();
	cases
}

fn read_case(path: &Path) -> String {
	fs::read_to_string(path)
		.unwrap_or_else(|err| panic!("read corpus file {}: {err}", path.display()))
}

fn case_name(path: &Path) -> String {
	path.file_stem()
		.and_then(|stem| stem.to_str())
		.expect("UTF-8 corpus filename")
		.replace('-', "_")
}

fn snapshot_expr(expr: &Expr) -> Value {
	json!({
		"root": snapshot_node(&expr.root),
	})
}

fn snapshot_node(node: &Node) -> Value {
	match node {
		Node::Atom(atom) => snapshot_atom(atom),
		Node::And(nodes) => json!({
			"type": "and",
			"nodes": nodes.iter().map(snapshot_node).collect::<Vec<_>>(),
		}),
		Node::Or(nodes) => json!({
			"type": "or",
			"nodes": nodes.iter().map(snapshot_node).collect::<Vec<_>>(),
		}),
		Node::Not(inner) => json!({
			"type": "not",
			"node": snapshot_node(inner),
		}),
		Node::Implies(lhs, rhs) => json!({
			"type": "implies",
			"lhs": snapshot_node(lhs),
			"rhs": snapshot_node(rhs),
		}),
		Node::Require(pattern) => json!({
			"type": "require",
			"pattern": pattern,
		}),
		Node::Quantifier {
			kind,
			domain,
			filter,
		} => json!({
			"type": "quantifier",
			"kind": snapshot_quant_kind(*kind),
			"domain": snapshot_domain(domain),
			"filter": snapshot_node(filter),
		}),
	}
}

fn snapshot_atom(atom: &Atom) -> Value {
	json!({
		"type": "atom",
		"lhs": snapshot_lhs_expr(&atom.lhs),
		"op": snapshot_op(atom.op),
		"rhs": snapshot_rhs(&atom.rhs),
		"raw": atom.raw,
	})
}

fn snapshot_lhs_expr(lhs: &LhsExpr) -> Value {
	match lhs {
		LhsExpr::Attr(lhs) => json!({
			"type": "attr",
			"name": lhs.as_str(),
		}),
		LhsExpr::Number(expr) => json!({
			"type": "number",
			"expr": snapshot_number_expr(expr),
		}),
		LhsExpr::Collection(expr) => json!({
			"type": "collection",
			"expr": snapshot_collection_expr(expr),
		}),
		LhsExpr::Mode(expr) => json!({
			"type": "mode",
			"expr": snapshot_domain_value_expr(expr),
		}),
		LhsExpr::PairProjection(projection) => json!({
			"type": "pair_projection",
			"projection": snapshot_pair_projection(projection),
		}),
		LhsExpr::SegmentOf { scope, kind } => json!({
			"type": "segment_of",
			"scope": snapshot_segment_scope(*scope),
			"kind": kind,
		}),
	}
}

fn snapshot_number_expr(expr: &NumberExpr) -> Value {
	match expr {
		NumberExpr::Literal(value) => json!({
			"type": "literal",
			"value": value,
		}),
		NumberExpr::Projection(lhs) => json!({
			"type": "projection",
			"name": lhs.as_str(),
		}),
		NumberExpr::Count { domain, filter } => json!({
			"type": "count",
			"domain": snapshot_domain(domain),
			"filter": filter.as_ref().map(|filter| snapshot_node(filter)),
		}),
		NumberExpr::Aggregate {
			kind,
			domain,
			expr,
			percentile,
		} => json!({
			"type": "aggregate",
			"kind": snapshot_aggregate_kind(*kind),
			"domain": snapshot_domain(domain),
			"expr": snapshot_number_expr(expr),
			"percentile": percentile,
		}),
		NumberExpr::Metric { kind, binding } => json!({
			"type": "metric",
			"kind": kind.as_str(),
			"binding": snapshot_binding(*binding),
		}),
		NumberExpr::Entropy(expr) => json!({
			"type": "entropy",
			"expr": snapshot_domain_value_expr(expr),
		}),
		NumberExpr::Size(expr) => json!({
			"type": "size",
			"expr": snapshot_collection_expr(expr),
		}),
	}
}

fn snapshot_domain_value_expr(expr: &DomainValueExpr) -> Value {
	json!({
		"domain": snapshot_domain(&expr.domain),
		"expr": snapshot_value_expr(&expr.expr),
	})
}

fn snapshot_value_expr(expr: &ValueExpr) -> Value {
	match expr {
		ValueExpr::Item => json!({
			"type": "item",
		}),
		ValueExpr::Projection(lhs) => json!({
			"type": "projection",
			"name": lhs.as_str(),
		}),
		ValueExpr::Number(expr) => json!({
			"type": "number",
			"expr": snapshot_number_expr(expr),
		}),
	}
}

fn snapshot_collection_expr(expr: &CollectionExpr) -> Value {
	match expr {
		CollectionExpr::Projection(projection) => json!({
			"type": "projection",
			"domain": snapshot_domain(&projection.domain),
			"path": projection.path,
		}),
		CollectionExpr::PairProjection(projection) => json!({
			"type": "pair_projection",
			"side": snapshot_pair_side(projection.side),
			"domain": snapshot_domain(&projection.domain),
			"path": projection.path,
		}),
		CollectionExpr::Unique(expr) => json!({
			"type": "unique",
			"expr": snapshot_collection_expr(expr),
		}),
		CollectionExpr::Binary { op, left, right } => json!({
			"type": "binary",
			"op": snapshot_collection_op(*op),
			"left": snapshot_collection_expr(left),
			"right": snapshot_collection_expr(right),
		}),
	}
}

fn snapshot_rhs(rhs: &Rhs) -> Value {
	match rhs {
		Rhs::Number(expr) => json!({
			"type": "number",
			"expr": snapshot_number_expr(expr),
		}),
		Rhs::RegexStr(pattern) => json!({
			"type": "regex",
			"pattern": pattern,
		}),
		Rhs::Moniker(moniker) => json!({
			"type": "moniker",
			"segments": snapshot_moniker(moniker),
		}),
		Rhs::Str(value) => json!({
			"type": "string",
			"value": value,
		}),
		Rhs::PathPattern(pattern) => json!({
			"type": "path_pattern",
			"pattern": pattern.raw,
		}),
		Rhs::Projection(lhs) => json!({
			"type": "projection",
			"name": lhs.as_str(),
		}),
		Rhs::PairProjection(projection) => json!({
			"type": "pair_projection",
			"projection": snapshot_pair_projection(projection),
		}),
		Rhs::Collection(expr) => json!({
			"type": "collection",
			"expr": snapshot_collection_expr(expr),
		}),
	}
}

fn snapshot_domain(domain: &Domain) -> Value {
	match domain {
		Domain::Children(kind) => json!({
			"type": "children",
			"kind": kind,
		}),
		Domain::ChildrenByShape(shape) => json!({
			"type": "children_by_shape",
			"shape": shape,
		}),
		Domain::Pairs(domain) => json!({
			"type": "pairs",
			"domain": snapshot_domain(domain),
		}),
		Domain::Segments => json!({
			"type": "segments",
		}),
		Domain::OutRefs => json!({
			"type": "out_refs",
		}),
		Domain::InRefs => json!({
			"type": "in_refs",
		}),
	}
}

fn snapshot_pair_projection(projection: &PairProjection) -> Value {
	json!({
		"side": snapshot_pair_side(projection.side),
		"lhs": projection.lhs.as_str(),
	})
}

fn snapshot_moniker(moniker: &code_moniker_core::core::moniker::Moniker) -> Value {
	let view = moniker.as_view();
	json!({
		"project": String::from_utf8_lossy(view.project()),
		"segments": view
			.segments()
			.map(|segment| {
				json!({
					"kind": String::from_utf8_lossy(segment.kind),
					"name": String::from_utf8_lossy(segment.name),
				})
			})
			.collect::<Vec<_>>(),
	})
}

fn snapshot_op(op: Op) -> &'static str {
	match op {
		Op::Eq => "eq",
		Op::Ne => "ne",
		Op::Lt => "lt",
		Op::Le => "le",
		Op::Gt => "gt",
		Op::Ge => "ge",
		Op::RegexMatch => "regex_match",
		Op::RegexNoMatch => "regex_no_match",
		Op::AncestorOf => "ancestor_of",
		Op::DescendantOf => "descendant_of",
		Op::BindMatch => "bind_match",
		Op::PathMatch => "path_match",
		Op::Subset => "subset",
	}
}

fn snapshot_aggregate_kind(kind: AggregateKind) -> &'static str {
	match kind {
		AggregateKind::Sum => "sum",
		AggregateKind::Max => "max",
		AggregateKind::Min => "min",
		AggregateKind::Avg => "avg",
		AggregateKind::Median => "median",
		AggregateKind::Percentile => "percentile",
		AggregateKind::Stddev => "stddev",
		AggregateKind::Var => "var",
		AggregateKind::Cv => "cv",
		AggregateKind::Gini => "gini",
	}
}

fn snapshot_binding(binding: Binding) -> &'static str {
	match binding {
		Binding::Self_ => "self",
		Binding::Each => "each",
	}
}

fn snapshot_collection_op(op: CollectionOp) -> &'static str {
	match op {
		CollectionOp::Intersect => "intersect",
		CollectionOp::Union => "union",
		CollectionOp::Difference => "diff",
	}
}

fn snapshot_pair_side(side: PairSide) -> &'static str {
	match side {
		PairSide::A => "a",
		PairSide::B => "b",
	}
}

fn snapshot_segment_scope(scope: SegmentScope) -> &'static str {
	match scope {
		SegmentScope::Def => "def",
		SegmentScope::Source => "source",
		SegmentScope::Target => "target",
	}
}

fn snapshot_quant_kind(kind: QuantKind) -> &'static str {
	match kind {
		QuantKind::Any => "any",
		QuantKind::All => "all",
		QuantKind::None => "none",
	}
}
