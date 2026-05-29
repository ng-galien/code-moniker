use tree_sitter::Node;

use crate::core::code_graph::Position;
use crate::core::moniker::{Moniker, MonikerBuilder};
use crate::lang::callable::extend_segment;
use crate::lang::kinds::{HINT_CALL, HINT_MEMBER, HINT_SELF};
use crate::lang::sdk::{
	DiscoveredDef, ImportLeaf, ImportLeafKind, RefHints, ResolvedRef, TypeEnv, TypeExpr,
	flatten_import_tree, import_leaf_binding_name, importable_parent,
};
use crate::lang::tree_util::{node_position, node_slice};

use super::super::kinds;
use super::imports::import_tree;
use super::syntax::{named_children, path_pieces};

#[derive(Clone, Copy)]
pub(super) struct RefEnv<'a> {
	pub source: &'a [u8],
	pub defs: &'a [DiscoveredDef],
	pub imported_symbols: &'a [ImportedSymbol],
	pub wildcard_imports: &'a [(Moniker, Moniker)],
}

#[derive(Clone, Debug)]
pub(super) struct ImportedSymbol {
	pub scope: Moniker,
	pub name: Vec<u8>,
	pub target: Moniker,
	pub confidence: &'static [u8],
}

#[derive(Default)]
pub(super) struct ImportExpansion {
	pub refs: Vec<ResolvedRef>,
	pub symbols: Vec<ImportedSymbol>,
	pub wildcard_modules: Vec<Moniker>,
}

pub(super) fn macro_call_ref(
	env: RefEnv<'_>,
	scope: &Moniker,
	node: Node<'_>,
) -> Option<ResolvedRef> {
	let macro_node = node.child_by_field_name("macro")?;
	let pieces = path_pieces(macro_node, env.source);
	let name = pieces.last()?;
	let (target, confidence) = resolve_macro_target(&env, scope, &pieces, name);
	Some(call_ref(
		scope,
		target,
		kinds::CALLS,
		confidence,
		node,
		None,
	))
}

pub(super) fn type_refs_from_signature(
	env: RefEnv<'_>,
	node: Node<'_>,
	source: &Moniker,
) -> Vec<ResolvedRef> {
	let mut refs = Vec::new();
	let type_params = type_parameters(node, env.source);
	if let Some(params) = node.child_by_field_name("parameters") {
		for param in named_children(params).filter(|child| child.kind() == "parameter") {
			if let Some(ty) = param.child_by_field_name("type") {
				type_refs_from_node(&env, ty, source, &type_params, &mut refs);
			}
		}
	}
	if let Some(return_type) = node.child_by_field_name("return_type") {
		type_refs_from_node(&env, return_type, source, &type_params, &mut refs);
	}
	refs
}

pub(super) fn type_refs_from_type_node(
	env: RefEnv<'_>,
	node: Node<'_>,
	source: &Moniker,
	type_params: &[Vec<u8>],
) -> Vec<ResolvedRef> {
	let mut refs = Vec::new();
	type_refs_from_node(&env, node, source, type_params, &mut refs);
	refs
}

pub(super) fn trait_refs_from_node(
	env: RefEnv<'_>,
	node: Node<'_>,
	source: &Moniker,
	kind: &'static [u8],
) -> Vec<ResolvedRef> {
	let mut refs = Vec::new();
	trait_refs_from_node_into(&env, node, source, kind, &mut refs);
	refs
}

pub(super) fn expand_import(env: RefEnv<'_>, node: Node<'_>, scope: &Moniker) -> ImportExpansion {
	let Some(tree) = import_tree(node, env.source) else {
		return ImportExpansion::default();
	};
	let mut expansion = ImportExpansion::default();
	for leaf in flatten_import_tree(&tree) {
		expand_import_leaf(&env, scope, node, leaf, &mut expansion);
	}
	expansion
}

pub(super) fn read_refs(
	env: RefEnv<'_>,
	function_node: Node<'_>,
	body: Node<'_>,
	function: &Moniker,
) -> Vec<ResolvedRef> {
	let mut refs = Vec::new();
	let local_types = local_type_bindings(&env, function_node, body, function);
	collect_expr_refs(&env, &local_types, body, function, &mut refs);
	refs
}

pub(super) fn attribute_refs(env: RefEnv<'_>, node: Node<'_>, scope: &Moniker) -> Vec<ResolvedRef> {
	let mut refs = Vec::new();
	for attribute in named_children(node).filter(|child| child.kind() == "attribute") {
		refs.extend(attribute_ref_items(&env, attribute, scope));
	}
	refs
}

fn attribute_ref_items(env: &RefEnv<'_>, attribute: Node<'_>, scope: &Moniker) -> Vec<ResolvedRef> {
	let pieces = path_pieces(attribute, env.source);
	let Some((name, rest)) = pieces.split_first() else {
		return Vec::new();
	};
	if name == b"derive" {
		return derive_refs(env, scope, rest, attribute);
	}
	let (target, confidence) = resolve_attribute_target(env, scope, name);
	vec![import_ref(
		scope,
		target,
		kinds::ANNOTATES,
		confidence,
		attribute,
	)]
}

fn derive_refs(
	env: &RefEnv<'_>,
	scope: &Moniker,
	pieces: &[Vec<u8>],
	attribute: Node<'_>,
) -> Vec<ResolvedRef> {
	let mut refs = Vec::new();
	let mut index = 0;
	while index < pieces.len() {
		let name = &pieces[index];
		if is_external_import_root(name)
			&& let Some(item) = pieces.get(index + 1)
		{
			let target = external_target(scope, &[name, item]);
			refs.push(import_ref(
				scope,
				target,
				kinds::ANNOTATES,
				kinds::CONF_EXTERNAL,
				attribute,
			));
			index += 2;
			continue;
		}
		let (target, confidence) = resolve_derive_target(env, scope, name);
		refs.push(import_ref(
			scope,
			target,
			kinds::ANNOTATES,
			confidence,
			attribute,
		));
		index += 1;
	}
	refs
}

fn type_refs_from_node(
	env: &RefEnv<'_>,
	node: Node<'_>,
	source: &Moniker,
	type_params: &[Vec<u8>],
	out: &mut Vec<ResolvedRef>,
) {
	match node.kind() {
		"type_identifier" => {
			let name = node_slice(node, env.source);
			if should_skip_type_ref(name, type_params) {
				return;
			}
			let (target, confidence) = resolve_type_name(env, source, name);
			out.push(type_ref(source, target, node_position(node), confidence));
		}
		"scoped_type_identifier" => {
			let pieces = path_pieces(node, env.source);
			let Some(name) = pieces.last() else {
				return;
			};
			if should_skip_type_ref(name, type_params) {
				return;
			}
			let (target, confidence) = resolve_type_path(env, source, &pieces);
			out.push(type_ref(source, target, node_position(node), confidence));
		}
		"type_binding" => {
			if let Some(bound_type) = node.child_by_field_name("type") {
				type_refs_from_node(env, bound_type, source, type_params, out);
			}
		}
		_ => {
			for child in named_children(node) {
				type_refs_from_node(env, child, source, type_params, out);
			}
		}
	}
}

fn trait_refs_from_node_into(
	env: &RefEnv<'_>,
	node: Node<'_>,
	source: &Moniker,
	kind: &'static [u8],
	out: &mut Vec<ResolvedRef>,
) {
	match node.kind() {
		"type_identifier" => {
			let name = node_slice(node, env.source);
			if should_skip_type_ref(name, &[]) {
				return;
			}
			let (target, confidence) = resolve_type_name(env, source, name);
			out.push(typed_ref(
				source,
				target,
				node_position(node),
				confidence,
				kind,
			));
		}
		"scoped_type_identifier" => {
			let pieces = path_pieces(node, env.source);
			let Some(name) = pieces.last() else {
				return;
			};
			if should_skip_type_ref(name, &[]) {
				return;
			}
			let (target, confidence) = resolve_type_path(env, source, &pieces);
			out.push(typed_ref(
				source,
				target,
				node_position(node),
				confidence,
				kind,
			));
		}
		_ => {
			for child in named_children(node) {
				trait_refs_from_node_into(env, child, source, kind, out);
			}
		}
	}
}

pub(super) fn type_parameters(node: Node<'_>, source: &[u8]) -> Vec<Vec<u8>> {
	let Some(params) = node.child_by_field_name("type_parameters") else {
		return Vec::new();
	};
	named_children(params)
		.filter(|child| child.kind() == "type_parameter")
		.filter_map(|child| child.child_by_field_name("name"))
		.map(|name| node_slice(name, source).to_vec())
		.collect()
}

