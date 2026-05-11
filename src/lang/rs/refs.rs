use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, RefAttrs};
use crate::core::moniker::{Moniker, MonikerBuilder};

use super::canonicalize::{extend_callable_arity, extend_segment, impl_type_name, node_position};
use super::kinds;
use super::walker::Walker;

impl<'src> Walker<'src> {
	pub(super) fn handle_use(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let Some(arg) = node.child_by_field_name("argument") else {
			return;
		};
		let pos = node_position(node);
		let mut leaves: Vec<Vec<String>> = Vec::new();
		collect_use_leaves(arg, self.source_bytes, &mut Vec::new(), &mut leaves);
		for path in leaves {
			let target = self.build_use_target(&path);
			let _ = graph.add_ref(parent, target.clone(), kinds::IMPORTS_SYMBOL, Some(pos));
			if let Some(parent_module) = drop_leaf_segment(&target) {
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

	pub(super) fn handle_impl_trait_for(
		&self,
		impl_node: Node<'_>,
		type_moniker: &Moniker,
		graph: &mut CodeGraph,
	) {
		let Some(trait_node) = impl_node.child_by_field_name("trait") else {
			return;
		};
		let Some(trait_name) = impl_type_name(trait_node, self.source_bytes) else {
			return;
		};
		let trait_moniker = MonikerBuilder::from_view(self.module.as_view())
			.segment(kinds::TRAIT, trait_name.as_bytes())
			.build();
		let _ = graph.add_ref(
			type_moniker,
			trait_moniker,
			kinds::IMPLEMENTS,
			Some(node_position(impl_node)),
		);
	}

	pub(super) fn handle_call(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if let Some(func) = node.child_by_field_name("function") {
			match func.kind() {
				"field_expression" => self.emit_method_call(node, func, scope, graph),
				"identifier" => self.emit_free_fn_call(node, func, scope, graph),
				"scoped_identifier" => self.emit_path_call(node, func, scope, graph),
				_ => {}
			}
		}
		if let Some(args) = node.child_by_field_name("arguments") {
			self.walk(args, scope, graph);
		}
	}

	pub(super) fn handle_struct_literal(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		if let Some(name_node) = node.child_by_field_name("name")
			&& let Some(name) = type_name_text(name_node, self.source_bytes)
		{
			let target = extend_segment(&self.module, kinds::STRUCT, name.as_bytes());
			let attrs = RefAttrs {
				confidence: kinds::CONF_NAME_MATCH,
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
		self.walk(node, scope, graph);
	}

	pub(super) fn handle_field_declaration(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		if let Some(ty) = node.child_by_field_name("type") {
			self.emit_uses_type_walk(ty, scope, graph);
		}
	}

	pub(super) fn handle_macro(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(macro_node) = node.child_by_field_name("macro") else {
			return;
		};
		let Some(name) = type_name_text(macro_node, self.source_bytes) else {
			return;
		};
		let target = extend_callable_arity(&self.module, kinds::FN, name.as_bytes(), 0);
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
		self.walk(node, scope, graph);
	}

	pub(super) fn handle_attribute(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
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

	pub(super) fn handle_trait_bounds_extends(
		&self,
		trait_def: &Moniker,
		bounds: Node<'_>,
		graph: &mut CodeGraph,
	) {
		let mut cursor = bounds.walk();
		for child in bounds.named_children(&mut cursor) {
			if child.kind() == "lifetime" {
				continue;
			}
			if let Some(name) = type_name_text(child, self.source_bytes) {
				let target = extend_segment(&self.module, kinds::TRAIT, name.as_bytes());
				let attrs = RefAttrs {
					confidence: kinds::CONF_NAME_MATCH,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(
					trait_def,
					target,
					kinds::EXTENDS,
					Some(node_position(child)),
					&attrs,
				);
			}
		}
	}

	pub(super) fn handle_identifier_read(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		if !self.deep {
			return;
		}
		let Ok(name) = node.utf8_text(self.source_bytes) else {
			return;
		};
		if !self.is_local_in_scope(name.as_bytes()) {
			return;
		}
		let Some(callable) = enclosing_callable_moniker(scope) else {
			return;
		};
		let target = extend_segment(&callable, kinds::LOCAL, name.as_bytes());
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

	pub(super) fn handle_scoped_read(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let Some(name_node) = node.child_by_field_name("name") else {
			return;
		};
		let Ok(name) = name_node.utf8_text(self.source_bytes) else {
			return;
		};
		let target = extend_segment(&self.module, kinds::PATH, name.as_bytes());
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

	pub(super) fn emit_uses_type_walk(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
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
		let Ok(name) = name_node.utf8_text(self.source_bytes) else {
			return;
		};
		if is_self_type(name) || is_primitive_type(name) {
			return;
		}
		if self.is_type_param_in_scope(name.as_bytes()) {
			return;
		}
		let target = extend_segment(&self.module, kinds::STRUCT, name.as_bytes());
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
		let Ok(name) = field.utf8_text(self.source_bytes) else {
			return;
		};
		let arity = count_call_args(call);
		let target = if receiver.kind() == "self"
			&& let Some(t) = enclosing_type_moniker(scope)
		{
			extend_callable_arity(&t, kinds::METHOD, name.as_bytes(), arity)
		} else {
			extend_callable_arity(&self.module, kinds::METHOD, name.as_bytes(), arity)
		};
		let attrs = RefAttrs {
			confidence: kinds::CONF_UNRESOLVED,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			target,
			kinds::METHOD_CALL,
			Some(node_position(call)),
			&attrs,
		);
		self.dispatch(receiver, scope, graph);
	}

	fn emit_free_fn_call(
		&self,
		call: Node<'_>,
		func: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let Ok(name) = func.utf8_text(self.source_bytes) else {
			return;
		};
		let arity = count_call_args(call);
		if starts_uppercase(name) {
			let target = extend_segment(&self.module, kinds::STRUCT, name.as_bytes());
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
		if self.is_local_in_scope(name.as_bytes())
			&& let Some(callable) = enclosing_callable_moniker(scope)
		{
			let target = extend_callable_arity(&callable, kinds::FN, name.as_bytes(), arity);
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
		let target = extend_callable_arity(&self.module, kinds::FN, name.as_bytes(), arity);
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
		let Ok(name) = name_node.utf8_text(self.source_bytes) else {
			return;
		};
		let arity = count_call_args(call);
		let path_name = func
			.child_by_field_name("path")
			.and_then(|p| type_name_text(p, self.source_bytes));
		if let Some(type_name) = path_name
			&& starts_uppercase(type_name)
		{
			if name == "new" {
				self.emit_instantiates_ref(call, scope, graph, kinds::STRUCT, type_name.as_bytes());
				return;
			}
			if starts_uppercase(name) {
				self.emit_instantiates_ref(call, scope, graph, kinds::ENUM, type_name.as_bytes());
				return;
			}
		}
		let target = extend_callable_arity(&self.module, kinds::FN, name.as_bytes(), arity);
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
}

fn count_call_args(call: Node<'_>) -> u16 {
	let Some(args) = call.child_by_field_name("arguments") else {
		return 0;
	};
	let mut count = 0u16;
	let mut cursor = args.walk();
	for child in args.named_children(&mut cursor) {
		if !matches!(child.kind(), "line_comment" | "block_comment") {
			count = count.saturating_add(1);
		}
	}
	count
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

fn is_ident_token(s: &str) -> bool {
	let mut chars = s.chars();
	match chars.next() {
		Some(c) if c.is_alphabetic() || c == '_' => {}
		_ => return false,
	}
	chars.all(|c| c.is_alphanumeric() || c == '_')
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

fn is_self_type(name: &str) -> bool {
	name == "Self"
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
			collect_scoped_path(node, source, &mut prefix);
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

fn collect_scoped_path(node: Node<'_>, source: &[u8], out: &mut Vec<String>) {
	collect_scoped_path_into(node, source, out);
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
