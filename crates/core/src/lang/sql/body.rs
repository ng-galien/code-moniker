use std::ops::Range;

use tree_sitter::{Node, Parser, Tree};

use crate::core::moniker::Moniker;
use crate::lang::tree_util::{find_descendant, find_named_child};
use crate::lang::{ParsedDocument, SyntaxEntryPoint, SyntaxInjection, covering_node};

use super::sdk_pipeline::discover::{
	CallableSearchPaths, SqlBuilder, new_sql_parser, run_inner_sql,
};

pub(super) fn parse_document(primary: Tree, source: &str) -> ParsedDocument {
	let mut injections = Vec::new();
	collect_routine_injections(primary.root_node(), source, &mut injections);
	ParsedDocument::with_injections(primary, injections)
}

fn collect_routine_injections(node: Node<'_>, source: &str, injections: &mut Vec<SyntaxInjection>) {
	let body = match node.kind() {
		"CreateFunctionStmt" => routine_body(node, source),
		"DoStmt" => do_body(node, source),
		_ => None,
	};
	if let Some(body) = body
		&& let Some(tree) = parse_embedded(&body.language, body.text)
	{
		let entry_point = if body.language_tag() == "plpgsql" {
			SyntaxEntryPoint::Block
		} else {
			SyntaxEntryPoint::Script
		};
		let nested = if body.language_tag() == "plpgsql" {
			sql_expression_injections(&tree, body.text, body.content_byte_range.start)
		} else {
			Vec::new()
		};
		injections.push(
			SyntaxInjection::new(
				body.language_tag(),
				entry_point,
				body.host_byte_range,
				body.content_byte_range,
				tree,
			)
			.with_nested(nested),
		);
		return;
	}
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		collect_routine_injections(child, source, injections);
	}
}

/// Injects opaque PL/pgSQL `sql_expression` regions in document coordinates.
/// Complete statements use the script grammar; fragment errors stay local to
/// their injection and do not mark the containing document invalid.
pub(super) fn sql_expression_injections(
	tree: &Tree,
	content: &str,
	origin: usize,
) -> Vec<SyntaxInjection> {
	let mut injections = Vec::new();
	collect_sql_expressions(tree.root_node(), content, origin, &mut injections);
	injections
}

fn collect_sql_expressions(
	node: Node<'_>,
	content: &str,
	origin: usize,
	injections: &mut Vec<SyntaxInjection>,
) {
	if node.kind() == "sql_expression"
		&& let Some(text) = content.get(node.start_byte()..node.end_byte())
	{
		let entry_point = sql_expression_entry(node);
		let range = origin + node.start_byte()..origin + node.end_byte();
		if let Some(injection) = sql_expression_injection(entry_point, range, text) {
			injections.push(injection);
		}
		return;
	}
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		collect_sql_expressions(child, content, origin, injections);
	}
}

/// PL/pgSQL expressions are parsed the way PostgreSQL parses them: prefixed with `SELECT `,
/// exactly like plpgsql's read_sql_expression. The injection's tree keeps the wrapper, and
/// `analysis_prefix` marks it so a renderer roots the region at the expression itself — a
/// script-recovery tree over a bare expression yields wrong facts, never merely fewer.
const SQL_EXPRESSION_PREFIX: &str = "SELECT ";

fn sql_expression_injection(
	entry_point: SyntaxEntryPoint,
	range: Range<usize>,
	text: &str,
) -> Option<SyntaxInjection> {
	if entry_point == SyntaxEntryPoint::Expression {
		let body = text.trim_end();
		if !body.is_empty() {
			let wrapped = format!("{SQL_EXPRESSION_PREFIX}{body}");
			if let Some(tree) = parse_embedded(b"sql", &wrapped) {
				let expression_range =
					SQL_EXPRESSION_PREFIX.len()..SQL_EXPRESSION_PREFIX.len() + body.len();
				if covering_node(tree.root_node(), &expression_range).is_some() {
					let content_range = range.start..range.start + body.len();
					return Some(
						SyntaxInjection::new(
							"sql",
							entry_point,
							content_range.clone(),
							content_range,
							tree,
						)
						.with_analysis(wrapped, SQL_EXPRESSION_PREFIX.len()),
					);
				}
			}
		}
	}
	let tree = parse_embedded(b"sql", text)?;
	Some(SyntaxInjection::new(
		"sql",
		entry_point,
		range.clone(),
		range,
		tree,
	))
}

