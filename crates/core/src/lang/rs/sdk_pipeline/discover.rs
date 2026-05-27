use tree_sitter::Node;

use crate::core::code_graph::Position;
use crate::core::moniker::Moniker;
use crate::lang::callable::{CallableSlot, extend_segment};
use crate::lang::sdk::{
	DiscoveredDef, ImportLeafKind, Namespace, ResolvedRef, flatten_import_tree,
	import_leaf_binding_name,
};
use crate::lang::tree_util::{node_position, node_slice};

use super::super::kinds;
use super::defs::{
	CallableDefInput, DefEnv, callable_def, enum_constant_def, inferred_struct_def,
	local_binding_def, nested_type_def, proptest_def, reexport_path_def, simple_def,
	synthetic_enum_constant_def, synthetic_lang_enum_def,
};
use super::imports::import_tree;
use super::refs::{
	ImportedSymbol, RefEnv, attribute_refs, expand_import, macro_call_ref, read_refs,
	trait_refs_from_node, type_parameters, type_refs_from_signature, type_refs_from_type_node,
};
use super::syntax::{
	is_test_function, language_macro_variants, named_children, should_skip_binding,
};

pub(super) struct DiscoveredRustFile {
	pub root: Moniker,
	pub defs: Vec<DiscoveredDef>,
	pub refs: Vec<ResolvedRef>,
}

pub(super) struct RustDiscover<'src> {
	root: Moniker,
	source: &'src [u8],
	deep: bool,
	defs: Vec<DiscoveredDef>,
	refs: Vec<ResolvedRef>,
	imported_symbols: Vec<ImportedSymbol>,
	wildcard_imports: Vec<(Moniker, Moniker)>,
}

impl<'src> RustDiscover<'src> {
	pub fn run(
		root: Moniker,
		source: &'src [u8],
		deep: bool,
		root_node: Node<'_>,
	) -> DiscoveredRustFile {
		let mut discover = Self {
			root: root.clone(),
			source,
			deep,
			defs: Vec::new(),
			refs: Vec::new(),
			imported_symbols: Vec::new(),
			wildcard_imports: Vec::new(),
		};
		walk_items(&mut discover, root_node, &root, false);
		collect_refs(&mut discover, root_node, &root, false);
		DiscoveredRustFile {
			root,
			defs: discover.defs,
			refs: discover.refs,
		}
	}

	fn push_def(&mut self, def: DiscoveredDef) {
		if !self
			.defs
			.iter()
			.any(|existing| existing.moniker == def.moniker)
		{
			self.defs.push(def);
		}
	}

	fn push_ref(&mut self, reference: ResolvedRef) {
		if !self
			.refs
			.iter()
			.any(|existing| same_ref(existing, &reference))
		{
			self.refs.push(reference);
		}
	}

	fn extend_refs(&mut self, refs: Vec<ResolvedRef>) {
		for reference in refs {
			self.push_ref(reference);
		}
	}

	fn def_env(&self) -> DefEnv<'_> {
		DefEnv {
			root: &self.root,
			source: self.source,
		}
	}

	fn ref_env(&self) -> RefEnv<'_> {
		RefEnv {
			source: self.source,
			defs: &self.defs,
			imported_symbols: &self.imported_symbols,
			wildcard_imports: &self.wildcard_imports,
		}
	}
}

fn walk_items(state: &mut RustDiscover<'_>, node: Node<'_>, scope: &Moniker, trait_impl: bool) {
	for child in named_children(node) {
		visit_item(state, child, scope, trait_impl);
	}
}

fn visit_item(state: &mut RustDiscover<'_>, node: Node<'_>, scope: &Moniker, trait_impl: bool) {
	match item_kind(node.kind()) {
		ItemKind::Ignore => {}
		ItemKind::Simple(kind, namespace) => {
			push_simple_def(state, node, scope, kind, namespace);
		}
		ItemKind::Enum => enum_def(state, node, scope),
		ItemKind::Trait => trait_def(state, node, scope),
		ItemKind::Function => function_def(state, node, scope, trait_impl),
		ItemKind::Use => use_declaration(state, node, scope),
		ItemKind::Attribute => {}
		ItemKind::Impl => impl_items(state, node, scope),
		ItemKind::Module => module_def(state, node, scope),
		ItemKind::MacroInvocation => macro_invocation(state, node, scope),
		ItemKind::Recurse => walk_items(state, node, scope, trait_impl),
	}
}