fn should_skip_type_ref(name: &[u8], type_params: &[Vec<u8>]) -> bool {
	name == b"Self"
		|| name == b"_"
		|| is_primitive_type(name)
		|| type_params.iter().any(|param| param == name)
}

fn collect_expr_refs(
	env: &RefEnv<'_>,
	type_env: &TypeEnv,
	node: Node<'_>,
	function: &Moniker,
	out: &mut Vec<ResolvedRef>,
) {
	match node.kind() {
		"call_expression" => {
			call_expression_ref(env, type_env, node, function, out);
			return;
		}
		"macro_invocation" => {
			if let Some(reference) = macro_call_ref(*env, function, node) {
				out.push(reference);
			}
			let macro_node = node.child_by_field_name("macro");
			for child in named_children(node) {
				if macro_node.is_some_and(|macro_node| same_syntax_node(macro_node, child)) {
					continue;
				}
				collect_expr_refs(env, type_env, child, function, out);
			}
			return;
		}
		"struct_expression" => {
			struct_instantiation_ref(env, node, function, out);
			for child in named_children(node) {
				collect_expr_refs(env, type_env, child, function, out);
			}
			return;
		}
		"identifier" => {
			identifier_read(env, node, function, out);
			return;
		}
		"scoped_identifier" => {
			scoped_read(env, node, function, out);
			return;
		}
		"let_declaration" => {
			if let Some(ty) = node.child_by_field_name("type") {
				type_refs_from_node(env, ty, function, &[], out);
			}
			if let Some(value) = node.child_by_field_name("value") {
				collect_expr_refs(env, type_env, value, function, out);
			}
			return;
		}
		_ => {}
	}
	for child in named_children(node) {
		collect_expr_refs(env, type_env, child, function, out);
	}
}

fn identifier_read(
	env: &RefEnv<'_>,
	node: Node<'_>,
	function: &Moniker,
	out: &mut Vec<ResolvedRef>,
) {
	let name = node_slice(node, env.source);
	let Some(target) = resolve_local_binding(env.defs, function, name) else {
		return;
	};
	out.push(ResolvedRef {
		source: function.clone(),
		target,
		kind: kinds::READS,
		position: Some(node_position(node)),
		confidence: kinds::CONF_LOCAL,
		hints: RefHints::default(),
	});
}

fn scoped_read(env: &RefEnv<'_>, node: Node<'_>, function: &Moniker, out: &mut Vec<ResolvedRef>) {
	let pieces = path_pieces(node, env.source);
	let Some((target, confidence)) = imported_path_target(env, function, &pieces) else {
		return;
	};
	out.push(ResolvedRef {
		source: function.clone(),
		target,
		kind: kinds::READS,
		position: Some(node_position(node)),
		confidence,
		hints: RefHints::default(),
	});
}

fn call_expression_ref(
	env: &RefEnv<'_>,
	type_env: &TypeEnv,
	call: Node<'_>,
	function: &Moniker,
	out: &mut Vec<ResolvedRef>,
) {
	let Some(func) = call.child_by_field_name("function") else {
		return;
	};
	let external_callback_origin = external_method_call_target(env, type_env, func, function);
	match func.kind() {
		"field_expression" => method_call_ref(env, type_env, call, func, function, out),
		"identifier" => free_fn_call_ref(env, call, func, function, out),
		"scoped_identifier" => path_call_ref(env, call, func, function, out),
		_ => {}
	}
	if let Some(args) = call.child_by_field_name("arguments") {
		for child in named_children(args) {
			collect_call_arg_refs(
				env,
				type_env,
				child,
				function,
				external_callback_origin.as_ref(),
				out,
			);
		}
	}
}

fn collect_call_arg_refs(
	env: &RefEnv<'_>,
	type_env: &TypeEnv,
	arg: Node<'_>,
	function: &Moniker,
	external_callback_origin: Option<&Moniker>,
	out: &mut Vec<ResolvedRef>,
) {
	if arg.kind() == "closure_expression"
		&& let Some(origin) = external_callback_origin
	{
		let mut closure_env = type_env.clone();
		bind_closure_params(env, arg, function, &mut closure_env, origin);
		if let Some(body) = arg.child_by_field_name("body") {
			collect_expr_refs(env, &closure_env, body, function, out);
		}
		return;
	}
	collect_expr_refs(env, type_env, arg, function, out);
}

fn bind_closure_params(
	env: &RefEnv<'_>,
	closure: Node<'_>,
	function: &Moniker,
	type_env: &mut TypeEnv,
	external_origin: &Moniker,
) {
	let Some(params) = closure.child_by_field_name("parameters") else {
		return;
	};
	for param in named_children(params) {
		let (pattern, ty) = if param.kind() == "parameter" {
			(
				param.child_by_field_name("pattern"),
				param.child_by_field_name("type"),
			)
		} else {
			(Some(param), None)
		};
		let Some(pattern) = pattern else {
			continue;
		};
		let Some(name) = binding_name(pattern, env.source) else {
			continue;
		};
		let ty = ty
			.and_then(|ty| type_node_expr(env, ty, function, type_env))
			.unwrap_or_else(|| TypeExpr::external_opaque(external_origin.clone()));
		type_env.bind_local(name, ty);
	}
}

fn method_call_ref(
	env: &RefEnv<'_>,
	type_env: &TypeEnv,
	call: Node<'_>,
	func: Node<'_>,
	function: &Moniker,
	out: &mut Vec<ResolvedRef>,
) {
	let Some(receiver) = func.child_by_field_name("value") else {
		return;
	};
	let Some(field) = func.child_by_field_name("field") else {
		return;
	};
	let name = node_slice(field, env.source);
	let (target, confidence) = if receiver.kind() == "self" {
		enclosing_type(function)
			.map(|target| resolve_callable(env, &target, kinds::METHOD, name))
			.unwrap_or_else(|| unresolved_method(function, name))
	} else if let Some(receiver_type) = receiver_type_target(env, type_env, receiver, function) {
		resolve_receiver_method_target(env, &receiver_type, name)
	} else if is_common_std_method(name) {
		(
			common_std_method_target(function, name),
			kinds::CONF_EXTERNAL,
		)
	} else {
		unresolved_method(function, name)
	};
	out.push(call_ref(
		function,
		target,
		kinds::METHOD_CALL,
		confidence,
		call,
		Some(CallHints {
			name,
			arity: call_argument_count(call),
			receiver: receiver_hint(receiver, env.source),
		}),
	));
	collect_expr_refs(env, type_env, receiver, function, out);
}

fn external_method_call_target(
	env: &RefEnv<'_>,
	type_env: &TypeEnv,
	func: Node<'_>,
	function: &Moniker,
) -> Option<Moniker> {
	(func.kind() == "field_expression").then_some(())?;
	let receiver = func.child_by_field_name("value")?;
	let field = func.child_by_field_name("field")?;
	let receiver_type = receiver_type_target(env, type_env, receiver, function)?;
	let (target, confidence) =
		resolve_receiver_method_target(env, &receiver_type, node_slice(field, env.source));
	(confidence == kinds::CONF_EXTERNAL).then_some(target)
}

fn free_fn_call_ref(
	env: &RefEnv<'_>,
	call: Node<'_>,
	func: Node<'_>,
	function: &Moniker,
	out: &mut Vec<ResolvedRef>,
) {
	let name = node_slice(func, env.source);
	if starts_uppercase(name) {
		let (target, confidence) = resolve_constructor_target(env, function, kinds::STRUCT, name);
		out.push(call_ref(
			function,
			target,
			kinds::INSTANTIATES,
			confidence,
			call,
			None,
		));
		return;
	}
	let (target, confidence) = if let Some(import) = direct_imported_symbol(env, function, name) {
		(import.target.clone(), import.confidence)
	} else {
		resolve_callable_parent(env, function, name)
			.map(|parent| resolve_callable(env, &parent, kinds::FN, name))
			.unwrap_or_else(|| {
				(
					extend_segment(&enclosing_module(function), kinds::FN, name),
					kinds::CONF_UNRESOLVED,
				)
			})
	};
	out.push(call_ref(
		function,
		target,
		kinds::CALLS,
		confidence,
		call,
		Some(CallHints {
			name,
			arity: call_argument_count(call),
			receiver: b"",
		}),
	));
}

