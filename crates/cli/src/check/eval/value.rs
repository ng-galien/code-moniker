use std::collections::HashMap;

use crate::check::expr::{
	AggregateKind, Atom, Binding, CollectionExpr, CollectionOp, Domain, DomainValueExpr,
	NumberExpr, Op, PairProjection, PairSide, Rhs, ValueExpr,
};
use code_moniker_core::core::moniker::Moniker;

use super::AtomOutcome;

/// Value-vs-Value comparison for the cases where the RHS is itself a
/// projection. Restricted to the ops that pair naturally (equality and
/// numeric ordering); a structural moniker op against a string projection
/// stays `NotApplicable`.
pub(super) fn apply_op_values(lhs: &Value, op: Op, rhs: &Value) -> AtomOutcome {
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

#[derive(Clone)]
pub(super) enum Value {
	Str(String),
	Number(f64),
	Moniker(Moniker),
}

pub(super) fn apply_op(value: &Value, atom: &Atom) -> AtomOutcome {
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

pub(super) fn render_value(v: &Value) -> String {
	match v {
		Value::Str(s) => s.clone(),
		Value::Number(n) => render_number(*n),
		Value::Moniker(m) => format!("{}b moniker", m.as_bytes().len()),
	}
}

fn render_number(n: f64) -> String {
	if n.fract() == 0.0 {
		format!("{n:.0}")
	} else {
		format!("{n:.3}")
			.trim_end_matches('0')
			.trim_end_matches('.')
			.to_string()
	}
}

fn render_rhs(r: &Rhs) -> String {
	match r {
		Rhs::Str(s) => s.clone(),
		Rhs::Number(expr) => render_number_expr(expr),
		Rhs::RegexStr(s) => s.clone(),
		Rhs::Moniker(m) => format!("{}b moniker", m.as_bytes().len()),
		Rhs::PathPattern(p) => format!("path `{}`", p.raw),
		Rhs::Projection(l) => l.as_str().to_string(),
		Rhs::PairProjection(projection) => pair_projection_label(*projection),
		Rhs::Collection(collection) => collection_label(collection),
	}
}

pub(super) fn number_expr_label(expr: &NumberExpr) -> &'static str {
	match expr {
		NumberExpr::Literal(_) => "number",
		NumberExpr::Projection(lhs) => lhs.as_str(),
		NumberExpr::Count { .. } => "count",
		NumberExpr::Aggregate { kind, .. } => aggregate_label(*kind),
		NumberExpr::Metric { kind, .. } => kind.as_str(),
		NumberExpr::Entropy(_) => "entropy",
		NumberExpr::Size(_) => "size",
	}
}

pub(super) fn render_number_expr(expr: &NumberExpr) -> String {
	match expr {
		NumberExpr::Literal(n) => render_number(*n),
		NumberExpr::Projection(lhs) => lhs.as_str().to_string(),
		NumberExpr::Count { domain, .. } => match domain {
			Domain::Children(kind) => format!("count({kind})"),
			Domain::ChildrenByShape(shape) => format!("count(shape:{shape})"),
			Domain::Pairs(inner) => format!("count(pairs({}))", domain_label(inner)),
			Domain::ProjectDefs => "count(project.def)".to_string(),
			Domain::Segments => "count(segment)".to_string(),
			Domain::OutRefs => "count(out_refs)".to_string(),
			Domain::InRefs => "count(in_refs)".to_string(),
		},
		NumberExpr::Aggregate { kind, domain, .. } => {
			format!("{}({})", aggregate_label(*kind), domain_label(domain))
		}
		NumberExpr::Metric { kind, binding } => {
			format!("{}({})", kind.as_str(), binding_label(*binding))
		}
		NumberExpr::Entropy(collection) => format!("entropy({})", domain_value_label(collection)),
		NumberExpr::Size(collection) => format!("size({})", collection_label(collection)),
	}
}

fn aggregate_label(kind: AggregateKind) -> &'static str {
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

fn binding_label(binding: Binding) -> &'static str {
	match binding {
		Binding::Self_ => "self",
		Binding::Each => "each",
	}
}

