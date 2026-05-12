use tree_sitter::{Node, Parser, Tree};

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

use crate::lang::canonical_walker::CanonicalWalker;
use crate::lang::strategy::{LangStrategy, NodeShape, Ref, Symbol};
use crate::lang::tree_util::{find_descendant, find_named_child, node_position, node_slice};

use super::canonicalize::{
	extend_callable_arity, extend_callable_typed, extend_segment, maybe_schema,
};
use super::kinds;

use find_named_child as find_child;

pub(super) fn new_sql_parser() -> Parser {
	let mut parser = Parser::new();
	parser
		.set_language(&tree_sitter_postgres::LANGUAGE.into())
		.expect("failed to load tree-sitter-postgres SQL grammar");
	parser
}

pub(super) fn parse(source: &str) -> Tree {
	parse_with(&mut new_sql_parser(), source)
}

pub(super) fn parse_with(parser: &mut Parser, source: &str) -> Tree {
	parser
		.parse(source, None)
		.expect("tree-sitter parse returned None on a non-cancelled call")
}

pub(super) struct Strategy<'src> {
	pub(super) module: Moniker,
	pub(super) source_str: &'src str,
}

impl LangStrategy for Strategy<'_> {
	fn classify<'src>(
		&self,
		node: Node<'src>,
		_scope: &Moniker,
		source: &'src [u8],
	) -> NodeShape<'src> {
		match node.kind() {
			"CreateFunctionStmt" => classify_create_function(node, source, &self.module),
			"CreateStmt" => {
				classify_qualified_relation(node, source, &self.module, kinds::TABLE, None)
			}
			"ViewStmt" => classify_qualified_relation(
				node,
				source,
				&self.module,
				kinds::VIEW,
				find_child(node, "SelectStmt"),
			),
			"func_application" => classify_call(node, source, &self.module),
			_ => NodeShape::Recurse,
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
		if sym_kind != kinds::FUNCTION {
			return;
		}
		if !function_language(node, source).eq_ignore_ascii_case(b"plpgsql") {
			return;
		}
		let Some(body_text) = dollar_body(node, self.source_str) else {
			return;
		};
		super::body::walk_plpgsql_body(body_text, sym_moniker, &self.module, graph);
	}
}

pub(super) fn run_inner_sql(
	parser: &mut Parser,
	source: &str,
	scope: &Moniker,
	module: &Moniker,
	graph: &mut CodeGraph,
) {
	let tree = parse_with(parser, source);
	let strategy = Strategy {
		module: module.clone(),
		source_str: source,
	};
	let walker = CanonicalWalker::new(&strategy, source.as_bytes());
	walker.walk(tree.root_node(), scope, graph);
}

fn classify_create_function<'src>(
	node: Node<'src>,
	source: &'src [u8],
	module: &Moniker,
) -> NodeShape<'src> {
	let Some(func_name) = find_child(node, "func_name") else {
		return NodeShape::Recurse;
	};
	let (schema, name) = split_qualified_name(func_name, source);
	if name.is_empty() {
		return NodeShape::Recurse;
	}
	let params = find_child(node, "func_args_with_defaults");
	let arg_types = params
		.map(|p| collect_param_types(p, source))
		.unwrap_or_default();
	let parent = maybe_schema(module, schema);
	let moniker = extend_callable_typed(&parent, kinds::FUNCTION, name, &arg_types);
	let signature = arg_types.join(b",".as_ref());
	NodeShape::Symbol(Symbol {
		moniker,
		kind: kinds::FUNCTION,
		visibility: kinds::VIS_NONE,
		signature: Some(signature),
		body: None,
		position: node_position(node),
	})
}

fn classify_qualified_relation<'src>(
	node: Node<'src>,
	source: &'src [u8],
	module: &Moniker,
	kind: &'static [u8],
	body: Option<Node<'src>>,
) -> NodeShape<'src> {
	let Some(q) = find_child(node, "qualified_name") else {
		return NodeShape::Recurse;
	};
	let (schema, name) = split_qualified_name(q, source);
	if name.is_empty() {
		return NodeShape::Recurse;
	}
	let parent = maybe_schema(module, schema);
	let moniker = extend_segment(&parent, kind, name);
	NodeShape::Symbol(Symbol {
		moniker,
		kind,
		visibility: kinds::VIS_NONE,
		signature: None,
		body,
		position: node_position(node),
	})
}

fn classify_call<'src>(node: Node<'src>, source: &'src [u8], module: &Moniker) -> NodeShape<'src> {
	let Some(name_node) = find_child(node, "func_name") else {
		return NodeShape::Recurse;
	};
	let (schema, name) = split_qualified_name(name_node, source);
	if name.is_empty() {
		return NodeShape::Recurse;
	}
	let arity = func_call_arity(node);
	let parent = maybe_schema(module, schema);
	let target = extend_callable_arity(&parent, kinds::FUNCTION, name, arity);
	let s = node.start_byte() as u32;
	NodeShape::Ref(Ref {
		kind: kinds::REF_CALLS,
		target,
		confidence: kinds::CONF_UNRESOLVED,
		position: (s, s),
	})
}