fn path_call_ref(
	env: &RefEnv<'_>,
	call: Node<'_>,
	func: Node<'_>,
	function: &Moniker,
	out: &mut Vec<ResolvedRef>,
) {
	let pieces = path_pieces(func, env.source);
	let Some((name, parent_pieces)) = pieces.split_last() else {
		return;
	};
	if let Some((target, confidence)) = constructor_call_target(env, function, name, parent_pieces)
	{
		out.push(call_ref(
			function,
			target,
			kinds::INSTANTIATES,
			confidence,
			call,
			None,
		));
		return;
	}
	let (target, confidence) = resolve_path_call_target(env, function, &pieces);
	out.push(call_ref(
		function,
		target,
		kinds::CALLS,
		confidence,
		call,
		Some(CallHints {
			name,
			arity: call_argument_count(call),
			receiver: b"",
		}),
	));
}

fn struct_instantiation_ref(
	env: &RefEnv<'_>,
	node: Node<'_>,
	function: &Moniker,
	out: &mut Vec<ResolvedRef>,
) {
	let Some(name_node) = node.child_by_field_name("name") else {
		return;
	};
	let pieces = path_pieces(name_node, env.source);
	let (target, confidence) = if let Some((variant, parent_pieces)) = pieces.split_last()
		&& !parent_pieces.is_empty()
		&& let Some((parent, confidence)) =
			resolve_associated_type_target(env, function, parent_pieces)
	{
		(extend_segment(&parent, kinds::PATH, variant), confidence)
	} else if pieces.len() > 1 {
		resolve_type_path(env, function, &pieces)
	} else {
		let name = node_slice(name_node, env.source);
		resolve_constructor_target(env, function, kinds::STRUCT, name)
	};
	out.push(call_ref(
		function,
		target,
		kinds::INSTANTIATES,
		confidence,
		node,
		None,
	));
}

struct CallHints<'a> {
	name: &'a [u8],
	arity: usize,
	receiver: &'a [u8],
}

fn call_ref(
	source: &Moniker,
	target: Moniker,
	kind: &'static [u8],
	confidence: &'static [u8],
	node: Node<'_>,
	hints: Option<CallHints<'_>>,
) -> ResolvedRef {
	let mut ref_hints = RefHints::default();
	if let Some(hints) = hints {
		ref_hints.call_name = hints.name.to_vec();
		ref_hints.call_arity = Some(hints.arity);
		ref_hints.receiver_hint = hints.receiver.to_vec();
	}
	ResolvedRef {
		source: source.clone(),
		target,
		kind,
		position: Some(node_position(node)),
		confidence,
		hints: ref_hints,
	}
}

fn local_type_bindings(
	env: &RefEnv<'_>,
	function_node: Node<'_>,
	body: Node<'_>,
	function: &Moniker,
) -> TypeEnv {
	let mut type_env = TypeEnv::default();
	for param in type_parameters(function_node, env.source) {
		type_env.bind_type_param(param);
	}
	if let Some(params) = function_node.child_by_field_name("parameters") {
		for param in named_children(params).filter(|child| child.kind() == "parameter") {
			let Some(pattern) = param.child_by_field_name("pattern") else {
				continue;
			};
			let Some(ty) = param.child_by_field_name("type") else {
				continue;
			};
			let Some(name) = binding_name(pattern, env.source) else {
				continue;
			};
			if let Some(ty) = type_node_expr(env, ty, function, &type_env) {
				type_env.bind_local(name, ty);
			}
		}
	}
	collect_local_type_bindings(env, body, function, &mut type_env);
	type_env
}

fn collect_local_type_bindings(
	env: &RefEnv<'_>,
	node: Node<'_>,
	function: &Moniker,
	type_env: &mut TypeEnv,
) {
	if node.kind() == "let_declaration" {
		let Some(pattern) = node.child_by_field_name("pattern") else {
			return;
		};
		let Some(name) = binding_name(pattern, env.source) else {
			return;
		};
		let target = node
			.child_by_field_name("type")
			.and_then(|ty| type_node_expr(env, ty, function, type_env))
			.or_else(|| {
				node.child_by_field_name("value")
					.and_then(|value| infer_value_type_expr(env, type_env, value, function))
			});
		if let Some(ty) = target {
			type_env.bind_local(name, ty);
		}
		return;
	}
	if node.kind() == "for_expression" {
		bind_for_item_type(env, node, function, type_env);
		if let Some(body) = node.child_by_field_name("body") {
			collect_local_type_bindings(env, body, function, type_env);
		}
		return;
	}
	for child in named_children(node) {
		collect_local_type_bindings(env, child, function, type_env);
	}
}

fn bind_for_item_type(
	env: &RefEnv<'_>,
	node: Node<'_>,
	function: &Moniker,
	type_env: &mut TypeEnv,
) {
	let Some(pattern) = node.child_by_field_name("pattern") else {
		return;
	};
	let Some(name) = binding_name(pattern, env.source) else {
		return;
	};
	let Some(value) = node.child_by_field_name("value") else {
		return;
	};
	let Some(ty) = infer_value_type_expr(env, type_env, value, function) else {
		return;
	};
	let item_ty = ty.iterable_item().cloned().unwrap_or(ty);
	type_env.bind_local(name, item_ty);
}

fn binding_name<'a>(pattern: Node<'a>, source: &'a [u8]) -> Option<&'a [u8]> {
	(pattern.kind() == "identifier").then(|| node_slice(pattern, source))
}

fn type_node_expr(
	env: &RefEnv<'_>,
	node: Node<'_>,
	source: &Moniker,
	type_env: &TypeEnv,
) -> Option<TypeExpr> {
	match node.kind() {
		"reference_type" | "mutable_reference_type" => carrier_type_node(node)
			.and_then(|inner| type_node_expr(env, inner, source, type_env))
			.map(|ty| TypeExpr::Ref(Box::new(ty))),
		"pointer_type" => carrier_type_node(node)
			.and_then(|inner| type_node_expr(env, inner, source, type_env))
			.map(|ty| TypeExpr::Pointer(Box::new(ty))),
		"array_type" => node
			.child_by_field_name("element")
			.and_then(|inner| type_node_expr(env, inner, source, type_env))
			.map(|ty| TypeExpr::Array(Box::new(ty))),
		"generic_type" => {
			let base = carrier_type_node(node)
				.and_then(|inner| type_node_expr(env, inner, source, type_env))?;
			Some(TypeExpr::Generic {
				base: Box::new(base),
				args: generic_type_args(env, node, source, type_env),
			})
		}
		"type_identifier" => {
			let name = node_slice(node, env.source);
			if type_env.is_type_param(name) {
				Some(TypeExpr::TypeParam(name.to_vec()))
			} else {
				Some(TypeExpr::resolved(resolve_type_name(env, source, name).0))
			}
		}
		"scoped_type_identifier" => {
			let pieces = path_pieces(node, env.source);
			Some(TypeExpr::resolved(
				resolve_type_path(env, source, &pieces).0,
			))
		}
		"tuple_type" => Some(TypeExpr::Tuple(
			named_children(node)
				.filter_map(|child| type_node_expr(env, child, source, type_env))
				.collect(),
		)),
		"type_binding" => node
			.child_by_field_name("type")
			.and_then(|ty| type_node_expr(env, ty, source, type_env)),
		_ => named_children(node).find_map(|child| type_node_expr(env, child, source, type_env)),
	}
}

fn generic_type_args(
	env: &RefEnv<'_>,
	node: Node<'_>,
	source: &Moniker,
	type_env: &TypeEnv,
) -> Vec<TypeExpr> {
	node.child_by_field_name("type_arguments")
		.into_iter()
		.flat_map(named_children)
		.filter_map(|arg| type_node_expr(env, arg, source, type_env))
		.collect()
}

