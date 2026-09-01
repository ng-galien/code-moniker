use crate::check::expr::{
	AggregateKind, Domain, DomainValueExpr, Lhs, LhsExpr, Node, NumberExpr, Rhs, ValueExpr,
};
use code_moniker_core::core::code_graph::{DefRecord, RefRecord};
use code_moniker_core::core::kinds::KIND_MODULE;
use code_moniker_core::lang::covering_node;
use std::collections::HashSet;
use std::rc::Rc;
use tree_sitter::Node as AstNode;

use super::value::{Value, apply_op, apply_op_values, mode_value, value_counts};
use super::{
	AtomOutcome, EvalCtx, NodeOutcome, def_has_shape, eval_external_def_node, eval_node_segment,
	eval_node_with_self, eval_number_expr_def, eval_number_expr_ref, eval_number_expr_segment,
	eval_ref_node, resolve_def_lhs, resolve_ref_lhs, walk_node,
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
			DomainItem::Ast { .. } => {
				if let NumberExpr::Projection(lhs) = expr
					&& let Some(Value::Number(value)) = project_lhs_value(item, *lhs, ctx)
				{
					values.push(value);
				}
			}
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
	Ast {
		node: AstNode<'a>,
	},
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AstScopeError {
	DocumentUnavailable,
	ParseErrors,
	InjectionsUnsupported,
	MissingSymbolRange,
	NoExactScopeNode,
	TraversalLimitExceeded,
}

impl std::fmt::Display for AstScopeError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let message = match self {
			Self::DocumentUnavailable => "AST document is unavailable",
			Self::ParseErrors => "AST contains parse errors",
			Self::InjectionsUnsupported => "AST injections are not supported",
			Self::MissingSymbolRange => "symbol has no source range for AST scope",
			Self::NoExactScopeNode => "symbol range does not identify an exact AST node",
			Self::TraversalLimitExceeded => "AST scope exceeds the traversal limit",
		};
		f.write_str(message)
	}
}

impl DomainItem<'_> {
	pub(super) fn position(self) -> Option<(u32, u32)> {
		match self {
			Self::Ast { node } => Some((node.start_byte() as u32, node.end_byte() as u32)),
			Self::Def { def, .. } => def.position,
			Self::Ref { record } => record.position,
			Self::Segment { .. } => None,
		}
	}

	pub(super) fn def_idx(self) -> Option<usize> {
		match self {
			Self::Def { idx, .. } => idx,
			_ => None,
		}
	}
}

pub(super) fn domain_items<'a>(
	domain: &Domain,
	def_idx: usize,
	ctx: &'a EvalCtx<'_, '_>,
) -> Vec<DomainItem<'a>> {
	match domain {
		Domain::Ast => Vec::new(),
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

pub(super) fn ast_scope_node<'src>(
	def_idx: usize,
	ctx: &EvalCtx<'_, 'src>,
) -> Result<AstNode<'src>, AstScopeError> {
	let Some(document) = ctx.ast_document.as_ref() else {
		return Err(AstScopeError::DocumentUnavailable);
	};
	let def = ctx.graph.def_at(def_idx);
	let root = document.primary().root_node();
	let scope = if def.kind.as_ref() == KIND_MODULE && def.position.is_none() {
		root
	} else {
		let Some((start, end)) = def.position else {
			return Err(AstScopeError::MissingSymbolRange);
		};
		covering_node(root, &(start as usize..end as usize))
			.ok_or(AstScopeError::NoExactScopeNode)?
	};
	if scope.has_error() {
		return Err(AstScopeError::ParseErrors);
	}
	if document
		.injection_within(scope.start_byte()..scope.end_byte())
		.is_some()
	{
		return Err(AstScopeError::InjectionsUnsupported);
	}
	Ok(scope)
}

pub(super) fn ast_domain_items<'src>(
	def_idx: usize,
	ctx: &EvalCtx<'_, 'src>,
) -> Result<Rc<Vec<DomainItem<'src>>>, AstScopeError> {
	if let Some(cached) = ctx.ast_items_by_def.borrow().get(&def_idx).cloned() {
		return cached;
	}
	let result = ast_scope_node(def_idx, ctx).and_then(named_descendants);
	let retained = match &result {
		Ok(items) => ctx
			.ast_cached_nodes
			.get()
			.checked_add(items.len())
			.filter(|total| *total <= MAX_AST_CACHED_NODES),
		Err(_) => Some(ctx.ast_cached_nodes.get()),
	};
	if let Some(retained) = retained {
		ctx.ast_cached_nodes.set(retained);
		ctx.ast_items_by_def
			.borrow_mut()
			.insert(def_idx, result.clone());
	}
	result
}

const MAX_AST_SCOPE_NODES: usize = 100_000;
const MAX_AST_CACHED_NODES: usize = 100_000;