pub(super) fn visit<F: FnMut(Node)>(node: Node, f: &mut F) {
	f(node);
	let mut cur = node.walk();
	for c in node.named_children(&mut cur) {
		visit(c, f);
	}
}

pub(super) fn split_qualified_name<'src>(
	node: Node<'src>,
	src: &'src [u8],
) -> (&'src [u8], &'src [u8]) {
	let mut parts: Vec<&'src [u8]> = Vec::new();
	collect_qualified_parts(node, src, &mut parts);
	match parts.len() {
		0 => (&[], &[]),
		1 => (&[], parts[0]),
		_ => (parts[0], parts[parts.len() - 1]),
	}
}

fn collect_qualified_parts<'src>(node: Node<'src>, src: &'src [u8], out: &mut Vec<&'src [u8]>) {
	let mut cur = node.walk();
	for c in node.named_children(&mut cur) {
		match c.kind() {
			"ColId" | "ColLabel" | "type_function_name" => {
				if let Some(id) = find_descendant(c, "identifier") {
					out.push(node_slice(id, src));
				}
			}
			"indirection" | "indirection_el" => collect_qualified_parts(c, src, out),
			"attr_name" => {
				if let Some(id) = find_descendant(c, "identifier") {
					out.push(node_slice(id, src));
				}
			}
			"identifier" => out.push(node_slice(c, src)),
			_ => collect_qualified_parts(c, src, out),
		}
	}
}

fn collect_param_types(params: Node, src: &[u8]) -> Vec<Vec<u8>> {
	let mut out = Vec::new();
	visit(params, &mut |n| {
		if n.kind() != "func_arg" {
			return;
		}
		if let Some(ft) = find_child(n, "func_type") {
			out.push(normalize_type(node_slice(ft, src)));
		}
	});
	out
}

fn normalize_type(raw: &[u8]) -> Vec<u8> {
	let s = std::str::from_utf8(raw).unwrap_or("");
	let mut collapsed = String::new();
	for w in s.split_whitespace() {
		if !collapsed.is_empty() {
			collapsed.push(' ');
		}
		collapsed.push_str(w);
	}
	match collapsed.as_str() {
		"int" | "integer" => b"int4".to_vec(),
		"bigint" => b"int8".to_vec(),
		"smallint" => b"int2".to_vec(),
		"real" => b"float4".to_vec(),
		"double precision" => b"float8".to_vec(),
		_ => collapsed.into_bytes(),
	}
}

fn function_language<'src>(node: Node<'src>, src: &'src [u8]) -> &'src [u8] {
	let Some(opts) = find_descendant(node, "createfunc_opt_list") else {
		return &[];
	};
	find_language_in(opts, src).unwrap_or(&[])
}

fn find_language_in<'src>(node: Node<'src>, src: &'src [u8]) -> Option<&'src [u8]> {
	if node.kind() == "createfunc_opt_item" {
		let mut has_lang = false;
		let mut cur = node.walk();
		for c in node.named_children(&mut cur) {
			if c.kind() == "kw_language" {
				has_lang = true;
			} else if has_lang && let Some(id) = find_descendant(c, "identifier") {
				return Some(node_slice(id, src));
			}
		}
	}
	let mut cur = node.walk();
	for c in node.named_children(&mut cur) {
		if let Some(found) = find_language_in(c, src) {
			return Some(found);
		}
	}
	None
}

fn dollar_body<'a>(node: Node<'_>, source: &'a str) -> Option<&'a str> {
	let dollar = find_descendant(node, "dollar_quoted_string")?;
	let full = source.get(dollar.start_byte()..dollar.end_byte())?;
	let first = full.find('$')?;
	let end_delim = full[first + 1..].find('$')? + first + 2;
	let close = full.rfind(&full[first..end_delim])?;
	if close <= end_delim {
		return None;
	}
	source.get(dollar.start_byte() + end_delim..dollar.start_byte() + close)
}

fn func_call_arity(call: Node) -> u16 {
	let args = match find_child(call, "func_arg_list") {
		Some(n) => n,
		None => return 0,
	};
	let mut count = 0u16;
	walk_arg_list(args, &mut count);
	count
}

fn walk_arg_list(list: Node, count: &mut u16) {
	let mut cur = list.walk();
	for c in list.named_children(&mut cur) {
		match c.kind() {
			"func_arg_expr" => *count = count.saturating_add(1),
			"func_arg_list" => walk_arg_list(c, count),
			_ => {}
		}
	}
}