fn carrier_type_node(node: Node<'_>) -> Option<Node<'_>> {
	node.child_by_field_name("type").or_else(|| {
		named_children(node).find(|child| {
			matches!(
				child.kind(),
				"type_identifier"
					| "scoped_type_identifier"
					| "generic_type"
					| "reference_type"
					| "mutable_reference_type"
					| "pointer_type"
			)
		})
	})
}

fn infer_value_type_expr(
	env: &RefEnv<'_>,
	type_env: &TypeEnv,
	value: Node<'_>,
	function: &Moniker,
) -> Option<TypeExpr> {
	match value.kind() {
		"call_expression" => infer_call_type_expr(env, type_env, value, function),
		"struct_expression" => {
			let name = value.child_by_field_name("name")?;
			Some(TypeExpr::resolved(
				resolve_type_path(env, function, &path_pieces(name, env.source)).0,
			))
		}
		"identifier" => type_env
			.resolve_local(node_slice(value, env.source))
			.cloned(),
		_ => None,
	}
}

fn infer_call_type_expr(
	env: &RefEnv<'_>,
	type_env: &TypeEnv,
	call: Node<'_>,
	function: &Moniker,
) -> Option<TypeExpr> {
	let func = call.child_by_field_name("function")?;
	match func.kind() {
		"scoped_identifier" => {
			let pieces = path_pieces(func, env.source);
			let (_, type_pieces) = pieces.split_last()?;
			resolve_associated_type_target(env, function, type_pieces)
				.map(|(target, _)| TypeExpr::resolved(target))
		}
		"field_expression" => {
			let receiver = func.child_by_field_name("value")?;
			let field = func.child_by_field_name("field")?;
			let name = node_slice(field, env.source);
			let receiver_type = receiver_type_target(env, type_env, receiver, function)?;
			let (target, confidence) = resolve_receiver_method_target(env, &receiver_type, name);
			(confidence == kinds::CONF_EXTERNAL).then(|| TypeExpr::external_opaque(target))
		}
		"identifier" => {
			let name = node_slice(func, env.source);
			starts_uppercase(name).then(|| {
				TypeExpr::resolved(resolve_constructor_target(env, function, kinds::STRUCT, name).0)
			})
		}
		_ => None,
	}
}

fn receiver_type_target(
	env: &RefEnv<'_>,
	type_env: &TypeEnv,
	receiver: Node<'_>,
	function: &Moniker,
) -> Option<Moniker> {
	match receiver.kind() {
		"identifier" => type_env
			.resolve_local(node_slice(receiver, env.source))
			.and_then(TypeExpr::receiver_owner)
			.cloned(),
		"call_expression" => infer_call_type_expr(env, type_env, receiver, function)
			.and_then(|ty| ty.receiver_owner().cloned()),
		_ => None,
	}
}

fn resolve_local_binding(
	defs: &[DiscoveredDef],
	function: &Moniker,
	name: &[u8],
) -> Option<Moniker> {
	let local = extend_segment(function, kinds::LOCAL, name);
	if defs.iter().any(|def| {
		def.parent == *function
			&& (def.kind == kinds::LOCAL || def.kind == kinds::PARAM)
			&& def.name == name
	}) {
		Some(local)
	} else {
		None
	}
}

fn resolve_type(defs: &[DiscoveredDef], source: &Moniker, name: &[u8]) -> Option<Moniker> {
	let mut current = Some(source.clone());
	while let Some(scope) = current {
		if let Some(target) = find_local_type(defs, &scope, name) {
			return Some(target);
		}
		current = scope.parent();
	}
	None
}

fn resolve_type_name(env: &RefEnv<'_>, source: &Moniker, name: &[u8]) -> (Moniker, &'static [u8]) {
	if let Some(target) = resolve_type(env.defs, source, name) {
		return (target, kinds::CONF_RESOLVED);
	}
	resolve_imported_type(env, source, name).unwrap_or_else(|| {
		let target = extend_segment(&enclosing_module(source), kinds::STRUCT, name);
		let confidence = if env.defs.iter().any(|def| def.moniker == target) {
			kinds::CONF_RESOLVED
		} else {
			kinds::CONF_NAME_MATCH
		};
		(target, confidence)
	})
}

fn resolve_type_path(
	env: &RefEnv<'_>,
	source: &Moniker,
	pieces: &[Vec<u8>],
) -> (Moniker, &'static [u8]) {
	let Some((head, rest)) = pieces.split_first() else {
		return (source.clone(), kinds::CONF_UNRESOLVED);
	};
	match head.as_slice() {
		b"crate" => {
			return (
				local_crate_path_target(source, rest),
				kinds::CONF_NAME_MATCH,
			);
		}
		b"self" => {
			return (
				local_relative_path_target(&enclosing_module(source), rest),
				kinds::CONF_NAME_MATCH,
			);
		}
		b"super" => {
			let module = rust_parent_module(&enclosing_module(source))
				.unwrap_or_else(|| enclosing_module(source));
			return (
				local_relative_path_target(&module, rest),
				kinds::CONF_NAME_MATCH,
			);
		}
		_ => {}
	}
	if let Some(import) = direct_imported_symbol(env, source, head) {
		return (
			append_path_segments(import.target.clone(), rest),
			import.confidence,
		);
	}
	if local_module_exists(env, source, head) {
		return (
			local_relative_path_target(&enclosing_module(source), pieces),
			kinds::CONF_NAME_MATCH,
		);
	}
	if is_external_import_root(head) || is_rust_builtin_external_root(head) {
		return (
			external_target_from_vec(source, pieces),
			kinds::CONF_EXTERNAL,
		);
	}
	(
		local_relative_path_target(&enclosing_module(source), pieces),
		kinds::CONF_NAME_MATCH,
	)
}

fn resolve_imported_type(
	env: &RefEnv<'_>,
	source: &Moniker,
	name: &[u8],
) -> Option<(Moniker, &'static [u8])> {
	resolve_direct_imported_type(env, source, name)
		.or_else(|| rust_prelude_type(source, name))
		.or_else(|| {
			wildcard_module(env, source).map(|module| {
				(
					extend_segment(&module, kinds::PATH, name),
					kinds::CONF_IMPORTED,
				)
			})
		})
}

fn resolve_direct_imported_type(
	env: &RefEnv<'_>,
	source: &Moniker,
	name: &[u8],
) -> Option<(Moniker, &'static [u8])> {
	direct_imported_symbol(env, source, name)
		.map(|import| (import.target.clone(), import.confidence))
}

fn direct_imported_symbol<'a>(
	env: &'a RefEnv<'_>,
	source: &Moniker,
	name: &[u8],
) -> Option<&'a ImportedSymbol> {
	env.imported_symbols.iter().find(|import| {
		import.name == name && (import.scope == *source || import.scope.is_ancestor_of(source))
	})
}

fn imported_path_target(
	env: &RefEnv<'_>,
	source: &Moniker,
	pieces: &[Vec<u8>],
) -> Option<(Moniker, &'static [u8])> {
	let (head, rest) = pieces.split_first()?;
	if let Some(target) = rust_std_associated_path(source, pieces) {
		return Some((target, kinds::CONF_EXTERNAL));
	}
	if let Some(import) = direct_imported_symbol(env, source, head) {
		return Some((
			append_path_segments(import.target.clone(), rest),
			import.confidence,
		));
	}
	if let Some(target) = resolve_type(env.defs, source, head) {
		return Some((append_path_segments(target, rest), kinds::CONF_RESOLVED));
	}
	if let Some((target, confidence)) = rust_prelude_type(source, head) {
		return Some((append_path_segments(target, rest), confidence));
	}
	if local_module_exists(env, source, head) {
		return Some((
			local_relative_path_target(&enclosing_module(source), pieces),
			kinds::CONF_NAME_MATCH,
		));
	}
	if is_external_import_root(head) {
		return Some((
			external_target_from_vec(source, pieces),
			kinds::CONF_EXTERNAL,
		));
	}
	let mut target = wildcard_module(env, source)?;
	for piece in pieces {
		target = extend_segment(&target, kinds::PATH, piece);
	}
	Some((target, kinds::CONF_IMPORTED))
}

fn resolve_macro_target(
	env: &RefEnv<'_>,
	scope: &Moniker,
	pieces: &[Vec<u8>],
	name: &[u8],
) -> (Moniker, &'static [u8]) {
	if let Some(target) = rust_builtin_macro(scope, name) {
		return (target, kinds::CONF_EXTERNAL);
	}
	if pieces.len() > 1 {
		let head = &pieces[0];
		if let Some(import) = direct_imported_symbol(env, scope, head) {
			return (
				append_path_segments(import.target.clone(), &pieces[1..]),
				import.confidence,
			);
		}
		if is_external_import_root(head) {
			return (
				external_target_from_vec(scope, pieces),
				kinds::CONF_EXTERNAL,
			);
		}
		return (
			local_relative_path_target(&enclosing_module(scope), pieces),
			kinds::CONF_NAME_MATCH,
		);
	}
	if let Some(import) = direct_imported_symbol(env, scope, name) {
		return (import.target.clone(), import.confidence);
	}
	(
		extend_segment(&enclosing_module(scope), kinds::MACRO, name),
		kinds::CONF_UNRESOLVED,
	)
}

