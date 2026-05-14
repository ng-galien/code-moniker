use tree_sitter::{Node, Parser, Tree};

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

use crate::core::code_graph::RefAttrs;

use crate::core::moniker::MonikerBuilder;
use crate::lang::canonical_walker::CanonicalWalker;
use crate::lang::strategy::{LangStrategy, NodeShape, Symbol};
use crate::lang::tree_util::{find_descendant, find_named_child, node_position, node_slice};

use crate::lang::callable::{
	CallableSlot, extend_callable_slots, join_bytes_with_comma, slot_signature_bytes,
};

use super::canonicalize::{extend_segment, maybe_schema};
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

pub(super) type CallableTable = std::collections::HashMap<(Moniker, Vec<u8>), Moniker>;

pub(super) struct Strategy<'src> {
	pub(super) module: Moniker,
	pub(super) source_str: &'src str,
	pub(super) emit_comments: bool,
	pub(super) callable_table: &'src CallableTable,
}

impl LangStrategy for Strategy<'_> {
	fn classify<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut CodeGraph,
	) -> NodeShape<'src> {
		match node.kind() {
			"comment" if self.emit_comments => NodeShape::Annotation {
				kind: kinds::COMMENT,
			},
			"comment" => NodeShape::Skip,
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
			"func_application" => {
				emit_call(
					node,
					source,
					scope,
					&self.module,
					self.callable_table,
					graph,
				);
				NodeShape::Recurse
			}
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
		if sym_kind == kinds::FUNCTION {
			emit_function_type_refs(node, source, sym_moniker, &self.module, graph);
			if function_language(node, source).eq_ignore_ascii_case(b"plpgsql")
				&& let Some(body_text) = dollar_body(node, self.source_str)
			{
				super::body::walk_plpgsql_body(
					body_text,
					sym_moniker,
					&self.module,
					self.callable_table,
					graph,
				);
			}
		} else if sym_kind == kinds::TABLE {
			emit_table_column_type_refs(node, source, sym_moniker, &self.module, graph);
		}
	}
}

