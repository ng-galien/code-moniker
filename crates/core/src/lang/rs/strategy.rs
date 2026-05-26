// code-moniker: ignore-file[smell-feature-envy-local, smell-data-clumps-param-names, smell-god-type-local-metrics, smell-large-type]
// TODO(smell): split Rust Strategy into classification, impl/use handling, type/call resolution, and graph emission phases before enabling these guardrails here.
use std::cell::{Cell, RefCell};
use std::collections::HashSet;

use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, DefAttrs, RefAttrs};
use crate::core::moniker::query::bare_callable_name;
use crate::core::moniker::{Moniker, MonikerBuilder};

use crate::lang::callable::{callable_segment_slots, extend_callable_slots, extend_segment};
use crate::lang::strategy::{LangStrategy, NodeShape, Symbol};
use crate::lang::tree_util::{node_position, node_slice};

use super::canonicalize::{closure_param_slots, function_param_slots, impl_type_name};
use super::kinds;

use std::collections::HashMap;

pub(super) struct Strategy<'src> {
	pub(super) module: Moniker,
	pub(super) source_bytes: &'src [u8],
	pub(super) deep: bool,
	pub(super) local_mods: HashSet<String>,
	pub(super) local_scope: RefCell<Vec<HashSet<Vec<u8>>>>,
	pub(super) local_types: RefCell<Vec<HashMap<Vec<u8>, Moniker>>>,
	pub(super) type_params: RefCell<Vec<HashSet<Vec<u8>>>>,
	pub(super) callable_table: HashMap<(Moniker, Vec<u8>), Vec<u8>>,
	pub(super) type_table: HashMap<Vec<u8>, Moniker>,
	pub(super) in_trait_impl: Cell<bool>,
	pub(super) imported_modules: RefCell<HashSet<Moniker>>,
	pub(super) imported_symbols: RefCell<HashMap<Vec<u8>, Moniker>>,
	pub(super) imported_wildcard_modules: RefCell<HashMap<Moniker, Vec<Moniker>>>,
}

impl<'a> LangStrategy for Strategy<'a> {
	fn classify<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut CodeGraph,
	) -> NodeShape<'src> {
		match node.kind() {
			"line_comment" | "block_comment" => NodeShape::Annotation {
				kind: kinds::COMMENT,
			},
			"struct_item" => self.classify_simple_def(node, scope, source, kinds::STRUCT),
			"enum_item" => self.classify_simple_def(node, scope, source, kinds::ENUM),
			"type_item" => self.classify_simple_def(node, scope, source, kinds::TYPE),
			"const_item" => self.classify_simple_def(node, scope, source, kinds::CONST),
			"static_item" => self.classify_simple_def(node, scope, source, kinds::STATIC),
			"trait_item" => self.classify_trait(node, scope, source, graph),
			"function_item" | "function_signature_item" => {
				self.classify_function(node, scope, source)
			}
			"impl_item" => {
				self.handle_impl(node, scope, source, graph);
				NodeShape::Skip
			}
			"use_declaration" => {
				self.handle_use(node, scope, graph);
				NodeShape::Skip
			}
			"let_declaration" => {
				self.handle_let(node, scope, source, graph);
				NodeShape::Skip
			}
			"for_expression" => {
				self.handle_for(node, scope, graph);
				NodeShape::Skip
			}
			"call_expression" => {
				self.handle_call(node, scope, graph);
				NodeShape::Skip
			}
			"macro_definition" => {
				self.handle_macro_definition(node, scope, source, graph);
				NodeShape::Skip
			}
			"macro_invocation" => {
				self.handle_macro(node, scope, graph);
				NodeShape::Skip
			}
			"struct_expression" => {
				self.handle_struct_literal(node, scope, graph);
				NodeShape::Skip
			}
			"field_declaration" => {
				if let Some(ty) = node.child_by_field_name("type") {
					self.emit_uses_type_walk(ty, scope, graph);
				}
				NodeShape::Skip
			}
			"attribute_item" => {
				self.handle_attribute(node, scope, graph);
				NodeShape::Skip
			}
			"identifier" => {
				self.handle_identifier_read(node, scope, graph);
				NodeShape::Skip
			}
			"scoped_identifier" => {
				self.handle_scoped_read(node, scope, graph);
				NodeShape::Skip
			}
			"mod_item" => self.classify_inline_module(node, scope, source),
			_ => NodeShape::Recurse,
		}
	}

	fn before_body(
		&self,
		node: Node<'_>,
		kind: &[u8],
		moniker: &Moniker,
		_source: &[u8],
		graph: &mut CodeGraph,
	) {
		if kind == kinds::ENUM {
			self.emit_enum_constants(node, moniker, graph);
			return;
		}
		if kind != kinds::FN && kind != kinds::METHOD && kind != kinds::TEST {
			return;
		}
		if let Some(params) = node.child_by_field_name("parameters") {
			self.emit_param_type_refs(params, moniker, graph);
			if self.deep {
				self.emit_params(params, moniker, graph);
			}
		}
		if let Some(rt) = node.child_by_field_name("return_type") {
			self.emit_uses_type_walk(rt, moniker, graph);
		}
	}

	fn after_body(&self, kind: &[u8], _moniker: &Moniker) {
		if kind == kinds::FN || kind == kinds::METHOD || kind == kinds::TEST {
			self.pop_local_scope();
		}
	}

	fn on_symbol_emitted(
		&self,
		node: Node<'_>,
		sym_kind: &[u8],
		sym_moniker: &Moniker,
		source: &[u8],
		graph: &mut CodeGraph,
	) {
		if !matches!(sym_kind, kinds::FN | kinds::METHOD | kinds::TEST) {
			return;
		}
		let Some(name_node) = node.child_by_field_name("name") else {
			return;
		};
		let name = node_slice(name_node, source);
		let arity = function_call_arity(node, source);
		graph.set_def_call_metadata(sym_moniker, name, arity);
	}
}

impl<'src_lang> Strategy<'src_lang> {
	fn classify_simple_def<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		kind: &'static [u8],
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let moniker = extend_segment(scope, kind, name);
		self.push_type_params_from(node, source);