fn push_simple_def(
	state: &mut RustDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	kind: &'static [u8],
	namespace: Namespace,
) -> Option<Moniker> {
	let def = simple_def(state.def_env(), node, scope, kind, namespace, false)?;
	let moniker = def.moniker.clone();
	state.push_def(def);
	Some(moniker)
}

fn enum_def(state: &mut RustDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(enum_moniker) = push_simple_def(state, node, scope, kinds::ENUM, Namespace::Type)
	else {
		return;
	};
	if let Some(body) = node.child_by_field_name("body") {
		for child in named_children(body).filter(|child| child.kind() == "enum_variant") {
			if let Some(name_node) = child.child_by_field_name("name") {
				state.push_def(enum_constant_def(
					state.def_env(),
					child,
					name_node,
					&enum_moniker,
				));
			}
		}
	}
}

fn trait_def(state: &mut RustDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(trait_moniker) = push_simple_def(state, node, scope, kinds::TRAIT, Namespace::Type)
	else {
		return;
	};
	if let Some(body) = node.child_by_field_name("body") {
		walk_items(state, body, &trait_moniker, false);
	}
}

fn function_def(state: &mut RustDiscover<'_>, node: Node<'_>, scope: &Moniker, trait_impl: bool) {
	let Some(name_node) = node.child_by_field_name("name") else {
		return;
	};
	let slots = function_param_slots(node, state.source);
	let kind = callable_kind(node, scope, state.source);
	let def = callable_def(
		state.def_env(),
		CallableDefInput {
			node,
			name_node,
			scope,
			kind,
			slots: &slots,
			trait_impl,
		},
	);
	let function = def.moniker.clone();
	state.push_def(def);
	if state.deep {
		param_defs(state, node, &function);
		if let Some(body) = node.child_by_field_name("body") {
			local_defs(state, body, &function);
		}
	}
}

fn use_declaration(state: &mut RustDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	if !has_visibility(node) {
		return;
	}
	let Some(argument) = node.child_by_field_name("argument") else {
		return;
	};
	let Some(tree) = import_tree(argument, state.source) else {
		return;
	};
	for leaf in flatten_import_tree(&tree) {
		if leaf.kind == ImportLeafKind::Wildcard {
			continue;
		}
		let Some(name) = import_leaf_binding_name(&leaf) else {
			continue;
		};
		state.push_def(reexport_path_def(state.def_env(), node, scope, name));
	}
}

fn impl_items(state: &mut RustDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(type_node) = node.child_by_field_name("type") else {
		return;
	};
	let Some(type_name) = impl_type_name(type_node, state.source) else {
		return;
	};
	let target = find_local_type(&state.defs, scope, type_name.as_bytes())
		.unwrap_or_else(|| extend_segment(&state.root, kinds::STRUCT, type_name.as_bytes()));
	if !state.defs.iter().any(|def| def.moniker == target) {
		state.push_def(inferred_struct_def(
			state.def_env(),
			node,
			&target,
			type_name,
		));
	}
	if let Some(body) = node.child_by_field_name("body") {
		walk_items(
			state,
			body,
			&target,
			node.child_by_field_name("trait").is_some(),
		);
	}
}

fn module_def(state: &mut RustDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(def) = simple_def(
		state.def_env(),
		node,
		scope,
		kinds::MODULE,
		Namespace::Module,
		false,
	) else {
		return;
	};
	let module = def.moniker.clone();
	state.push_def(def);
	if let Some(body) = node.child_by_field_name("body") {
		walk_items(state, body, &module, false);
	}
}

fn macro_invocation(state: &mut RustDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let text = node.utf8_text(state.source).unwrap_or_default();
	if text.trim_start().starts_with("proptest!") {
		proptest_macro(state, node, scope);
		return;
	}
	if !text.trim_start().starts_with("define_languages!") {
		return;
	}
	let enum_moniker = extend_segment(scope, kinds::ENUM, b"Lang");
	state.push_def(synthetic_lang_enum_def(node, scope, &enum_moniker));
	for variant in language_macro_variants(text) {
		state.push_def(synthetic_enum_constant_def(
			node,
			&enum_moniker,
			variant.into_bytes(),
		));
	}
}