fn constructor_call_target(
	env: &RefEnv<'_>,
	function: &Moniker,
	name: &[u8],
	parent_pieces: &[Vec<u8>],
) -> Option<(Moniker, &'static [u8])> {
	if parent_pieces.is_empty() {
		return starts_uppercase(name)
			.then(|| resolve_constructor_target(env, function, kinds::STRUCT, name));
	}
	let type_name = parent_pieces.last()?;
	if name == b"new" && starts_uppercase(type_name) {
		return resolve_associated_type_target(env, function, parent_pieces);
	}
	if starts_uppercase(name) && starts_uppercase(type_name) {
		let (target, confidence) = resolve_associated_type_target(env, function, parent_pieces)?;
		return Some((
			extend_segment(&target, kinds::ENUM_CONSTANT, name),
			confidence,
		));
	}
	None
}

fn resolve_path_call_target(
	env: &RefEnv<'_>,
	function: &Moniker,
	pieces: &[Vec<u8>],
) -> (Moniker, &'static [u8]) {
	if let Some(target) = rust_std_associated_path(function, pieces) {
		return (target, kinds::CONF_EXTERNAL);
	}
	let Some((call_name, type_pieces)) = pieces.split_last() else {
		return (function.clone(), kinds::CONF_UNRESOLVED);
	};
	let Some(head) = pieces.first() else {
		return (function.clone(), kinds::CONF_UNRESOLVED);
	};
	if let Some(import) = direct_imported_symbol(env, function, head) {
		return (
			append_path_segments(import.target.clone(), &pieces[1..]),
			import.confidence,
		);
	}
	if let Some((receiver_type, confidence)) =
		resolve_associated_type_target(env, function, type_pieces)
	{
		return (
			extend_segment(&receiver_type, kinds::METHOD, call_name),
			if external_root(&receiver_type).is_some() {
				kinds::CONF_EXTERNAL
			} else {
				confidence
			},
		);
	}
	if is_external_import_root(head) {
		return (
			external_target_from_vec(function, pieces),
			kinds::CONF_EXTERNAL,
		);
	}
	(
		extend_segment(&enclosing_module(function), kinds::FN, call_name),
		kinds::CONF_UNRESOLVED,
	)
}

fn resolve_associated_type_target(
	env: &RefEnv<'_>,
	function: &Moniker,
	type_pieces: &[Vec<u8>],
) -> Option<(Moniker, &'static [u8])> {
	let head = type_pieces.first()?;
	if type_pieces.len() == 1 {
		if head == b"Self" {
			return enclosing_type(function).map(|target| (target, kinds::CONF_RESOLVED));
		}
		if let Some(target) = resolve_type(env.defs, function, head) {
			return Some((target, kinds::CONF_RESOLVED));
		}
		if let Some(import) = direct_imported_symbol(env, function, head) {
			return Some((import.target.clone(), import.confidence));
		}
		if let Some(target) = rust_prelude_type(function, head) {
			return Some(target);
		}
		return starts_uppercase(head)
			.then(|| resolve_constructor_target(env, function, kinds::STRUCT, head));
	}
	match head.as_slice() {
		b"crate" | b"self" | b"super" => Some(resolve_type_path(env, function, type_pieces)),
		_ => {
			if let Some(import) = direct_imported_symbol(env, function, head) {
				return Some((
					append_path_segments(import.target.clone(), &type_pieces[1..]),
					import.confidence,
				));
			}
			if is_external_import_root(head) || is_rust_builtin_external_root(head) {
				return Some((
					external_target_from_vec(function, type_pieces),
					kinds::CONF_EXTERNAL,
				));
			}
			None
		}
	}
}

fn resolve_constructor_target(
	env: &RefEnv<'_>,
	function: &Moniker,
	kind: &'static [u8],
	name: &[u8],
) -> (Moniker, &'static [u8]) {
	if name == b"Self"
		&& let Some(target) = enclosing_type(function)
	{
		return (target, kinds::CONF_RESOLVED);
	}
	if let Some(import) = direct_imported_symbol(env, function, name) {
		return (import.target.clone(), import.confidence);
	}
	if let Some(target) = rust_prelude_constructor(function, name) {
		return (target, kinds::CONF_EXTERNAL);
	}
	let target = extend_segment(&enclosing_module(function), kind, name);
	let confidence = if env.defs.iter().any(|def| def.moniker == target) {
		kinds::CONF_RESOLVED
	} else {
		kinds::CONF_NAME_MATCH
	};
	(target, confidence)
}

fn resolve_receiver_method_target(
	env: &RefEnv<'_>,
	receiver_type: &Moniker,
	name: &[u8],
) -> (Moniker, &'static [u8]) {
	let target = extend_segment(receiver_type, kinds::METHOD, name);
	if external_root(receiver_type).is_some() {
		return (target, kinds::CONF_EXTERNAL);
	}
	if env.defs.iter().any(|def| def.moniker == target) {
		return (target, kinds::CONF_RESOLVED);
	}
	if is_common_std_method(name) {
		return (
			common_std_method_target(receiver_type, name),
			kinds::CONF_EXTERNAL,
		);
	}
	(target, kinds::CONF_NAME_MATCH)
}

fn resolve_callable(
	env: &RefEnv<'_>,
	parent: &Moniker,
	kind: &'static [u8],
	name: &[u8],
) -> (Moniker, &'static [u8]) {
	env.defs
		.iter()
		.find(|def| def.parent == *parent && def.kind == kind && def.call_name == name)
		.map(|def| (def.moniker.clone(), kinds::CONF_RESOLVED))
		.unwrap_or_else(|| (extend_segment(parent, kind, name), kinds::CONF_UNRESOLVED))
}

fn resolve_callable_parent(env: &RefEnv<'_>, function: &Moniker, name: &[u8]) -> Option<Moniker> {
	let module = enclosing_module(function);
	env.defs
		.iter()
		.any(|def| def.parent == module && def.kind == kinds::FN && def.call_name == name)
		.then_some(module)
}

fn unresolved_method(function: &Moniker, name: &[u8]) -> (Moniker, &'static [u8]) {
	(
		extend_segment(&enclosing_module(function), kinds::METHOD, name),
		kinds::CONF_UNRESOLVED,
	)
}

fn expand_import_leaf(
	env: &RefEnv<'_>,
	scope: &Moniker,
	node: Node<'_>,
	leaf: ImportLeaf,
	expansion: &mut ImportExpansion,
) {
	if leaf.path.is_empty() {
		return;
	}
	let refs = if import_leaf_is_external(env, scope, &leaf) {
		external_import_refs(scope, node, &leaf)
	} else {
		local_import_refs(env, scope, node, &leaf)
	};
	if let Some(symbol) = import_symbol_binding(scope, &leaf, &refs) {
		expansion.symbols.push(symbol);
	}
	if leaf.kind == ImportLeafKind::Wildcard
		&& let Some(module_ref) = refs.iter().find(|reference| {
			reference.kind == kinds::IMPORTS_MODULE && reference.confidence != kinds::CONF_EXTERNAL
		}) {
		expansion.wildcard_modules.push(module_ref.target.clone());
	}
	expansion.refs.extend(refs);
}

fn import_leaf_is_external(env: &RefEnv<'_>, scope: &Moniker, leaf: &ImportLeaf) -> bool {
	let Some(head) = leaf.path.first() else {
		return false;
	};
	(is_external_import_root(head) || is_rust_builtin_external_root(head))
		&& !local_module_exists(env, scope, head)
}