/// Classifies complete statements versus expression fragments.
/// `RETURN QUERY`, query loops, and `OPEN FOR` are statements unless
/// `EXECUTE` turns their position into a string expression.
fn sql_expression_entry(node: Node<'_>) -> SyntaxEntryPoint {
	let Some(parent) = node.parent() else {
		return SyntaxEntryPoint::Expression;
	};
	let has = |kind: &str| find_named_child(parent, kind).is_some();
	match parent.kind() {
		"stmt_execsql" | "for_query" => SyntaxEntryPoint::Statement,
		"stmt_return" if has("kw_query") && !has("kw_execute") => SyntaxEntryPoint::Statement,
		"stmt_open" if has("kw_for") && !has("kw_execute") => SyntaxEntryPoint::Statement,
		_ => SyntaxEntryPoint::Expression,
	}
}

fn parse_embedded(language: &[u8], source: &str) -> Option<Tree> {
	let grammar = if language.eq_ignore_ascii_case(b"plpgsql") {
		super::plpgsql_grammar::LANGUAGE
	} else if language.eq_ignore_ascii_case(b"sql") {
		tree_sitter_postgres::LANGUAGE
	} else {
		return None;
	};
	let mut parser = Parser::new();
	parser.set_language(&grammar.into()).unwrap_or_else(|err| {
		panic!("failed to load embedded SQL grammar: {err}");
	});
	parser.parse(source, None)
}

pub(super) fn parse_plpgsql(source: &str) -> Tree {
	let Some(tree) = parse_embedded(b"plpgsql", source) else {
		unreachable!("PL/pgSQL is a supported embedded grammar");
	};
	tree
}

struct RoutineBody<'a> {
	language: Vec<u8>,
	text: &'a str,
	host_byte_range: Range<usize>,
	content_byte_range: Range<usize>,
}

impl RoutineBody<'_> {
	fn language_tag(&self) -> &'static str {
		if self.language.eq_ignore_ascii_case(b"plpgsql") {
			"plpgsql"
		} else {
			"sql"
		}
	}
}

fn routine_body<'a>(node: Node<'_>, source: &'a str) -> Option<RoutineBody<'a>> {
	let language = function_language(node, source.as_bytes());
	let dollar = find_routine_body_literal(node)?;
	dollar_body(dollar, language, source)
}

fn do_body<'a>(node: Node<'_>, source: &'a str) -> Option<RoutineBody<'a>> {
	let language = do_language(node, source.as_bytes()).unwrap_or_else(|| b"plpgsql".to_vec());
	let dollar = find_do_body_literal(node)?;
	dollar_body(dollar, language, source)
}

/// An empty body (`$e$$e$`) keeps its injection: the region exists at the content offset.
fn dollar_body<'a>(
	dollar: Node<'_>,
	language: Vec<u8>,
	source: &'a str,
) -> Option<RoutineBody<'a>> {
	let full = source.get(dollar.start_byte()..dollar.end_byte())?;
	let first = full.find('$')?;
	let end_delim = full[first + 1..].find('$')? + first + 2;
	let close = full.rfind(&full[first..end_delim])?;
	if close < end_delim {
		return None;
	}
	let content_byte_range = dollar.start_byte() + end_delim..dollar.start_byte() + close;
	Some(RoutineBody {
		language,
		text: source.get(content_byte_range.clone())?,
		host_byte_range: dollar.start_byte()..dollar.end_byte(),
		content_byte_range,
	})
}

/// The DO body literal: the option item that carries code, never the LANGUAGE option.
fn find_do_body_literal(node: Node<'_>) -> Option<Node<'_>> {
	if node.kind() == "dostmt_opt_item" && find_named_child(node, "kw_language").is_none() {
		return find_descendant(node, "dollar_quoted_string");
	}
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		if let Some(body) = find_do_body_literal(child) {
			return Some(body);
		}
	}
	None
}

