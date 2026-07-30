//! PL/pgSQL Tree-sitter grammar used by Code Moniker.

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
	fn tree_sitter_code_moniker_plpgsql() -> *const ();
}

/// The Tree-sitter language function for PL/pgSQL.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_code_moniker_plpgsql) };

/// The generated Tree-sitter node types.
pub const NODE_TYPES: &str = include_str!("node-types.json");

#[cfg(test)]
mod tests {
	fn parse(source: &str) -> tree_sitter::Tree {
		let mut parser = tree_sitter::Parser::new();
		parser
			.set_language(&super::LANGUAGE.into())
			.expect("PL/pgSQL grammar must load");
		parser
			.parse(source, None)
			.expect("a non-cancelled parse must return a tree")
	}

	#[test]
	fn grammar_loads() {
		let mut parser = tree_sitter::Parser::new();
		parser
			.set_language(&super::LANGUAGE.into())
			.expect("PL/pgSQL grammar must load");
	}

	#[test]
	fn quoted_block_and_loop_labels_parse() {
		for source in [
			r#"
<<"outer block">>
DECLARE
  total integer := 0;
BEGIN
  total := total + 1;
END "outer block";
"#,
			r#"
BEGIN
  <<"outer ""loop">>
  FOR i IN 1..10 LOOP
    EXIT "outer ""loop";
  END LOOP "outer ""loop";
END;
"#,
		] {
			let tree = parse(source);
			assert!(
				!tree.root_node().has_error(),
				"quoted labels must parse without recovery:\n{}\n{}",
				source,
				tree.root_node().to_sexp()
			);
		}
	}
}
