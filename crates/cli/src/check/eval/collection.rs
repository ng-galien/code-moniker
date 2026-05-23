use std::collections::{HashMap, HashSet};

use crate::check::expr::{CollectionExpr, CollectionOp, CollectionProjection, Domain, Lhs};

use super::local::{DomainItem, domain_items};
use super::value::{Value, ValueKey, value_counts};
use super::{EvalCtx, resolve_def_lhs, resolve_ref_lhs};

pub(super) fn eval_collection_size(
	collection: &CollectionExpr,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> usize {
	eval_collection(collection, def_idx, self_idx, ctx).len()
}

pub(super) fn eval_collection_subset(
	left: &CollectionExpr,
	right: &CollectionExpr,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> bool {
	let left = eval_collection(left, def_idx, self_idx, ctx);
	let right = eval_collection(right, def_idx, self_idx, ctx);
	is_subset(&left, &right)
}

fn eval_collection(
	collection: &CollectionExpr,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> Vec<Value> {
	match collection {
		CollectionExpr::Projection(projection) => {
			collect_projection(projection, def_idx, self_idx, ctx)
		}
		CollectionExpr::Unique(inner) => unique(eval_collection(inner, def_idx, self_idx, ctx)),
		CollectionExpr::Binary { op, left, right } => {
			let left = eval_collection(left, def_idx, self_idx, ctx);
			let right = eval_collection(right, def_idx, self_idx, ctx);
			match op {
				CollectionOp::Intersect => intersect(&left, &right),
				CollectionOp::Union => union(&left, &right),
				CollectionOp::Difference => difference(&left, &right),
			}
		}
	}
}

fn collect_projection(
	projection: &CollectionProjection,
	def_idx: usize,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> Vec<Value> {
	let mut values = Vec::new();
	for item in domain_items(&projection.domain, def_idx, ctx) {
		values.extend(project_item_path(item, &projection.path, self_idx, ctx));
	}
	values
}

fn project_item_path(
	item: DomainItem<'_>,
	path: &[String],
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> Vec<Value> {
	match item {
		DomainItem::Def { idx, def } => {
			if let Some((head, tail)) = path.split_first()
				&& let Some(domain) = nested_domain(head)
			{
				let mut values = Vec::new();
				for nested in domain_items(&domain, idx, ctx) {
					values.extend(project_item_path(nested, tail, self_idx, ctx));
				}
				return values;
			}
			project_def_path(def, path, ctx).into_iter().collect()
		}
		DomainItem::Ref { record } => project_ref_path(record, path, ctx).into_iter().collect(),
		DomainItem::Segment { kind, name } => {
			project_segment_path(kind, name, path).into_iter().collect()
		}
	}
}

fn nested_domain(segment: &str) -> Option<Domain> {
	match segment {
		"out_refs" => Some(Domain::OutRefs),
		"in_refs" => Some(Domain::InRefs),
		_ => None,
	}
}

fn project_def_path(
	def: &code_moniker_core::core::code_graph::DefRecord,
	path: &[String],
	ctx: &EvalCtx<'_, '_>,
) -> Option<Value> {
	let lhs = match path {
		[] => Lhs::Moniker,
		[name] if name == "self" => Lhs::Moniker,
		[name] if name == "name" => Lhs::Name,
		[name] if name == "kind" => Lhs::Kind,
		[name] if name == "shape" => Lhs::Shape,
		[name] if name == "visibility" => Lhs::Visibility,
		[name] if name == "lines" => Lhs::Lines,
		[name] if name == "depth" => Lhs::Depth,
		[name] if name == "parent" => Lhs::ParentMoniker,
		[parent, child] if parent == "parent" && child == "name" => Lhs::ParentName,
		[parent, child] if parent == "parent" && child == "kind" => Lhs::ParentKind,
		[parent, child] if parent == "parent" && child == "shape" => Lhs::ParentShape,
		_ => return None,
	};
	resolve_def_lhs(lhs, def, ctx)
}

fn project_ref_path(
	record: &code_moniker_core::core::code_graph::RefRecord,
	path: &[String],
	ctx: &EvalCtx<'_, '_>,
) -> Option<Value> {
	let lhs = match path {
		[] => Lhs::TargetMoniker,
		[name] if name == "kind" => Lhs::Kind,
		[name] if name == "source" => Lhs::SourceMoniker,
		[name] if name == "target" => Lhs::TargetMoniker,
		[source, parent] if source == "source" && parent == "parent" => Lhs::SourceParentMoniker,
		[target, parent] if target == "target" && parent == "parent" => Lhs::TargetParentMoniker,
		[source, child] if source == "source" && child == "name" => Lhs::SourceName,
		[source, child] if source == "source" && child == "kind" => Lhs::SourceKind,
		[source, child] if source == "source" && child == "shape" => Lhs::SourceShape,
		[source, child] if source == "source" && child == "visibility" => Lhs::SourceVisibility,
		[target, child] if target == "target" && child == "name" => Lhs::TargetName,
		[target, child] if target == "target" && child == "kind" => Lhs::TargetKind,
		[target, child] if target == "target" && child == "shape" => Lhs::TargetShape,
		[target, child] if target == "target" && child == "visibility" => Lhs::TargetVisibility,
		_ => return None,
	};
	resolve_ref_lhs(lhs, record, ctx)
}

fn project_segment_path(kind: &[u8], name: &[u8], path: &[String]) -> Option<Value> {
	match path {
		[projection] if projection == "kind" => {
			Some(Value::Str(std::str::from_utf8(kind).ok()?.to_string()))
		}
		[projection] if projection == "name" => {
			Some(Value::Str(std::str::from_utf8(name).ok()?.to_string()))
		}
		_ => None,
	}
}

fn unique(values: Vec<Value>) -> Vec<Value> {
	let mut seen = HashSet::new();
	let mut out = Vec::new();
	for value in values {
		let key = ValueKey::from_value(value.clone());
		if seen.insert(key) {
			out.push(value);
		}
	}
	out
}

fn intersect(left: &[Value], right: &[Value]) -> Vec<Value> {
	combine_counts(left, right, |l, r| l.min(r))
}

fn union(left: &[Value], right: &[Value]) -> Vec<Value> {
	combine_counts(left, right, |l, r| l.max(r))
}

fn difference(left: &[Value], right: &[Value]) -> Vec<Value> {
	combine_counts(left, right, |l, r| l.saturating_sub(r))
}

fn is_subset(left: &[Value], right: &[Value]) -> bool {
	let left = value_counts(left.iter().cloned());
	let right = value_counts(right.iter().cloned());
	left.into_iter()
		.all(|(key, count)| count <= right.get(&key).copied().unwrap_or(0))
}

fn combine_counts(
	left: &[Value],
	right: &[Value],
	merge: impl Fn(usize, usize) -> usize,
) -> Vec<Value> {
	let left_counts = value_counts(left.iter().cloned());
	let right_counts = value_counts(right.iter().cloned());
	let mut keys: HashMap<ValueKey, (usize, usize)> = HashMap::new();
	for (key, count) in left_counts {
		keys.entry(key).or_default().0 = count;
	}
	for (key, count) in right_counts {
		keys.entry(key).or_default().1 = count;
	}
	let mut out = Vec::new();
	for (key, (left_count, right_count)) in keys {
		for _ in 0..merge(left_count, right_count) {
			out.push(key.clone().into_value());
		}
	}
	out
}