/// `DO` defaults to PL/pgSQL; an explicit LANGUAGE option overrides it.
fn do_language(node: Node<'_>, src: &[u8]) -> Option<Vec<u8>> {
	if node.kind() == "dostmt_opt_item"
		&& find_named_child(node, "kw_language").is_some()
		&& let Some(value) = find_descendant(node, "NonReservedWord_or_Sconst")
		&& let Some(raw) = src.get(value.start_byte()..value.end_byte())
	{
		return Some(normalize_sql_string(raw));
	}
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		if let Some(found) = do_language(child, src) {
			return Some(found);
		}
	}
	None
}

fn normalize_sql_string(raw: &[u8]) -> Vec<u8> {
	if raw.first() == Some(&b'$')
		&& let Some(delimiter_end) = raw.get(1..).and_then(|tail| {
			tail.iter()
				.position(|byte| *byte == b'$')
				.map(|index| index + 2)
		}) {
		let delimiter = &raw[..delimiter_end];
		if let Some(inner) = raw
			.strip_prefix(delimiter)
			.and_then(|value| value.strip_suffix(delimiter))
		{
			return inner.to_vec();
		}
	}
	let (quoted, escape) =
		if let Some(value) = raw.strip_prefix(b"E").or_else(|| raw.strip_prefix(b"e")) {
			(value, true)
		} else {
			(raw, false)
		};
	let Some(inner) = quoted
		.strip_prefix(b"'")
		.and_then(|value| value.strip_suffix(b"'"))
	else {
		return raw.to_vec();
	};
	let mut out = Vec::with_capacity(inner.len());
	let mut index = 0;
	while index < inner.len() {
		if inner[index] == b'\'' && inner.get(index + 1) == Some(&b'\'') {
			out.push(b'\'');
			index += 2;
		} else if escape && inner[index] == b'\\' && index + 1 < inner.len() {
			index += decode_escape(&inner[index + 1..], &mut out);
		} else {
			out.push(inner[index]);
			index += 1;
		}
	}
	out
}

fn decode_escape(raw: &[u8], out: &mut Vec<u8>) -> usize {
	match raw[0] {
		b'b' => out.push(8),
		b'f' => out.push(12),
		b'n' => out.push(b'\n'),
		b'r' => out.push(b'\r'),
		b't' => out.push(b'\t'),
		b'0'..=b'7' => {
			let length = raw
				.iter()
				.take(3)
				.take_while(|byte| (b'0'..=b'7').contains(byte))
				.count();
			let value = raw[..length].iter().fold(0_u8, |value, digit| {
				value.wrapping_mul(8).wrapping_add(digit - b'0')
			});
			out.push(value);
			return length + 1;
		}
		b'x' => {
			let length = raw[1..]
				.iter()
				.take(2)
				.take_while(|byte| byte.is_ascii_hexdigit())
				.count();
			if length > 0 {
				let value = raw[1..=length]
					.iter()
					.fold(0_u8, |value, digit| value * 16 + hex_value(*digit));
				out.push(value);
				return length + 2;
			}
			out.push(b'x');
		}
		b'u' => return decode_unicode_escape(raw, 4, out),
		b'U' => return decode_unicode_escape(raw, 8, out),
		byte => out.push(byte),
	}
	2
}

fn decode_unicode_escape(raw: &[u8], digits: usize, out: &mut Vec<u8>) -> usize {
	let Some(hex) = raw
		.get(1..=digits)
		.filter(|value| value.iter().all(u8::is_ascii_hexdigit))
	else {
		out.push(raw[0]);
		return 2;
	};
	let value = hex.iter().fold(0_u32, |value, digit| {
		value * 16 + u32::from(hex_value(*digit))
	});
	if let Some(character) = char::from_u32(value) {
		let mut encoded = [0; 4];
		out.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
	} else {
		out.push(b'\\');
		out.extend_from_slice(&raw[..=digits]);
	}
	digits + 2
}