		NodeShape::Symbol(Symbol {
			moniker,
			kind,
			visibility: self.visibility_of(node, scope, source),
			signature: None,
			body: Some(node),
			position: node_position(node),
			annotated_by: Vec::new(),
		})
	}

	fn classify_trait<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut CodeGraph,
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let moniker = extend_segment(scope, kinds::TRAIT, name);
		self.push_type_params_from(node, source);

		let mut annotated_by = Vec::new();
		if let Some(bounds) = node.child_by_field_name("bounds") {
			self.collect_trait_bounds_extends(bounds, &mut annotated_by);
		}
		let _ = graph;

		NodeShape::Symbol(Symbol {
			moniker,
			kind: kinds::TRAIT,
			visibility: self.visibility_of(node, scope, source),
			signature: None,
			body: node.child_by_field_name("body"),
			position: node_position(node),
			annotated_by,
		})
	}

	fn classify_function<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let is_test = has_rust_attribute(node, source, "test");
		let kind = if is_test {
			kinds::TEST
		} else if is_type_scope(scope) {
			kinds::METHOD
		} else {
			kinds::FN
		};
		let slots = function_param_slots(node, source);
		let moniker = extend_callable_slots(scope, kind, name, &slots);
		let signature = if is_test {
			Some(test_signature(
				b"rust-test",
				name,
				has_rust_attribute(node, source, "ignore"),
				rust_attribute_value(node, source, "ignore").as_deref(),
			))
		} else {
			None
		};
		self.push_type_params_from(node, source);
		self.push_local_scope();
		if let Some(params) = node.child_by_field_name("parameters") {
			self.record_param_names(params, &moniker);
		}

		NodeShape::Symbol(Symbol {
			moniker,
			kind,
			visibility: self.visibility_of(node, scope, source),
			signature,
			body: node.child_by_field_name("body"),
			position: node_position(node),
			annotated_by: Vec::new(),
		})
	}

	fn emit_enum_constants(&self, enum_node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let Some(body) = enum_node.child_by_field_name("body") else {
			return;
		};
		let mut cursor = body.walk();
		for variant in body.named_children(&mut cursor) {
			if variant.kind() != "enum_variant" {
				continue;
			}
			let Some(name_node) = variant
				.child_by_field_name("name")
				.or_else(|| variant.named_child(0))
			else {
				continue;
			};
			let name = node_slice(name_node, self.source_bytes);
			if name.is_empty() {
				continue;
			}
			let moniker = extend_segment(parent, kinds::ENUM_CONSTANT, name);
			let _ = graph.add_def(
				moniker,
				kinds::ENUM_CONSTANT,
				parent,
				Some(node_position(variant)),
			);
		}
	}

	fn classify_inline_module<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
	) -> NodeShape<'src> {
		let Some(body) = node.child_by_field_name("body") else {
			return NodeShape::Skip;
		};
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		NodeShape::Symbol(Symbol {
			moniker: extend_segment(scope, kinds::MODULE, name),
			kind: kinds::MODULE,
			visibility: self.visibility_of(node, scope, source),
			signature: None,
			body: Some(body),
			position: node_position(node),
			annotated_by: Vec::new(),
		})
	}

	fn collect_trait_bounds_extends(
		&self,
		bounds: Node<'_>,
		out: &mut Vec<crate::lang::strategy::RefSpec>,
	) {
		let mut cursor = bounds.walk();
		for child in bounds.named_children(&mut cursor) {
			if child.kind() == "lifetime" {
				continue;
			}
			if let Some(name) = type_name_text(child, self.source_bytes) {
				let (target, confidence) = rust_prelude_trait_target(&self.module, name.as_bytes())
					.map(|target| (target, kinds::CONF_EXTERNAL))
					.unwrap_or_else(|| {
						(
							extend_segment(&self.module, kinds::TRAIT, name.as_bytes()),
							kinds::CONF_NAME_MATCH,
						)
					});
				out.push(crate::lang::strategy::RefSpec {
					kind: kinds::EXTENDS,
					target,
					confidence,
					position: node_position(child),
					receiver_hint: b"",
					alias: b"",
				});
			}
		}
	}

	fn handle_impl(&self, node: Node<'_>, scope: &Moniker, source: &[u8], graph: &mut CodeGraph) {
		let Some(type_node) = node.child_by_field_name("type") else {
			return;
		};
		let Some(type_name) = impl_type_name(type_node, source) else {
			return;
		};
		let type_moniker = self.resolve_impl_target(type_name, graph);
		if !graph.contains(&type_moniker) {
			self.ensure_inferred_struct(&type_moniker, node, graph);
		}
		let trait_node = node.child_by_field_name("trait");
		if let Some(trait_node) = trait_node
			&& let Some(trait_name) = impl_type_name(trait_node, source)
		{
			let (trait_moniker, confidence) =
				self.resolve_impl_trait_target(scope, trait_node, trait_name, graph);
			let attrs = RefAttrs {
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(
				&type_moniker,
				trait_moniker,
				kinds::IMPLEMENTS,
				Some(node_position(node)),
				&attrs,
			);
		}
		if let Some(body) = node.child_by_field_name("body") {
			let prev = self.in_trait_impl.replace(trait_node.is_some());
			self.walk_children(body, &type_moniker, graph);
			self.in_trait_impl.set(prev);
		}
	}

	fn resolve_impl_trait_target(
		&self,
		scope: &Moniker,
		trait_node: Node<'_>,
		trait_name: &str,
		graph: &CodeGraph,
	) -> (Moniker, &'static [u8]) {
		let mut pieces = Vec::new();
		collect_scoped_path_into(trait_node, self.source_bytes, &mut pieces);
		if pieces.len() > 1 {
			return self.resolve_scoped_type_ref(scope, &pieces, graph);
		}
		if let Some(imported) = self.imported_symbols.borrow().get(trait_name.as_bytes()) {
			return (imported.clone(), import_confidence(imported));
		}
		if let Some(target) = rust_prelude_trait_target(&self.module, trait_name.as_bytes()) {
			return (target, kinds::CONF_EXTERNAL);
		}
		let target = extend_segment(&self.module, kinds::TRAIT, trait_name.as_bytes());
		let confidence = if graph.contains(&target) {
			kinds::CONF_RESOLVED
		} else {
			kinds::CONF_NAME_MATCH
		};
		(target, confidence)
	}

	fn visibility_of(&self, node: Node<'_>, scope: &Moniker, source: &[u8]) -> &'static [u8] {
		let mut cursor = node.walk();
		let modifier = node
			.children(&mut cursor)
			.find(|c| c.kind() == "visibility_modifier");
		match modifier {
			Some(vm) => match vm.utf8_text(source).unwrap_or("").trim() {
				"pub" => kinds::VIS_PUBLIC,
				_ => kinds::VIS_MODULE,
			},
			None => {
				let in_trait_scope = scope.last_kind().as_deref() == Some(b"trait");
				if in_trait_scope || self.in_trait_impl.get() {
					kinds::VIS_PUBLIC
				} else {
					kinds::VIS_PRIVATE
				}
			}
		}
	}

	fn resolve_impl_target(&self, type_name: &str, graph: &CodeGraph) -> Moniker {
		for kind in [kinds::ENUM, kinds::TRAIT, kinds::TYPE, kinds::STRUCT] {
			let m = extend_segment(&self.module, kind, type_name.as_bytes());
			if graph.contains(&m) {
				return m;
			}
		}
		extend_segment(&self.module, kinds::STRUCT, type_name.as_bytes())
	}

	fn ensure_inferred_struct(&self, m: &Moniker, anchor: Node<'_>, graph: &mut CodeGraph) {
		if graph.contains(m) {
			return;
		}
		let attrs = DefAttrs {
			visibility: kinds::VIS_NONE,
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(
			m.clone(),
			kinds::STRUCT,
			&self.module,
			Some(node_position(anchor)),
			&attrs,
		);
	}

	fn handle_let(&self, node: Node<'_>, callable: &Moniker, source: &[u8], graph: &mut CodeGraph) {
		let Some(pattern) = node.child_by_field_name("pattern") else {
			return;
		};
		self.record_pattern_names(pattern);
		if let Some(ty) = node.child_by_field_name("type") {
			self.record_pattern_type(pattern, ty, callable);
			self.emit_uses_type_walk(ty, callable, graph);
		}
		let has_explicit_type = node.child_by_field_name("type").is_some();
		if self.deep {
			self.emit_pattern_defs(pattern, callable, kinds::LOCAL, node, graph);
		}
		let Some(value) = node.child_by_field_name("value") else {
			return;
		};
		if value.kind() == "closure_expression"
			&& let Some(bind_name) = first_identifier(pattern, source)
		{
			self.record_local(bind_name.as_bytes());
			self.emit_named_closure(value, callable, bind_name.as_bytes(), source, graph);
			return;
		}
		self.recurse_subtree(value, callable, graph);
		if !has_explicit_type
			&& let Some(bind_name) = first_identifier(pattern, source)
			&& let Some(target) = self.infer_value_type_target(value, callable, graph)
		{
			self.record_local_type(bind_name.as_bytes(), target);
		}
	}

	fn handle_for(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(pattern) = node.child_by_field_name("pattern") else {
			self.walk_children(node, scope, graph);
			return;
		};
		let value = node.child_by_field_name("value");
		if let Some(value) = value {
			self.recurse_subtree(value, scope, graph);
		}
		self.push_local_scope();
		self.record_pattern_names(pattern);
		if self.deep {
			self.emit_pattern_defs(pattern, scope, kinds::LOCAL, node, graph);
		}
		if let Some(value) = value
			&& let Some(bind_name) = first_identifier(pattern, self.source_bytes)
			&& let Some(target) = self.infer_value_type_target(value, scope, graph)
		{
			self.record_local_type(bind_name.as_bytes(), target);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.recurse_subtree(body, scope, graph);
		}
		self.pop_local_scope();
	}

	fn emit_named_closure(
		&self,
		closure: Node<'_>,
		callable: &Moniker,
		name: &[u8],
		source: &[u8],
		graph: &mut CodeGraph,
	) {
		let slots = closure_param_slots(closure, source);
		let moniker = extend_callable_slots(callable, kinds::FN, name, &slots);
		let _ = graph.add_def(
			moniker.clone(),
			kinds::FN,
			callable,
			Some(node_position(closure)),
		);
		self.push_local_scope();
		if let Some(params) = closure.child_by_field_name("parameters") {
			self.record_param_names(params, &moniker);
			if self.deep {
				self.emit_params(params, &moniker, graph);
			}
		}
		if let Some(body) = closure.child_by_field_name("body") {
			self.recurse_subtree(body, &moniker, graph);
		}
		self.pop_local_scope();
	}

	fn handle_use(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let Some(arg) = node.child_by_field_name("argument") else {
			return;
		};
		let pos = node_position(node);
		let mut leaves: Vec<Vec<String>> = Vec::new();
		collect_use_leaves(arg, self.source_bytes, &mut Vec::new(), &mut leaves);
		let mut aliases: Vec<(Vec<String>, String)> = Vec::new();
		collect_use_aliases(arg, self.source_bytes, &mut Vec::new(), &mut aliases);
		let is_reexport = use_is_reexport(node);
		for path in leaves {
			let target = normalize_use_self_target(self.build_use_target(parent, &path));
			if let Some(parent_module) = wildcard_import_module(&target) {
				self.record_wildcard_module(parent, &parent_module);
				self.emit_imports_module(parent, parent_module, pos, graph);
				continue;
			}
			self.record_imported_symbol(&target);
			if is_reexport {
				self.emit_reexport_alias(parent, &target, node, graph);
			}
			let ref_kind = if is_reexport {
				kinds::REEXPORTS
			} else {
				kinds::IMPORTS_SYMBOL
			};
			let attrs = RefAttrs {
				confidence: import_confidence(&target),
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(parent, target.clone(), ref_kind, Some(pos), &attrs);
			if let Some(parent_module) = drop_leaf_segment(&target)
				&& self
					.imported_modules
					.borrow_mut()
					.insert(parent_module.clone())
			{
				self.emit_imports_module(parent, parent_module, pos, graph);
			}
		}
		for (path, alias) in aliases {
			let target = normalize_use_self_target(self.build_use_target(parent, &path));
			self.record_imported_alias(&target, alias.as_bytes());
		}
	}

	fn emit_imports_module(
		&self,
		parent: &Moniker,
		target: Moniker,
		pos: crate::core::code_graph::Position,
		graph: &mut CodeGraph,
	) {
		let attrs = RefAttrs {
			confidence: import_confidence(&target),
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(parent, target, kinds::IMPORTS_MODULE, Some(pos), &attrs);
	}

	fn emit_reexport_alias(
		&self,
		parent: &Moniker,
		target: &Moniker,
		node: Node<'_>,
		graph: &mut CodeGraph,
	) {
		let Some(last) = target.as_view().segments().last() else {
			return;
		};
		let name = bare_callable_name(last.name);
		if name.is_empty() || matches!(name, b"self" | b"*") {
			return;
		}
		let alias = extend_segment(parent, kinds::PATH, name);
		let attrs = DefAttrs {
			visibility: self.visibility_of(node, parent, self.source_bytes),
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(
			alias,
			kinds::PATH,
			parent,
			Some(node_position(node)),
			&attrs,
		);
	}

	fn record_imported_symbol(&self, target: &Moniker) {
		let Some(last) = target.as_view().segments().last() else {
			return;
		};
		let name = bare_callable_name(last.name);
		if name == b"self" {
			self.record_imported_self_module(target);
			return;
		}
		if name.is_empty() || matches!(name, b"self" | b"*") {
			return;
		}
		self.imported_symbols
			.borrow_mut()
			.insert(name.to_vec(), target.clone());
	}

	fn record_imported_self_module(&self, target: &Moniker) {
		let Some(parent) = drop_leaf_segment(target) else {
			return;
		};
		let Some(module_name) = parent
			.as_view()
			.segments()
			.last()
			.map(|segment| segment.name)
		else {
			return;
		};
		self.imported_symbols
			.borrow_mut()
			.insert(module_name.to_vec(), parent);
	}

	fn record_imported_alias(&self, target: &Moniker, alias: &[u8]) {
		if alias.is_empty() || alias == b"_" {
			return;
		}
		self.imported_symbols
			.borrow_mut()
			.insert(alias.to_vec(), target.clone());
	}

	fn record_wildcard_module(&self, scope: &Moniker, target: &Moniker) {
		self.imported_wildcard_modules
			.borrow_mut()
			.entry(scope.clone())
			.or_default()
			.push(target.clone());
	}

	fn build_use_target(&self, scope: &Moniker, path: &[String]) -> Moniker {
		if path.is_empty() {
			return scope.clone();
		}
		match path[0].as_str() {
			"crate" => target_under_project(scope, &path[1..]),
			"self" => target_under_module(scope, &path[1..], 0),
			"super" => {
				let up = path.iter().take_while(|s| s.as_str() == "super").count();
				target_under_module(scope, &path[up..], up)
			}
			first if self.local_mods.contains(first) => target_under_module(scope, path, 0),
			first => {
				if let Some(imported) = self.imported_symbols.borrow().get(first.as_bytes()) {
					return append_path_segments(imported, &path[1..]);
				}
				if let Some(target) = self.type_table.get(first.as_bytes()) {
					return append_path_segments(target, &path[1..]);
				}
				if let Some(target) = self.resolve_wildcard_imported_type(scope, first.as_bytes()) {
					return append_path_segments(&target, &path[1..]);
				}
				target_external(scope, path)
			}
		}
	}

	fn handle_call(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if let Some(func) = node.child_by_field_name("function") {
			match func.kind() {
				"field_expression" => self.emit_method_call(node, func, scope, graph),
				"identifier" => self.emit_free_fn_call(node, func, scope, graph),
				"scoped_identifier" => self.emit_path_call(node, func, scope, graph),
				_ => {}
			}
		}
		if let Some(args) = node.child_by_field_name("arguments") {
			self.recurse_subtree(args, scope, graph);
		}
	}

	fn handle_struct_literal(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if let Some(name_node) = node.child_by_field_name("name")
			&& let Some(name) = type_name_text(name_node, self.source_bytes)
		{
			let (target, confidence) =
				self.resolve_struct_literal_target(name_node, name, scope, graph);
			let attrs = RefAttrs {
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(
				scope,
				target,
				kinds::INSTANTIATES,
				Some(node_position(node)),
				&attrs,
			);
		}
		self.walk_children(node, scope, graph);
	}

	fn resolve_struct_literal_target(
		&self,
		name_node: Node<'_>,
		name: &str,
		scope: &Moniker,
		graph: &CodeGraph,
	) -> (Moniker, &'static [u8]) {
		if is_self_type(name)
			&& let Some(t) = enclosing_type_moniker(scope)
		{
			return (t, kinds::CONF_RESOLVED);
		}

		let mut pieces = Vec::new();
		collect_scoped_path_into(name_node, self.source_bytes, &mut pieces);
		if pieces.len() > 1 {
			return self.resolve_scoped_value_target(scope, &pieces);
		}

		self.resolve_constructor_target(kinds::STRUCT, name.as_bytes(), graph)
	}

	fn handle_macro(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(macro_node) = node.child_by_field_name("macro") else {
			return;
		};
		let Some(name) = type_name_text(macro_node, self.source_bytes) else {
			return;
		};
		if name == "define_languages" {
			self.emit_define_languages_defs(node, scope, graph);
		}
		let (target, confidence) = self.resolve_macro_target(macro_node, name);
		let attrs = RefAttrs {
			confidence,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			target,
			kinds::CALLS,
			Some(node_position(node)),
			&attrs,
		);
		if name == "proptest" {
			self.emit_proptest_tests(node, scope, graph);
		}
		self.walk_macro_children(node, macro_node, scope, graph);
	}

	fn walk_macro_children(
		&self,
		node: Node<'_>,
		macro_node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let mut cursor = node.walk();
		for child in node.named_children(&mut cursor) {
			if same_syntax_node(child, macro_node) {
				continue;
			}
			self.recurse_subtree(child, scope, graph);
		}
	}

	fn handle_macro_definition(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		source: &[u8],
		graph: &mut CodeGraph,
	) {
		let Some(name) = node
			.child_by_field_name("name")
			.and_then(|name| type_name_text(name, source))
			.or_else(|| first_identifier(node, source))
		else {
			return;
		};
		let moniker = extend_segment(scope, kinds::MACRO, name.as_bytes());
		let _ = graph.add_def(moniker, kinds::MACRO, scope, Some(node_position(node)));
	}

	fn emit_define_languages_defs(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Ok(text) = node.utf8_text(self.source_bytes) else {
			return;
		};
		let variants = parse_define_languages_variants(text);
		if variants.is_empty() {
			return;
		}
		let enum_moniker = extend_segment(scope, kinds::ENUM, b"Lang");
		let position = Some(node_position(node));
		let _ = graph.add_def(enum_moniker.clone(), kinds::ENUM, scope, position);
		for variant in variants {
			let moniker = extend_segment(&enum_moniker, kinds::ENUM_CONSTANT, variant.as_bytes());
			let _ = graph.add_def(moniker, kinds::ENUM_CONSTANT, &enum_moniker, position);
		}
	}

	fn resolve_macro_target(&self, macro_node: Node<'_>, name: &str) -> (Moniker, &'static [u8]) {
		if is_builtin_macro(name) {
			return (
				target_external_std(&self.module, &[("path", "macros"), ("macro", name)]),
				kinds::CONF_EXTERNAL,
			);
		}
		if let Some(target) = rust_known_external_macro_target(&self.module, name) {
			return (target, kinds::CONF_EXTERNAL);
		}
		let mut pieces = Vec::new();
		collect_scoped_path_into(macro_node, self.source_bytes, &mut pieces);
		if let Some(head) = pieces.first()
			&& pieces.len() > 1
		{
			if let Some(imported) = self.imported_symbols.borrow().get(head.as_bytes()) {
				return (
					append_path_segments(imported, &pieces[1..]),
					import_confidence(imported),
				);
			}
			if head
				.chars()
				.next()
				.is_some_and(|ch| ch.is_ascii_lowercase())
			{
				return (target_external(&self.module, &pieces), kinds::CONF_EXTERNAL);
			}
			return (
				target_path_under_module(&self.module, &pieces),
				kinds::CONF_NAME_MATCH,
			);
		}
		if let Some(imported) = self.imported_symbols.borrow().get(name.as_bytes()) {
			return (imported.clone(), import_confidence(imported));
		}
		(
			extend_segment(&self.module, kinds::MACRO, name.as_bytes()),
			kinds::CONF_UNRESOLVED,
		)
	}

	fn emit_proptest_tests(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Ok(text) = node.utf8_text(self.source_bytes) else {
			return;
		};
		for parsed in parse_proptest_tests(text) {
			let moniker = extend_segment(scope, kinds::TEST, parsed.segment.as_bytes());
			let attrs = DefAttrs {
				visibility: kinds::VIS_PRIVATE,
				signature: &parsed.signature,
				..DefAttrs::default()
			};
			let start = node.start_byte() as u32 + parsed.start_offset as u32;
			let end = node.start_byte() as u32 + parsed.end_offset as u32;
			let _ = graph.add_def_attrs(moniker, kinds::TEST, scope, Some((start, end)), &attrs);
		}
	}

	fn handle_attribute(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = node.walk();
		for child in node.named_children(&mut cursor) {
			if child.kind() == "attribute" {
				self.emit_attribute_refs(child, scope, graph);
			}
		}
	}

	fn emit_attribute_refs(&self, attr: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = attr.walk();
		let Some(name) = attr
			.named_children(&mut cursor)
			.find_map(|c| type_name_text(c, self.source_bytes))
		else {
			return;
		};
		if name == "derive"
			&& let Some(args) = attr.child_by_field_name("arguments")
		{
			let Ok(args_text) = args.utf8_text(self.source_bytes) else {
				return;
			};
			for trait_path in parse_derive_trait_paths(args_text) {
				let (target, confidence) = self.resolve_derive_trait_path(&trait_path);
				let attrs = RefAttrs {
					confidence,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(
					scope,
					target,
					kinds::ANNOTATES,
					Some(node_position(args)),
					&attrs,
				);
			}
			return;
		}
		let (target, confidence) = self.resolve_attribute_target(name.as_bytes());
		let attrs = RefAttrs {
			confidence,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			target,
			kinds::ANNOTATES,
			Some(node_position(attr)),
			&attrs,
		);
	}

	fn resolve_derive_trait_target(&self, name: &[u8]) -> (Moniker, &'static [u8]) {
		if let Some(target) = self.imported_symbols.borrow().get(name) {
			return (target.clone(), import_confidence(target));
		}
		if let Some(target) = rust_builtin_derive_trait_target(&self.module, name) {
			return (target, kinds::CONF_EXTERNAL);
		}
		(
			extend_segment(&self.module, kinds::TRAIT, name),
			kinds::CONF_NAME_MATCH,
		)
	}

	fn resolve_derive_trait_path(&self, path: &str) -> (Moniker, &'static [u8]) {
		if is_ident_token(path) {
			return self.resolve_derive_trait_target(path.as_bytes());
		}
		let pieces = path
			.split("::")
			.map(str::trim)
			.filter(|piece| !piece.is_empty())
			.map(ToOwned::to_owned)
			.collect::<Vec<_>>();
		let Some(head) = pieces.first() else {
			return (self.module.clone(), kinds::CONF_UNRESOLVED);
		};
		if head
			.chars()
			.next()
			.is_some_and(|ch| ch.is_ascii_lowercase())
		{
			return (target_external(&self.module, &pieces), kinds::CONF_EXTERNAL);
		}
		(
			target_path_under_module(&self.module, &pieces),
			kinds::CONF_NAME_MATCH,
		)
	}

	fn resolve_attribute_target(&self, name: &[u8]) -> (Moniker, &'static [u8]) {
		if let Some(target) = rust_known_attribute_target(&self.module, name) {
			return (target, kinds::CONF_EXTERNAL);
		}
		if let Some(target) = self.imported_symbols.borrow().get(name) {
			return (target.clone(), import_confidence(target));
		}
		(
			extend_segment(&self.module, kinds::FN, name),
			kinds::CONF_NAME_MATCH,
		)
	}

	fn handle_identifier_read(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if !self.deep {
			return;
		}
		let name = node_slice(node, self.source_bytes);
		if !self.is_local_in_scope(name) {
			return;
		}
		let Some(callable) = enclosing_callable_moniker(scope) else {
			return;
		};
		let target = extend_segment(&callable, kinds::LOCAL, name);
		let attrs = RefAttrs {
			confidence: kinds::CONF_LOCAL,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			target,
			kinds::READS,
			Some(node_position(node)),
			&attrs,
		);
	}

	fn handle_scoped_read(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name_node) = node.child_by_field_name("name") else {
			return;
		};
		let name = node_slice(name_node, self.source_bytes);
		let mut pieces = Vec::new();
		collect_scoped_path_into(node, self.source_bytes, &mut pieces);
		let (target, confidence) = if pieces.len() > 1 {
			self.resolve_scoped_value_target(scope, &pieces)
		} else {
			(
				extend_segment(&self.module, kinds::PATH, name),
				kinds::CONF_NAME_MATCH,
			)
		};
		let attrs = RefAttrs {
			confidence,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			target,
			kinds::READS,
			Some(node_position(node)),
			&attrs,
		);
	}

	fn resolve_scoped_value_target(
		&self,
		scope: &Moniker,
		pieces: &[String],
	) -> (Moniker, &'static [u8]) {
		if let Some(target) = rust_std_associated_path_target(&self.module, pieces) {
			return (target, kinds::CONF_EXTERNAL);
		}
		if let Some(head) = pieces.first()
			&& let Some(imported) = self.imported_symbols.borrow().get(head.as_bytes())
		{
			let target = append_path_segments(imported, &pieces[1..]);
			return (target, import_confidence(imported));
		}
		if let Some(head) = pieces.first()
			&& let Some(target) = self.type_table.get(head.as_bytes())
		{
			return (
				append_path_segments(target, &pieces[1..]),
				kinds::CONF_RESOLVED,
			);
		}
		if let Some(head) = pieces.first()
			&& !self.local_mods.contains(head)
			&& head
				.chars()
				.next()
				.is_some_and(|ch| ch.is_ascii_lowercase())
		{
			return (target_external(&self.module, pieces), kinds::CONF_EXTERNAL);
		}
		if let Some(head) = pieces.first()
			&& !self.local_mods.contains(head)
			&& let Some(target) = self.resolve_wildcard_imported_path(scope, pieces)
		{
			return (target, kinds::CONF_IMPORTED);
		}
		(
			target_path_under_module(&self.module, pieces),
			kinds::CONF_NAME_MATCH,
		)
	}

	fn resolve_wildcard_imported_path(
		&self,
		scope: &Moniker,
		pieces: &[String],
	) -> Option<Moniker> {
		let modules = self.wildcard_modules_for_scope(scope);
		wildcard_path_module(&modules, pieces).map(|module| append_path_segments(module, pieces))
	}

	fn resolve_callable(
		&self,
		parent: &Moniker,
		kind: &[u8],
		name: &[u8],
	) -> (Moniker, &'static [u8]) {
		match self.callable_table.get(&(parent.clone(), name.to_vec())) {
			Some(seg) => (extend_segment(parent, kind, seg), kinds::CONF_RESOLVED),
			None => (extend_segment(parent, kind, name), kinds::CONF_UNRESOLVED),
		}
	}

	fn resolve_callable_parent(&self, scope: &Moniker, name: &[u8]) -> Option<Moniker> {
		let parent = enclosing_module_moniker(scope).unwrap_or_else(|| self.module.clone());
		[parent].into_iter().find(|parent| {
			self.callable_table
				.contains_key(&(parent.clone(), name.to_vec()))
		})
	}

	fn receiver_type_target(
		&self,
		receiver: Node<'_>,
		scope: &Moniker,
		graph: &CodeGraph,
	) -> Option<Moniker> {
		match receiver.kind() {
			"identifier" => self.local_type_in_scope(node_slice(receiver, self.source_bytes)),
			"call_expression" => self.infer_call_type_target(receiver, scope, graph),
			_ => None,
		}
	}

	fn resolve_receiver_method_target(
		&self,
		receiver_type: &Moniker,
		name: &[u8],
		graph: &CodeGraph,
	) -> (Moniker, &'static [u8]) {
		let target = extend_segment(receiver_type, kinds::METHOD, name);
		if external_root(receiver_type).is_some() {
			return (target, kinds::CONF_EXTERNAL);
		}
		if graph.contains(&target) {
			return (target, kinds::CONF_RESOLVED);
		}
		if is_common_std_method(name) {
			return (
				common_std_method_target(&self.module, name),
				kinds::CONF_EXTERNAL,
			);
		}
		(target, kinds::CONF_NAME_MATCH)
	}

	fn emit_method_call(
		&self,
		call: Node<'_>,
		func: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let Some(receiver) = func.child_by_field_name("value") else {
			return;
		};
		let Some(field) = func.child_by_field_name("field") else {
			return;
		};
		let name = node_slice(field, self.source_bytes);
		let (target, confidence) = if receiver.kind() == "self"
			&& let Some(t) = enclosing_type_moniker(scope)
		{
			self.resolve_callable(&t, kinds::METHOD, name)
		} else if let Some(receiver_type) = self.receiver_type_target(receiver, scope, graph) {
			self.resolve_receiver_method_target(&receiver_type, name, graph)
		} else if let Some(target) = rust_known_external_method_target(&self.module, name) {
			(target, kinds::CONF_EXTERNAL)
		} else if is_common_std_method(name) {
			(
				common_std_method_target(&self.module, name),
				kinds::CONF_EXTERNAL,
			)
		} else {
			(
				extend_segment(&self.module, kinds::METHOD, name),
				kinds::CONF_UNRESOLVED,
			)
		};
		let arity = call_argument_count(call);
		let attrs = RefAttrs {
			confidence,
			receiver_hint: receiver_hint(receiver, self.source_bytes),
			call_name: name,
			call_arity: Some(arity),
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			target,
			kinds::METHOD_CALL,
			Some(node_position(call)),
			&attrs,
		);
		self.recurse_subtree(receiver, scope, graph);
	}

	fn emit_free_fn_call(
		&self,
		call: Node<'_>,
		func: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let name = node_slice(func, self.source_bytes);
		let name_str = std::str::from_utf8(name).unwrap_or("");
		let arity = call_argument_count(call);
		if starts_uppercase(name_str) {
			if is_self_type(name_str)
				&& let Some(target) = enclosing_type_moniker(scope)
			{
				let attrs = RefAttrs {
					confidence: kinds::CONF_RESOLVED,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(
					scope,
					target,
					kinds::INSTANTIATES,
					Some(node_position(call)),
					&attrs,
				);
				return;
			}
			let (target, confidence) = self.resolve_constructor_target(kinds::STRUCT, name, graph);
			let attrs = RefAttrs {
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(
				scope,
				target,
				kinds::INSTANTIATES,
				Some(node_position(call)),
				&attrs,
			);
			return;
		}
		if let Some(imported) = self.imported_symbols.borrow().get(name) {
			let attrs = RefAttrs {
				confidence: import_confidence(imported),
				call_name: name,
				call_arity: Some(arity),
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(
				scope,
				imported.clone(),
				kinds::CALLS,
				Some(node_position(call)),
				&attrs,
			);
			return;
		}
		if self.is_local_in_scope(name)
			&& let Some(callable) = enclosing_callable_moniker(scope)
		{
			let target = extend_segment(&callable, kinds::FN, name);
			let attrs = RefAttrs {
				confidence: kinds::CONF_LOCAL,
				call_name: name,
				call_arity: Some(arity),
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(
				scope,
				target,
				kinds::CALLS,
				Some(node_position(call)),
				&attrs,
			);
			return;
		}
		let parent = self
			.resolve_callable_parent(scope, name)
			.unwrap_or_else(|| {
				enclosing_module_moniker(scope).unwrap_or_else(|| self.module.clone())
			});
		let (target, confidence) = self.resolve_callable(&parent, kinds::FN, name);
		let attrs = RefAttrs {
			confidence,
			call_name: name,
			call_arity: Some(arity),
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			target,
			kinds::CALLS,
			Some(node_position(call)),
			&attrs,
		);
	}

	fn emit_path_call(
		&self,
		call: Node<'_>,
		func: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let Some(name_node) = func.child_by_field_name("name") else {
			return;
		};
		let name = node_slice(name_node, self.source_bytes);
		let name_str = std::str::from_utf8(name).unwrap_or("");
		let arity = call_argument_count(call);
		let path_name = func
			.child_by_field_name("path")
			.and_then(|p| type_name_text(p, self.source_bytes));
		if let Some(path_node) = func.child_by_field_name("path")
			&& let Some(parent) = self.resolve_module_path_callable_parent(scope, path_node, name)
		{
			let (target, confidence) = self.resolve_callable(&parent, kinds::FN, name);
			let attrs = RefAttrs {
				confidence,
				call_name: name,
				call_arity: Some(arity),
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(
				scope,
				target,
				kinds::CALLS,
				Some(node_position(call)),
				&attrs,
			);
			return;
		}
		if let Some(type_name) = path_name
			&& starts_uppercase(type_name)
		{
			if is_self_type(type_name)
				&& let Some(t) = enclosing_type_moniker(scope)
			{
				let attrs = RefAttrs {
					confidence: kinds::CONF_RESOLVED,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(
					scope,
					t,
					kinds::INSTANTIATES,
					Some(node_position(call)),
					&attrs,
				);
				return;
			}
			if name_str == "new" {
				let (target, confidence) = self
					.resolve_path_constructor_target(func, graph)
					.unwrap_or_else(|| {
						self.resolve_constructor_target(kinds::STRUCT, type_name.as_bytes(), graph)
					});
				let attrs = RefAttrs {
					confidence,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(
					scope,
					target,
					kinds::INSTANTIATES,
					Some(node_position(call)),
					&attrs,
				);
				return;
			}
			if starts_uppercase(name_str) {
				self.emit_instantiates_ref(call, scope, graph, kinds::ENUM, type_name.as_bytes());
				return;
			}
		}
		if let Some((target, confidence)) = self.resolve_path_call_target(scope, func, graph) {
			let attrs = RefAttrs {
				confidence,
				call_name: name,
				call_arity: Some(arity),
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(
				scope,
				target,
				kinds::CALLS,
				Some(node_position(call)),
				&attrs,
			);
			return;
		}
		let target = extend_segment(&self.module, kinds::FN, name);
		let attrs = RefAttrs {
			confidence: kinds::CONF_UNRESOLVED,
			call_name: name,
			call_arity: Some(arity),
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			target,
			kinds::CALLS,
			Some(node_position(call)),
			&attrs,
		);
	}

	fn resolve_path_call_target(
		&self,
		scope: &Moniker,
		func: Node<'_>,
		graph: &CodeGraph,
	) -> Option<(Moniker, &'static [u8])> {
		let mut pieces = Vec::new();
		collect_scoped_path_into(func, self.source_bytes, &mut pieces);
		let head = pieces.first()?;
		if let Some(target) = rust_std_associated_path_target(&self.module, &pieces) {
			return Some((target, kinds::CONF_EXTERNAL));
		}
		if let Some(target) = rust_known_associated_call_target(&self.module, &pieces) {
			return Some((target, kinds::CONF_EXTERNAL));
		}
		if let Some(imported) = self.imported_symbols.borrow().get(head.as_bytes()) {
			return Some((
				append_path_segments(imported, &pieces[1..]),
				import_confidence(imported),
			));
		}
		if let Some((call_name, type_pieces)) = pieces.split_last()
			&& let Some((receiver_type, confidence)) =
				self.resolve_associated_type_target(scope, type_pieces, graph)
		{
			let target = extend_segment(&receiver_type, kinds::METHOD, call_name.as_bytes());
			let confidence = if external_root(&receiver_type).is_some() {
				kinds::CONF_EXTERNAL
			} else if graph.contains(&target) {
				kinds::CONF_RESOLVED
			} else {
				confidence
			};
			return Some((target, confidence));
		}
		if head
			.chars()
			.next()
			.is_some_and(|ch| ch.is_ascii_lowercase())
		{
			return Some((target_external(&self.module, &pieces), kinds::CONF_EXTERNAL));
		}
		None
	}

	fn infer_value_type_target(
		&self,
		value: Node<'_>,
		scope: &Moniker,
		graph: &CodeGraph,
	) -> Option<Moniker> {
		match value.kind() {
			"call_expression" => self.infer_call_type_target(value, scope, graph),
			"struct_expression" => self.infer_struct_literal_type_target(value, scope, graph),
			"identifier" => self.local_type_in_scope(node_slice(value, self.source_bytes)),
			_ => None,
		}
	}

	fn infer_call_type_target(
		&self,
		call: Node<'_>,
		scope: &Moniker,
		graph: &CodeGraph,
	) -> Option<Moniker> {
		let func = call.child_by_field_name("function")?;
		match func.kind() {
			"scoped_identifier" => {
				let mut pieces = Vec::new();
				collect_scoped_path_into(func, self.source_bytes, &mut pieces);
				let (_, type_pieces) = pieces.split_last()?;
				self.resolve_associated_type_target(scope, type_pieces, graph)
					.map(|(target, _)| target)
			}
			"field_expression" => {
				let receiver = func.child_by_field_name("value")?;
				let target = self.receiver_type_target(receiver, scope, graph)?;
				external_root(&target).is_some().then_some(target)
			}
			"identifier" => {
				let name = node_slice(func, self.source_bytes);
				let name_str = std::str::from_utf8(name).ok()?;
				if is_self_type(name_str) {
					enclosing_type_moniker(scope)
				} else if starts_uppercase(name_str) {
					Some(
						self.resolve_constructor_target(kinds::STRUCT, name, graph)
							.0,
					)
				} else {
					None
				}
			}
			_ => None,
		}
	}

	fn infer_struct_literal_type_target(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &CodeGraph,
	) -> Option<Moniker> {
		let name_node = node.child_by_field_name("name")?;
		let name = type_name_text(name_node, self.source_bytes)?;
		Some(
			self.resolve_struct_literal_target(name_node, name, scope, graph)
				.0,
		)
	}

	fn resolve_associated_type_target(
		&self,
		scope: &Moniker,
		type_pieces: &[String],
		graph: &CodeGraph,
	) -> Option<(Moniker, &'static [u8])> {
		let head = type_pieces.first()?;
		if type_pieces.len() == 1 {
			let name = head.as_bytes();
			if is_self_type(head) {
				return enclosing_type_moniker(scope).map(|target| (target, kinds::CONF_RESOLVED));
			}
			if let Some(target) = self.type_table.get(name) {
				return Some((target.clone(), kinds::CONF_RESOLVED));
			}
			if let Some(target) = self.imported_symbols.borrow().get(name) {
				return Some((target.clone(), import_confidence(target)));
			}
			if let Some(target) = rust_prelude_type_target(&self.module, name) {
				return Some((target, kinds::CONF_EXTERNAL));
			}
			if starts_uppercase(head) {
				return Some(self.resolve_constructor_target(kinds::STRUCT, name, graph));
			}
			return None;
		}
		match head.as_str() {
			"crate" | "self" | "super" => {
				Some(self.resolve_scoped_type_ref(scope, type_pieces, graph))
			}
			_ => {
				if let Some(imported) = self.imported_symbols.borrow().get(head.as_bytes()) {
					return Some((
						append_path_segments(imported, &type_pieces[1..]),
						import_confidence(imported),
					));
				}
				if is_rust_builtin_external_root(head)
					|| head
						.chars()
						.next()
						.is_some_and(|ch| ch.is_ascii_lowercase())
				{
					return Some((
						target_external(&self.module, type_pieces),
						kinds::CONF_EXTERNAL,
					));
				}
				None
			}
		}
	}

	fn resolve_path_constructor_target(
		&self,
		func: Node<'_>,
		graph: &CodeGraph,
	) -> Option<(Moniker, &'static [u8])> {
		let mut pieces = Vec::new();
		collect_scoped_path_into(func, self.source_bytes, &mut pieces);
		if pieces.len() < 2 || pieces.last().is_none_or(|name| name != "new") {
			return None;
		}
		let type_pieces = &pieces[..pieces.len() - 1];
		if type_pieces.len() == 1 {
			return Some(self.resolve_constructor_target(
				kinds::STRUCT,
				type_pieces[0].as_bytes(),
				graph,
			));
		}
		let head = type_pieces.first()?;
		if let Some(imported) = self.imported_symbols.borrow().get(head.as_bytes()) {
			return Some((
				append_path_segments(imported, &type_pieces[1..]),
				import_confidence(imported),
			));
		}
		if head
			.chars()
			.next()
			.is_some_and(|ch| ch.is_ascii_lowercase())
		{
			return Some((
				target_external(&self.module, type_pieces),
				kinds::CONF_EXTERNAL,
			));
		}
		None
	}

	fn resolve_module_path_callable_parent(
		&self,
		scope: &Moniker,
		path_node: Node<'_>,
		name: &[u8],
	) -> Option<Moniker> {
		let mut pieces = Vec::new();
		collect_scoped_path_into(path_node, self.source_bytes, &mut pieces);
		if pieces.is_empty() {
			return None;
		}
		let parents = self.module_path_candidates(scope, &pieces);
		parents.into_iter().find(|parent| {
			self.callable_table
				.contains_key(&(parent.clone(), name.to_vec()))
		})
	}

	fn module_path_candidates(&self, scope: &Moniker, pieces: &[String]) -> Vec<Moniker> {
		let base = enclosing_module_moniker(scope).unwrap_or_else(|| self.module.clone());
		if pieces.first().is_some_and(|p| p == "crate") {
			module_path_from_relative(&self.module, &pieces[1..])
				.map(|module| vec![module])
				.unwrap_or_default()
		} else if pieces.first().is_some_and(|p| p == "super") {
			module_path_from_relative(&base, pieces)
				.map(|module| vec![module])
				.unwrap_or_default()
		} else {
			module_path_from_relative(&base, pieces)
				.map(|module| vec![module])
				.unwrap_or_default()
		}
	}

	fn emit_instantiates_ref(
		&self,
		call: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
		kind: &[u8],
		type_name: &[u8],
	) {
		let (target, confidence) = self.resolve_constructor_target(kind, type_name, graph);
		let attrs = RefAttrs {
			confidence,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			target,
			kinds::INSTANTIATES,
			Some(node_position(call)),
			&attrs,
		);
	}

	fn record_param_names(&self, params: Node<'_>, scope: &Moniker) {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			match child.kind() {
				"self_parameter" => self.record_local(b"self"),
				"parameter" => {
					if let Some(pattern) = child.child_by_field_name("pattern") {
						self.record_pattern_names(pattern);
						if let Some(ty) = child.child_by_field_name("type") {
							self.record_pattern_type(pattern, ty, scope);
						}
					}
				}
				_ => self.record_pattern_names(child),
			}
		}
	}

	fn record_pattern_names(&self, pattern: Node<'_>) {
		visit_pattern_identifiers(pattern, &mut |ident| {
			let bytes = node_slice(ident, self.source_bytes);
			self.record_local(bytes);
		});
	}

	fn record_pattern_type(&self, pattern: Node<'_>, ty: Node<'_>, scope: &Moniker) {
		let Some(name) = first_identifier(pattern, self.source_bytes) else {
			return;
		};
		let Some(target) = self.resolve_type_binding_node(ty, scope) else {
			return;
		};
		self.record_local_type(name.as_bytes(), target);
	}

	fn resolve_type_binding_node(&self, ty: Node<'_>, scope: &Moniker) -> Option<Moniker> {
		match ty.kind() {
			"reference_type" | "mutable_reference_type" | "pointer_type" | "generic_type" => ty
				.child_by_field_name("type")
				.and_then(|inner| self.resolve_type_binding_node(inner, scope)),
			"scoped_type_identifier" => {
				let mut pieces = Vec::new();
				collect_scoped_path_into(ty, self.source_bytes, &mut pieces);
				(!pieces.is_empty()).then(|| self.resolve_type_binding_path(scope, &pieces))
			}
			_ => type_name_text(ty, self.source_bytes)
				.map(|type_name| self.resolve_type_binding_target(type_name.as_bytes())),
		}
	}

	fn resolve_type_binding_path(&self, scope: &Moniker, pieces: &[String]) -> Moniker {
		let Some(head) = pieces.first() else {
			return self.module.clone();
		};
		match head.as_str() {
			"crate" => target_under_project(scope, &pieces[1..]),
			"self" => target_under_module(scope, &pieces[1..], 0),
			"super" => {
				let up = pieces
					.iter()
					.take_while(|piece| piece.as_str() == "super")
					.count();
				target_under_module(scope, &pieces[up..], up)
			}
			_ => {
				if let Some(imported) = self.imported_symbols.borrow().get(head.as_bytes()) {
					return append_path_segments(imported, &pieces[1..]);
				}
				if is_rust_builtin_external_root(head)
					|| head
						.chars()
						.next()
						.is_some_and(|ch| ch.is_ascii_lowercase())
				{
					return target_external(&self.module, pieces);
				}
				target_path_under_module(&self.module, pieces)
			}
		}
	}

	fn resolve_type_binding_target(&self, name: &[u8]) -> Moniker {
		if let Some(target) = self.type_table.get(name) {
			return target.clone();
		}
		if let Some(target) = self.imported_symbols.borrow().get(name) {
			return target.clone();
		}
		if let Some(target) = rust_prelude_type_target(&self.module, name) {
			return target;
		}
		extend_segment(&self.module, kinds::STRUCT, name)
	}

	fn emit_param_type_refs(&self, params: Node<'_>, callable: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			if child.kind() == "parameter"
				&& let Some(ty) = child.child_by_field_name("type")
			{
				self.emit_uses_type_walk(ty, callable, graph);
			}
		}
	}

	fn emit_params(&self, params: Node<'_>, callable: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			match child.kind() {
				"self_parameter" => {
					self.emit_pattern_leaf(callable, kinds::PARAM, b"self", child, graph)
				}
				"parameter" => {
					if let Some(pattern) = child.child_by_field_name("pattern") {
						self.emit_pattern_defs(pattern, callable, kinds::PARAM, child, graph);
					}
				}
				_ => self.emit_pattern_defs(child, callable, kinds::PARAM, child, graph),
			}
		}
	}

	fn emit_pattern_defs(
		&self,
		pattern: Node<'_>,
		callable: &Moniker,
		kind: &[u8],
		anchor: Node<'_>,
		graph: &mut CodeGraph,
	) {
		visit_pattern_identifiers(pattern, &mut |ident| {
			let name = node_slice(ident, self.source_bytes);
			self.emit_pattern_leaf(callable, kind, name, anchor, graph);
		});
	}

	fn emit_pattern_leaf(
		&self,
		callable: &Moniker,
		kind: &[u8],
		name: &[u8],
		anchor: Node<'_>,
		graph: &mut CodeGraph,
	) {
		let m = extend_segment(callable, kind, name);
		let _ = graph.add_def(m, kind, callable, Some(node_position(anchor)));
	}

	fn emit_uses_type_walk(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		match node.kind() {
			"type_identifier" => self.emit_uses_type_at(node, scope, graph),
			"scoped_type_identifier" => self.emit_uses_scoped_type_at(node, scope, graph),
			"type_binding" => {
				if let Some(bound_type) = node.child_by_field_name("type") {
					self.emit_uses_type_walk(bound_type, scope, graph);
				}
			}
			_ => {
				let mut cursor = node.walk();
				for child in node.named_children(&mut cursor) {
					self.emit_uses_type_walk(child, scope, graph);
				}
			}
		}
	}

	fn emit_uses_type_at(&self, name_node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let name = node_slice(name_node, self.source_bytes);
		let name_str = std::str::from_utf8(name).unwrap_or("");
		if is_placeholder_type(name_str) || is_self_type(name_str) || is_primitive_type(name_str) {
			return;
		}
		if self.is_type_param_in_scope(name) {
			return;
		}
		let (target, confidence) = self.resolve_type_ref(scope, name, graph);
		let attrs = RefAttrs {
			confidence,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			target,
			kinds::USES_TYPE,
			Some(node_position(name_node)),
			&attrs,
		);
	}

	fn emit_uses_scoped_type_at(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let mut pieces = Vec::new();
		collect_scoped_path_into(node, self.source_bytes, &mut pieces);
		let Some(name) = pieces.last() else {
			return;
		};
		if is_placeholder_type(name) || is_self_type(name) || is_primitive_type(name) {
			return;
		}
		if pieces.len() <= 1 {
			if let Some(name_node) = node.child_by_field_name("name") {
				self.emit_uses_type_at(name_node, scope, graph);
			}
			return;
		}
		let (target, confidence) = self.resolve_scoped_type_ref(scope, &pieces, graph);
		let attrs = RefAttrs {
			confidence,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			target,
			kinds::USES_TYPE,
			Some(node_position(node)),
			&attrs,
		);
	}

	fn resolve_type_ref(
		&self,
		scope: &Moniker,
		name: &[u8],
		graph: &CodeGraph,
	) -> (Moniker, &'static [u8]) {
		if let Some(target) = self.type_table.get(name) {
			return (target.clone(), kinds::CONF_RESOLVED);
		}
		if let Some(target) = self.imported_symbols.borrow().get(name) {
			return (target.clone(), import_confidence(target));
		}
		if let Some(target) = rust_prelude_type_target(&self.module, name) {
			return (target, kinds::CONF_EXTERNAL);
		}
		if let Some(target) = self.resolve_wildcard_imported_type(scope, name) {
			return (target, kinds::CONF_IMPORTED);
		}
		let target = extend_segment(&self.module, kinds::STRUCT, name);
		let confidence = if graph.contains(&target) {
			kinds::CONF_RESOLVED
		} else {
			kinds::CONF_NAME_MATCH
		};
		(target, confidence)
	}

	fn resolve_wildcard_imported_type(&self, scope: &Moniker, name: &[u8]) -> Option<Moniker> {
		let modules = self.wildcard_modules_for_scope(scope);
		unambiguous_wildcard_module(&modules).map(|module| extend_segment(module, kinds::PATH, name))
	}

	fn wildcard_modules_for_scope(&self, scope: &Moniker) -> Vec<Moniker> {
		let modules = self.imported_wildcard_modules.borrow();
		let enclosing = enclosing_module_moniker(scope);
		let mut out = Vec::new();
		if let Some(enclosing) = enclosing.as_ref().filter(|module| *module != scope)
			&& let Some(parent_modules) = modules.get(enclosing)
		{
			out.extend(parent_modules.iter().cloned());
		}
		if let Some(scope_modules) = modules.get(scope) {
			out.extend(scope_modules.iter().cloned());
		}
		out
	}

	fn resolve_scoped_type_ref(
		&self,
		scope: &Moniker,
		pieces: &[String],
		graph: &CodeGraph,
	) -> (Moniker, &'static [u8]) {
		let Some(head) = pieces.first() else {
			return (self.module.clone(), kinds::CONF_UNRESOLVED);
		};
		match head.as_str() {
			"crate" => {
				return (
					target_under_project(scope, &pieces[1..]),
					kinds::CONF_NAME_MATCH,
				);
			}
			"self" => {
				return (
					target_under_module(scope, &pieces[1..], 0),
					kinds::CONF_NAME_MATCH,
				);
			}
			"super" => {
				let up = pieces
					.iter()
					.take_while(|piece| piece.as_str() == "super")
					.count();
				return (
					target_under_module(scope, &pieces[up..], up),
					kinds::CONF_NAME_MATCH,
				);
			}
			_ => {}
		}
		if let Some(imported) = self.imported_symbols.borrow().get(head.as_bytes()) {
			let target = append_path_segments(imported, &pieces[1..]);
			return (target, import_confidence(imported));
		}
		if is_rust_builtin_external_root(head) {
			return (target_external(&self.module, pieces), kinds::CONF_EXTERNAL);
		}
		let local_module = extend_segment(&self.module, kinds::MODULE, head.as_bytes());
		if self.local_mods.contains(head) || graph.contains(&local_module) {
			return (
				target_path_under_module(&self.module, pieces),
				kinds::CONF_NAME_MATCH,
			);
		}
		if head
			.chars()
			.next()
			.is_some_and(|ch| ch.is_ascii_lowercase())
		{
			return (target_external(&self.module, pieces), kinds::CONF_EXTERNAL);
		}
		(
			target_path_under_module(&self.module, pieces),
			kinds::CONF_NAME_MATCH,
		)
	}

	fn resolve_constructor_target(
		&self,
		kind: &[u8],
		name: &[u8],
		graph: &CodeGraph,
	) -> (Moniker, &'static [u8]) {
		if let Some(target) = self.imported_symbols.borrow().get(name) {
			return (target.clone(), import_confidence(target));
		}
		if let Some(target) = rust_prelude_constructor_target(&self.module, name) {
			return (target, kinds::CONF_EXTERNAL);
		}
		let target = extend_segment(&self.module, kind, name);
		let confidence = if graph.contains(&target) {
			kinds::CONF_RESOLVED
		} else {
			kinds::CONF_NAME_MATCH
		};
		(target, confidence)
	}

	fn recurse_subtree(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let walker = crate::lang::canonical_walker::CanonicalWalker::new(self, self.source_bytes);
		walker.dispatch(node, scope, graph);
	}

	fn walk_children(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let walker = crate::lang::canonical_walker::CanonicalWalker::new(self, self.source_bytes);
		walker.walk(node, scope, graph);
	}

	fn push_local_scope(&self) {
		self.local_scope.borrow_mut().push(HashSet::new());
		self.local_types.borrow_mut().push(HashMap::new());
	}

	fn pop_local_scope(&self) {
		self.local_scope.borrow_mut().pop();
		self.local_types.borrow_mut().pop();
	}

	fn record_local(&self, name: &[u8]) {
		if let Some(top) = self.local_scope.borrow_mut().last_mut() {
			top.insert(name.to_vec());
		}
	}

	fn record_local_type(&self, name: &[u8], target: Moniker) {
		if let Some(top) = self.local_types.borrow_mut().last_mut() {
			top.insert(name.to_vec(), target);
		}
	}

	fn is_local_in_scope(&self, name: &[u8]) -> bool {
		self.local_scope
			.borrow()
			.iter()
			.any(|frame| frame.contains(name))
	}

	fn local_type_in_scope(&self, name: &[u8]) -> Option<Moniker> {
		self.local_types
			.borrow()
			.iter()
			.rev()
			.find_map(|frame| frame.get(name).cloned())
	}

	fn push_type_params_from(&self, node: Node<'_>, source: &[u8]) -> bool {
		let Some(tp) = node.child_by_field_name("type_parameters") else {
			return false;
		};
		let mut names: HashSet<Vec<u8>> = HashSet::new();
		let mut cursor = tp.walk();
		for child in tp.named_children(&mut cursor) {
			if child.kind() == "type_parameter"
				&& let Some(name_node) = child.child_by_field_name("name")
			{
				names.insert(node_slice(name_node, source).to_vec());
			}
		}
		if names.is_empty() {
			return false;
		}
		self.type_params.borrow_mut().push(names);
		true
	}

	fn is_type_param_in_scope(&self, name: &[u8]) -> bool {
		self.type_params
			.borrow()
			.iter()
			.any(|frame| frame.contains(name))
	}
}

pub(super) fn collect_local_mods(root: Node<'_>, source: &[u8]) -> HashSet<String> {
	let mut out = HashSet::new();
	let mut cursor = root.walk();
	for child in root.children(&mut cursor) {
		if child.kind() == "mod_item"
			&& let Some(name) = child.child_by_field_name("name")
			&& let Ok(s) = name.utf8_text(source)
		{
			out.insert(s.to_string());
		}
	}
	out
}

fn visit_pattern_identifiers(pattern: Node<'_>, leaf: &mut impl FnMut(Node<'_>)) {
	match pattern.kind() {
		"identifier" => leaf(pattern),
		"_" => {}
		_ => {
			let mut cursor = pattern.walk();
			for inner in pattern.named_children(&mut cursor) {
				visit_pattern_identifiers(inner, leaf);
			}
		}
	}
}

fn same_syntax_node(left: Node<'_>, right: Node<'_>) -> bool {
	left.kind() == right.kind()
		&& left.start_byte() == right.start_byte()
		&& left.end_byte() == right.end_byte()
}

fn is_type_scope(scope: &Moniker) -> bool {
	matches!(
		scope.last_kind().as_deref(),
		Some(b"struct") | Some(b"trait") | Some(b"enum")
	)
}

fn first_identifier<'a>(node: Node<'_>, source: &'a [u8]) -> Option<&'a str> {
	if node.kind() == "identifier" {
		return node.utf8_text(source).ok();
	}
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		if let Some(found) = first_identifier(child, source) {
			return Some(found);
		}
	}
	None
}

pub(super) fn collect_callable_table<'src>(
	node: Node<'src>,
	source: &'src [u8],
	parent: &Moniker,
	out: &mut HashMap<(Moniker, Vec<u8>), Vec<u8>>,
) {
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		match child.kind() {
			"function_item" | "function_signature_item" => {
				let Some(name_node) = child.child_by_field_name("name") else {
					continue;
				};
				let name = node_slice(name_node, source);
				let slots = function_param_slots(child, source);
				let seg = callable_segment_slots(name, &slots);
				out.insert((parent.clone(), name.to_vec()), seg);
				if let Some(body) = child.child_by_field_name("body") {
					let scope = function_scope(child, source, parent);
					collect_callable_table(body, source, &scope, out);
				}
			}
			"struct_item" | "enum_item" | "trait_item" => {
				let Some(name_node) = child.child_by_field_name("name") else {
					continue;
				};
				let name = node_slice(name_node, source);
				let kind: &[u8] = match child.kind() {
					"struct_item" => kinds::STRUCT,
					"enum_item" => kinds::ENUM,
					"trait_item" => kinds::TRAIT,
					_ => continue,
				};
				let scope = extend_segment(parent, kind, name);
				if let Some(body) = child.child_by_field_name("body") {
					collect_callable_table(body, source, &scope, out);
				}
			}
			"impl_item" => {
				if let Some(type_node) = child.child_by_field_name("type")
					&& let Some(name) = impl_type_name(type_node, source)
				{
					let scope = extend_segment(parent, kinds::STRUCT, name.as_bytes());
					if let Some(body) = child.child_by_field_name("body") {
						collect_callable_table(body, source, &scope, out);
					}
				}
			}
			"mod_item" => {
				let Some(name_node) = child.child_by_field_name("name") else {
					continue;
				};
				let name = node_slice(name_node, source);
				let scope = extend_segment(parent, kinds::MODULE, name);
				if let Some(body) = child.child_by_field_name("body") {
					collect_callable_table(body, source, &scope, out);
				}
			}
			_ => collect_callable_table(child, source, parent, out),
		}
	}
}

pub(super) fn collect_type_table<'src>(
	node: Node<'src>,
	source: &'src [u8],
	parent: &Moniker,
	out: &mut HashMap<Vec<u8>, Moniker>,
) {
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		match child.kind() {
			"struct_item" | "enum_item" | "trait_item" | "type_item" => {
				let Some(name_node) = child.child_by_field_name("name") else {
					continue;
				};
				let name = node_slice(name_node, source);
				let kind: &[u8] = match child.kind() {
					"struct_item" => kinds::STRUCT,
					"enum_item" => kinds::ENUM,
					"trait_item" => kinds::TRAIT,
					"type_item" => kinds::TYPE,
					_ => continue,
				};
				let scope = extend_segment(parent, kind, name);
				out.insert(name.to_vec(), scope.clone());
				if let Some(body) = child.child_by_field_name("body") {
					collect_type_table(body, source, &scope, out);
				}
			}
			"function_item" | "function_signature_item" => {
				if let Some(body) = child.child_by_field_name("body") {
					let scope = function_scope(child, source, parent);
					collect_type_table(body, source, &scope, out);
				}
			}
			"impl_item" => {
				if let Some(type_node) = child.child_by_field_name("type")
					&& let Some(name) = impl_type_name(type_node, source)
				{
					let scope = extend_segment(parent, kinds::STRUCT, name.as_bytes());
					if let Some(body) = child.child_by_field_name("body") {
						collect_type_table(body, source, &scope, out);
					}
				}
			}
			"mod_item" => {
				let Some(name_node) = child.child_by_field_name("name") else {
					continue;
				};
				let name = node_slice(name_node, source);
				let scope = extend_segment(parent, kinds::MODULE, name);
				if let Some(body) = child.child_by_field_name("body") {
					collect_type_table(body, source, &scope, out);
				}
			}
			_ => collect_type_table(child, source, parent, out),
		}
	}
}

fn function_scope(node: Node<'_>, source: &[u8], parent: &Moniker) -> Moniker {
	let Some(name_node) = node.child_by_field_name("name") else {
		return parent.clone();
	};
	let name = node_slice(name_node, source);
	let slots = function_param_slots(node, source);
	let kind = if has_rust_attribute(node, source, "test") {
		kinds::TEST
	} else if is_type_scope(parent) {
		kinds::METHOD
	} else {
		kinds::FN
	};
	extend_callable_slots(parent, kind, name, &slots)
}

fn enclosing_callable_moniker(scope: &Moniker) -> Option<Moniker> {
	enclosing_segment(scope, |kind| kind == kinds::FN || kind == kinds::METHOD)
}

fn enclosing_module_moniker(scope: &Moniker) -> Option<Moniker> {
	enclosing_segment(scope, |kind| kind == kinds::MODULE)
}

fn parent_module(scope: &Moniker) -> Option<Moniker> {
	let view = scope.as_view();
	let modules: Vec<usize> = view
		.segments()
		.enumerate()
		.filter_map(|(i, seg)| (seg.kind == kinds::MODULE).then_some(i))
		.collect();
	let i = *modules.get(modules.len().checked_sub(2)?)?;
	let mut b = MonikerBuilder::from_view(view);
	b.truncate(i + 1);
	Some(b.build())
}

fn module_path_from_base(base: &Moniker, pieces: &[String]) -> Moniker {
	let mut b = MonikerBuilder::from_view(base.as_view());
	for piece in pieces {
		b.segment(kinds::MODULE, piece.as_bytes());
	}
	b.build()
}

fn module_path_from_relative(base: &Moniker, pieces: &[String]) -> Option<Moniker> {
	let mut current = base.clone();
	let mut i = 0;
	while let Some(piece) = pieces.get(i) {
		if piece == "self" {
			i += 1;
		} else if piece == "super" {
			current = parent_module(&current)?;
			i += 1;
		} else {
			break;
		}
	}
	Some(module_path_from_base(&current, &pieces[i..]))
}

fn enclosing_segment(scope: &Moniker, pred: impl Fn(&[u8]) -> bool) -> Option<Moniker> {
	let view = scope.as_view();
	let mut last_match: Option<usize> = None;
	for (i, seg) in view.segments().enumerate() {
		if pred(seg.kind) {
			last_match = Some(i);
		}
	}
	let i = last_match?;
	let mut b = MonikerBuilder::from_view(view);
	b.truncate(i + 1);
	Some(b.build())
}

fn enclosing_type_moniker(scope: &Moniker) -> Option<Moniker> {
	enclosing_segment(scope, |kind| {
		kind == kinds::STRUCT || kind == kinds::TRAIT || kind == kinds::ENUM
	})
}

fn drop_leaf_segment(target: &Moniker) -> Option<Moniker> {
	let view = target.as_view();
	let depth = view.segment_count() as usize;
	if depth < 2 {
		return None;
	}
	let mut b = MonikerBuilder::from_view(view);
	b.truncate(depth - 1);
	let parent = b.build();
	parent
		.as_view()
		.segments()
		.last()
		.is_some_and(|segment| {
			segment.kind != crate::lang::kinds::LANG && segment.kind != crate::lang::kinds::DIR
		})
		.then_some(parent)
}

fn wildcard_import_module(target: &Moniker) -> Option<Moniker> {
	let leaf = target.as_view().segments().last()?;
	if leaf.name != b"*" {
		return None;
	}
	drop_leaf_segment(target)
}

fn normalize_use_self_target(target: Moniker) -> Moniker {
	let is_self_leaf = target
		.as_view()
		.segments()
		.last()
		.is_some_and(|segment| segment.name == b"self");
	if !is_self_leaf {
		return target;
	}
	drop_leaf_segment(&target).unwrap_or(target)
}

fn unambiguous_wildcard_module(modules: &[Moniker]) -> Option<&Moniker> {
	if modules.len() == 1 {
		return modules.first();
	}
	let mut local_modules = modules
		.iter()
		.filter(|module| external_root(module).is_none());
	let local = local_modules.next()?;
	local_modules.next().is_none().then_some(local)
}

fn wildcard_path_module<'a>(modules: &'a [Moniker], pieces: &[String]) -> Option<&'a Moniker> {
	if pieces.len() <= 1 {
		return unambiguous_wildcard_module(modules);
	}
	let mut module_wildcards = modules
		.iter()
		.filter(|module| external_root(module).is_none())
		.filter(|module| module.last_kind().as_deref() == Some(kinds::MODULE));
	let module = module_wildcards.next()?;
	module_wildcards.next().is_none().then_some(module)
}

fn import_confidence(target: &Moniker) -> &'static [u8] {
	let Some(head) = target.as_view().segments().next() else {
		return b"";
	};
	if head.kind == kinds::EXTERNAL_PKG {
		kinds::CONF_EXTERNAL
	} else {
		kinds::CONF_IMPORTED
	}
}