fn external_import_refs(scope: &Moniker, node: Node<'_>, leaf: &ImportLeaf) -> Vec<ResolvedRef> {
	if leaf.kind == ImportLeafKind::Wildcard {
		return vec![import_ref(
			scope,
			external_target_from_vec(scope, &leaf.path),
			kinds::IMPORTS_MODULE,
			kinds::CONF_EXTERNAL,
			node,
		)];
	}
	if leaf.kind == ImportLeafKind::SelfImport {
		let target = external_target_from_vec(scope, &leaf.path);
		return vec![
			import_ref(
				scope,
				target.clone(),
				kinds::IMPORTS_MODULE,
				kinds::CONF_EXTERNAL,
				node,
			),
			import_ref(
				scope,
				target,
				kinds::IMPORTS_SYMBOL,
				kinds::CONF_EXTERNAL,
				node,
			),
		];
	}
	let mut refs = Vec::new();
	if leaf.path.len() > 1 {
		refs.push(import_ref(
			scope,
			external_target_from_vec(scope, &leaf.path[..leaf.path.len() - 1]),
			kinds::IMPORTS_MODULE,
			kinds::CONF_EXTERNAL,
			node,
		));
	}
	refs.push(import_ref(
		scope,
		external_target_from_vec(scope, &leaf.path),
		kinds::IMPORTS_SYMBOL,
		kinds::CONF_EXTERNAL,
		node,
	));
	refs
}

fn local_import_refs(
	env: &RefEnv<'_>,
	scope: &Moniker,
	node: Node<'_>,
	leaf: &ImportLeaf,
) -> Vec<ResolvedRef> {
	if leaf.kind == ImportLeafKind::Wildcard {
		return vec![import_ref(
			scope,
			local_wildcard_target(env, scope, &leaf.path),
			kinds::IMPORTS_MODULE,
			kinds::CONF_IMPORTED,
			node,
		)];
	}
	if leaf.kind == ImportLeafKind::SelfImport {
		let target = local_module_target(scope, &leaf.path);
		return vec![
			import_ref(
				scope,
				target.clone(),
				kinds::IMPORTS_MODULE,
				kinds::CONF_IMPORTED,
				node,
			),
			import_ref(
				scope,
				target,
				kinds::IMPORTS_SYMBOL,
				kinds::CONF_IMPORTED,
				node,
			),
		];
	}
	let target = local_symbol_target(scope, &leaf.path);
	let mut refs = Vec::new();
	if let Some(parent) = importable_parent(&target, rust_importable_namespace) {
		refs.push(import_ref(
			scope,
			parent,
			kinds::IMPORTS_MODULE,
			kinds::CONF_IMPORTED,
			node,
		));
	}
	refs.push(import_ref(
		scope,
		target,
		kinds::IMPORTS_SYMBOL,
		kinds::CONF_IMPORTED,
		node,
	));
	refs
}

fn local_wildcard_target(env: &RefEnv<'_>, scope: &Moniker, path: &[Vec<u8>]) -> Moniker {
	if path.last().is_some_and(|name| starts_uppercase(name)) {
		if path.len() == 1 {
			return resolve_type_name(env, scope, &path[0]).0;
		}
		return resolve_type_path(env, scope, path).0;
	}
	local_module_target(scope, path)
}

fn import_symbol_binding(
	scope: &Moniker,
	leaf: &ImportLeaf,
	refs: &[ResolvedRef],
) -> Option<ImportedSymbol> {
	if leaf.kind == ImportLeafKind::Wildcard {
		return None;
	}
	let name = import_leaf_binding_name(leaf)?.to_vec();
	let target = refs
		.iter()
		.rev()
		.find(|reference| reference.kind == kinds::IMPORTS_SYMBOL)?;
	Some(ImportedSymbol {
		scope: scope.clone(),
		name,
		target: target.target.clone(),
		confidence: target.confidence,
	})
}

fn rust_importable_namespace(target: &Moniker) -> bool {
	target.last_kind().as_deref() == Some(kinds::MODULE)
}

fn append_path_segments(mut target: Moniker, pieces: &[Vec<u8>]) -> Moniker {
	let Some(last_kind) = target.last_kind() else {
		return target;
	};
	if last_kind.as_slice() == kinds::MODULE {
		let mut builder = MonikerBuilder::from_view(target.as_view());
		append_symbol_path(&mut builder, pieces);
		return builder.build();
	}
	for piece in pieces {
		target = extend_segment(&target, kinds::PATH, piece);
	}
	target
}

fn import_ref(
	source: &Moniker,
	target: Moniker,
	kind: &'static [u8],
	confidence: &'static [u8],
	node: Node<'_>,
) -> ResolvedRef {
	ResolvedRef {
		source: source.clone(),
		target,
		kind,
		position: Some(node_position(node)),
		confidence,
		hints: RefHints::default(),
	}
}

fn wildcard_module(env: &RefEnv<'_>, source: &Moniker) -> Option<Moniker> {
	env.wildcard_imports
		.iter()
		.find(|(scope, _)| scope == source || scope.is_ancestor_of(source))
		.map(|(_, module)| module.clone())
}

fn find_local_type(defs: &[DiscoveredDef], scope: &Moniker, name: &[u8]) -> Option<Moniker> {
	[kinds::ENUM, kinds::TRAIT, kinds::TYPE, kinds::STRUCT]
		.into_iter()
		.map(|kind| extend_segment(scope, kind, name))
		.find(|candidate| defs.iter().any(|def| def.moniker == *candidate))
}

fn local_module_exists(env: &RefEnv<'_>, scope: &Moniker, name: &[u8]) -> bool {
	let file_module = local_module_target(scope, &[name.to_vec()]);
	let lexical_module = extend_segment(scope, kinds::MODULE, name);
	env.defs
		.iter()
		.any(|def| def.moniker == file_module || def.moniker == lexical_module)
}

fn same_syntax_node(left: Node<'_>, right: Node<'_>) -> bool {
	left.kind() == right.kind()
		&& left.start_byte() == right.start_byte()
		&& left.end_byte() == right.end_byte()
}

fn type_ref(
	source: &Moniker,
	target: Moniker,
	position: Position,
	confidence: &'static [u8],
) -> ResolvedRef {
	typed_ref(source, target, position, confidence, kinds::USES_TYPE)
}

fn typed_ref(
	source: &Moniker,
	target: Moniker,
	position: Position,
	confidence: &'static [u8],
	kind: &'static [u8],
) -> ResolvedRef {
	ResolvedRef {
		source: source.clone(),
		target,
		kind,
		position: Some(position),
		confidence,
		hints: RefHints::default(),
	}
}

fn call_argument_count(call: Node<'_>) -> usize {
	let Some(args) = call.child_by_field_name("arguments") else {
		return 0;
	};
	named_children(args).count()
}

fn receiver_hint<'a>(receiver: Node<'a>, source: &'a [u8]) -> &'a [u8] {
	match receiver.kind() {
		"self" => HINT_SELF,
		"identifier" => node_slice(receiver, source),
		"field_expression" => HINT_MEMBER,
		"call_expression" => HINT_CALL,
		_ => b"",
	}
}

fn external_target(scope: &Moniker, pieces: &[&[u8]]) -> Moniker {
	let mut builder = MonikerBuilder::new();
	builder.project(scope.as_view().project());
	if let Some((head, rest)) = pieces.split_first() {
		builder.segment(kinds::EXTERNAL_PKG, head);
		for piece in rest {
			builder.segment(kinds::PATH, piece);
		}
	}
	builder.build()
}

fn external_target_from_vec(scope: &Moniker, pieces: &[Vec<u8>]) -> Moniker {
	let borrowed = pieces.iter().map(Vec::as_slice).collect::<Vec<_>>();
	external_target(scope, &borrowed)
}

fn external_crate_path(scope: &Moniker, root: &[u8], pieces: &[Vec<u8>]) -> Moniker {
	let mut builder = MonikerBuilder::new();
	builder.project(scope.as_view().project());
	builder.segment(kinds::EXTERNAL_PKG, root);
	for piece in pieces {
		builder.segment(kinds::PATH, piece);
	}
	builder.build()
}

fn external_crate_item(scope: &Moniker, root: &[u8], pieces: &[(&[u8], &[u8])]) -> Moniker {
	let mut builder = MonikerBuilder::new();
	builder.project(scope.as_view().project());
	builder.segment(kinds::EXTERNAL_PKG, root);
	for (kind, name) in pieces {
		builder.segment(kind, name);
	}
	builder.build()
}

fn external_std_item(scope: &Moniker, pieces: &[(&[u8], &[u8])]) -> Moniker {
	external_crate_item(scope, b"std", pieces)
}