fn domain_value_label(collection: &DomainValueExpr) -> String {
	match collection.expr.as_ref() {
		ValueExpr::Item => domain_label(&collection.domain),
		ValueExpr::Projection(lhs) => {
			format!("{}, {}", domain_label(&collection.domain), lhs.as_str())
		}
		ValueExpr::Number(expr) => {
			format!(
				"{}, {}",
				domain_label(&collection.domain),
				render_number_expr(expr)
			)
		}
	}
}

fn domain_label(domain: &Domain) -> String {
	match domain {
		Domain::Children(kind) => kind.clone(),
		Domain::ChildrenByShape(shape) => format!("shape:{shape}"),
		Domain::Pairs(inner) => format!("pairs({})", domain_label(inner)),
		Domain::ProjectDefs => "project.def".to_string(),
		Domain::Segments => "segment".to_string(),
		Domain::OutRefs => "out_refs".to_string(),
		Domain::InRefs => "in_refs".to_string(),
	}
}

pub(super) fn pair_projection_label(projection: PairProjection) -> String {
	let side = match projection.side {
		PairSide::A => "a",
		PairSide::B => "b",
	};
	format!("{side}.{}", projection.lhs.as_str())
}

fn collection_label(collection: &CollectionExpr) -> String {
	match collection {
		CollectionExpr::Projection(projection) => {
			if projection.path.is_empty() {
				domain_label(&projection.domain)
			} else {
				format!(
					"{}.{}",
					domain_label(&projection.domain),
					projection.path.join(".")
				)
			}
		}
		CollectionExpr::PairProjection(projection) => {
			let side = match projection.side {
				PairSide::A => "a",
				PairSide::B => "b",
			};
			if projection.path.is_empty() {
				format!("{side}.{}", domain_label(&projection.domain))
			} else {
				format!(
					"{side}.{}.{}",
					domain_label(&projection.domain),
					projection.path.join(".")
				)
			}
		}
		CollectionExpr::Unique(inner) => format!("unique({})", collection_label(inner)),
		CollectionExpr::Binary { op, left, right } => {
			let op = match op {
				CollectionOp::Intersect => "intersect",
				CollectionOp::Union => "union",
				CollectionOp::Difference => "diff",
			};
			format!(
				"{} {op} {}",
				collection_label(left),
				collection_label(right)
			)
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(super) enum ValueKey {
	Str(String),
	Number(u64),
	Moniker(Vec<u8>),
}

impl ValueKey {
	pub(super) fn from_value(value: Value) -> Self {
		match value {
			Value::Str(s) => Self::Str(s),
			Value::Number(n) => Self::Number(n.to_bits()),
			Value::Moniker(m) => Self::Moniker(m.as_bytes().to_vec()),
		}
	}

	pub(super) fn into_value(self) -> Value {
		match self {
			Self::Str(s) => Value::Str(s),
			Self::Number(bits) => Value::Number(f64::from_bits(bits)),
			Self::Moniker(bytes) => Value::Moniker(Moniker::from_canonical_bytes(bytes)),
		}
	}
}

pub(super) fn mode_value(values: Vec<Value>) -> Option<Value> {
	let mut counts = HashMap::new();
	let mut order = Vec::new();
	for value in values {
		let key = ValueKey::from_value(value);
		if !counts.contains_key(&key) {
			order.push(key.clone());
		}
		*counts.entry(key).or_insert(0) += 1;
	}
	let mut best = None;
	for key in order {
		let count = counts.get(&key).copied().unwrap_or(0);
		if best
			.as_ref()
			.is_none_or(|(_, best_count)| count > *best_count)
		{
			best = Some((key, count));
		}
	}
	best.map(|(key, _)| key.into_value())
}

pub(super) fn value_counts(values: impl Iterator<Item = Value>) -> HashMap<ValueKey, usize> {
	let mut counts = HashMap::new();
	for value in values {
		*counts.entry(ValueKey::from_value(value)).or_insert(0) += 1;
	}
	counts
}