fn external_root(target: &Moniker) -> Option<&[u8]> {
	target
		.as_view()
		.segments()
		.next()
		.and_then(|head| (head.kind == kinds::EXTERNAL_PKG).then_some(head.name))
}

fn is_rust_builtin_external_root(name: &str) -> bool {
	matches!(name, "std" | "core" | "alloc" | "proc_macro")
}

fn type_name_text<'a>(node: Node<'_>, source: &'a [u8]) -> Option<&'a str> {
	match node.kind() {
		"type_identifier" | "identifier" => node.utf8_text(source).ok(),
		"scoped_type_identifier" | "scoped_identifier" => node
			.child_by_field_name("name")
			.and_then(|n| n.utf8_text(source).ok()),
		"generic_type" => node
			.child_by_field_name("type")
			.and_then(|n| type_name_text(n, source)),
		"reference_type" | "mutable_reference_type" | "pointer_type" => node
			.child_by_field_name("type")
			.and_then(|n| type_name_text(n, source)),
		_ => None,
	}
}

fn call_argument_count(call: Node<'_>) -> usize {
	call.child_by_field_name("arguments")
		.map(argument_count)
		.unwrap_or(0)
}

fn argument_count(args: Node<'_>) -> usize {
	let mut cursor = args.walk();
	args.named_children(&mut cursor).count()
}