fn rust_prelude_type(scope: &Moniker, name: &[u8]) -> Option<(Moniker, &'static [u8])> {
	let (module, kind) = match name {
		b"Box" => (b"boxed".as_slice(), kinds::STRUCT),
		b"Fn" | b"FnMut" | b"FnOnce" => (b"ops".as_slice(), kinds::TRAIT),
		b"Iterator" | b"IntoIterator" => (b"iter".as_slice(), kinds::TRAIT),
		b"AsMut" | b"AsRef" | b"From" | b"Into" | b"TryFrom" | b"TryInto" => {
			(b"convert".as_slice(), kinds::TRAIT)
		}
		b"Clone" => (b"clone".as_slice(), kinds::TRAIT),
		b"Debug" => (b"fmt".as_slice(), kinds::TRAIT),
		b"Default" => (b"default".as_slice(), kinds::TRAIT),
		b"Display" => (b"fmt".as_slice(), kinds::TRAIT),
		b"Eq" | b"Ord" | b"PartialEq" | b"PartialOrd" => (b"cmp".as_slice(), kinds::TRAIT),
		b"Hash" => (b"hash".as_slice(), kinds::TRAIT),
		b"Send" | b"Sync" => (b"marker".as_slice(), kinds::TRAIT),
		b"Option" => (b"option".as_slice(), kinds::ENUM),
		b"Result" => (b"result".as_slice(), kinds::ENUM),
		b"String" => (b"string".as_slice(), kinds::STRUCT),
		b"Vec" => (b"vec".as_slice(), kinds::STRUCT),
		_ => return None,
	};
	let mut builder = MonikerBuilder::new();
	builder.project(scope.as_view().project());
	builder.segment(kinds::EXTERNAL_PKG, b"std");
	builder.segment(kinds::PATH, module);
	builder.segment(kind, name);
	Some((builder.build(), kinds::CONF_EXTERNAL))
}

fn enclosing_module(scope: &Moniker) -> Moniker {
	enclosing_segment(scope, |kind| kind == kinds::MODULE).unwrap_or_else(|| scope.clone())
}

fn enclosing_type(scope: &Moniker) -> Option<Moniker> {
	enclosing_segment(scope, |kind| {
		kind == kinds::STRUCT || kind == kinds::TRAIT || kind == kinds::ENUM
	})
}

fn enclosing_segment(scope: &Moniker, pred: impl Fn(&[u8]) -> bool) -> Option<Moniker> {
	let view = scope.as_view();
	let mut last = None;
	for (index, segment) in view.segments().enumerate() {
		if pred(segment.kind) {
			last = Some(index);
		}
	}
	let index = last?;
	let mut builder = MonikerBuilder::from_view(view);
	builder.truncate(index + 1);
	Some(builder.build())
}

fn external_root(target: &Moniker) -> Option<&[u8]> {
	target
		.as_view()
		.segments()
		.next()
		.and_then(|segment| (segment.kind == kinds::EXTERNAL_PKG).then_some(segment.name))
}

fn resolve_derive_target(
	env: &RefEnv<'_>,
	scope: &Moniker,
	name: &[u8],
) -> (Moniker, &'static [u8]) {
	if let Some(import) = direct_imported_symbol(env, scope, name) {
		return (import.target.clone(), import.confidence);
	}
	if let Some(target) = rust_builtin_derive_trait(scope, name) {
		return (target, kinds::CONF_EXTERNAL);
	}
	if is_external_import_root(name) {
		return (external_target(scope, &[name]), kinds::CONF_EXTERNAL);
	}
	(
		extend_segment(scope, kinds::TRAIT, name),
		kinds::CONF_NAME_MATCH,
	)
}

fn resolve_attribute_target(
	env: &RefEnv<'_>,
	scope: &Moniker,
	name: &[u8],
) -> (Moniker, &'static [u8]) {
	if let Some(target) = rust_known_attribute(scope, name) {
		return (target, kinds::CONF_EXTERNAL);
	}
	if let Some(import) = direct_imported_symbol(env, scope, name) {
		return (import.target.clone(), import.confidence);
	}
	(
		extend_segment(scope, kinds::FN, name),
		kinds::CONF_NAME_MATCH,
	)
}

fn rust_builtin_derive_trait(scope: &Moniker, name: &[u8]) -> Option<Moniker> {
	match name {
		b"Clone" => Some(external_std_trait(scope, b"clone", b"Clone")),
		b"Copy" => Some(external_std_trait(scope, b"marker", b"Copy")),
		b"Debug" => Some(external_std_trait(scope, b"fmt", b"Debug")),
		b"Default" => Some(external_std_trait(scope, b"default", b"Default")),
		b"Eq" => Some(external_std_trait(scope, b"cmp", b"Eq")),
		b"Hash" => Some(external_std_trait(scope, b"hash", b"Hash")),
		b"Ord" => Some(external_std_trait(scope, b"cmp", b"Ord")),
		b"PartialEq" => Some(external_std_trait(scope, b"cmp", b"PartialEq")),
		b"PartialOrd" => Some(external_std_trait(scope, b"cmp", b"PartialOrd")),
		_ => None,
	}
}

fn rust_known_attribute(scope: &Moniker, name: &[u8]) -> Option<Moniker> {
	let root = match name {
		b"allow" | b"cfg" | b"cfg_attr" | b"default" | b"derive" | b"doc" | b"should_panic"
		| b"test" => b"std".as_slice(),
		_ => return None,
	};
	Some(external_attribute(scope, root, name))
}

fn external_std_trait(scope: &Moniker, module: &[u8], name: &[u8]) -> Moniker {
	let mut builder = MonikerBuilder::new();
	builder.project(scope.as_view().project());
	builder.segment(kinds::EXTERNAL_PKG, b"std");
	builder.segment(kinds::PATH, module);
	builder.segment(kinds::TRAIT, name);
	builder.build()
}

fn external_attribute(scope: &Moniker, root: &[u8], name: &[u8]) -> Moniker {
	let mut builder = MonikerBuilder::new();
	builder.project(scope.as_view().project());
	builder.segment(kinds::EXTERNAL_PKG, root);
	builder.segment(kinds::PATH, b"attributes");
	builder.segment(kinds::FN, name);
	builder.build()
}

fn is_external_import_root(name: &[u8]) -> bool {
	name != b"self"
		&& name != b"crate"
		&& name != b"super"
		&& name
			.first()
			.is_some_and(|first| first.is_ascii_lowercase() || *first == b'_')
}

fn starts_uppercase(name: &[u8]) -> bool {
	name.first().is_some_and(u8::is_ascii_uppercase)
}

fn is_primitive_type(name: &[u8]) -> bool {
	matches!(
		name,
		b"i8"
			| b"i16" | b"i32"
			| b"i64" | b"i128"
			| b"isize"
			| b"u8" | b"u16"
			| b"u32" | b"u64"
			| b"u128" | b"usize"
			| b"f32" | b"f64"
			| b"bool" | b"char"
			| b"str" | b"String"
			| b"()"
	)
}

fn is_rust_builtin_external_root(name: &[u8]) -> bool {
	matches!(name, b"std" | b"core" | b"alloc")
}

fn rust_prelude_constructor(scope: &Moniker, name: &[u8]) -> Option<Moniker> {
	match name {
		b"Ok" | b"Err" => Some(external_std_item(
			scope,
			&[
				(kinds::PATH, b"result".as_slice()),
				(kinds::ENUM, b"Result".as_slice()),
				(kinds::ENUM_CONSTANT, name),
			],
		)),
		b"Some" | b"None" => Some(external_std_item(
			scope,
			&[
				(kinds::PATH, b"option".as_slice()),
				(kinds::ENUM, b"Option".as_slice()),
				(kinds::ENUM_CONSTANT, name),
			],
		)),
		b"Box" | b"String" | b"Vec" => rust_prelude_type(scope, name).map(|(target, _)| target),
		_ => None,
	}
}

fn rust_std_associated_path(scope: &Moniker, pieces: &[Vec<u8>]) -> Option<Moniker> {
	let head = pieces.first()?;
	if !is_primitive_type(head) && rust_prelude_type(scope, head).is_none() {
		return None;
	}
	Some(external_crate_path(scope, b"std", pieces))
}

fn rust_builtin_macro(scope: &Moniker, name: &[u8]) -> Option<Moniker> {
	let builtin =
		matches!(
			name,
			b"assert"
				| b"assert_eq"
				| b"assert_ne"
				| b"cfg" | b"compile_error"
				| b"concat" | b"dbg"
				| b"debug_assert"
				| b"debug_assert_eq"
				| b"debug_assert_ne"
				| b"env" | b"eprintln"
				| b"format" | b"format_args"
				| b"include" | b"include_bytes"
				| b"include_str"
				| b"line" | b"matches"
				| b"module_path"
				| b"option_env"
				| b"panic" | b"println"
				| b"todo" | b"unimplemented"
				| b"unreachable"
				| b"vec" | b"write"
				| b"writeln"
		);
	builtin.then(|| {
		external_std_item(
			scope,
			&[(kinds::PATH, b"macros".as_slice()), (kinds::MACRO, name)],
		)
	})
}

