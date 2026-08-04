use crate::check::expr::{
	AggregateKind, Domain, DomainValueExpr, Lhs, Node, NumberExpr, ValueExpr,
};
use code_moniker_core::core::code_graph::{DefRecord, RefRecord};
use std::collections::HashSet;

use super::value::{Value, mode_value, value_counts};
use super::{
	EvalCtx, NodeOutcome, def_has_shape, eval_external_def_node, eval_node_segment,
	eval_node_with_self, eval_number_expr_def, eval_number_expr_ref, eval_number_expr_segment,
	eval_ref_node, resolve_def_lhs, resolve_ref_lhs,
};

pub(super) struct AggregateEval<'a> {
	pub(super) kind: AggregateKind,
	pub(super) domain: &'a Domain,
	pub(super) expr: &'a NumberExpr,
	pub(super) percentile: Option<f64>,
	pub(super) def_idx: usize,
	pub(super) self_idx: usize,
}

pub(super) fn eval_aggregate(input: AggregateEval<'_>, ctx: &EvalCtx<'_, '_>) -> Option<f64> {
	let values =
		collect_domain_numbers(input.domain, input.expr, input.def_idx, input.self_idx, ctx);
	super::stats::aggregate(input.kind, values, input.percentile)
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
			DomainItem::Def {
				idx: Some(idx),
				def,
			} => {
				if let Some(value) = eval_number_expr_def(expr, def, idx, self_idx, ctx) {
					values.push(value);
				}
			}
			DomainItem::Def { idx: None, .. } => {}
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
	Def {
		idx: Option<usize>,
		def: &'a DefRecord,
	},
	Ref {
		record: &'a RefRecord,
	},
	Segment {
		kind: &'a [u8],
		name: &'a [u8],
	},
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
				(def.kind.as_ref() == kind.as_bytes()).then_some(DomainItem::Def {
					idx: Some(*idx),
					def,
				})
			})
			.collect(),
		Domain::ChildrenByShape(shape) => ctx
			.children_by_parent
			.get(&def_idx)
			.into_iter()
			.flatten()
			.filter_map(|idx| {
				let def = ctx.graph.def_at(*idx);
				def_has_shape(def, shape).then_some(DomainItem::Def {
					idx: Some(*idx),
					def,
				})
			})
			.collect(),
		Domain::Descendants(inner) => descendant_items(inner, def_idx, ctx),
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
		Domain::OutRefs | Domain::SourceOutRefs => ctx
			.out_refs_by_source
			.get(&def_idx)
			.into_iter()
			.flatten()
			.map(|idx| DomainItem::Ref {
				record: ctx.graph.ref_at(*idx),
			})
			.collect(),
		Domain::InRefs | Domain::SourceInRefs => {
			let key = ctx.graph.def_at(def_idx).moniker.as_encoded();
			ctx.in_refs_by_target
				.get(key)
				.into_iter()
				.flatten()
				.map(|idx| DomainItem::Ref {
					record: ctx.graph.ref_at(*idx),
				})
				.collect()
		}
		Domain::TargetOutRefs | Domain::TargetInRefs => Vec::new(),
		Domain::SourceAncestorOutRefs => ancestor_ref_items(def_idx, ctx, true),
		Domain::SourceAncestorInRefs => ancestor_ref_items(def_idx, ctx, false),
	}
}

fn ancestor_ref_items<'a>(
	def_idx: usize,
	ctx: &'a EvalCtx<'_, '_>,
	outgoing: bool,
) -> Vec<DomainItem<'a>> {
	let mut items = Vec::new();
	let mut parent = ctx.graph.def_at(def_idx).parent;
	while let Some(idx) = parent {
		if outgoing {
			if let Some(refs) = ctx.out_refs_by_source.get(&idx) {
				items.extend(refs.iter().map(|ref_idx| DomainItem::Ref {
					record: ctx.graph.ref_at(*ref_idx),
				}));
			}
		} else {
			let key = ctx.graph.def_at(idx).moniker.as_encoded();
			if let Some(refs) = ctx.in_refs_by_target.get(key) {
				items.extend(refs.iter().map(|ref_idx| DomainItem::Ref {
					record: ctx.graph.ref_at(*ref_idx),
				}));
			}
		}
		parent = ctx.graph.def_at(idx).parent;
	}
	items
}

fn descendant_items<'a>(
	inner: &Domain,
	def_idx: usize,
	ctx: &'a EvalCtx<'_, '_>,
) -> Vec<DomainItem<'a>> {
	let root = &ctx.graph.def_at(def_idx).moniker;
	let mut seen = HashSet::new();
	let mut items: Vec<_> = ctx
		.graph
		.defs()
		.enumerate()
		.filter(|(idx, def)| {
			*idx != def_idx && root.is_ancestor_of(&def.moniker) && def_matches_domain(inner, def)
		})
		.map(|(idx, def)| {
			seen.insert(def.moniker.as_encoded().to_vec());
			DomainItem::Def {
				idx: Some(idx),
				def,
			}
		})
		.collect();
	if let Some(requirements) = ctx.requirements {
		let owner = ctx.graph.def_at(def_idx);
		for def in requirements.descendant_defs(owner, inner) {
			if seen.insert(def.moniker.as_encoded().to_vec()) && def_matches_domain(inner, def) {
				items.push(DomainItem::Def { idx: None, def });
			}
		}
	}
	items
}

fn def_matches_domain(domain: &Domain, def: &DefRecord) -> bool {
	match domain {
		Domain::Children(kind) => def.kind.as_ref() == kind.as_bytes(),
		Domain::ChildrenByShape(shape) => def_has_shape(def, shape),
		Domain::Descendants(inner) => def_matches_domain(inner, def),
		_ => false,
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
		if collection
			.filter
			.as_deref()
			.is_some_and(|filter| !domain_item_matches(item, filter, self_idx, ctx))
		{
			continue;
		}
		if let Some(value) = eval_domain_value_item(item, &collection.expr, self_idx, ctx) {
			values.push(value);
		}
	}
	values
}

fn domain_item_matches(
	item: DomainItem<'_>,
	filter: &Node,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> bool {
	let outcome = match item {
		DomainItem::Def {
			idx: Some(idx),
			def,
		} => eval_node_with_self(filter, def, idx, self_idx, ctx),
		DomainItem::Def { idx: None, def } => eval_external_def_node(filter, def, ctx),
		DomainItem::Ref { record } => eval_ref_node(filter, record, ctx),
		DomainItem::Segment { kind, name } => eval_node_segment(filter, kind, name),
	};
	matches!(outcome, NodeOutcome::Pass)
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
			DomainItem::Def {
				idx: Some(idx),
				def,
			} => eval_number_expr_def(expr, def, idx, self_idx, ctx).map(Value::Number),
			DomainItem::Def { idx: None, .. } => None,
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
		DomainItem::Def { idx, def } => project_def_lhs_value(idx, def, lhs, ctx),
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

pub(super) fn project_def_lhs_value(
	idx: Option<usize>,
	def: &DefRecord,
	lhs: Lhs,
	ctx: &EvalCtx<'_, '_>,
) -> Option<Value> {
	if idx.is_none() && source_dependent_lhs(lhs) {
		return None;
	}
	resolve_def_lhs(lhs, def, ctx)
}

fn source_dependent_lhs(lhs: Lhs) -> bool {
	matches!(
		lhs,
		Lhs::Lines | Lhs::StartLine | Lhs::EndLine | Lhs::StartByte | Lhs::EndByte | Lhs::Text
	)
}