pub(super) fn run_inner_sql(
	parser: &mut Parser,
	source: &str,
	scope: &Moniker,
	module: &Moniker,
	callable_table: &CallableTable,
	graph: &mut CodeGraph,
) {
	let tree = parse_with(parser, source);
	let strategy = Strategy {
		module: module.clone(),
		source_str: source,
		emit_comments: false,
		callable_table,
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
	let slots = params
		.map(|p| collect_param_slots(p, source))
		.unwrap_or_default();
	let parent = maybe_schema(module, schema);
	let moniker = extend_callable_slots(&parent, kinds::FUNCTION, name, &slots);
	let signature =
		join_bytes_with_comma(&slots.iter().map(slot_signature_bytes).collect::<Vec<_>>());
	NodeShape::Symbol(Symbol {
		moniker,
		kind: kinds::FUNCTION,
		visibility: kinds::VIS_NONE,
		signature: Some(signature),
		body: None,
		position: node_position(node),
		annotated_by: Vec::new(),
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
		annotated_by: Vec::new(),
	})
}

pub(super) fn collect_callable_table(
	root: Node<'_>,
	source: &[u8],
	module: &Moniker,
) -> CallableTable {
	let mut out = CallableTable::new();
	visit(root, &mut |n| {
		if n.kind() != "CreateFunctionStmt" {
			return;
		}
		let Some(func_name) = find_child(n, "func_name") else {
			return;
		};
		let (schema, name) = split_qualified_name(func_name, source);
		if name.is_empty() {
			return;
		}
		let params = find_child(n, "func_args_with_defaults");
		let slots = params
			.map(|p| collect_param_slots(p, source))
			.unwrap_or_default();
		let parent = maybe_schema(module, schema);
		let m = extend_callable_slots(&parent, kinds::FUNCTION, name, &slots);
		out.insert((parent, name.to_vec()), m);
	});
	out
}

fn emit_call(
	node: Node<'_>,
	source: &[u8],
	scope: &Moniker,
	module: &Moniker,
	callable_table: &CallableTable,
	graph: &mut CodeGraph,
) {
	let Some(name_node) = find_child(node, "func_name") else {
		return;
	};
	let (schema, name) = split_qualified_name(name_node, source);
	if name.is_empty() {
		return;
	}
	let mut confidence =
		if schema == b"pg_catalog" || (schema.is_empty() && is_builtin_function(name)) {
			kinds::CONF_EXTERNAL
		} else {
			kinds::CONF_NAME_MATCH
		};
	let target = if confidence == kinds::CONF_EXTERNAL && schema != b"pg_catalog" {
		let mut b = MonikerBuilder::new();
		b.project(module.as_view().project());
		b.segment(kinds::EXTERNAL_PKG, b"pg_catalog");
		b.segment(kinds::PATH, name);
		b.build()
	} else {
		let parent = maybe_schema(module, schema);
		if let Some(resolved) = callable_table.get(&(parent.clone(), name.to_vec())) {
			confidence = kinds::CONF_RESOLVED;
			resolved.clone()
		} else {
			extend_segment(&parent, kinds::FUNCTION, name)
		}
	};
	let s = node.start_byte() as u32;
	let attrs = RefAttrs {
		confidence,
		..RefAttrs::default()
	};
	let _ = graph.add_ref_attrs(scope, target, kinds::REF_CALLS, Some((s, s)), &attrs);
}

fn is_builtin_function(name: &[u8]) -> bool {
	matches!(
		name,
		b"format"
			| b"format_type"
			| b"to_regtype"
			| b"to_regtypemod"
			| b"to_regclass"
			| b"to_regproc"
			| b"current_setting"
			| b"current_database"
			| b"current_schema"
			| b"current_user"
			| b"session_user"
			| b"version"
			| b"now" | b"clock_timestamp"
			| b"transaction_timestamp"
			| b"statement_timestamp"
			| b"timeofday"
			| b"coalesce"
			| b"nullif"
			| b"greatest"
			| b"least"
			| b"length"
			| b"char_length"
			| b"character_length"
			| b"octet_length"
			| b"lower"
			| b"upper"
			| b"initcap"
			| b"substring"
			| b"substr"
			| b"trim" | b"ltrim"
			| b"rtrim"
			| b"btrim"
			| b"replace"
			| b"translate"
			| b"position"
			| b"strpos"
			| b"concat"
			| b"concat_ws"
			| b"string_agg"
			| b"array_agg"
			| b"array_length"
			| b"array_to_string"
			| b"string_to_array"
			| b"unnest"
			| b"generate_series"
			| b"jsonb_build_object"
			| b"jsonb_build_array"
			| b"jsonb_object_keys"
			| b"to_json"
			| b"to_jsonb"
			| b"row_to_json"
			| b"abs" | b"floor"
			| b"ceil" | b"ceiling"
			| b"round"
			| b"trunc"
			| b"mod" | b"power"
			| b"sqrt" | b"random"
			| b"count"
			| b"sum" | b"avg"
			| b"min" | b"max"
			| b"nextval"
			| b"currval"
			| b"setval"
			| b"pg_typeof"
			| b"pg_size_pretty"
	)
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

fn collect_param_slots(params: Node, src: &[u8]) -> Vec<CallableSlot> {
	let mut out = Vec::new();
	visit(params, &mut |n| {
		if n.kind() != "func_arg" {
			return;
		}
		let r#type = find_child(n, "func_type")
			.map(|ft| normalize_type(node_slice(ft, src)))
			.unwrap_or_default();
		let name = find_child(n, "param_name")
			.map(|pn| node_slice(pn, src).to_vec())
			.unwrap_or_default();
		out.push(CallableSlot { name, r#type });
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

fn emit_function_type_refs(
	node: Node<'_>,
	source: &[u8],
	source_moniker: &Moniker,
	module: &Moniker,
	graph: &mut CodeGraph,
) {
	if let Some(params) = find_child(node, "func_args_with_defaults") {
		visit(params, &mut |n| {
			if n.kind() != "func_arg" {
				return;
			}
			if let Some(ft) = find_child(n, "func_type") {
				emit_uses_type(ft, source, source_moniker, module, graph);
			}
		});
	}
	if let Some(ft) = find_descendant(node, "func_return")
		&& let Some(t) = find_descendant(ft, "func_type")
	{
		emit_uses_type(t, source, source_moniker, module, graph);
	}
}

fn emit_table_column_type_refs(
	node: Node<'_>,
	source: &[u8],
	source_moniker: &Moniker,
	module: &Moniker,
	graph: &mut CodeGraph,
) {
	visit(node, &mut |n| {
		if n.kind() != "columnDef" {
			return;
		}
		if let Some(t) = find_child(n, "Typename") {
			emit_uses_type(t, source, source_moniker, module, graph);
		}
	});
}

fn emit_uses_type(
	type_node: Node<'_>,
	source: &[u8],
	source_moniker: &Moniker,
	module: &Moniker,
	graph: &mut CodeGraph,
) {
	let raw = node_slice(type_node, source);
	let canonical = normalize_type(raw);
	if canonical.is_empty() {
		return;
	}
	let (target, confidence) = type_target(&canonical, module);
	let attrs = RefAttrs {
		confidence,
		..RefAttrs::default()
	};
	let _ = graph.add_ref_attrs(
		source_moniker,
		target,
		kinds::USES_TYPE,
		Some(node_position(type_node)),
		&attrs,
	);
}

fn type_target(canonical: &[u8], module: &Moniker) -> (Moniker, &'static [u8]) {
	if is_builtin_type(canonical) {
		let mut b = MonikerBuilder::new();
		b.project(module.as_view().project());
		b.segment(kinds::EXTERNAL_PKG, b"pg_catalog");
		b.segment(kinds::PATH, canonical);
		return (b.build(), kinds::CONF_EXTERNAL);
	}
	let target = extend_segment(module, kinds::TYPE, canonical);
	(target, kinds::CONF_NAME_MATCH)
}

fn is_builtin_type(name: &[u8]) -> bool {
	matches!(
		name,
		b"int2"
			| b"int4" | b"int8"
			| b"float4"
			| b"float8"
			| b"numeric"
			| b"text" | b"varchar"
			| b"bpchar"
			| b"char" | b"bool"
			| b"date" | b"time"
			| b"timestamp"
			| b"timestamptz"
			| b"interval"
			| b"uuid" | b"json"
			| b"jsonb"
			| b"bytea"
			| b"oid" | b"regclass"
			| b"regproc"
			| b"regprocedure"
			| b"regtype"
			| b"cstring"
			| b"name" | b"void"
			| b"trigger"
			| b"record"
			| b"any" | b"anyelement"
			| b"anyarray"
	)
}