fn common_std_method_target(scope: &Moniker, name: &[u8]) -> Moniker {
	external_std_item(
		scope,
		&[(kinds::PATH, b"prelude".as_slice()), (kinds::METHOD, name)],
	)
}

fn is_common_std_method(name: &[u8]) -> bool {
	is_common_iterator_method(name)
		|| is_common_collection_method(name)
		|| is_common_text_method(name)
		|| is_common_result_option_method(name)
		|| is_common_io_path_time_method(name)
		|| is_common_misc_method(name)
}

fn is_common_iterator_method(name: &[u8]) -> bool {
	matches!(
		name,
		b"all"
			| b"any" | b"cloned"
			| b"collect"
			| b"count"
			| b"copied"
			| b"enumerate"
			| b"filter"
			| b"filter_map"
			| b"find" | b"find_map"
			| b"flat_map"
			| b"flatten"
			| b"into_iter"
			| b"iter" | b"iter_mut"
			| b"map" | b"map_err"
			| b"map_or"
			| b"max" | b"min"
			| b"nth" | b"rev"
			| b"rposition"
			| b"sum" | b"take"
			| b"zip"
	)
}

fn is_common_collection_method(name: &[u8]) -> bool {
	matches!(
		name,
		b"as_mut"
			| b"as_ptr"
			| b"as_ref"
			| b"as_slice"
			| b"binary_search"
			| b"clear"
			| b"contains_key"
			| b"copy_from_slice"
			| b"entry"
			| b"extend"
			| b"extend_from_slice"
			| b"first"
			| b"get" | b"get_mut"
			| b"insert"
			| b"is_empty"
			| b"join" | b"keys"
			| b"last" | b"last_mut"
			| b"len" | b"or_default"
			| b"or_insert"
			| b"pop" | b"push"
			| b"push_str"
			| b"remove"
			| b"retain"
			| b"sort_by"
			| b"split_first"
			| b"values"
			| b"windows"
	)
}

fn is_common_text_method(name: &[u8]) -> bool {
	matches!(
		name,
		b"bytes"
			| b"char_indices"
			| b"chars"
			| b"ends_with"
			| b"is_ascii_alphabetic"
			| b"is_ascii_alphanumeric"
			| b"is_ascii_lowercase"
			| b"is_ascii_uppercase"
			| b"is_ascii_whitespace"
			| b"lines"
			| b"repeat"
			| b"replace"
			| b"rsplit"
			| b"split"
			| b"split_once"
			| b"starts_with"
			| b"strip_prefix"
			| b"strip_suffix"
			| b"to_ascii_lowercase"
			| b"to_str"
			| b"trim" | b"trim_end_matches"
	)
}

fn is_common_result_option_method(name: &[u8]) -> bool {
	matches!(
		name,
		b"and_then"
			| b"as_deref"
			| b"expect"
			| b"get_or_insert"
			| b"is_none"
			| b"is_ok"
			| b"is_some"
			| b"is_some_and"
			| b"ok" | b"ok_or"
			| b"ok_or_else"
			| b"or_else"
			| b"then" | b"then_some"
			| b"then_with"
			| b"unwrap"
			| b"unwrap_err"
			| b"unwrap_or"
			| b"unwrap_or_default"
			| b"unwrap_or_else"
	)
}

fn is_common_io_path_time_method(name: &[u8]) -> bool {
	matches!(
		name,
		b"as_nanos"
			| b"as_os_str"
			| b"canonicalize"
			| b"display"
			| b"elapsed"
			| b"exists"
			| b"file_name"
			| b"is_absolute"
			| b"is_dir"
			| b"is_file"
			| b"lock" | b"path"
			| b"read" | b"to_path_buf"
			| b"to_string_lossy"
			| b"write"
			| b"write_all"
	)
}

fn is_common_misc_method(name: &[u8]) -> bool {
	matches!(
		name,
		b"add_modifier"
			| b"as_table"
			| b"borrow"
			| b"borrow_mut"
			| b"clamp"
			| b"clone"
			| b"cmp" | b"env"
			| b"get_or_init"
			| b"into" | b"into_owned"
			| b"saturating_add"
			| b"saturating_sub"
			| b"to_le_bytes"
			| b"to_string"
			| b"to_vec"
			| b"try_into"
	)
}

fn local_symbol_target(scope: &Moniker, path: &[Vec<u8>]) -> Moniker {
	let (base, rest) = local_import_base(scope, path);
	let mut builder = MonikerBuilder::from_view(base.as_view());
	append_symbol_path(&mut builder, rest);
	builder.build()
}

fn local_crate_path_target(scope: &Moniker, path: &[Vec<u8>]) -> Moniker {
	let root = project_source_root(scope);
	let mut builder = MonikerBuilder::from_view(root.as_view());
	append_symbol_path(&mut builder, path);
	builder.build()
}

fn local_relative_path_target(scope: &Moniker, path: &[Vec<u8>]) -> Moniker {
	let mut builder = MonikerBuilder::from_view(scope.as_view());
	append_symbol_path(&mut builder, path);
	builder.build()
}

fn local_module_target(scope: &Moniker, path: &[Vec<u8>]) -> Moniker {
	let (base, rest) = local_import_base(scope, path);
	let mut builder = MonikerBuilder::from_view(base.as_view());
	append_module_path(&mut builder, rest);
	builder.build()
}

fn local_import_base<'a>(scope: &Moniker, path: &'a [Vec<u8>]) -> (Moniker, &'a [Vec<u8>]) {
	match path.first().map(Vec::as_slice) {
		Some(b"crate") => (project_source_root(scope), &path[1..]),
		Some(b"self") => (scope.clone(), &path[1..]),
		Some(b"super") => (
			rust_parent_module(scope).unwrap_or_else(|| scope.clone()),
			&path[1..],
		),
		_ => (unqualified_import_base(scope), path),
	}
}

fn unqualified_import_base(scope: &Moniker) -> Moniker {
	let segments = scope.as_view().segments().collect::<Vec<_>>();
	let Some((last_index, last)) = segments.iter().enumerate().next_back() else {
		return scope.clone();
	};
	if last.kind == kinds::MODULE && last.name == b"mod" {
		let mut builder = MonikerBuilder::from_view(scope.as_view());
		builder.truncate(last_index);
		return builder.build();
	}
	scope.clone()
}

fn append_symbol_path(builder: &mut MonikerBuilder, pieces: &[Vec<u8>]) {
	let n = pieces.len();
	for (index, piece) in pieces.iter().enumerate() {
		let kind = if n == 1 || index == n - 1 {
			kinds::PATH
		} else {
			kinds::MODULE
		};
		builder.segment(kind, piece);
	}
}

fn append_module_path(builder: &mut MonikerBuilder, pieces: &[Vec<u8>]) {
	for piece in pieces {
		builder.segment(kinds::MODULE, piece);
	}
}

fn project_source_root(scope: &Moniker) -> Moniker {
	let view = scope.as_view();
	let root_depth = view
		.segments()
		.enumerate()
		.filter_map(|(index, segment)| {
			(segment.kind == kinds::DIR && segment.name == b"src").then_some(index + 1)
		})
		.last()
		.unwrap_or(1);
	let mut builder = MonikerBuilder::from_view(view);
	builder.truncate(root_depth);
	builder.build()
}

fn rust_parent_module(scope: &Moniker) -> Option<Moniker> {
	let segments = scope.as_view().segments().collect::<Vec<_>>();
	let (module_index, module_segment) = segments
		.iter()
		.enumerate()
		.rev()
		.find(|(_, segment)| segment.kind == kinds::MODULE)?;
	if module_index == 0 || segments[module_index - 1].kind != kinds::DIR {
		return scope.parent();
	}
	if module_segment.name == b"mod" || segments[module_index - 1].name == b"src" {
		return None;
	}
	let parent_dir = segments[module_index - 1];
	let mut builder = MonikerBuilder::from_view(scope.as_view());
	builder.truncate(module_index - 1);
	builder.segment(kinds::MODULE, parent_dir.name);
	Some(builder.build())
}
