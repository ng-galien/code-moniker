use std::collections::{HashMap, HashSet};

use crate::check::expr::{Binding, MetricKind};
use code_moniker_core::core::code_graph::DefRecord;
use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::core::shape::{Shape, shape_of};

use super::EvalCtx;

pub(super) fn resolve_binding_idx(binding: Binding, def_idx: usize, self_idx: usize) -> usize {
	match binding {
		Binding::Self_ => self_idx,
		Binding::Each => def_idx,
	}
}

pub(super) fn eval_metric(
	kind: MetricKind,
	binding: Binding,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> Option<f64> {
	let target_idx = resolve_binding_idx(binding, def_idx, self_idx);
	let value = match kind {
		MetricKind::Lcom4 => lcom4(target_idx, ctx),
		MetricKind::Cbo => cbo(target_idx, ctx),
		MetricKind::Rfc => rfc(target_idx, ctx),
		MetricKind::Wmc => wmc(target_idx, ctx),
		MetricKind::Dit => dit(target_idx, ctx),
		MetricKind::Noc => noc(target_idx, ctx),
		MetricKind::FanIn => fan_in(target_idx, ctx),
		MetricKind::FanOut => fan_out(target_idx, ctx),
	};
	Some(value as f64)
}

fn fan_out(def_idx: usize, ctx: &EvalCtx<'_, '_>) -> u32 {
	ctx.out_refs_by_source
		.get(&def_idx)
		.map_or(0, |refs| refs.len() as u32)
}

fn fan_in(def_idx: usize, ctx: &EvalCtx<'_, '_>) -> u32 {
	let key = ctx.graph.def_at(def_idx).moniker.as_bytes();
	ctx.in_refs_by_target
		.get(key)
		.map_or(0, |refs| refs.len() as u32)
}

fn wmc(def_idx: usize, ctx: &EvalCtx<'_, '_>) -> u32 {
	direct_callable_idxs(def_idx, ctx).len() as u32
}

fn rfc(def_idx: usize, ctx: &EvalCtx<'_, '_>) -> u32 {
	let mut responses: HashSet<Vec<u8>> = HashSet::new();
	for method_idx in direct_callable_idxs(def_idx, ctx) {
		let method = ctx.graph.def_at(method_idx);
		responses.insert(method.moniker.as_bytes().to_vec());
		for ref_idx in ctx
			.out_refs_by_source
			.get(&method_idx)
			.into_iter()
			.flatten()
		{
			let record = ctx.graph.ref_at(*ref_idx);
			if !is_call_kind(&record.kind) {
				continue;
			}
			responses.insert(record.target.as_bytes().to_vec());
		}
	}
	responses.len() as u32
}

fn cbo(def_idx: usize, ctx: &EvalCtx<'_, '_>) -> u32 {
	let owner = &ctx.graph.def_at(def_idx).moniker;
	let scope = descendant_idxs(owner, ctx);
	let scope_set: HashSet<usize> = scope.iter().copied().collect();
	let mut coupled: HashSet<Vec<u8>> = HashSet::new();

	for source_idx in &scope {
		for ref_idx in ctx.out_refs_by_source.get(source_idx).into_iter().flatten() {
			let record = ctx.graph.ref_at(*ref_idx);
			add_external_coupling(owner, &record.target, ctx, &mut coupled);
		}
	}

	for target_idx in &scope {
		let target = ctx.graph.def_at(*target_idx);
		for ref_idx in ctx
			.in_refs_by_target
			.get(target.moniker.as_bytes())
			.into_iter()
			.flatten()
		{
			let record = ctx.graph.ref_at(*ref_idx);
			if !scope_set.contains(&record.source) {
				let source = &ctx.graph.def_at(record.source).moniker;
				add_external_coupling(owner, source, ctx, &mut coupled);
			}
		}
	}

	coupled.len() as u32
}

fn lcom4(def_idx: usize, ctx: &EvalCtx<'_, '_>) -> u32 {
	let methods = direct_callable_idxs(def_idx, ctx);
	if methods.is_empty() {
		return 0;
	}

	let method_pos: HashMap<Vec<u8>, usize> = methods
		.iter()
		.enumerate()
		.map(|(pos, idx)| (ctx.graph.def_at(*idx).moniker.as_bytes().to_vec(), pos))
		.collect();
	let fields: HashSet<Vec<u8>> = direct_value_idxs(def_idx, ctx)
		.into_iter()
		.map(|idx| ctx.graph.def_at(idx).moniker.as_bytes().to_vec())
		.collect();
	let mut graph: Vec<HashSet<usize>> = (0..methods.len()).map(|_| HashSet::new()).collect();
	let mut field_users: HashMap<Vec<u8>, Vec<usize>> = HashMap::new();

	for (pos, method_idx) in methods.iter().copied().enumerate() {
		for ref_idx in ctx
			.out_refs_by_source
			.get(&method_idx)
			.into_iter()
			.flatten()
		{
			let target = ctx.graph.ref_at(*ref_idx).target.as_bytes();
			if let Some(other_pos) = method_pos.get(target).copied() {
				connect(&mut graph, pos, other_pos);
			}
			if fields.contains(target) {
				field_users.entry(target.to_vec()).or_default().push(pos);
			}
		}
	}

	for users in field_users.values() {
		if let Some((&first, rest)) = users.split_first() {
			for other in rest {
				connect(&mut graph, first, *other);
			}
		}
	}

	connected_components(&graph) as u32
}

fn dit(def_idx: usize, ctx: &EvalCtx<'_, '_>) -> u32 {
	let mut visiting = HashSet::new();
	dit_from(def_idx, ctx, &mut visiting)
}

fn dit_from(def_idx: usize, ctx: &EvalCtx<'_, '_>, visiting: &mut HashSet<usize>) -> u32 {
	if !visiting.insert(def_idx) {
		return 0;
	}
	let mut max_depth = 0;
	for ref_idx in ctx.out_refs_by_source.get(&def_idx).into_iter().flatten() {
		let record = ctx.graph.ref_at(*ref_idx);
		if !is_inheritance_kind(&record.kind) {
			continue;
		}
		let depth = match find_def_idx(&record.target, ctx) {
			Some(parent_idx) => 1 + dit_from(parent_idx, ctx, visiting),
			None => 1,
		};
		max_depth = max_depth.max(depth);
	}
	visiting.remove(&def_idx);
	max_depth
}

fn noc(def_idx: usize, ctx: &EvalCtx<'_, '_>) -> u32 {
	let key = ctx.graph.def_at(def_idx).moniker.as_bytes();
	let mut children = HashSet::new();
	for ref_idx in ctx.in_refs_by_target.get(key).into_iter().flatten() {
		let record = ctx.graph.ref_at(*ref_idx);
		if is_inheritance_kind(&record.kind) {
			children.insert(record.source);
		}
	}
	children.len() as u32
}

fn direct_callable_idxs(def_idx: usize, ctx: &EvalCtx<'_, '_>) -> Vec<usize> {
	direct_child_idxs(def_idx, ctx)
		.into_iter()
		.filter(|idx| has_shape(ctx.graph.def_at(*idx), Shape::Callable))
		.collect()
}

fn direct_value_idxs(def_idx: usize, ctx: &EvalCtx<'_, '_>) -> Vec<usize> {
	direct_child_idxs(def_idx, ctx)
		.into_iter()
		.filter(|idx| has_shape(ctx.graph.def_at(*idx), Shape::Value))
		.collect()
}

fn direct_child_idxs(def_idx: usize, ctx: &EvalCtx<'_, '_>) -> Vec<usize> {
	ctx.children_by_parent
		.get(&def_idx)
		.cloned()
		.unwrap_or_default()
}

fn descendant_idxs(root: &Moniker, ctx: &EvalCtx<'_, '_>) -> Vec<usize> {
	ctx.graph
		.defs()
		.enumerate()
		.filter_map(|(idx, def)| root.is_ancestor_of(&def.moniker).then_some(idx))
		.collect()
}

fn add_external_coupling(
	owner: &Moniker,
	target: &Moniker,
	ctx: &EvalCtx<'_, '_>,
	coupled: &mut HashSet<Vec<u8>>,
) {
	if owner.is_ancestor_of(target) {
		return;
	}
	let bucket = coupling_bucket(target, ctx);
	if !owner.is_ancestor_of(&bucket) {
		coupled.insert(bucket.as_bytes().to_vec());
	}
}

fn coupling_bucket(target: &Moniker, ctx: &EvalCtx<'_, '_>) -> Moniker {
	if let Some(idx) = find_def_idx(target, ctx) {
		let def = ctx.graph.def_at(idx);
		if is_namespace_or_type(def) {
			return def.moniker.clone();
		}
		if let Some(owner) = nearest_namespace_or_type(def, ctx) {
			return owner;
		}
	}
	if let Some(kind) = target.last_kind()
		&& matches!(shape_of(&kind), Some(Shape::Namespace | Shape::Type))
	{
		return target.clone();
	}
	target.parent().unwrap_or_else(|| target.clone())
}

fn nearest_namespace_or_type(def: &DefRecord, ctx: &EvalCtx<'_, '_>) -> Option<Moniker> {
	let mut cursor = def.moniker.parent();
	while let Some(moniker) = cursor {
		if let Some(idx) = find_def_idx(&moniker, ctx) {
			let parent = ctx.graph.def_at(idx);
			if is_namespace_or_type(parent) {
				return Some(parent.moniker.clone());
			}
		}
		cursor = moniker.parent();
	}
	None
}

fn has_shape(def: &DefRecord, shape: Shape) -> bool {
	def.shape().is_some_and(|actual| actual == shape)
}

fn is_namespace_or_type(def: &DefRecord) -> bool {
	def.shape()
		.is_some_and(|shape| matches!(shape, Shape::Namespace | Shape::Type))
}

fn is_inheritance_kind(kind: &[u8]) -> bool {
	matches!(
		kind,
		b"extends" | b"inherits" | b"inheritance" | b"subclasses"
	)
}

fn is_call_kind(kind: &[u8]) -> bool {
	matches!(kind, b"calls" | b"method_call")
}

fn find_def_idx(target: &Moniker, ctx: &EvalCtx<'_, '_>) -> Option<usize> {
	ctx.graph
		.defs()
		.enumerate()
		.find_map(|(idx, def)| (def.moniker == *target).then_some(idx))
}

fn connect(graph: &mut [HashSet<usize>], a: usize, b: usize) {
	if a == b {
		return;
	}
	graph[a].insert(b);
	graph[b].insert(a);
}

fn connected_components(graph: &[HashSet<usize>]) -> usize {
	let mut seen = vec![false; graph.len()];
	let mut components = 0;
	for start in 0..graph.len() {
		if seen[start] {
			continue;
		}
		components += 1;
		let mut stack = vec![start];
		seen[start] = true;
		while let Some(idx) = stack.pop() {
			for next in &graph[idx] {
				if !seen[*next] {
					seen[*next] = true;
					stack.push(*next);
				}
			}
		}
	}
	components
}
