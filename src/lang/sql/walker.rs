//! Top-level statement walker using the `tree_sitter_postgres::LANGUAGE`
//! grammar. Pure-Rust — no postgres runtime, no symbol clash with pgrx.

use tree_sitter::{Node, Parser, Tree};

use crate::core::code_graph::{CodeGraph, DefAttrs, Position, RefAttrs};
use crate::core::moniker::Moniker;

use super::body;
use super::canonicalize::{
	extend_callable_arity, extend_callable_typed, extend_segment, maybe_schema,
};
use super::kinds;

pub(super) fn parse(source: &str) -> Tree {
	let mut parser = Parser::new();
	parser
		.set_language(&tree_sitter_postgres::LANGUAGE.into())
		.expect("failed to load tree-sitter-postgres SQL grammar");
	parser
		.parse(source, None)
		.expect("tree-sitter parse returned None on a non-cancelled call")
}

pub(super) struct Walker<'src> {
	pub(super) source: &'src str,
	pub(super) module: Moniker,
	#[allow(dead_code)]
	pub(super) deep: bool,
}

impl<'src> Walker<'src> {
	fn source_bytes(&self) -> &'src [u8] {
		self.source.as_bytes()
	}

	pub(super) fn walk(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			self.dispatch(child, scope, graph);
		}
	}

	pub(super) fn dispatch(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		match node.kind() {
			"toplevel_stmt" => self.handle_toplevel_stmt(node, scope, graph),
			_ => self.walk(node, scope, graph),
		}
	}

	fn handle_toplevel_stmt(&self, top: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(stmt) = find_child(top, "stmt") else {
			return;
		};
		let Some(inner) = stmt.named_child(0) else {
			return;
		};
		match inner.kind() {
			"CreateFunctionStmt" => self.handle_create_function(inner, scope, graph),
			"CreateStmt" => self.handle_create_table(inner, scope, graph),
			"ViewStmt" => self.handle_view(inner, scope, graph),
			_ => collect_calls_in(inner, self.source_bytes(), scope, &self.module, graph),
		}
	}

	fn handle_create_function(&self, node: Node<'_>, module: &Moniker, graph: &mut CodeGraph) {
		let Some(func_name) = find_child(node, "func_name") else {
			return;
		};
		let (schema, name) = split_qualified_name(func_name, self.source_bytes());
		let params = find_child(node, "func_args_with_defaults");
		let arg_types = params
			.map(|p| collect_param_types(p, self.source_bytes()))
			.unwrap_or_default();
		let parent = maybe_schema(module, &schema);
		let func_moniker = extend_callable_typed(&parent, kinds::FUNCTION, &name, &arg_types);
		let signature = arg_types.join(b",".as_ref());
		let attrs = DefAttrs {
			visibility: kinds::VIS_NONE,
			signature: &signature,
			..DefAttrs::default()
		};
		if graph
			.add_def_attrs(
				func_moniker.clone(),
				kinds::FUNCTION,
				module,
				node_position(node),
				&attrs,
			)
			.is_err()
		{
			return;
		}

		if function_language(node, self.source_bytes()).eq_ignore_ascii_case(b"plpgsql")
			&& let Some(body_text) = dollar_body(node, self.source)
		{
			body::walk_plpgsql_body(body_text, &func_moniker, module, graph);
		}
	}

	fn handle_create_table(&self, node: Node<'_>, module: &Moniker, graph: &mut CodeGraph) {
		let Some(q) = find_child(node, "qualified_name") else {
			return;
		};
		let (schema, name) = split_qualified_name(q, self.source_bytes());
		if name.is_empty() {
			return;
		}
		let parent = maybe_schema(module, &schema);
		let moniker = extend_segment(&parent, kinds::TABLE, &name);
		let _ = graph.add_def(moniker, kinds::TABLE, module, node_position(node));
	}

	fn handle_view(&self, node: Node<'_>, module: &Moniker, graph: &mut CodeGraph) {
		let Some(q) = find_child(node, "qualified_name") else {
			return;
		};
		let (schema, name) = split_qualified_name(q, self.source_bytes());
		if name.is_empty() {
			return;
		}
		let parent = maybe_schema(module, &schema);
		let moniker = extend_segment(&parent, kinds::VIEW, &name);
		let _ = graph.add_def(moniker.clone(), kinds::VIEW, module, node_position(node));

		if let Some(sel) = find_child(node, "SelectStmt") {
			collect_calls_in(sel, self.source_bytes(), &moniker, module, graph);
		}
	}
}

pub(super) fn collect_calls_in(
	node: Node,
	src: &[u8],
	source_def: &Moniker,
	module: &Moniker,
	graph: &mut CodeGraph,
) {
	visit(node, &mut |n| {
		if n.kind() == "func_application" {
			emit_call(n, src, source_def, module, graph);
		}
	});
}

