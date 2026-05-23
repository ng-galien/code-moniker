use crate::check::expr::{AggregateKind, Domain, DomainValueExpr, Lhs, NumberExpr, ValueExpr};
use code_moniker_core::core::code_graph::{DefRecord, RefRecord};

use super::value::{Value, mode_value, value_counts};
use super::{
	EvalCtx, def_has_shape, eval_number_expr_def, eval_number_expr_ref, eval_number_expr_segment,
	resolve_def_lhs, resolve_ref_lhs,
};

pub(super) fn eval_aggregate(
	kind: AggregateKind,
	domain: &Domain,
	expr: &NumberExpr,
	percentile: Option<f64>,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> Option<f64> {
	let values = collect_domain_numbers(domain, expr, def_idx, self_idx, ctx);
	match kind {
		AggregateKind::Sum => Some(values.iter().sum()),
		AggregateKind::Max => values.into_iter().reduce(f64::max),
		AggregateKind::Min => values.into_iter().reduce(f64::min),
		AggregateKind::Avg => average(&values),
		AggregateKind::Median => percentile_value(values, 50.0),
		AggregateKind::Percentile => percentile_value(values, percentile?),
		AggregateKind::Stddev => variance(&values).map(f64::sqrt),
		AggregateKind::Var => variance(&values),
		AggregateKind::Cv => {
			let mean = average(&values)?;
			if mean == 0.0 {
				return None;
			}
			Some(variance(&values)?.sqrt() / mean.abs())
		}
		AggregateKind::Gini => gini(&values),
	}
}

fn collect_domain_numbers(
	domain: &Domain,
	expr: &NumberExpr,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> Vec<f64> {
	let mut values = Vec::new();
	for item in domain_items(domain, def_idx, ctx) {
		match item {
			DomainItem::Def { idx, def } => {
				if let Some(value) = eval_number_expr_def(expr, def, idx, self_idx, ctx) {
					values.push(value);
				}
			}
			DomainItem::Ref { record } => {
				if let Some(value) = eval_number_expr_ref(expr, record, ctx) {
					values.push(value);
				}
			}
			DomainItem::Segment { .. } => {
				if let Some(value) = eval_number_expr_segment(expr) {
					values.push(value);
				}
			}
		}
	}
	values
}

fn average(values: &[f64]) -> Option<f64> {
	(!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn variance(values: &[f64]) -> Option<f64> {
	let mean = average(values)?;
	Some(values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64)
}

fn percentile_value(mut values: Vec<f64>, percentile: f64) -> Option<f64> {
	if values.is_empty() || !(0.0..=100.0).contains(&percentile) {
		return None;
	}
	values.sort_by(|a, b| a.total_cmp(b));
	let rank = (percentile / 100.0) * (values.len().saturating_sub(1)) as f64;
	let lo = rank.floor() as usize;
	let hi = rank.ceil() as usize;
	if lo == hi {
		return values.get(lo).copied();
	}
	let weight = rank - lo as f64;
	Some(values[lo] + (values[hi] - values[lo]) * weight)
}

fn gini(values: &[f64]) -> Option<f64> {
	if values.is_empty() {
		return None;
	}
	let mut sorted: Vec<f64> = values.iter().copied().filter(|v| *v >= 0.0).collect();
	if sorted.len() != values.len() {
		return None;
	}
	sorted.sort_by(|a, b| a.total_cmp(b));
	let sum: f64 = sorted.iter().sum();
	if sum == 0.0 {
		return Some(0.0);
	}
	let weighted: f64 = sorted
		.iter()
		.enumerate()
		.map(|(idx, value)| (idx as f64 + 1.0) * value)
		.sum();
	Some(
		(2.0 * weighted) / (sorted.len() as f64 * sum)
			- (sorted.len() as f64 + 1.0) / sorted.len() as f64,
	)
}

pub(super) fn eval_entropy(
	collection: &DomainValueExpr,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> Option<f64> {
	let values = collect_domain_values(collection, def_idx, self_idx, ctx);
	normalized_entropy(&values)
}

pub(super) fn eval_mode(
	collection: &DomainValueExpr,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> Option<Value> {
	let values = collect_domain_values(collection, def_idx, self_idx, ctx);
	mode_value(values)
}

fn normalized_entropy(values: &[Value]) -> Option<f64> {
	if values.is_empty() {
		return None;
	}
	let counts = value_counts(values.iter().cloned());
	if counts.len() <= 1 {
		return Some(0.0);
	}
	let total = values.len() as f64;
	let entropy = counts.values().fold(0.0, |acc, count| {
		let p = *count as f64 / total;
		acc - p * p.log2()
	});
	Some(entropy / (counts.len() as f64).log2())
}

#[derive(Clone, Copy)]
pub(super) enum DomainItem<'a> {
	Def { idx: usize, def: &'a DefRecord },
	Ref { record: &'a RefRecord },
	Segment { kind: &'a [u8], name: &'a [u8] },
}

pub(super) fn domain_items<'a>(
	domain: &Domain,
	def_idx: usize,
	ctx: &'a EvalCtx<'_, '_>,
) -> Vec<DomainItem<'a>> {
	match domain {
		Domain::Children(kind) => ctx
			.children_by_parent
			.get(&def_idx)
			.into_iter()
			.flatten()
			.filter_map(|idx| {
				let def = ctx.graph.def_at(*idx);
				(def.kind.as_slice() == kind.as_bytes())
					.then_some(DomainItem::Def { idx: *idx, def })
			})
			.collect(),
		Domain::ChildrenByShape(shape) => ctx
			.children_by_parent
			.get(&def_idx)
			.into_iter()
			.flatten()
			.filter_map(|idx| {
				let def = ctx.graph.def_at(*idx);
				def_has_shape(def, shape).then_some(DomainItem::Def { idx: *idx, def })
			})
			.collect(),
		Domain::Pairs(_) => Vec::new(),
		Domain::Segments => ctx
			.graph
			.def_at(def_idx)
			.moniker
			.as_view()
			.segments()
			.map(|seg| DomainItem::Segment {
				kind: seg.kind,
				name: seg.name,
			})
			.collect(),
		Domain::OutRefs => ctx
			.out_refs_by_source
			.get(&def_idx)
			.into_iter()
			.flatten()
			.map(|idx| DomainItem::Ref {
				record: ctx.graph.ref_at(*idx),
			})
			.collect(),
		Domain::InRefs => {
			let key = ctx.graph.def_at(def_idx).moniker.as_bytes();
			ctx.in_refs_by_target
				.get(key)
				.into_iter()
				.flatten()
				.map(|idx| DomainItem::Ref {
					record: ctx.graph.ref_at(*idx),
				})
				.collect()
		}
	}
}

fn collect_domain_values(
	collection: &DomainValueExpr,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> Vec<Value> {
	let mut values = Vec::new();
	for item in domain_items(&collection.domain, def_idx, ctx) {
		if let Some(value) = eval_domain_value_item(item, &collection.expr, self_idx, ctx) {
			values.push(value);
		}
	}
	values
}

fn eval_domain_value_item(
	item: DomainItem<'_>,
	expr: &ValueExpr,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> Option<Value> {
	match expr {
		ValueExpr::Item => project_item_value(item, ctx),
		ValueExpr::Projection(lhs) => project_lhs_value(item, *lhs, ctx),
		ValueExpr::Number(expr) => match item {
			DomainItem::Def { idx, def } => {
				eval_number_expr_def(expr, def, idx, self_idx, ctx).map(Value::Number)
			}
			DomainItem::Ref { record } => {
				eval_number_expr_ref(expr, record, ctx).map(Value::Number)
			}
			DomainItem::Segment { .. } => eval_number_expr_segment(expr).map(Value::Number),
		},
	}
}

fn project_item_value(item: DomainItem<'_>, ctx: &EvalCtx<'_, '_>) -> Option<Value> {
	match item {
		DomainItem::Def { def, .. } => resolve_def_lhs(Lhs::Moniker, def, ctx),
		DomainItem::Ref { record } => resolve_ref_lhs(Lhs::TargetMoniker, record, ctx),
		DomainItem::Segment { .. } => None,
	}
}

pub(super) fn project_lhs_value(
	item: DomainItem<'_>,
	lhs: Lhs,
	ctx: &EvalCtx<'_, '_>,
) -> Option<Value> {
	match item {
		DomainItem::Def { def, .. } => resolve_def_lhs(lhs, def, ctx),
		DomainItem::Ref { record } => resolve_ref_lhs(lhs, record, ctx),
		DomainItem::Segment { kind, name } => match lhs {
			Lhs::Kind | Lhs::SegmentKind => {
				Some(Value::Str(std::str::from_utf8(kind).ok()?.to_string()))
			}
			Lhs::Name | Lhs::SegmentName => {
				Some(Value::Str(std::str::from_utf8(name).ok()?.to_string()))
			}
			_ => None,
		},
	}
}