fn hex_value(digit: u8) -> u8 {
	match digit {
		b'0'..=b'9' => digit - b'0',
		b'a'..=b'f' => digit - b'a' + 10,
		b'A'..=b'F' => digit - b'A' + 10,
		_ => unreachable!("hex digit was validated"),
	}
}

fn find_routine_body_literal(node: Node<'_>) -> Option<Node<'_>> {
	if node.kind() == "createfunc_opt_item"
		&& find_named_child(node, "kw_as").is_some()
		&& let Some(body) = find_named_child(node, "func_as")
	{
		return find_descendant(body, "dollar_quoted_string");
	}
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		if let Some(body) = find_routine_body_literal(child) {
			return Some(body);
		}
	}
	None
}

fn function_language(node: Node<'_>, src: &[u8]) -> Vec<u8> {
	if let Some(opts) = find_descendant(node, "createfunc_opt_list")
		&& let Some(language) = find_language_in(opts, src)
	{
		return language;
	}
	let mut after_language = false;
	let Some(node_source) = src.get(node.start_byte()..node.end_byte()) else {
		return Vec::new();
	};
	for token in node_source
		.split(|byte| !(byte.is_ascii_alphanumeric() || *byte == b'_'))
		.filter(|token| !token.is_empty())
	{
		if after_language {
			return token.to_vec();
		}
		after_language = token.eq_ignore_ascii_case(b"language");
	}
	Vec::new()
}

fn find_language_in(node: Node<'_>, src: &[u8]) -> Option<Vec<u8>> {
	if node.kind() == "createfunc_opt_item" {
		let mut has_lang = false;
		let mut cursor = node.walk();
		for child in node.named_children(&mut cursor) {
			if child.kind() == "kw_language" {
				has_lang = true;
			} else if has_lang && let Some(identifier) = find_descendant(child, "identifier") {
				return src
					.get(identifier.start_byte()..identifier.end_byte())
					.map(<[u8]>::to_vec);
			}
		}
	}
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		if let Some(found) = find_language_in(child, src) {
			return Some(found);
		}
	}
	None
}

pub(super) fn walk_plpgsql_body(
	body: &str,
	tree: &Tree,
	source_def: &Moniker,
	module: &Moniker,
	search_paths: &CallableSearchPaths,
	builder: &mut SqlBuilder,
) {
	if body.trim().is_empty() {
		return;
	}
	let mut sql_parser = new_sql_parser();
	for_each_sql_expression(tree.root_node(), &mut |expr| {
		if inside_dynamic_execute(expr) {
			return;
		}
		let raw = &body[expr.start_byte()..expr.end_byte().min(body.len())];
		let trimmed = raw.trim_end_matches(';').trim();
		if trimmed.is_empty() {
			return;
		}
		let prepared = if starts_with_sql_statement(trimmed) {
			trimmed.to_string()
		} else if inside_call_statement(expr) {
			format!("CALL {trimmed}")
		} else if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2 {
			trimmed[1..trimmed.len() - 1].to_string()
		} else {
			format!("SELECT {trimmed}")
		};
		run_inner_sql(
			&mut sql_parser,
			&prepared,
			source_def,
			module,
			search_paths,
			builder,
		);
	});
}

fn inside_dynamic_execute(mut node: Node<'_>) -> bool {
	while let Some(parent) = node.parent() {
		if parent.kind() == "stmt_dynexecute" {
			return true;
		}
		node = parent;
	}
	false
}

fn starts_with_sql_statement(value: &str) -> bool {
	[
		"call", "create", "delete", "insert", "select", "update", "with",
	]
	.into_iter()
	.any(|keyword| starts_with_keyword(value, keyword))
}

fn inside_call_statement(mut node: Node<'_>) -> bool {
	while let Some(parent) = node.parent() {
		if parent
			.kind()
			.as_bytes()
			.windows(b"call".len())
			.any(|window| window.eq_ignore_ascii_case(b"call"))
		{
			return true;
		}
		node = parent;
	}
	false
}

