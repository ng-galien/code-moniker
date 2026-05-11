//! PL/pgSQL body walker on `tree_sitter_postgres::LANGUAGE_PLPGSQL`.
//! For every embedded `sql_expression`, re-parse with the SQL grammar and
//! collect `func_application` refs via the shared walker.

use tree_sitter::{Node, Parser};

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

use super::walker::collect_calls_in;

pub(super) fn walk_plpgsql_body(
	body: &str,
	source_def: &Moniker,
	module: &Moniker,
	graph: &mut CodeGraph,
) {
	if body.trim().is_empty() {
		return;
	}
	let mut plpgsql_parser = Parser::new();
	plpgsql_parser
		.set_language(&tree_sitter_postgres::LANGUAGE_PLPGSQL.into())
		.expect("failed to load tree-sitter-postgres PL/pgSQL grammar");
	let Some(tree) = plpgsql_parser.parse(body, None) else {
		return;
	};
	let mut sql_parser = Parser::new();
	sql_parser
		.set_language(&tree_sitter_postgres::LANGUAGE.into())
		.expect("failed to load tree-sitter-postgres SQL grammar");
	for_each_sql_expression(tree.root_node(), &mut |expr| {
		emit_calls_from_sql(expr, body, &mut sql_parser, source_def, module, graph);
	});
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

/// Re-parses the raw text of a PL/pgSQL `sql_expression` envelope (the body of
/// `PERFORM <expr>;`, `IF <expr> THEN`, `<lhs> := <rhs>;`, …) with the SQL
/// grammar. Bare expressions get wrapped in `SELECT …`. `EXECUTE 'literal'`
/// strips the quotes so the inner SQL parses; Postgres `''` escape handling
/// is deliberately omitted.
fn emit_calls_from_sql(
	expr: Node,
	body: &str,
	sql_parser: &mut Parser,
	source_def: &Moniker,
	module: &Moniker,
	graph: &mut CodeGraph,
) {
	let raw = &body[expr.start_byte()..expr.end_byte().min(body.len())];
	let trimmed = raw.trim_end_matches(';').trim();
	if trimmed.is_empty() {
		return;
	}
	let prepared = if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2 {
		trimmed[1..trimmed.len() - 1].to_string()
	} else {
		format!("SELECT {trimmed}")
	};
	let Some(tree) = sql_parser.parse(&prepared, None) else {
		return;
	};
	let root = tree.root_node();
	let src = prepared.as_bytes();
	for c in root.children(&mut root.walk()) {
		if c.kind() != "toplevel_stmt" {
			continue;
		}
		if let Some(stmt) = c.named_child(0).and_then(|s| s.named_child(0)) {
			collect_calls_in(stmt, src, source_def, module, graph);
		}
	}
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

	fn ref_targets(g: &CodeGraph) -> Vec<String> {
		g.refs()
			.map(|r| crate::core::uri::to_uri(&r.target, &Default::default()).unwrap())
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
			ref_targets(&g)
				.iter()
				.any(|t| t
					== "code+moniker://app/lang:sql/module:foo/schema:esac/function:inner_fn(1)"),
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
				.any(|t| t == "code+moniker://app/lang:sql/module:foo/function:step_fn(1)"),
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
}
