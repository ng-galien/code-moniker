use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, RefAttrs};
use crate::core::moniker::{Moniker, MonikerBuilder};

use super::canonicalize::{extend_callable_arity, impl_type_name, node_position};
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
			let _ = graph.add_ref(parent, target, kinds::IMPORTS_SYMBOL, Some(pos));
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
		if let Some(func) = node.child_by_field_name("function")
			&& func.kind() == "field_expression"
		{
			self.emit_self_method_call(node, func, scope, graph);
		}
		if let Some(args) = node.child_by_field_name("arguments") {
			self.walk(args, scope, graph);
		}
	}

	fn emit_self_method_call(
		&self,
		call: Node<'_>,
		func: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let Some(receiver) = func.child_by_field_name("value") else {
			return;
		};
		if receiver.kind() != "self" {
			return;
		}
		let Some(field) = func.child_by_field_name("field") else {
			return;
		};
		let Ok(name) = field.utf8_text(self.source_bytes) else {
			return;
		};
		let Some(type_moniker) = enclosing_type_moniker(scope) else {
			return;
		};
		let arity = count_call_args(call);
		let target = extend_callable_arity(&type_moniker, kinds::METHOD, name.as_bytes(), arity);
		let attrs = RefAttrs {
			confidence: kinds::CONF_UNRESOLVED,
			..RefAttrs::default()
		};
		let pos = node_position(call);
		let _ = graph.add_ref_attrs(scope, target, kinds::METHOD_CALL, Some(pos), &attrs);
	}
}

fn enclosing_type_moniker(scope: &Moniker) -> Option<Moniker> {
	let view = scope.as_view();
	let segs: Vec<_> = view.segments().collect();
	for (i, seg) in segs.iter().enumerate().rev() {
		if seg.kind == kinds::STRUCT || seg.kind == kinds::TRAIT || seg.kind == kinds::ENUM {
			let mut b = MonikerBuilder::from_view(view);
			b.truncate(i + 1);
			return Some(b.build());
		}
	}
	None
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