fn emit_call(
	call: Node,
	src: &[u8],
	source_def: &Moniker,
	module: &Moniker,
	graph: &mut CodeGraph,
) {
	let name_node = match find_child(call, "func_name") {
		Some(n) => n,
		None => return,
	};
	let (schema, name) = split_qualified_name(name_node, src);
	if name.is_empty() {
		return;
	}
	let arity = func_call_arity(call);
	let parent = maybe_schema(module, &schema);
	let target = extend_callable_arity(&parent, kinds::FUNCTION, &name, arity);
	let position = node_position(call).map(|(s, _)| (s, s));
	let attrs = RefAttrs {
		confidence: kinds::CONF_UNRESOLVED,
		..RefAttrs::default()
	};
	let _ = graph.add_ref_attrs(source_def, target, kinds::REF_CALLS, position, &attrs);
}

/// `func_arg_list` is left-recursive — `(a, b, c)` produces
/// `func_arg_list(func_arg_list(func_arg_list(a), b), c)`. Walk only the chain;
/// `func_arg_expr` may itself contain another `func_application` whose own
/// args must not be counted.
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

fn visit<F: FnMut(Node)>(node: Node, f: &mut F) {
	f(node);
	let mut cur = node.walk();
	for c in node.named_children(&mut cur) {
		visit(c, f);
	}
}

fn find_child<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
	let mut cur = node.walk();
	node.named_children(&mut cur).find(|c| c.kind() == kind)
}

fn find_descendant<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
	if node.kind() == kind {
		return Some(node);
	}
	let mut cur = node.walk();
	for c in node.named_children(&mut cur) {
		if let Some(d) = find_descendant(c, kind) {
			return Some(d);
		}
	}
	None
}

/// `func_name` / `qualified_name` shape:
/// `ColId(name)` then optional `indirection > indirection_el > attr_name(name)`.
/// One segment → (empty schema, last). Two+ segments → (first, last).
fn split_qualified_name(node: Node, src: &[u8]) -> (Vec<u8>, Vec<u8>) {
	let mut parts: Vec<Vec<u8>> = Vec::new();
	collect_qualified_parts(node, src, &mut parts);
	match parts.len() {
		0 => (Vec::new(), Vec::new()),
		1 => (Vec::new(), parts.into_iter().next().unwrap()),
		_ => {
			let last = parts.last().cloned().unwrap();
			let first = parts.into_iter().next().unwrap();
			(first, last)
		}
	}
}

fn collect_qualified_parts(node: Node, src: &[u8], out: &mut Vec<Vec<u8>>) {
	let mut cur = node.walk();
	for c in node.named_children(&mut cur) {
		match c.kind() {
			"ColId" | "ColLabel" | "type_function_name" => {
				if let Some(id) = find_descendant(c, "identifier") {
					out.push(node_bytes(id, src));
				}
			}
			"indirection" | "indirection_el" => collect_qualified_parts(c, src, out),
			"attr_name" => {
				if let Some(id) = find_descendant(c, "identifier") {
					out.push(node_bytes(id, src));
				}
			}
			"identifier" => out.push(node_bytes(c, src)),
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
			let raw = node_bytes(ft, src);
			let bytes = normalize_type(&raw);
			out.push(bytes);
		}
	});
	out
}

/// `func_type` text comes back with `pg_catalog.` qualification on builtin
/// keyword aliases (`int → pg_catalog.int4`) and arbitrary surrounding
/// whitespace. Collapse runs to single spaces (preserves `double precision`)
/// and strip the `pg_catalog.` prefix.
fn normalize_type(raw: &[u8]) -> Vec<u8> {
	let s = std::str::from_utf8(raw).unwrap_or("");
	let trimmed = s.trim();
	let collapsed: String = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
	let canon = match collapsed.as_str() {
		"int" => "int4".to_string(),
		"integer" => "int4".to_string(),
		"bigint" => "int8".to_string(),
		"smallint" => "int2".to_string(),
		"real" => "float4".to_string(),
		"double precision" => "float8".to_string(),
		_ => collapsed,
	};
	canon.into_bytes()
}