fn starts_with_keyword(value: &str, keyword: &str) -> bool {
	value
		.get(..keyword.len())
		.is_some_and(|prefix| prefix.eq_ignore_ascii_case(keyword))
		&& value
			.as_bytes()
			.get(keyword.len())
			.is_some_and(u8::is_ascii_whitespace)
}

fn for_each_sql_expression<F: FnMut(Node)>(node: Node, f: &mut F) {
	if node.kind() == "sql_expression" {
		f(node);
	}
	let mut cur = node.walk();
	for c in node.named_children(&mut cur) {
		for_each_sql_expression(c, f);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::code_graph::CodeGraph;
	use crate::core::moniker::MonikerBuilder;
	use crate::lang::sql::Presets;
	use crate::lang::sql::extract;

	fn anchor() -> Moniker {
		MonikerBuilder::new().project(b"app").build()
	}

	fn run(uri: &str, src: &str) -> CodeGraph {
		extract(uri, src, &anchor(), false, &Presets::default())
	}

	fn ref_targets(g: &CodeGraph) -> Vec<String> {
		g.refs()
			.map(|r| crate::core::uri::to_uri(&r.target, &Default::default()))
			.collect()
	}

	#[test]
	fn perform_in_body_emits_call_ref() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION outer_fn(x int) RETURNS void LANGUAGE plpgsql AS $$\n\
			 BEGIN\n\
			 PERFORM esac.inner_fn(x);\n\
			 END;\n\
			 $$;",
		);
		assert!(
			ref_targets(&g).iter().any(|t| t
				== "code+moniker://app/lang:sql/module:foo/schema:esac/function:inner_fn(int4)"),
			"got refs: {:?}",
			ref_targets(&g)
		);
	}

	#[test]
	fn perform_in_if_branch_is_picked_up() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION outer_fn(x int) RETURNS void LANGUAGE plpgsql AS $$\n\
			 BEGIN\n\
			 IF x > 0 THEN\n\
			   PERFORM other_fn();\n\
			 END IF;\n\
			 END;\n\
			 $$;",
		);
		assert!(
			ref_targets(&g)
				.iter()
				.any(|t| t == "code+moniker://app/lang:sql/module:foo/function:other_fn()"),
			"got refs: {:?}",
			ref_targets(&g)
		);
	}

	#[test]
	fn nested_blocks_recurse() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION outer_fn() RETURNS void LANGUAGE plpgsql AS $$\n\
			 BEGIN\n\
			 BEGIN\n\
			   PERFORM deep_fn();\n\
			 END;\n\
			 END;\n\
			 $$;",
		);
		assert!(
			ref_targets(&g)
				.iter()
				.any(|t| t == "code+moniker://app/lang:sql/module:foo/function:deep_fn()"),
			"got refs: {:?}",
			ref_targets(&g)
		);
	}

	#[test]
	fn while_body_picks_up_calls() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION outer_fn(x int) RETURNS void LANGUAGE plpgsql AS $$\n\
			 BEGIN\n\
			 WHILE x > 0 LOOP\n\
			   PERFORM step_fn(x);\n\
			 END LOOP;\n\
			 END;\n\
			 $$;",
		);
		assert!(
			ref_targets(&g)
				.iter()
				.any(|t| t == "code+moniker://app/lang:sql/module:foo/function:step_fn(int4)"),
			"got refs: {:?}",
			ref_targets(&g)
		);
	}

	#[test]
	fn malformed_body_is_silent() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION bad() RETURNS void LANGUAGE plpgsql AS $$ this is not valid plpgsql $$;",
		);
		assert!(g.defs().any(|d| d.kind == b"function"));
	}

	#[test]
	fn call_statement_in_body_targets_a_procedure() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION outer_fn(x int) RETURNS void LANGUAGE plpgsql AS $$ BEGIN CALL jobs.refresh(x); END; $$;",
		);
		assert!(
			ref_targets(&g).iter().any(|target| target
				== "code+moniker://app/lang:sql/module:foo/schema:jobs/procedure:refresh(int4)"),
			"got refs: {:?}",
			ref_targets(&g)
		);
	}
}