fn proptest_macro(state: &mut RustDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let text = node.utf8_text(state.source).unwrap_or_default();
	let Some(test_pos) = text.find("fn ") else {
		return;
	};
	let rest = &text[test_pos + 3..];
	let Some((name, after_name)) = rest.split_once('(') else {
		return;
	};
	let Some((param, _)) = after_name.split_once(" in ") else {
		return;
	};
	state.push_def(proptest_def(
		node,
		scope,
		name.trim().as_bytes(),
		param.trim().as_bytes(),
	));
}

fn collect_refs(state: &mut RustDiscover<'_>, node: Node<'_>, scope: &Moniker, trait_impl: bool) {
	for child in named_children(node) {
		collect_item_refs(state, child, scope, trait_impl);
	}
}

fn collect_item_refs(
	state: &mut RustDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	trait_impl: bool,
) {
	match item_kind(node.kind()) {
		ItemKind::Ignore => {}
		ItemKind::Function => collect_function_refs(state, node, scope, trait_impl),
		ItemKind::Use => collect_use_refs(state, node, scope),
		ItemKind::Attribute => state.extend_refs(attribute_refs(state.ref_env(), node, scope)),
		ItemKind::Impl => collect_impl_refs(state, node, scope),
		ItemKind::Module => collect_module_refs(state, node, scope),
		ItemKind::Trait => collect_trait_refs(state, node, scope),
		ItemKind::MacroInvocation => collect_macro_refs(state, node, scope),
		ItemKind::Enum => collect_enum_refs(state, node, scope),
		ItemKind::Simple(kind, _) if kind == kinds::STRUCT => {
			collect_struct_refs(state, node, scope)
		}
		ItemKind::Recurse | ItemKind::Simple(_, _) => collect_refs(state, node, scope, trait_impl),
	}
}

fn collect_struct_refs(state: &mut RustDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(struct_moniker) = named_def_moniker(state, node, scope, kinds::STRUCT) else {
		return;
	};
	state.extend_refs(attribute_refs(state.ref_env(), node, &struct_moniker));
	collect_field_type_refs(state, node, &struct_moniker);
	collect_refs(state, node, &struct_moniker, false);
}

fn collect_field_type_refs(state: &mut RustDiscover<'_>, node: Node<'_>, source: &Moniker) {
	let type_params = type_parameters(node, state.source);
	collect_field_type_refs_with_params(state, node, source, &type_params);
}

fn collect_field_type_refs_with_params(
	state: &mut RustDiscover<'_>,
	node: Node<'_>,
	source: &Moniker,
	type_params: &[Vec<u8>],
) {
	for child in named_children(node) {
		if child.kind() == "field_declaration"
			&& let Some(ty) = child.child_by_field_name("type")
		{
			state.extend_refs(type_refs_from_type_node(
				state.ref_env(),
				ty,
				source,
				type_params,
			));
			continue;
		}
		collect_field_type_refs_with_params(state, child, source, type_params);
	}
}

fn collect_enum_refs(state: &mut RustDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(enum_moniker) = named_def_moniker(state, node, scope, kinds::ENUM) else {
		return;
	};
	if let Some(body) = node.child_by_field_name("body") {
		collect_refs(state, body, &enum_moniker, false);
	}
}

fn collect_function_refs(
	state: &mut RustDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	trait_impl: bool,
) {
	let Some(function) = function_moniker(state, node, scope, trait_impl) else {
		return;
	};
	state.extend_refs(type_refs_from_signature(state.ref_env(), node, &function));
	if state.deep
		&& let Some(body) = node.child_by_field_name("body")
	{
		collect_body_use_refs(state, body, &function);
		state.extend_refs(read_refs(state.ref_env(), node, body, &function));
		collect_local_ref_items(state, body, &function);
	}
}

fn collect_body_use_refs(state: &mut RustDiscover<'_>, node: Node<'_>, function: &Moniker) {
	for child in named_children(node) {
		if child.kind() == "use_declaration" {
			collect_use_refs(state, child, function);
			continue;
		}
		if matches!(local_item_kind(child.kind()), LocalItemKind::NestedFunction) {
			continue;
		}
		collect_body_use_refs(state, child, function);
	}
}

