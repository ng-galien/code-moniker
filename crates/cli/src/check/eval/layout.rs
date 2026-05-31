use std::collections::{HashMap, HashSet, VecDeque};

use code_moniker_core::core::code_graph::DefRecord;
use code_moniker_workspace::lines::line_range;

use crate::check::expr::{Domain, VerticalLayout};

use super::{EvalCtx, Failure, NodeOutcome, def_has_shape, def_name, is_call_ref_kind};

pub(super) fn eval_vertical_layout(
	layout: &VerticalLayout,
	_owner: &DefRecord,
	owner_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> NodeOutcome {
	let Some(mut items) = layout_items(&layout.domain, owner_idx, ctx) else {
		return NodeOutcome::NotApplicable;
	};
	if items.len() < 2 {
		return NodeOutcome::Pass;
	}
	items.sort_by_key(|item| (item.start_byte, item.original_order));
	for group in layout_groups(&items) {
		if group.len() < 2 {
			continue;
		}
		let outcome = eval_vertical_layout_group(layout, ctx, &group);
		if !matches!(outcome, NodeOutcome::Pass) {
			return outcome;
		}
	}
	NodeOutcome::Pass
}

fn eval_vertical_layout_group(
	layout: &VerticalLayout,
	ctx: &EvalCtx<'_, '_>,
	items: &[LayoutItem],
) -> NodeOutcome {
	let selected: HashSet<usize> = items.iter().map(|item| item.idx).collect();
	let regions_by_idx: HashMap<usize, LayoutRegion> =
		items.iter().map(|item| (item.idx, item.region)).collect();
	let mut moves = Vec::new();
	if layout.public_first {
		let last_visible = items.iter().rposition(|item| !item.is_private);
		if let Some(last_visible) = last_visible {
			for (position, item) in items.iter().enumerate() {
				if item.is_private && position < last_visible {
					moves.push(LayoutMove {
						target_idx: item.idx,
						caller_idx: None,
						symbol: item.name.clone(),
						first_use: "public surface".to_string(),
						from_line: item.start_line,
						ideal_after_line: items[last_visible].end_line,
						gap: item.start_line.saturating_sub(items[last_visible].end_line),
						reason: "private declaration appears before visible API".to_string(),
					});
				}
			}
		}
	}
	if layout.private_after_first_use {
		for item in items.iter().filter(|item| item.is_private) {
			let Some(first_use) = first_local_use(item, &selected, &regions_by_idx, ctx) else {
				continue;
			};
			let caller = ctx.graph.def_at(first_use.source_idx);
			let caller_name = def_name(caller).unwrap_or_else(|| item.name.clone());
			let caller_end_line = line_range_of(caller, ctx).map(|(_, end)| end).unwrap_or(0);
			let declared_before_use = item.start_line < first_use.line;
			let gap = if declared_before_use {
				first_use.line.saturating_sub(item.start_line)
			} else {
				item.start_line.saturating_sub(caller_end_line)
			};
			if declared_before_use || gap > layout.max_gap {
				moves.push(LayoutMove {
					target_idx: item.idx,
					caller_idx: Some(first_use.source_idx),
					symbol: item.name.clone(),
					first_use: caller_name,
					from_line: item.start_line,
					ideal_after_line: caller_end_line.max(first_use.line),
					gap,
					reason: if declared_before_use {
						"private declaration appears before its first local use".to_string()
					} else {
						format!(
							"private declaration is more than {} lines after its first local use",
							layout.max_gap
						)
					},
				});
			}
		}
	}
	if moves.is_empty() {
		return NodeOutcome::Pass;
	}
	let current = order_text(items.iter().map(|item| item.name.as_str()));
	let ideal_items = suggested_order(&items, &moves);
	let suggested = order_text(ideal_items.iter().map(|item| item.name.as_str()));
	let first_move = &moves[0];
	let details = format!(
		"current: {current}\nsuggested: {suggested}\nmove: helper `{}` closer to `{}` (decl L{}, suggested after L{}, gap {})\nreason: {}",
		first_move.symbol,
		first_move.first_use,
		first_move.from_line,
		first_move.ideal_after_line,
		first_move.gap,
		first_move.reason
	);
	NodeOutcome::Fail(Failure {
		atom_raw: layout.raw.clone(),
		lhs_label: "layout".to_string(),
		actual: current,
		expected: suggested,
		def_idx: None,
		details: Some(details),
	})
}

#[derive(Clone)]
struct LayoutItem {
	idx: usize,
	name: String,
	start_byte: u32,
	start_line: u32,
	end_line: u32,
	is_private: bool,
	original_order: usize,
	region: LayoutRegion,
}

struct FirstUse {
	source_idx: usize,
	line: u32,
	byte: u32,
}

struct LayoutMove {
	target_idx: usize,
	caller_idx: Option<usize>,
	symbol: String,
	first_use: String,
	from_line: u32,
	ideal_after_line: u32,
	gap: u32,
	reason: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum MoveAnchor {
	Caller(usize),
	PublicSurface,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct LayoutRegion(Option<usize>);

fn layout_items(
	domain: &Domain,
	owner_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> Option<Vec<LayoutItem>> {
	let child_idxs = descendant_idxs(owner_idx, ctx)?;
	let mut items = Vec::new();
	for (original_order, idx) in child_idxs.iter().copied().enumerate() {
		let def = ctx.graph.def_at(idx);
		if !domain_matches(domain, def) {
			continue;
		}
		let Some((start_byte, _)) = def.position else {
			continue;
		};
		let Some((start_line, end_line)) = line_range_of(def, ctx) else {
			continue;
		};
		items.push(LayoutItem {
			idx,
			name: def_name(def).unwrap_or_default(),
			start_byte,
			start_line,
			end_line,
			is_private: def.visibility.as_ref() == b"private",
			original_order,
			region: layout_region(ctx, owner_idx, idx),
		});
	}
	Some(items)
}

fn domain_matches(domain: &Domain, def: &DefRecord) -> bool {
	match domain {
		Domain::Children(kind) => def.kind.as_ref() == kind.as_bytes(),
		Domain::ChildrenByShape(shape) => def_has_shape(def, shape),
		Domain::Pairs(_) | Domain::Segments | Domain::OutRefs | Domain::InRefs => false,
	}
}

fn first_local_use(
	item: &LayoutItem,
	selected: &HashSet<usize>,
	regions_by_idx: &HashMap<usize, LayoutRegion>,
	ctx: &EvalCtx<'_, '_>,
) -> Option<FirstUse> {
	let target = ctx.graph.def_at(item.idx).moniker.as_bytes();
	let refs = ctx.in_refs_by_target.get(target)?;
	refs.iter()
		.filter_map(|ref_idx| {
			let record = ctx.graph.ref_at(*ref_idx);
			if !is_call_ref_kind(&record.kind) {
				return None;
			}
			if record.source == item.idx
				|| regions_by_idx.get(&record.source) != Some(&item.region)
				|| !selected.contains(&record.source)
			{
				return None;
			}
			let (start_byte, end_byte) = record.position?;
			let (line, _) = line_range(ctx.source, start_byte, end_byte);
			Some(FirstUse {
				source_idx: record.source,
				line,
				byte: start_byte,
			})
		})
		.min_by_key(|first_use| (first_use.byte, first_use.source_idx))
}

fn line_range_of(def: &DefRecord, ctx: &EvalCtx<'_, '_>) -> Option<(u32, u32)> {
	let (start, end) = def.position?;
	Some(line_range(ctx.source, start, end))
}

fn descendant_idxs(owner_idx: usize, ctx: &EvalCtx<'_, '_>) -> Option<Vec<usize>> {
	let mut descendants = Vec::new();
	let mut todo = VecDeque::new();
	let direct_children = ctx.children_by_parent.get(&owner_idx)?;
	todo.extend(direct_children.iter().copied());
	while let Some(parent_idx) = todo.pop_front() {
		descendants.push(parent_idx);
		if let Some(children) = ctx.children_by_parent.get(&parent_idx) {
			todo.extend(children.iter().copied());
		}
	}
	Some(descendants)
}

fn layout_groups(items: &[LayoutItem]) -> Vec<Vec<LayoutItem>> {
	let mut groups: Vec<(LayoutRegion, Vec<LayoutItem>)> = Vec::new();
	for item in items {
		if let Some((_, group)) = groups.iter_mut().find(|(region, _)| *region == item.region) {
			group.push(item.clone());
		} else {
			groups.push((item.region, vec![item.clone()]));
		}
	}
	groups.into_iter().map(|(_, items)| items).collect()
}

fn layout_region(ctx: &EvalCtx<'_, '_>, owner_idx: usize, item_idx: usize) -> LayoutRegion {
	let mut cursor = Some(item_idx);
	while let Some(idx) = cursor {
		if idx == owner_idx {
			return LayoutRegion(Some(idx));
		}
		let def = ctx.graph.def_at(idx);
		if idx != item_idx && def.opens_scope() {
			return LayoutRegion(Some(idx));
		}
		cursor = def.parent;
	}
	LayoutRegion(None)
}

fn suggested_order<'a>(items: &'a [LayoutItem], moves: &[LayoutMove]) -> Vec<&'a LayoutItem> {
	let mut moved = HashSet::new();
	let mut groups: Vec<(MoveAnchor, Vec<usize>)> = Vec::new();
	for layout_move in moves {
		if !moved.insert(layout_move.target_idx) {
			continue;
		};
		let anchor = layout_move
			.caller_idx
			.map(MoveAnchor::Caller)
			.unwrap_or(MoveAnchor::PublicSurface);
		if let Some((_, target_idxs)) = groups
			.iter_mut()
			.find(|(group_anchor, _)| *group_anchor == anchor)
		{
			target_idxs.push(layout_move.target_idx);
		} else {
			groups.push((anchor, vec![layout_move.target_idx]));
		}
	}
	let item_by_idx: HashMap<usize, &LayoutItem> =
		items.iter().map(|item| (item.idx, item)).collect();
	let mut order: Vec<&LayoutItem> = items
		.iter()
		.filter(|item| !moved.contains(&item.idx))
		.collect();
	for (anchor, target_idxs) in groups {
		let insert_after = match anchor {
			MoveAnchor::Caller(caller_idx) => order.iter().position(|item| item.idx == caller_idx),
			MoveAnchor::PublicSurface => order.iter().rposition(|item| !item.is_private),
		};
		let insert_at = insert_after.map_or(order.len(), |idx| idx + 1);
		let mut insert_at = insert_at.min(order.len());
		for target_idx in target_idxs {
			if let Some(item) = item_by_idx.get(&target_idx) {
				order.insert(insert_at, *item);
				insert_at += 1;
			}
		}
	}
	order
}

fn order_text<'a>(names: impl Iterator<Item = &'a str>) -> String {
	let values: Vec<_> = names.collect();
	if values.is_empty() {
		"<empty>".to_string()
	} else {
		values.join(" -> ")
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn item(idx: usize, name: &str, is_private: bool) -> LayoutItem {
		LayoutItem {
			idx,
			name: name.to_string(),
			start_byte: idx as u32,
			start_line: idx as u32,
			end_line: idx as u32,
			is_private,
			original_order: idx,
			region: LayoutRegion(None),
		}
	}

	fn layout_move(target_idx: usize, caller_idx: Option<usize>) -> LayoutMove {
		LayoutMove {
			target_idx,
			caller_idx,
			symbol: String::new(),
			first_use: String::new(),
			from_line: 0,
			ideal_after_line: 0,
			gap: 0,
			reason: String::new(),
		}
	}

	#[test]
	fn suggested_order_preserves_relative_order_for_public_surface_moves() {
		let items = vec![
			item(1, "a", true),
			item(2, "b", true),
			item(3, "visible", false),
		];
		let moves = vec![layout_move(1, None), layout_move(2, None)];
		let order = suggested_order(&items, &moves);
		assert_eq!(
			order
				.iter()
				.map(|item| item.name.as_str())
				.collect::<Vec<_>>(),
			vec!["visible", "a", "b"]
		);
	}

	#[test]
	fn suggested_order_preserves_relative_order_for_same_caller_moves() {
		let items = vec![
			item(1, "caller", false),
			item(2, "other", false),
			item(3, "a", true),
			item(4, "b", true),
		];
		let moves = vec![layout_move(3, Some(1)), layout_move(4, Some(1))];
		let order = suggested_order(&items, &moves);
		assert_eq!(
			order
				.iter()
				.map(|item| item.name.as_str())
				.collect::<Vec<_>>(),
			vec!["caller", "a", "b", "other"]
		);
	}
}
