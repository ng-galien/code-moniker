use tree_sitter_language::LanguageFn;

unsafe extern "C" {
	fn tree_sitter_code_moniker_plpgsql() -> *const ();
}

pub(super) const LANGUAGE: LanguageFn =
	unsafe { LanguageFn::from_raw(tree_sitter_code_moniker_plpgsql) };

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
	fn quoted_block_and_loop_labels_parse() {
		for (source, label_kind, quoted_identifier_count) in [
			(
				r#"
<<"outer block">>
DECLARE
  total integer := 0;
BEGIN
  total := total + 1;
END "outer block";
"#,
				"block_label",
				2,
			),
			(
				r#"
BEGIN
  <<"outer ""loop">>
  FOR i IN 1..10 LOOP
    EXIT "outer ""loop";
  END LOOP "outer ""loop";
END;
"#,
				"loop_label",
				3,
			),
		] {
			let tree = parse(source);
			let syntax = tree.root_node().to_sexp();
			assert!(
				!tree.root_node().has_error(),
				"quoted labels must parse without recovery:\n{source}\n{}",
				syntax
			);
			assert!(
				syntax.contains(&format!("({label_kind}")),
				"expected {label_kind} in parsed syntax:\n{syntax}"
			);
			assert_eq!(
				syntax.matches("(quoted_identifier)").count(),
				quoted_identifier_count,
				"every quoted label declaration and reference must stay explicit:\n{syntax}"
			);
		}
	}
}
