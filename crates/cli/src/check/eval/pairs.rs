use crate::check::expr::{Atom, Domain, LhsExpr, Node, PairProjection, PairSide, QuantKind, Rhs};

use super::collection::{eval_pair_collection_size, eval_pair_collection_subset};
use super::local::{DomainItem, domain_items, eval_mode, project_lhs_value};
use super::value::{Value, apply_op, apply_op_values};
use super::{
	AtomOutcome, EvalCtx, Failure, NodeOutcome, eval_number_expr_def, resolve_def_lhs, walk_node,
};

pub(super) fn eval_pair_count(
	domain: &Domain,
	filter: Option<&Node>,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> u32 {
	let items = domain_items(domain, def_idx, ctx);
	let mut count = 0;
	for (a, b) in pair_iter(&items) {
		let passes = filter.is_none_or(|node| {
			matches!(
				eval_pair_node(node, a, b, def_idx, self_idx, ctx),
				NodeOutcome::Pass
			)
		});
		if passes {
			count += 1;
		}
	}
	count
}

pub(super) fn eval_pair_quantifier(
	kind: QuantKind,
	domain: &Domain,
	filter: &Node,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> NodeOutcome {
	let items = domain_items(domain, def_idx, ctx);
	let mut total = 0u32;
	let mut passes = 0u32;
	for (a, b) in pair_iter(&items) {
		total += 1;
		if matches!(
			eval_pair_node(filter, a, b, def_idx, self_idx, ctx),
			NodeOutcome::Pass
		) {
			passes += 1;
		}
	}
	pair_quantifier_outcome(kind, total, passes)
}

fn pair_iter<'a>(
	items: &'a [DomainItem<'a>],
) -> impl Iterator<Item = (DomainItem<'a>, DomainItem<'a>)> + 'a {
	items
		.iter()
		.enumerate()
		.flat_map(|(idx, a)| items[idx + 1..].iter().map(move |b| (*a, *b)))
}

fn eval_pair_node(
	node: &Node,
	a: DomainItem<'_>,
	b: DomainItem<'_>,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> NodeOutcome {
	walk_node(
		node,
		&|atom| eval_pair_atom(atom, a, b, def_idx, self_idx, ctx),
		&|_, _, _| NodeOutcome::NotApplicable,
	)
}

fn eval_pair_atom(
	atom: &Atom,
	a: DomainItem<'_>,
	b: DomainItem<'_>,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> AtomOutcome {
	if let (LhsExpr::Collection(left), crate::check::expr::Op::Subset, Rhs::Collection(right)) =
		(&atom.lhs, atom.op, &atom.rhs)
	{
		return match eval_pair_collection_subset(left, right, a, b, def_idx, self_idx, ctx) {
			Some(true) => AtomOutcome::Pass,
			Some(false) => AtomOutcome::Fail {
				actual: "not subset".to_string(),
				expected: "subset".to_string(),
			},
			None => AtomOutcome::NotApplicable,
		};
	}
	let Some(value) = pair_lhs_value(&atom.lhs, a, b, def_idx, self_idx, ctx) else {
		return AtomOutcome::NotApplicable;
	};
	if let Some(rhs_val) = pair_rhs_value(&atom.rhs, a, b, def_idx, self_idx, ctx) {
		return apply_op_values(&value, atom.op, &rhs_val);
	}
	apply_op(&value, atom)
}

fn pair_lhs_value(
	lhs: &LhsExpr,
	a: DomainItem<'_>,
	b: DomainItem<'_>,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> Option<Value> {
	match lhs {
		LhsExpr::PairProjection(projection) => pair_projection_value(*projection, a, b, ctx),
		_ => owner_lhs_value(lhs, a, b, def_idx, self_idx, ctx),
	}
}

fn pair_rhs_value(
	rhs: &Rhs,
	a: DomainItem<'_>,
	b: DomainItem<'_>,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> Option<Value> {
	match rhs {
		Rhs::PairProjection(projection) => pair_projection_value(*projection, a, b, ctx),
		Rhs::Projection(lhs) => resolve_def_lhs(*lhs, ctx.graph.def_at(def_idx), ctx),
		Rhs::Number(expr) => {
			eval_pair_number_expr(expr, a, b, def_idx, self_idx, ctx).map(Value::Number)
		}
		_ => None,
	}
}

fn pair_projection_value(
	projection: PairProjection,
	a: DomainItem<'_>,
	b: DomainItem<'_>,
	ctx: &EvalCtx<'_, '_>,
) -> Option<Value> {
	let item = match projection.side {
		PairSide::A => a,
		PairSide::B => b,
	};
	project_lhs_value(item, projection.lhs, ctx)
}

fn owner_lhs_value(
	lhs: &LhsExpr,
	a: DomainItem<'_>,
	b: DomainItem<'_>,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> Option<Value> {
	let owner = ctx.graph.def_at(def_idx);
	match lhs {
		LhsExpr::Attr(lhs) => resolve_def_lhs(*lhs, owner, ctx),
		LhsExpr::Number(expr) => {
			eval_pair_number_expr(expr, a, b, def_idx, self_idx, ctx).map(Value::Number)
		}
		LhsExpr::Mode(collection) => eval_mode(collection, def_idx, self_idx, ctx),
		_ => None,
	}
}

fn eval_pair_number_expr(
	expr: &crate::check::expr::NumberExpr,
	a: DomainItem<'_>,
	b: DomainItem<'_>,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> Option<f64> {
	match expr {
		crate::check::expr::NumberExpr::Size(collection) => {
			eval_pair_collection_size(collection, a, b, def_idx, self_idx, ctx)
				.map(|size| size as f64)
		}
		_ => eval_number_expr_def(expr, ctx.graph.def_at(def_idx), def_idx, self_idx, ctx),
	}
}

fn pair_quantifier_outcome(kind: QuantKind, total: u32, passes: u32) -> NodeOutcome {
	let ok = match kind {
		QuantKind::Any => passes > 0,
		QuantKind::All => total == 0 || passes == total,
		QuantKind::None => passes == 0,
	};
	if ok {
		return NodeOutcome::Pass;
	}
	let label = match kind {
		QuantKind::Any => "any",
		QuantKind::All => "all",
		QuantKind::None => "none",
	};
	NodeOutcome::Fail(Failure {
		atom_raw: format!("{label}(pairs(...))"),
		lhs_label: label.to_string(),
		actual: format!("{passes}/{total}"),
		expected: match kind {
			QuantKind::Any => ">= 1 match".to_string(),
			QuantKind::All => "all match".to_string(),
			QuantKind::None => "zero matches".to_string(),
		},
	})
}