fn function_language(node: Node, src: &[u8]) -> Vec<u8> {
	let opts = match find_descendant(node, "createfunc_opt_list") {
		Some(n) => n,
		None => return Vec::new(),
	};
	let mut found = Vec::new();
	visit(opts, &mut |item| {
		if item.kind() != "createfunc_opt_item" {
			return;
		}
		let mut has_lang = false;
		let mut cur = item.walk();
		for c in item.named_children(&mut cur) {
			if c.kind() == "kw_language" {
				has_lang = true;
			} else if has_lang && found.is_empty() {
				if let Some(id) = find_descendant(c, "identifier") {
					found = node_bytes(id, src);
				}
			}
		}
	});
	found
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

fn node_bytes(node: Node, src: &[u8]) -> Vec<u8> {
	src[node.start_byte()..node.end_byte().min(src.len())].to_vec()
}

fn node_position(node: Node) -> Option<Position> {
	let s = node.start_byte() as u32;
	let e = node.end_byte() as u32;
	Some((s, e))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::moniker::MonikerBuilder;
	use crate::lang::sql::Presets;
	use crate::lang::sql::extract;

	fn anchor() -> Moniker {
		MonikerBuilder::new().project(b"app").build()
	}

	fn run(uri: &str, src: &str) -> CodeGraph {
		extract(uri, src, &anchor(), false, &Presets::default())
	}

	fn def_monikers(g: &CodeGraph) -> Vec<String> {
		g.defs()
			.map(|d| crate::core::uri::to_uri(&d.moniker, &Default::default()).unwrap())
			.collect()
	}

	fn ref_targets(g: &CodeGraph) -> Vec<String> {
		g.refs()
			.map(|r| crate::core::uri::to_uri(&r.target, &Default::default()).unwrap())
			.collect()
	}

	#[test]
	fn qualified_function_emits_full_signature() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION public.bar(a int, b text) RETURNS int LANGUAGE sql AS $$ SELECT 1 $$;",
		);
		assert!(
			def_monikers(&g).iter().any(|m| m
				== "code+moniker://app/lang:sql/module:foo/schema:public/function:bar(int4,text)"),
			"got defs: {:?}",
			def_monikers(&g)
		);
		let func = g
			.defs()
			.find(|d| d.kind == b"function")
			.expect("function def");
		assert_eq!(func.signature, b"int4,text");
	}

	#[test]
	fn unqualified_function_omits_schema() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION bar() RETURNS void LANGUAGE sql AS $$ $$;",
		);
		assert!(
			def_monikers(&g)
				.iter()
				.any(|m| m == "code+moniker://app/lang:sql/module:foo/function:bar()")
		);
		assert_eq!(g.defs().filter(|d| d.kind == b"function").count(), 1);
	}

	#[test]
	fn overloads_with_different_types_both_land() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION m(x int) RETURNS int LANGUAGE sql AS $$ SELECT x $$;\
			 CREATE FUNCTION m(x text) RETURNS text LANGUAGE sql AS $$ SELECT x $$;",
		);
		assert_eq!(g.defs().filter(|d| d.kind == b"function").count(), 2);
	}

	#[test]
	fn create_table_emits_table_under_schema() {
		let g = run(
			"schema.sql",
			"CREATE TABLE esac.module_t (id uuid PRIMARY KEY);",
		);
		assert!(
			def_monikers(&g).iter().any(
				|m| m == "code+moniker://app/lang:sql/module:schema/schema:esac/table:module_t"
			)
		);
	}

	#[test]
	fn create_view_emits_view_and_call_ref() {
		let g = run("schema.sql", "CREATE VIEW v AS SELECT esac.foo() FROM t;");
		assert!(
			def_monikers(&g)
				.iter()
				.any(|m| m == "code+moniker://app/lang:sql/module:schema/view:v")
		);
		assert!(
			ref_targets(&g).iter().any(
				|t| t == "code+moniker://app/lang:sql/module:schema/schema:esac/function:foo()"
			),
			"got refs: {:?}",
			ref_targets(&g)
		);
	}

	#[test]
	fn top_level_select_emits_qualified_call() {
		let g = run("foo.sql", "SELECT public.bar(1, 2);");
		assert!(
			ref_targets(&g).iter().any(
				|t| t == "code+moniker://app/lang:sql/module:foo/schema:public/function:bar(2)"
			),
			"got refs: {:?}",
			ref_targets(&g)
		);
	}

	#[test]
	fn unqualified_top_level_call_omits_schema() {
		let g = run("foo.sql", "SELECT bar();");
		assert!(
			ref_targets(&g)
				.iter()
				.any(|t| t == "code+moniker://app/lang:sql/module:foo/function:bar()"),
			"got refs: {:?}",
			ref_targets(&g)
		);
	}

	#[test]
	fn empty_source_yields_only_module_root() {
		let g = run("db/functions/plan/create_plan.sql", "");
		let defs: Vec<_> = g.defs().collect();
		assert_eq!(defs.len(), 1);
		assert_eq!(
			crate::core::uri::to_uri(&defs[0].moniker, &Default::default()).unwrap(),
			"code+moniker://app/lang:sql/dir:db/dir:functions/dir:plan/module:create_plan"
		);
	}

	#[test]
	fn nested_call_arity_is_outer_only() {
		// `func_arg_list` is left-recursive; a naive `visit` would count the
		// inner `g(a, b)` args (2) on top of the outer single arg.
		let g = run("foo.sql", "SELECT f(g(a, b));");
		assert!(
			ref_targets(&g)
				.iter()
				.any(|t| t == "code+moniker://app/lang:sql/module:foo/function:f(1)"),
			"outer call f should have arity 1, got refs: {:?}",
			ref_targets(&g)
		);
		assert!(
			ref_targets(&g)
				.iter()
				.any(|t| t == "code+moniker://app/lang:sql/module:foo/function:g(2)"),
			"inner call g should have arity 2, got refs: {:?}",
			ref_targets(&g)
		);
	}

	#[test]
	fn function_def_has_byte_range() {
		let g = run(
			"pkg.sql",
			"CREATE FUNCTION f() RETURNS int LANGUAGE sql AS $$ SELECT 1 $$;",
		);
		let func = g.defs().find(|d| d.kind == b"function").expect("function");
		let (s, e) = func.position.expect("position");
		assert!(s <= e, "start={s} end={e}");
	}
}
