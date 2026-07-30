use std::ops::Range;

use tree_sitter::{Node, Parser, Tree};

use crate::core::moniker::Moniker;
use crate::lang::tree_util::find_descendant;
use crate::lang::{ParsedDocument, SyntaxInjection};

use super::sdk_pipeline::discover::{
	CallableSearchPaths, SqlBuilder, new_sql_parser, run_inner_sql,
};

pub(super) fn parse_document(primary: Tree, source: &str) -> ParsedDocument {
	let mut injections = Vec::new();
	collect_routine_injections(primary.root_node(), source, &mut injections);
	ParsedDocument::with_injections(primary, injections)
}

fn collect_routine_injections(node: Node<'_>, source: &str, injections: &mut Vec<SyntaxInjection>) {
	if node.kind() == "CreateFunctionStmt"
		&& let Some(body) = routine_body(node, source)
		&& let Some(tree) = parse_embedded(&body.language, body.text)
	{
		injections.push(SyntaxInjection::new(
			body.language_tag(),
			body.host_byte_range,
			body.content_byte_range,
			tree,
		));
		return;
	}
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		collect_routine_injections(child, source, injections);
	}
}

fn parse_embedded(language: &[u8], source: &str) -> Option<Tree> {
	let grammar = if language.eq_ignore_ascii_case(b"plpgsql") {
		tree_sitter_postgres::LANGUAGE_PLPGSQL
	} else if language.eq_ignore_ascii_case(b"sql") {
		tree_sitter_postgres::LANGUAGE
	} else {
		return None;
	};
	let mut parser = Parser::new();
	parser.set_language(&grammar.into()).unwrap_or_else(|err| {
		panic!("failed to load tree-sitter-postgres embedded grammar: {err}");
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
	let dollar = find_descendant(node, "dollar_quoted_string")?;
	let full = source.get(dollar.start_byte()..dollar.end_byte())?;
	let first = full.find('$')?;
	let end_delim = full[first + 1..].find('$')? + first + 2;
	let close = full.rfind(&full[first..end_delim])?;
	if close <= end_delim {
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
