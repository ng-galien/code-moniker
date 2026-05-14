use std::cell::{Cell, RefCell};
use std::collections::HashSet;

use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, DefAttrs, RefAttrs};
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
	pub(super) type_params: RefCell<Vec<HashSet<Vec<u8>>>>,
	pub(super) callable_table: HashMap<(Moniker, Vec<u8>), Vec<u8>>,
	pub(super) in_trait_impl: Cell<bool>,
	pub(super) imported_modules: RefCell<HashSet<Moniker>>,
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
			"trait_item" => self.classify_trait(node, scope, source, graph),
			"function_item" => self.classify_function(node, scope, source),
			"impl_item" => {
				self.handle_impl(node, source, graph);
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
			"call_expression" => {
				self.handle_call(node, scope, graph);
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
			self.record_param_names(params);
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
				let target = extend_segment(&self.module, kinds::TRAIT, name.as_bytes());
				out.push(crate::lang::strategy::RefSpec {
					kind: kinds::EXTENDS,
					target,
					confidence: kinds::CONF_NAME_MATCH,
					position: node_position(child),
					receiver_hint: b"",
					alias: b"",
				});
			}
		}
	}

	fn handle_impl(&self, node: Node<'_>, source: &[u8], graph: &mut CodeGraph) {
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
			let trait_moniker = MonikerBuilder::from_view(self.module.as_view())
				.segment(kinds::TRAIT, trait_name.as_bytes())
				.build();
			let _ = graph.add_ref(
				&type_moniker,
				trait_moniker,
				kinds::IMPLEMENTS,
				Some(node_position(node)),
			);
		}
		if let Some(body) = node.child_by_field_name("body") {
			let prev = self.in_trait_impl.replace(trait_node.is_some());
			self.walk_children(body, &type_moniker, graph);
			self.in_trait_impl.set(prev);
		}
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
		if self.deep {
			self.emit_pattern_defs(pattern, callable, kinds::LOCAL, node, graph);
		}
		if let Some(ty) = node.child_by_field_name("type") {
			self.emit_uses_type_walk(ty, callable, graph);
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
			self.record_param_names(params);
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
		for path in leaves {
			let target = self.build_use_target(&path);
			let _ = graph.add_ref(parent, target.clone(), kinds::IMPORTS_SYMBOL, Some(pos));
			if let Some(parent_module) = drop_leaf_segment(&target)
				&& self
					.imported_modules
					.borrow_mut()
					.insert(parent_module.clone())
			{
				let _ = graph.add_ref(parent, parent_module, kinds::IMPORTS_MODULE, Some(pos));
			}
		}
	}

	fn build_use_target(&self, path: &[String]) -> Moniker {
		if path.is_empty() {
			return self.module.clone();
		}
		match path[0].as_str() {
			"crate" => target_under_project(&self.module, &path[1..]),
			"self" => target_under_module(&self.module, &path[1..], 0),
			"super" => {
				let up = path.iter().take_while(|s| s.as_str() == "super").count();
				target_under_module(&self.module, &path[up..], up)
			}
			first if self.local_mods.contains(first) => target_under_module(&self.module, path, 0),
			_ => target_external(&self.module, path),
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
			let (target, confidence) = if is_self_type(name)
				&& let Some(t) = enclosing_type_moniker(scope)
			{
				(t, kinds::CONF_RESOLVED)
			} else {
				(
					extend_segment(&self.module, kinds::STRUCT, name.as_bytes()),
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
				kinds::INSTANTIATES,
				Some(node_position(node)),
				&attrs,
			);
		}
		self.walk_children(node, scope, graph);
	}

	fn handle_macro(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(macro_node) = node.child_by_field_name("macro") else {
			return;
		};
		let Some(name) = type_name_text(macro_node, self.source_bytes) else {
			return;
		};
		let target = extend_segment(&self.module, kinds::MACRO, name.as_bytes());
		let attrs = RefAttrs {
			confidence: kinds::CONF_UNRESOLVED,
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
		self.walk_children(node, scope, graph);
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
			let mut cursor = args.walk();
			for tok in args.named_children(&mut cursor) {
				if let Ok(trait_name) = tok.utf8_text(self.source_bytes)
					&& is_ident_token(trait_name)
				{
					let target = extend_segment(&self.module, kinds::TRAIT, trait_name.as_bytes());
					let attrs = RefAttrs {
						confidence: kinds::CONF_NAME_MATCH,
						..RefAttrs::default()
					};
					let _ = graph.add_ref_attrs(
						scope,
						target,
						kinds::ANNOTATES,
						Some(node_position(tok)),
						&attrs,
					);
				}
			}
			return;
		}
		let target = extend_segment(&self.module, kinds::FN, name.as_bytes());
		let attrs = RefAttrs {
			confidence: kinds::CONF_NAME_MATCH,
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
		let target = extend_segment(&self.module, kinds::PATH, name);
		let attrs = RefAttrs {
			confidence: kinds::CONF_NAME_MATCH,
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
		} else {
			(
				extend_segment(&self.module, kinds::METHOD, name),
				kinds::CONF_UNRESOLVED,
			)
		};
		let attrs = RefAttrs {
			confidence,
			receiver_hint: receiver_hint(receiver, self.source_bytes),
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
		if starts_uppercase(name_str) {
			let target = extend_segment(&self.module, kinds::STRUCT, name);
			let attrs = RefAttrs {
				confidence: kinds::CONF_NAME_MATCH,
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
		if self.is_local_in_scope(name)
			&& let Some(callable) = enclosing_callable_moniker(scope)
		{
			let target = extend_segment(&callable, kinds::FN, name);
			let attrs = RefAttrs {
				confidence: kinds::CONF_LOCAL,
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
		let module = self.module.clone();
		let (target, confidence) = self.resolve_callable(&module, kinds::FN, name);
		let attrs = RefAttrs {
			confidence,
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
		let path_name = func
			.child_by_field_name("path")
			.and_then(|p| type_name_text(p, self.source_bytes));
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
				self.emit_instantiates_ref(call, scope, graph, kinds::STRUCT, type_name.as_bytes());
				return;
			}
			if starts_uppercase(name_str) {
				self.emit_instantiates_ref(call, scope, graph, kinds::ENUM, type_name.as_bytes());
				return;
			}
		}
		let target = extend_segment(&self.module, kinds::FN, name);
		let attrs = RefAttrs {
			confidence: kinds::CONF_UNRESOLVED,
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

	fn emit_instantiates_ref(
		&self,
		call: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
		kind: &[u8],
		type_name: &[u8],
	) {
		let target = extend_segment(&self.module, kind, type_name);
		let attrs = RefAttrs {
			confidence: kinds::CONF_NAME_MATCH,
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

	fn record_param_names(&self, params: Node<'_>) {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			match child.kind() {
				"self_parameter" => self.record_local(b"self"),
				"parameter" => {
					if let Some(pattern) = child.child_by_field_name("pattern") {
						self.record_pattern_names(pattern);
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
			"scoped_type_identifier" => {
				if let Some(name_node) = node.child_by_field_name("name") {
					self.emit_uses_type_at(name_node, scope, graph);
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
		if is_self_type(name_str) || is_primitive_type(name_str) {
			return;
		}
		if self.is_type_param_in_scope(name) {
			return;
		}
		let target = extend_segment(&self.module, kinds::STRUCT, name);
		let attrs = RefAttrs {
			confidence: kinds::CONF_NAME_MATCH,
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
	}

	fn pop_local_scope(&self) {
		self.local_scope.borrow_mut().pop();
	}

	fn record_local(&self, name: &[u8]) {
		if let Some(top) = self.local_scope.borrow_mut().last_mut() {
			top.insert(name.to_vec());
		}
	}

	fn is_local_in_scope(&self, name: &[u8]) -> bool {
		self.local_scope
			.borrow()
			.iter()
			.any(|frame| frame.contains(name))
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

fn enclosing_callable_moniker(scope: &Moniker) -> Option<Moniker> {
	enclosing_segment(scope, |kind| kind == kinds::FN || kind == kinds::METHOD)
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
	Some(b.build())
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
		_ => None,
	}
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

fn is_self_type(name: &str) -> bool {
	name == "Self"
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
				out.push(leaf);
			}
		}
		_ => {}
	}
}

fn collect_scoped_path_into(node: Node<'_>, source: &[u8], out: &mut Vec<String>) {
	if node.kind() == "scoped_identifier" {
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

fn target_under_project(module: &Moniker, rest: &[String]) -> Moniker {
	let mut b = MonikerBuilder::new();
	b.project(module.as_view().project());
	b.segment(crate::lang::kinds::LANG, b"rs");
	append_use_pieces(&mut b, rest);
	b.build()
}

fn target_under_module(module: &Moniker, rest: &[String], walk_up: usize) -> Moniker {
	let view = module.as_view();
	let depth = view.segment_count() as usize;
	let new_depth = depth.saturating_sub(walk_up);
	let mut b = MonikerBuilder::from_view(view);
	b.truncate(new_depth);
	append_use_pieces(&mut b, rest);
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