fn collect_local_ref_items(state: &mut RustDiscover<'_>, node: Node<'_>, function: &Moniker) {
	for child in named_children(node) {
		match local_item_kind(child.kind()) {
			LocalItemKind::NestedFunction => collect_function_refs(state, child, function, false),
			LocalItemKind::NestedType(kind) if kind == kinds::STRUCT => {
				collect_struct_refs(state, child, function)
			}
			LocalItemKind::Recurse | LocalItemKind::Let | LocalItemKind::For => {
				if child.kind() == "attribute_item" {
					state.extend_refs(attribute_refs(state.ref_env(), child, function));
				}
				collect_local_ref_items(state, child, function)
			}
			LocalItemKind::NestedType(_) => {}
		}
	}
}

fn collect_use_refs(state: &mut RustDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(argument) = node.child_by_field_name("argument") else {
		return;
	};
	let mut expansion = expand_import(state.ref_env(), argument, scope);
	if has_visibility(node) {
		for reference in &mut expansion.refs {
			if reference.kind == kinds::IMPORTS_SYMBOL {
				reference.kind = kinds::REEXPORTS;
			}
		}
		expansion
			.refs
			.retain(|reference| reference.kind != kinds::IMPORTS_MODULE);
	}
	state.imported_symbols.extend(expansion.symbols);
	state.wildcard_imports.extend(
		expansion
			.wildcard_modules
			.into_iter()
			.map(|module| (scope.clone(), module)),
	);
	state.extend_refs(expansion.refs);
}

fn collect_impl_refs(state: &mut RustDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(type_node) = node.child_by_field_name("type") else {
		return;
	};
	let Some(type_name) = impl_type_name(type_node, state.source) else {
		return;
	};
	let target = find_local_type(&state.defs, scope, type_name.as_bytes())
		.unwrap_or_else(|| extend_segment(&state.root, kinds::STRUCT, type_name.as_bytes()));
	if let Some(trait_node) = node.child_by_field_name("trait") {
		state.extend_refs(trait_refs_from_node(
			state.ref_env(),
			trait_node,
			&target,
			kinds::IMPLEMENTS,
		));
	}
	if let Some(body) = node.child_by_field_name("body") {
		collect_refs(
			state,
			body,
			&target,
			node.child_by_field_name("trait").is_some(),
		);
	}
}

fn collect_module_refs(state: &mut RustDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(module) = module_moniker(state, node, scope) else {
		return;
	};
	if let Some(body) = node.child_by_field_name("body") {
		collect_refs(state, body, &module, false);
	}
}

fn collect_trait_refs(state: &mut RustDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(trait_moniker) = named_def_moniker(state, node, scope, kinds::TRAIT) else {
		return;
	};
	if let Some(bounds) = node.child_by_field_name("bounds") {
		state.extend_refs(trait_refs_from_node(
			state.ref_env(),
			bounds,
			&trait_moniker,
			kinds::EXTENDS,
		));
	}
	if let Some(body) = node.child_by_field_name("body") {
		collect_refs(state, body, &trait_moniker, false);
	}
}

fn collect_macro_refs(state: &mut RustDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	if let Some(reference) = macro_call_ref(state.ref_env(), scope, node) {
		state.push_ref(reference);
	}
}

fn function_moniker(
	state: &RustDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	trait_impl: bool,
) -> Option<Moniker> {
	let name_node = node.child_by_field_name("name")?;
	let slots = function_param_slots(node, state.source);
	let kind = callable_kind(node, scope, state.source);
	Some(
		callable_def(
			state.def_env(),
			CallableDefInput {
				node,
				name_node,
				scope,
				kind,
				slots: &slots,
				trait_impl,
			},
		)
		.moniker,
	)
}