fn named_descendants<'a>(scope: AstNode<'a>) -> Result<Rc<Vec<DomainItem<'a>>>, AstScopeError> {
	let mut items = Vec::new();
	let mut visited = 0usize;
	let mut cursor = scope.walk();
	if !cursor.goto_first_child() {
		return Ok(Rc::new(items));
	}
	loop {
		visited += 1;
		if visited > MAX_AST_SCOPE_NODES {
			return Err(AstScopeError::TraversalLimitExceeded);
		}
		let node = cursor.node();
		if node.is_named() {
			items.push(DomainItem::Ast { node });
		}
		if cursor.goto_first_child() {
			continue;
		}
		loop {
			if cursor.goto_next_sibling() {
				break;
			}
			if !cursor.goto_parent() || cursor.node() == scope {
				return Ok(Rc::new(items));
			}
		}
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
		DomainItem::Ast { .. } => eval_domain_item_node(item, filter, self_idx, ctx),
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
			DomainItem::Ast { .. } => match expr {
				NumberExpr::Projection(lhs) => {
					project_lhs_value(item, *lhs, ctx).and_then(|value| {
						if let Value::Number(number) = value {
							Some(Value::Number(number))
						} else {
							None
						}
					})
				}
				_ => None,
			},
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
		DomainItem::Ast { .. } => None,
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
		DomainItem::Ast { node } => project_ast_lhs_value(node, lhs, ctx),
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

fn project_ast_lhs_value(node: AstNode<'_>, lhs: Lhs, ctx: &EvalCtx<'_, '_>) -> Option<Value> {
	let value = match lhs {
		Lhs::Kind => Value::Str(node.kind().to_string()),
		Lhs::Text => Value::Str(
			ctx.source
				.get(node.start_byte()..node.end_byte())?
				.to_string(),
		),
		Lhs::StartByte => Value::Number(node.start_byte() as f64),
		Lhs::EndByte => Value::Number(node.end_byte() as f64),
		Lhs::StartLine => Value::Number(node.start_position().row as f64 + 1.0),
		Lhs::EndLine => Value::Number(ast_end_line(node) as f64),
		Lhs::Lines => Value::Number((ast_end_line(node) - node.start_position().row as u32) as f64),
		Lhs::ParentKind => {
			let mut parent = node.parent();
			while parent.is_some_and(|parent| !parent.is_named()) {
				parent = parent.and_then(|parent| parent.parent());
			}
			Value::Str(parent?.kind().to_string())
		}
		_ => return None,
	};
	Some(value)
}

fn ast_end_line(node: AstNode<'_>) -> u32 {
	let end = node.end_position();
	if end.column == 0 && end.row > node.start_position().row {
		end.row as u32
	} else {
		end.row as u32 + 1
	}
}

pub(super) fn eval_domain_item_node(
	item: DomainItem<'_>,
	filter: &Node,
	self_idx: usize,
	ctx: &EvalCtx<'_, '_>,
) -> NodeOutcome {
	match item {
		DomainItem::Ast { node } => eval_ast_node(filter, node, ctx),
		DomainItem::Def {
			idx: Some(idx),
			def,
		} => eval_node_with_self(filter, def, idx, self_idx, ctx),
		DomainItem::Def { idx: None, def } => eval_external_def_node(filter, def, ctx),
		DomainItem::Ref { record } => eval_ref_node(filter, record, ctx),
		DomainItem::Segment { kind, name } => eval_node_segment(filter, kind, name),
	}
}

fn eval_ast_node(filter: &Node, node: AstNode<'_>, ctx: &EvalCtx<'_, '_>) -> NodeOutcome {
	let mut outcome = walk_node(
		filter,
		&|atom| eval_ast_atom(atom, node, ctx),
		&|_, _, _| NodeOutcome::NotApplicable,
		&|_| NodeOutcome::NotApplicable,
		&|_| NodeOutcome::NotApplicable,
	);
	if let NodeOutcome::Fail(failure) = &mut outcome {
		failure.position = Some((node.start_byte() as u32, node.end_byte() as u32));
	}
	outcome
}

fn eval_ast_atom(
	atom: &crate::check::expr::Atom,
	node: AstNode<'_>,
	ctx: &EvalCtx<'_, '_>,
) -> AtomOutcome {
	let item = DomainItem::Ast { node };
	let value = match &atom.lhs {
		LhsExpr::Attr(lhs) => project_lhs_value(item, *lhs, ctx),
		LhsExpr::Number(NumberExpr::Projection(lhs)) => project_lhs_value(item, *lhs, ctx),
		_ => None,
	};
	let Some(value) = value else {
		return AtomOutcome::NotApplicable;
	};
	if let Rhs::Projection(lhs) = &atom.rhs {
		let Some(rhs) = project_lhs_value(item, *lhs, ctx) else {
			return AtomOutcome::NotApplicable;
		};
		return apply_op_values(&value, atom.op, &rhs);
	}
	if let Rhs::Number(number) = &atom.rhs {
		let rhs = match number {
			NumberExpr::Literal(number) => Value::Number(*number),
			NumberExpr::Projection(lhs) => {
				let Some(value) = project_lhs_value(item, *lhs, ctx) else {
					return AtomOutcome::NotApplicable;
				};
				value
			}
			_ => return AtomOutcome::NotApplicable,
		};
		return apply_op_values(&value, atom.op, &rhs);
	}
	apply_op(&value, atom)
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