fn function_call_arity(node: Node<'_>, source: &[u8]) -> usize {
	function_param_slots(node, source).len()
}

fn starts_uppercase(s: &str) -> bool {
	s.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

fn is_primitive_type(name: &str) -> bool {
	matches!(
		name,
		"i8" | "i16"
			| "i32" | "i64"
			| "i128" | "isize"
			| "u8" | "u16"
			| "u32" | "u64"
			| "u128" | "usize"
			| "f32" | "f64"
			| "bool" | "char"
			| "str" | "String"
			| "()"
	)
}

fn is_placeholder_type(name: &str) -> bool {
	name == "_"
}

fn is_self_type(name: &str) -> bool {
	name == "Self"
}

fn use_is_reexport(node: Node<'_>) -> bool {
	let mut cursor = node.walk();
	node.children(&mut cursor)
		.any(|child| child.kind() == "visibility_modifier")
}

fn receiver_hint<'a>(receiver: Node<'_>, source: &'a [u8]) -> &'a [u8] {
	use crate::lang::kinds::{HINT_CALL, HINT_MEMBER, HINT_SELF};
	match receiver.kind() {
		"self" => HINT_SELF,
		"identifier" => receiver.utf8_text(source).unwrap_or("").as_bytes(),
		"field_expression" => HINT_MEMBER,
		"call_expression" => HINT_CALL,
		_ => b"",
	}
}

fn is_ident_token(s: &str) -> bool {
	let mut chars = s.chars();
	match chars.next() {
		Some(c) if c.is_alphabetic() || c == '_' => {}
		_ => return false,
	}
	chars.all(|c| c.is_alphanumeric() || c == '_')
}

fn collect_use_leaves(
	node: Node<'_>,
	source: &[u8],
	path_prefix: &mut Vec<String>,
	out: &mut Vec<Vec<String>>,
) {
	match node.kind() {
		"identifier" | "crate" | "self" | "super" => {
			if let Ok(s) = node.utf8_text(source) {
				let mut leaf = path_prefix.clone();
				leaf.push(s.to_string());
				out.push(leaf);
			}
		}
		"scoped_identifier" => {
			let mut prefix = path_prefix.clone();
			collect_scoped_path_into(node, source, &mut prefix);
			if !prefix.is_empty() {
				out.push(prefix);
			}
		}
		"scoped_use_list" => {
			let mut prefix = path_prefix.clone();
			if let Some(path) = node.child_by_field_name("path") {
				collect_scoped_path_into(path, source, &mut prefix);
			}
			if let Some(list) = node.child_by_field_name("list") {
				let mut cursor = list.walk();
				for child in list.named_children(&mut cursor) {
					collect_use_leaves(child, source, &mut prefix.clone(), out);
				}
			}
		}
		"use_list" => {
			let mut cursor = node.walk();
			for child in node.named_children(&mut cursor) {
				collect_use_leaves(child, source, &mut path_prefix.clone(), out);
			}
		}
		"use_as_clause" => {
			if let Some(path) = node.child_by_field_name("path") {
				collect_use_leaves(path, source, path_prefix, out);
			}
		}
		"use_wildcard" => {
			let mut leaf = path_prefix.clone();
			let mut cursor = node.walk();
			for child in node.named_children(&mut cursor) {
				collect_scoped_path_into(child, source, &mut leaf);
			}
			if !leaf.is_empty() {
				leaf.push("*".to_string());
				out.push(leaf);
			}
		}
		_ => {}
	}
}

fn collect_scoped_path_into(node: Node<'_>, source: &[u8], out: &mut Vec<String>) {
	if matches!(node.kind(), "scoped_identifier" | "scoped_type_identifier") {
		if let Some(path) = node.child_by_field_name("path") {
			collect_scoped_path_into(path, source, out);
		}
		if let Some(name) = node.child_by_field_name("name")
			&& let Ok(s) = name.utf8_text(source)
		{
			out.push(s.to_string());
		}
		return;
	}
	if let Ok(s) = node.utf8_text(source) {
		out.push(s.to_string());
	}
}

fn collect_use_aliases(
	node: Node<'_>,
	source: &[u8],
	path_prefix: &mut Vec<String>,
	out: &mut Vec<(Vec<String>, String)>,
) {
	match node.kind() {
		"scoped_use_list" => {
			let mut prefix = path_prefix.clone();
			if let Some(path) = node.child_by_field_name("path") {
				collect_scoped_path_into(path, source, &mut prefix);
			}
			if let Some(list) = node.child_by_field_name("list") {
				let mut cursor = list.walk();
				for child in list.named_children(&mut cursor) {
					collect_use_aliases(child, source, &mut prefix.clone(), out);
				}
			}
		}
		"use_list" => {
			let mut cursor = node.walk();
			for child in node.named_children(&mut cursor) {
				collect_use_aliases(child, source, &mut path_prefix.clone(), out);
			}
		}
		"use_as_clause" => {
			let Some(alias) = use_alias_name(node, source) else {
				return;
			};
			let Some(path) = node.child_by_field_name("path") else {
				return;
			};
			let mut target = path_prefix.clone();
			collect_scoped_path_into(path, source, &mut target);
			if !target.is_empty() {
				out.push((target, alias));
			}
		}
		_ => {}
	}
}

fn use_alias_name(node: Node<'_>, source: &[u8]) -> Option<String> {
	if let Some(alias) = node.child_by_field_name("alias")
		&& let Some(name) = type_name_text(alias, source)
	{
		return Some(name.to_string());
	}
	let text = node.utf8_text(source).ok()?;
	let (_, alias) = text.rsplit_once(" as ")?;
	let alias = alias.trim();
	is_ident_token(alias).then(|| alias.to_string())
}

fn target_under_project(module: &Moniker, rest: &[String]) -> Moniker {
	let view = module.as_view();
	let root_depth = view
		.segments()
		.enumerate()
		.filter_map(|(idx, segment)| {
			(segment.kind == kinds::DIR && segment.name == b"src").then_some(idx + 1)
		})
		.last()
		.unwrap_or(1);
	let mut b = MonikerBuilder::from_view(view);
	b.truncate(root_depth);
	append_use_pieces(&mut b, rest);
	b.build()
}

fn target_under_module(module: &Moniker, rest: &[String], walk_up: usize) -> Moniker {
	let mut base = module.clone();
	for _ in 0..walk_up {
		if let Some(parent) = rust_parent_module(&base) {
			base = parent;
		}
	}
	let mut b = MonikerBuilder::from_view(base.as_view());
	append_use_pieces(&mut b, rest);
	b.build()
}

fn rust_parent_module(scope: &Moniker) -> Option<Moniker> {
	if let Some(parent) = parent_module(scope) {
		return Some(parent);
	}
	let segments = scope.as_view().segments().collect::<Vec<_>>();
	let (module_index, module_segment) = segments
		.iter()
		.enumerate()
		.rev()
		.find(|(_, segment)| segment.kind == kinds::MODULE)?;
	if module_index == 0 || segments[module_index - 1].kind != kinds::DIR {
		return None;
	}
	if module_segment.name == b"mod" {
		return None;
	}
	let parent_dir = segments[module_index - 1];
	if parent_dir.name == b"src" {
		return None;
	}
	let mut b = MonikerBuilder::from_view(scope.as_view());
	b.truncate(module_index - 1);
	b.segment(kinds::MODULE, parent_dir.name);
	Some(b.build())
}

fn target_path_under_module(module: &Moniker, rest: &[String]) -> Moniker {
	let mut b = MonikerBuilder::from_view(module.as_view());
	for piece in rest {
		b.segment(kinds::PATH, piece.as_bytes());
	}
	b.build()
}

fn append_path_segments(base: &Moniker, rest: &[String]) -> Moniker {
	let mut b = MonikerBuilder::from_view(base.as_view());
	for piece in rest {
		b.segment(kinds::PATH, piece.as_bytes());
	}
	b.build()
}

fn append_use_pieces(b: &mut MonikerBuilder, pieces: &[String]) {
	let n = pieces.len();
	if n == 0 {
		return;
	}
	if n == 1 {
		b.segment(kinds::PATH, pieces[0].as_bytes());
		return;
	}
	for (i, piece) in pieces.iter().enumerate() {
		let kind = if i == n - 2 {
			kinds::MODULE
		} else if i == n - 1 {
			kinds::PATH
		} else {
			kinds::DIR
		};
		b.segment(kind, piece.as_bytes());
	}
}

fn target_external(module: &Moniker, path: &[String]) -> Moniker {
	let mut b = MonikerBuilder::new();
	b.project(module.as_view().project());
	b.segment(kinds::EXTERNAL_PKG, path[0].as_bytes());
	for piece in &path[1..] {
		b.segment(kinds::PATH, piece.as_bytes());
	}
	b.build()
}

fn target_external_std(module: &Moniker, pieces: &[(&str, &str)]) -> Moniker {
	let mut b = MonikerBuilder::new();
	b.project(module.as_view().project());
	b.segment(kinds::EXTERNAL_PKG, b"std");
	for (kind, name) in pieces {
		b.segment(kind.as_bytes(), name.as_bytes());
	}
	b.build()
}

fn rust_prelude_type_target(module: &Moniker, name: &[u8]) -> Option<Moniker> {
	match name {
		b"Box" => Some(target_external_std(
			module,
			&[("path", "boxed"), ("struct", "Box")],
		)),
		b"Fn" => Some(target_external_std(
			module,
			&[("path", "ops"), ("trait", "Fn")],
		)),
		b"FnMut" => Some(target_external_std(
			module,
			&[("path", "ops"), ("trait", "FnMut")],
		)),
		b"FnOnce" => Some(target_external_std(
			module,
			&[("path", "ops"), ("trait", "FnOnce")],
		)),
		b"Iterator" => Some(target_external_std(
			module,
			&[("path", "iter"), ("trait", "Iterator")],
		)),
		b"IntoIterator" => Some(target_external_std(
			module,
			&[("path", "iter"), ("trait", "IntoIterator")],
		)),
		b"AsMut" => Some(target_external_std(
			module,
			&[("path", "convert"), ("trait", "AsMut")],
		)),
		b"AsRef" => Some(target_external_std(
			module,
			&[("path", "convert"), ("trait", "AsRef")],
		)),
		b"From" => Some(target_external_std(
			module,
			&[("path", "convert"), ("trait", "From")],
		)),
		b"Into" => Some(target_external_std(
			module,
			&[("path", "convert"), ("trait", "Into")],
		)),
		b"TryFrom" => Some(target_external_std(
			module,
			&[("path", "convert"), ("trait", "TryFrom")],
		)),
		b"TryInto" => Some(target_external_std(
			module,
			&[("path", "convert"), ("trait", "TryInto")],
		)),
		b"Option" => Some(target_external_std(
			module,
			&[("path", "option"), ("enum", "Option")],
		)),
		b"Result" => Some(target_external_std(
			module,
			&[("path", "result"), ("enum", "Result")],
		)),
		b"String" => Some(target_external_std(
			module,
			&[("path", "string"), ("struct", "String")],
		)),
		b"Vec" => Some(target_external_std(
			module,
			&[("path", "vec"), ("struct", "Vec")],
		)),
		_ => None,
	}
}

fn rust_prelude_constructor_target(module: &Moniker, name: &[u8]) -> Option<Moniker> {
	match name {
		b"Ok" | b"Err" => Some(target_external_std(
			module,
			&[
				("path", "result"),
				("enum", "Result"),
				("enum_constant", std::str::from_utf8(name).unwrap_or("")),
			],
		)),
		b"Some" | b"None" => Some(target_external_std(
			module,
			&[
				("path", "option"),
				("enum", "Option"),
				("enum_constant", std::str::from_utf8(name).unwrap_or("")),
			],
		)),
		b"Box" | b"String" | b"Vec" => rust_prelude_type_target(module, name),
		_ => None,
	}
}

fn rust_std_associated_path_target(module: &Moniker, pieces: &[String]) -> Option<Moniker> {
	let head = pieces.first()?;
	if !is_primitive_type(head) && rust_prelude_type_target(module, head.as_bytes()).is_none() {
		return None;
	}
	let path = pieces
		.iter()
		.map(|piece| ("path", piece.as_str()))
		.collect::<Vec<_>>();
	Some(target_external_crate(module, "std", &path))
}

fn rust_known_associated_call_target(module: &Moniker, pieces: &[String]) -> Option<Moniker> {
	let method = pieces.last()?.as_str();
	let root = match method {
		"parse" | "parse_from" | "try_parse" | "try_parse_from" => "clap",
		_ => return None,
	};
	Some(target_external_crate(
		module,
		root,
		&[("path", "Parser"), ("method", method)],
	))
}

fn rust_builtin_derive_trait_target(module: &Moniker, name: &[u8]) -> Option<Moniker> {
	match name {
		b"Clone" => Some(target_external_std(
			module,
			&[("path", "clone"), ("trait", "Clone")],
		)),
		b"Copy" => Some(target_external_std(
			module,
			&[("path", "marker"), ("trait", "Copy")],
		)),
		b"Debug" => Some(target_external_std(
			module,
			&[("path", "fmt"), ("trait", "Debug")],
		)),
		b"Default" => Some(target_external_std(
			module,
			&[("path", "default"), ("trait", "Default")],
		)),
		b"Eq" => Some(target_external_std(
			module,
			&[("path", "cmp"), ("trait", "Eq")],
		)),
		b"Hash" => Some(target_external_std(
			module,
			&[("path", "hash"), ("trait", "Hash")],
		)),
		b"Ord" => Some(target_external_std(
			module,
			&[("path", "cmp"), ("trait", "Ord")],
		)),
		b"PartialEq" => Some(target_external_std(
			module,
			&[("path", "cmp"), ("trait", "PartialEq")],
		)),
		b"PartialOrd" => Some(target_external_std(
			module,
			&[("path", "cmp"), ("trait", "PartialOrd")],
		)),
		_ => None,
	}
}

fn rust_prelude_trait_target(module: &Moniker, name: &[u8]) -> Option<Moniker> {
	match name {
		b"AsMut" => Some(target_external_std(
			module,
			&[("path", "convert"), ("trait", "AsMut")],
		)),
		b"AsRef" => Some(target_external_std(
			module,
			&[("path", "convert"), ("trait", "AsRef")],
		)),
		b"Clone" => Some(target_external_std(
			module,
			&[("path", "clone"), ("trait", "Clone")],
		)),
		b"Debug" => Some(target_external_std(
			module,
			&[("path", "fmt"), ("trait", "Debug")],
		)),
		b"Default" => Some(target_external_std(
			module,
			&[("path", "default"), ("trait", "Default")],
		)),
		b"Eq" => Some(target_external_std(
			module,
			&[("path", "cmp"), ("trait", "Eq")],
		)),
		b"Display" => Some(target_external_std(
			module,
			&[("path", "fmt"), ("trait", "Display")],
		)),
		b"From" => Some(target_external_std(
			module,
			&[("path", "convert"), ("trait", "From")],
		)),
		b"Into" => Some(target_external_std(
			module,
			&[("path", "convert"), ("trait", "Into")],
		)),
		b"Iterator" => Some(target_external_std(
			module,
			&[("path", "iter"), ("trait", "Iterator")],
		)),
		b"PartialEq" => Some(target_external_std(
			module,
			&[("path", "cmp"), ("trait", "PartialEq")],
		)),
		b"Send" => Some(target_external_std(
			module,
			&[("path", "marker"), ("trait", "Send")],
		)),
		b"Sync" => Some(target_external_std(
			module,
			&[("path", "marker"), ("trait", "Sync")],
		)),
		b"TryFrom" => Some(target_external_std(
			module,
			&[("path", "convert"), ("trait", "TryFrom")],
		)),
		b"TryInto" => Some(target_external_std(
			module,
			&[("path", "convert"), ("trait", "TryInto")],
		)),
		_ => None,
	}
}

fn rust_known_attribute_target(module: &Moniker, name: &[u8]) -> Option<Moniker> {
	let root = match name {
		b"allow"
		| b"cfg"
		| b"cfg_attr"
		| b"default"
		| b"derive"
		| b"doc"
		| b"should_panic"
		| b"test" => "std",
		b"arg" | b"command" | b"group" | b"value" => "clap",
		b"bikeshed_postgres_type_manually_impl_from_into_datum"
		| b"commutator"
		| b"extension_sql"
		| b"inoutfuncs"
		| b"negator"
		| b"pg_extern"
		| b"pg_operator"
		| b"pg_schema"
		| b"pg_test"
		| b"pg_trigger"
		| b"pgx"
		| b"pgrx"
		| b"opname"
		| b"postgres"
		| b"sql_entity_graph" => "pgrx",
		b"error" | b"from" | b"source" => "thiserror",
		b"serde" => "serde",
		_ => return None,
	};
	Some(target_external_crate(
		module,
		root,
		&[
			("path", "attributes"),
			("fn", std::str::from_utf8(name).unwrap_or("")),
		],
	))
}

fn target_external_crate(module: &Moniker, root: &str, pieces: &[(&str, &str)]) -> Moniker {
	let mut b = MonikerBuilder::new();
	b.project(module.as_view().project());
	b.segment(kinds::EXTERNAL_PKG, root.as_bytes());
	for (kind, name) in pieces {
		b.segment(kind.as_bytes(), name.as_bytes());
	}
	b.build()
}

fn common_std_method_target(module: &Moniker, name: &[u8]) -> Moniker {
	target_external_std(
		module,
		&[
			("path", "prelude"),
			("method", std::str::from_utf8(name).unwrap_or("")),
		],
	)
}

fn rust_known_external_method_target(module: &Moniker, name: &[u8]) -> Option<Moniker> {
	let (root, type_name) = match name {
		b"with_context" => ("anyhow", "Context"),
		b"bg" => ("ratatui", "Style"),
		b"render_widget" => ("ratatui", "Frame"),
		b"as_array" => ("serde_json", "Value"),
		b"parse" | b"set_language" => ("tree_sitter", "Parser"),
		b"child_by_field_name"
		| b"children"
		| b"end_byte"
		| b"kind"
		| b"named_children"
		| b"root_node"
		| b"start_byte"
		| b"utf8_text"
		| b"walk" => ("tree_sitter", "Node"),
		_ => return None,
	};
	Some(target_external_crate(
		module,
		root,
		&[
			("path", type_name),
			("method", std::str::from_utf8(name).unwrap_or("")),
		],
	))
}

fn is_builtin_macro(name: &str) -> bool {
	matches!(
		name,
		"assert"
			| "assert_eq"
			| "assert_ne"
			| "cfg" | "compile_error"
			| "concat"
			| "dbg" | "debug_assert"
			| "debug_assert_eq"
			| "debug_assert_ne"
			| "env" | "eprintln"
			| "format"
			| "format_args"
			| "include"
			| "include_bytes"
			| "include_str"
			| "line" | "matches"
			| "module_path"
			| "option_env"
			| "panic" | "print"
			| "println"
			| "stringify"
			| "todo" | "unimplemented"
			| "unreachable"
			| "vec" | "write"
			| "writeln"
	)
}

fn rust_known_external_macro_target(module: &Moniker, name: &str) -> Option<Moniker> {
	let root = match name {
		"error" => "pgrx",
		"proptest" => "proptest",
		_ => return None,
	};
	Some(target_external_crate(
		module,
		root,
		&[("path", "macros"), ("macro", name)],
	))
}

fn parse_define_languages_variants(text: &str) -> Vec<String> {
	let Some(start) = text.find(|ch| matches!(ch, '{' | '(' | '[')) else {
		return Vec::new();
	};
	let Some(end) = text.rfind(|ch| matches!(ch, '}' | ')' | ']')) else {
		return Vec::new();
	};
	if end <= start {
		return Vec::new();
	}
	text[start + 1..end]
		.split(',')
		.filter_map(|entry| {
			let (variant, _) = entry.split_once("=>")?;
			let variant = variant.trim();
			is_ident_token(variant).then(|| variant.to_string())
		})
		.collect()
}

fn parse_derive_trait_paths(text: &str) -> Vec<String> {
	let body = text
		.trim()
		.trim_start_matches('(')
		.trim_end_matches(')')
		.trim();
	body.split(',')
		.map(str::trim)
		.filter(|path| !path.is_empty())
		.map(ToOwned::to_owned)
		.collect()
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

fn has_rust_attribute(node: Node<'_>, source: &[u8], wanted: &str) -> bool {
	rust_attribute_items(node).into_iter().any(|attr_item| {
		bare_attr_name(attr_item, source)
			.as_deref()
			.is_some_and(|name| name == wanted)
	})
}

fn rust_attribute_value(node: Node<'_>, source: &[u8], wanted: &str) -> Option<String> {
	rust_attribute_items(node)
		.into_iter()
		.find_map(|attr_item| {
			if bare_attr_name(attr_item, source).as_deref()? != wanted {
				return None;
			}
			quoted_value(attr_item.utf8_text(source).ok()?)
		})
}

fn rust_attribute_items(node: Node<'_>) -> Vec<Node<'_>> {
	let mut cursor = node.walk();
	let mut out: Vec<Node<'_>> = node
		.children(&mut cursor)
		.filter(|child| child.kind() == "attribute_item")
		.collect();
	let mut prev = node.prev_named_sibling();
	while let Some(sibling) = prev {
		if sibling.kind() != "attribute_item" {
			break;
		}
		out.push(sibling);
		prev = sibling.prev_named_sibling();
	}
	out
}

fn bare_attr_name(attr_item: Node<'_>, source: &[u8]) -> Option<String> {
	let mut cursor = attr_item.walk();
	for child in attr_item.named_children(&mut cursor) {
		if child.kind() != "attribute" {
			continue;
		}
		let mut attr_cursor = child.walk();
		for item in child.named_children(&mut attr_cursor) {
			if item.kind() == "identifier" {
				return item.utf8_text(source).ok().map(str::to_string);
			}
		}
	}
	None
}

fn quoted_value(text: &str) -> Option<String> {
	let start = text.find('"')? + 1;
	let end = text[start..].find('"')? + start;
	Some(text[start..end].to_string())
}

fn test_signature(
	framework: &[u8],
	display: &[u8],
	ignored: bool,
	ignore_reason: Option<&str>,
) -> Vec<u8> {
	let mut out = Vec::new();
	out.extend_from_slice(b"framework=");
	out.extend_from_slice(framework);
	out.extend_from_slice(if ignored {
		b";enabled=false;display="
	} else {
		b";enabled=true;display="
	});
	out.extend_from_slice(display);
	if let Some(reason) = ignore_reason {
		out.extend_from_slice(b";ignore=");
		out.extend_from_slice(sanitize_signature_value(reason).as_bytes());
	}
	out
}

fn sanitize_signature_value(value: &str) -> String {
	value
		.chars()
		.map(|c| match c {
			'\n' | '\r' | '\t' => ' ',
			';' => ',',
			_ => c,
		})
		.collect()
}

struct ParsedProptest {
	segment: String,
	signature: Vec<u8>,
	start_offset: usize,
	end_offset: usize,
}

fn parse_proptest_tests(text: &str) -> Vec<ParsedProptest> {
	let mut out = Vec::new();
	let mut cursor = 0;
	while let Some(test_rel) = text[cursor..].find("#[test]") {
		let test_offset = cursor + test_rel;
		let Some(fn_rel) = find_keyword(&text[test_offset + 7..], "fn") else {
			break;
		};
		let fn_offset = test_offset + 7 + fn_rel;
		let mut name_start = fn_offset + 2;
		while text
			.as_bytes()
			.get(name_start)
			.is_some_and(u8::is_ascii_whitespace)
		{
			name_start += 1;
		}
		let mut name_end = name_start;
		while text
			.as_bytes()
			.get(name_end)
			.is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
		{
			name_end += 1;
		}
		if name_end == name_start {
			cursor = fn_offset + 2;
			continue;
		}
		let name = &text[name_start..name_end];
		let Some(params_open_rel) = text[name_end..].find('(') else {
			break;
		};
		let params_open = name_end + params_open_rel;
		let Some(params_close) = find_matching_paren(text, params_open) else {
			break;
		};
		let params = proptest_param_names(&text[params_open + 1..params_close]).join(",");
		let segment = format!("{name}({params})");
		let attrs = text[cursor..test_offset].trim();
		let ignored = attrs.contains("#[ignore");
		let ignore_reason = ignore_reason_from_attr_text(attrs);
		out.push(ParsedProptest {
			segment,
			signature: test_signature(
				b"proptest",
				name.as_bytes(),
				ignored,
				ignore_reason.as_deref(),
			),
			start_offset: test_offset,
			end_offset: params_close + 1,
		});
		cursor = params_close + 1;
	}
	out
}

fn ignore_reason_from_attr_text(text: &str) -> Option<String> {
	let offset = text.rfind("#[ignore")?;
	quoted_value(&text[offset..])
}

fn find_keyword(text: &str, keyword: &str) -> Option<usize> {
	let mut cursor = 0;
	while let Some(rel) = text[cursor..].find(keyword) {
		let pos = cursor + rel;
		let before = pos
			.checked_sub(1)
			.and_then(|i| text.as_bytes().get(i))
			.is_none_or(|b| !b.is_ascii_alphanumeric() && *b != b'_');
		let after = text
			.as_bytes()
			.get(pos + keyword.len())
			.is_none_or(|b| !b.is_ascii_alphanumeric() && *b != b'_');
		if before && after {
			return Some(pos);
		}
		cursor = pos + keyword.len();
	}
	None
}

fn find_matching_paren(text: &str, open: usize) -> Option<usize> {
	let mut depth = 0usize;
	for (offset, byte) in text.as_bytes()[open..].iter().enumerate() {
		match byte {
			b'(' => depth += 1,
			b')' => {
				depth = depth.checked_sub(1)?;
				if depth == 0 {
					return Some(open + offset);
				}
			}
			_ => {}
		}
	}
	None
}

fn proptest_param_names(params: &str) -> Vec<&str> {
	split_top_level_commas(params)
		.into_iter()
		.map(|param| {
			let trimmed = param.trim();
			trimmed
				.split_once(" in ")
				.or_else(|| trimmed.split_once(':'))
				.map(|(name, _)| name.trim())
				.unwrap_or("_")
		})
		.collect()
}

fn split_top_level_commas(text: &str) -> Vec<&str> {
	let mut parts = Vec::new();
	let mut start = 0usize;
	let mut depth = 0usize;
	for (idx, byte) in text.as_bytes().iter().enumerate() {
		match byte {
			b'(' | b'[' | b'{' | b'<' => depth += 1,
			b')' | b']' | b'}' | b'>' => depth = depth.saturating_sub(1),
			b',' if depth == 0 => {
				parts.push(&text[start..idx]);
				start = idx + 1;
			}
			_ => {}
		}
	}
	if start < text.len() || text.is_empty() {
		parts.push(&text[start..]);
	}
	parts
}