fn function_param_slots(node: Node<'_>, source: &[u8]) -> Vec<CallableSlot> {
	let Some(params) = node.child_by_field_name("parameters") else {
		return Vec::new();
	};
	let mut out = Vec::new();
	let mut cursor = params.walk();
	for child in params.named_children(&mut cursor) {
		match child.kind() {
			"parameter" => {
				let r#type = child
					.child_by_field_name("type")
					.and_then(|n| n.utf8_text(source).ok())
					.map(crate::lang::callable::normalize_type_text)
					.unwrap_or_default();
				let name = child
					.child_by_field_name("pattern")
					.filter(|p| p.kind() == "identifier")
					.and_then(|p| p.utf8_text(source).ok())
					.map(|s| s.as_bytes().to_vec())
					.unwrap_or_default();
				out.push(CallableSlot { name, r#type });
			}
			"variadic_parameter" => out.push(CallableSlot {
				name: Vec::new(),
				r#type: b"...".to_vec(),
			}),
			"self_parameter" => {}
			_ => {}
		}
	}
	out
}

fn impl_type_name<'a>(node: Node<'a>, source: &'a [u8]) -> Option<&'a str> {
	let target = match node.kind() {
		"generic_type" => node.child_by_field_name("type")?,
		_ => node,
	};
	match target.kind() {
		"type_identifier" | "primitive_type" => target.utf8_text(source).ok(),
		"scoped_type_identifier" => target
			.child_by_field_name("name")
			.and_then(|n| n.utf8_text(source).ok()),
		_ => target.utf8_text(source).ok(),
	}
}

fn module_moniker(state: &RustDiscover<'_>, node: Node<'_>, scope: &Moniker) -> Option<Moniker> {
	simple_def(
		state.def_env(),
		node,
		scope,
		kinds::MODULE,
		Namespace::Module,
		false,
	)
	.map(|def| def.moniker)
}

fn named_def_moniker(
	state: &RustDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	kind: &'static [u8],
) -> Option<Moniker> {
	simple_def(state.def_env(), node, scope, kind, Namespace::Type, false).map(|def| def.moniker)
}

fn param_defs(state: &mut RustDiscover<'_>, node: Node<'_>, function: &Moniker) {
	let Some(params) = node.child_by_field_name("parameters") else {
		return;
	};
	for child in named_children(params) {
		if let Some(def) = param_def(state.source, child, function) {
			state.push_def(def);
		}
	}
}

fn param_def(source: &[u8], node: Node<'_>, function: &Moniker) -> Option<DiscoveredDef> {
	let name_node = match node.kind() {
		"parameter" => node.child_by_field_name("pattern"),
		"self_parameter" => Some(node),
		_ => None,
	}?;
	let name = match name_node.kind() {
		"identifier" | "self_parameter" => node_slice(name_node, source),
		_ => return None,
	};
	let name = if name_node.kind() == "self_parameter" {
		b"self".as_slice()
	} else {
		name
	};
	(!should_skip_binding(name)).then(|| {
		local_binding_def(
			function,
			kinds::PARAM,
			name.to_vec(),
			Some(node_position(node)),
		)
	})
}

fn has_visibility(node: Node<'_>) -> bool {
	let mut cursor = node.walk();
	node.children(&mut cursor)
		.any(|child| child.kind() == "visibility_modifier")
}

fn local_defs(state: &mut RustDiscover<'_>, node: Node<'_>, function: &Moniker) {
	for child in named_children(node) {
		match local_item_kind(child.kind()) {
			LocalItemKind::Let => let_defs(state, child, function),
			LocalItemKind::For => for_defs(state, child, function),
			LocalItemKind::NestedType(kind) => nested_type_def_for(state, child, function, kind),
			LocalItemKind::NestedFunction => function_def(state, child, function, false),
			LocalItemKind::Recurse => local_defs(state, child, function),
		}
	}
}

fn let_defs(state: &mut RustDiscover<'_>, node: Node<'_>, function: &Moniker) {
	if let Some(pattern) = node.child_by_field_name("pattern") {
		pattern_defs(state, pattern, function, Some(node_position(node)));
	}
	if let Some(value) = node.child_by_field_name("value") {
		local_defs(state, value, function);
	}
}

fn for_defs(state: &mut RustDiscover<'_>, node: Node<'_>, function: &Moniker) {
	if let Some(pattern) = node.child_by_field_name("pattern") {
		pattern_defs(state, pattern, function, Some(node_position(node)));
	}
	local_defs(state, node, function);
}

fn nested_type_def_for(
	state: &mut RustDiscover<'_>,
	node: Node<'_>,
	function: &Moniker,
	kind: &'static [u8],
) {
	if let Some(name_node) = node.child_by_field_name("name") {
		state.push_def(nested_type_def(
			state.def_env(),
			node,
			function,
			kind,
			name_node,
		));
	}
}

fn pattern_defs(
	state: &mut RustDiscover<'_>,
	pattern: Node<'_>,
	function: &Moniker,
	position: Option<Position>,
) {
	if pattern.kind() == "identifier" || pattern.kind() == "shorthand_field_identifier" {
		pattern_binding(state, pattern, function, position);
		return;
	}
	for child in named_children(pattern) {
		pattern_defs(state, child, function, position);
	}
}

fn pattern_binding(
	state: &mut RustDiscover<'_>,
	pattern: Node<'_>,
	function: &Moniker,
	position: Option<Position>,
) {
	let name = node_slice(pattern, state.source);
	if should_skip_binding(name) {
		return;
	}
	state.push_def(local_binding_def(
		function,
		kinds::LOCAL,
		name.to_vec(),
		position,
	));
}

fn find_local_type(defs: &[DiscoveredDef], scope: &Moniker, name: &[u8]) -> Option<Moniker> {
	[kinds::ENUM, kinds::TRAIT, kinds::TYPE, kinds::STRUCT]
		.into_iter()
		.map(|kind| extend_segment(scope, kind, name))
		.find(|candidate| defs.iter().any(|def| def.moniker == *candidate))
}

enum ItemKind {
	Ignore,
	Simple(&'static [u8], Namespace),
	Enum,
	Trait,
	Function,
	Use,
	Attribute,
	Impl,
	Module,
	MacroInvocation,
	Recurse,
}

fn item_kind(kind: &str) -> ItemKind {
	match kind {
		"line_comment" | "block_comment" => ItemKind::Ignore,
		"struct_item" => ItemKind::Simple(kinds::STRUCT, Namespace::Type),
		"type_item" => ItemKind::Simple(kinds::TYPE, Namespace::Type),
		"const_item" => ItemKind::Simple(kinds::CONST, Namespace::Value),
		"static_item" => ItemKind::Simple(kinds::STATIC, Namespace::Value),
		"macro_definition" => ItemKind::Simple(kinds::MACRO, Namespace::Macro),
		"enum_item" => ItemKind::Enum,
		"trait_item" => ItemKind::Trait,
		"function_item" | "function_signature_item" => ItemKind::Function,
		"use_declaration" => ItemKind::Use,
		"attribute_item" => ItemKind::Attribute,
		"impl_item" => ItemKind::Impl,
		"mod_item" => ItemKind::Module,
		"macro_invocation" => ItemKind::MacroInvocation,
		_ => ItemKind::Recurse,
	}
}

fn callable_kind(node: Node<'_>, scope: &Moniker, source: &[u8]) -> &'static [u8] {
	if is_test_function(node, source) {
		kinds::TEST
	} else if scope.last_kind().as_deref() == Some(kinds::STRUCT)
		|| scope.last_kind().as_deref() == Some(kinds::ENUM)
		|| scope.last_kind().as_deref() == Some(kinds::TRAIT)
	{
		kinds::METHOD
	} else {
		kinds::FN
	}
}

enum LocalItemKind {
	Let,
	For,
	NestedType(&'static [u8]),
	NestedFunction,
	Recurse,
}

fn local_item_kind(kind: &str) -> LocalItemKind {
	match kind {
		"let_declaration" => LocalItemKind::Let,
		"for_expression" => LocalItemKind::For,
		"struct_item" => LocalItemKind::NestedType(kinds::STRUCT),
		"enum_item" => LocalItemKind::NestedType(kinds::ENUM),
		"function_item" => LocalItemKind::NestedFunction,
		_ => LocalItemKind::Recurse,
	}
}

fn same_ref(left: &ResolvedRef, right: &ResolvedRef) -> bool {
	left.source == right.source
		&& left.target == right.target
		&& left.kind == right.kind
		&& left.position == right.position
		&& left.confidence == right.confidence
		&& left.hints == right.hints
}
