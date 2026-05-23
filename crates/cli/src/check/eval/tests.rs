use super::*;
use code_moniker_core::core::code_graph::DefAttrs;
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};

const SCHEME: &str = "code+moniker://";

fn cfg_from(s: &str) -> Config {
	toml::from_str(s).expect("test config must parse")
}

fn build_module(name: &[u8]) -> Moniker {
	let mut b = MonikerBuilder::new();
	b.project(b".");
	b.segment(b"lang", b"ts");
	b.segment(b"module", name);
	b.build()
}

fn child(parent: &Moniker, kind: &[u8], name: &[u8]) -> Moniker {
	let mut b = MonikerBuilder::from_view(parent.as_view());
	b.segment(kind, name);
	b.build()
}

#[test]
fn no_rules_means_no_violations() {
	let cfg: Config = Config::default();
	let module = build_module(b"a");
	let g = CodeGraph::new(module, b"module");
	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert!(v.is_empty());
}

#[test]
fn name_regex_violation() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "name-pascal"
		expr = "name =~ ^[A-Z][A-Za-z0-9]*$"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let bad = child(&module, b"class", b"lower_case_bad");
	g.add_def(bad, b"class", &module, Some((0, 10))).unwrap();
	let v = evaluate(&g, "anything\n", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1);
	assert_eq!(v[0].rule_id, "ts.class.name-pascal");
}

#[test]
fn quoted_count_like_rhs_is_a_string_literal() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "literal-name"
		expr = "name = 'count(method)'"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let cls = child(&module, b"class", b"Other");
	g.add_def(cls, b"class", &module, Some((0, 10))).unwrap();
	let v = evaluate(&g, "anything\n", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1);
	assert_eq!(v[0].rule_id, "ts.class.literal-name");
	assert!(v[0].message.contains("expected count(method)"), "{v:?}");
}

#[test]
fn auto_id_when_user_omits_one() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		expr = "name =~ ^[A-Z]"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	g.add_def(
		child(&module, b"class", b"lower"),
		b"class",
		&module,
		Some((0, 10)),
	)
	.unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v[0].rule_id, "ts.class.where_0");
}

#[test]
fn lines_le_violation_uses_actual_count() {
	let cfg = cfg_from(
		r#"
		[[ts.function.where]]
		id   = "max-lines"
		expr = "lines <= 2"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let f = child(&module, b"function", b"foo");
	g.add_def(f, b"function", &module, Some((0, 14))).unwrap();
	let v = evaluate(&g, "a\nb\nc\n", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1);
	assert!(v[0].message.contains("3"));
	assert!(v[0].message.contains("expected 2"));
}

#[test]
fn forbid_name_via_regex_no_match() {
	let cfg = cfg_from(
		r#"
		[[ts.function.where]]
		id   = "no-helper-names"
		expr = "name !~ ^(helper|utils|manager)$"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	g.add_def(
		child(&module, b"function", b"helper"),
		b"function",
		&module,
		Some((0, 5)),
	)
	.unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1);
	assert_eq!(v[0].rule_id, "ts.function.no-helper-names");
}

#[test]
fn count_children_groups_by_parent() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "max-methods"
		expr = "count(method) <= 2"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(foo.clone(), b"class", &module, Some((0, 100)))
		.unwrap();
	g.add_def(child(&foo, b"method", b"a"), b"method", &foo, Some((1, 5)))
		.unwrap();
	g.add_def(child(&foo, b"method", b"b"), b"method", &foo, Some((6, 10)))
		.unwrap();
	g.add_def(
		child(&foo, b"method", b"c"),
		b"method",
		&foo,
		Some((11, 15)),
	)
	.unwrap();
	let bar = child(&module, b"class", b"Bar");
	g.add_def(bar.clone(), b"class", &module, Some((20, 50)))
		.unwrap();
	g.add_def(
		child(&bar, b"method", b"x"),
		b"method",
		&bar,
		Some((21, 25)),
	)
	.unwrap();
	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "Foo violates, Bar passes: {v:?}");
	assert!(v[0].moniker.contains("class:Foo"));
}

#[test]
fn count_shape_domain_groups_direct_children_by_shape() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "max-callables"
		expr = "count(shape:callable) <= 1"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(foo.clone(), b"class", &module, Some((0, 50)))
		.unwrap();
	g.add_def(child(&foo, b"method", b"a"), b"method", &foo, Some((1, 5)))
		.unwrap();
	g.add_def(
		child(&foo, b"function", b"b"),
		b"function",
		&foo,
		Some((6, 10)),
	)
	.unwrap();
	g.add_def(child(&foo, b"field", b"x"), b"field", &foo, Some((11, 12)))
		.unwrap();
	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "two callable children should violate: {v:?}");
	assert!(v[0].message.contains("expected 1"), "{v:?}");
}

#[test]
fn top_level_shape_scope_applies_to_matching_defs() {
	let cfg = cfg_from(
		r#"
		[[shape.callable.where]]
		id      = "max-lines"
		expr    = "lines <= 1"
		message = "{kind} `{name}` too long: {value}/{expected}"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let f = child(&module, b"function", b"foo");
	g.add_def(f, b"function", &module, Some((0, 8))).unwrap();
	let v = evaluate(&g, "a\nb\nc\n", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1);
	assert_eq!(v[0].rule_id, "shape.callable.max-lines");
	assert_eq!(v[0].kind, "function");
	assert_eq!(
		v[0].explanation.as_deref(),
		Some("function `foo` too long: 3/1")
	);
}

#[test]
fn kind_scope_overrides_shape_scope_with_same_rule_id() {
	let cfg = cfg_from(
		r#"
		[[shape.callable.where]]
		id   = "max-lines"
		expr = "lines <= 1"

		[[ts.function.where]]
		id   = "max-lines"
		expr = "lines <= 99"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let f = child(&module, b"function", b"foo");
	g.add_def(f, b"function", &module, Some((0, 8))).unwrap();
	let v = evaluate(&g, "a\nb\nc\n", Lang::Ts, &cfg, SCHEME).unwrap();
	assert!(
		v.is_empty(),
		"kind-specific max-lines should replace shape rule: {v:?}"
	);
}

#[test]
fn generated_kind_ids_do_not_override_shape_rules() {
	let cfg = cfg_from(
		r#"
		[[shape.callable.where]]
		expr = "lines <= 1"

		[[ts.function.where]]
		expr = "lines <= 99"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let f = child(&module, b"function", b"foo");
	g.add_def(f, b"function", &module, Some((0, 8))).unwrap();
	let v = evaluate(&g, "a\nb\nc\n", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(
		v.len(),
		1,
		"un-id'd shape and kind rules should be additive: {v:?}"
	);
	assert_eq!(v[0].rule_id, "shape.callable.where_0");
}

#[test]
fn lang_shape_scope_overrides_top_level_shape_rule_by_id() {
	let cfg = cfg_from(
		r#"
		[[shape.callable.where]]
		id   = "max-lines"
		expr = "lines <= 1"

		[[ts.shape.callable.where]]
		id   = "max-lines"
		expr = "lines <= 99"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let f = child(&module, b"function", b"foo");
	g.add_def(f, b"function", &module, Some((0, 8))).unwrap();
	let v = evaluate(&g, "a\nb\nc\n", Lang::Ts, &cfg, SCHEME).unwrap();
	assert!(
		v.is_empty(),
		"ts.shape.callable should replace shape.callable by id: {v:?}"
	);
}

#[test]
fn numeric_projection_rhs_is_evaluated() {
	let cfg = cfg_from(
		r#"
		[[ts.function.where]]
		id   = "lines-fit-depth"
		expr = "lines <= depth"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let f = child(&module, b"function", b"foo");
	g.add_def(f, b"function", &module, Some((0, 8))).unwrap();
	let v = evaluate(&g, "a\nb\nc\nd\n", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1);
	assert!(v[0].message.contains("lines = 4"), "{v:?}");
	assert!(v[0].message.contains("expected 3"), "{v:?}");
}

#[test]
fn count_rhs_is_evaluated() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "methods-vs-fields"
		expr = "count(method) <= count(field)"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(foo.clone(), b"class", &module, Some((0, 50)))
		.unwrap();
	g.add_def(child(&foo, b"method", b"a"), b"method", &foo, Some((1, 5)))
		.unwrap();
	g.add_def(child(&foo, b"method", b"b"), b"method", &foo, Some((6, 10)))
		.unwrap();
	g.add_def(child(&foo, b"field", b"x"), b"field", &foo, Some((11, 12)))
		.unwrap();
	let bar = child(&module, b"class", b"Bar");
	g.add_def(bar.clone(), b"class", &module, Some((20, 50)))
		.unwrap();
	g.add_def(
		child(&bar, b"method", b"m"),
		b"method",
		&bar,
		Some((21, 25)),
	)
	.unwrap();
	g.add_def(child(&bar, b"field", b"y"), b"field", &bar, Some((26, 27)))
		.unwrap();
	g.add_def(child(&bar, b"field", b"z"), b"field", &bar, Some((28, 29)))
		.unwrap();
	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "Foo violates, Bar passes: {v:?}");
	assert!(v[0].moniker.contains("class:Foo"));
	assert!(v[0].message.contains("count = 2"), "{v:?}");
	assert!(v[0].message.contains("expected 1"), "{v:?}");
}

#[test]
fn text_regex_on_comment() {
	let cfg = cfg_from(
		r#"
		[[ts.comment.where]]
		id   = "no-prose"
		expr = '''text =~ ^\s*//\s*TODO'''
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let cmt = child(&module, b"comment", b"0");
	let source = "// random prose\n";
	g.add_def(cmt, b"comment", &module, Some((0, source.len() as u32 - 1)))
		.unwrap();
	let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1);
}

#[test]
fn moniker_descendant_of() {
	let cfg = cfg_from(
		r#"
		[[ts.method.where]]
		id   = "stay-in-foo"
		expr = "moniker <@ code+moniker://./lang:ts/module:a/class:Foo"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(foo.clone(), b"class", &module, Some((0, 50)))
		.unwrap();
	g.add_def(child(&foo, b"method", b"a"), b"method", &foo, Some((1, 5)))
		.unwrap();
	let bar = child(&module, b"class", b"Bar");
	g.add_def(bar.clone(), b"class", &module, Some((10, 30)))
		.unwrap();
	g.add_def(
		child(&bar, b"method", b"b"),
		b"method",
		&bar,
		Some((11, 15)),
	)
	.unwrap();
	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "Bar.b violates, Foo.a passes");
	assert!(v[0].moniker.contains("class:Bar/method:b"));
}

#[test]
fn invalid_expression_surfaces_at_evaluate() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		expr = "name =~ [unclosed"
		"#,
	);
	let module = build_module(b"a");
	let g = CodeGraph::new(module, b"module");
	match evaluate(&g, "", Lang::Ts, &cfg, SCHEME) {
		Err(ConfigError::InvalidExpr { at, .. }) => {
			assert!(at.contains("ts.class"), "{at}");
		}
		other => panic!("expected InvalidExpr, got {other:?}"),
	}
}

#[test]
fn unknown_kind_section_still_rejected() {
	let r = toml::from_str::<Config>(
		r#"
		[[ts.classs.where]]
		expr = "name =~ ^X"
		"#,
	);
	// parses fine — kind validation happens in config::validate during load
	assert!(r.is_ok());
}

#[test]
fn require_doc_comment_skips_when_annotations_precede_def() {
	let cfg = cfg_from(
		r#"
		[ts.class]
		require_doc_comment = "public"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");

	// Doc comment at lines 1
	let mut b = MonikerBuilder::from_view(module.as_view());
	b.segment(b"comment", b"0");
	let cmt = b.build();
	g.add_def(cmt, b"comment", &module, Some((0, 10))).unwrap();

	// Class def header starts at line 3 (after `@Decorator` on line 2)
	let source = "/** doc */\n@Decorator\nclass Foo {}\n";
	let mut b = MonikerBuilder::from_view(module.as_view());
	b.segment(b"class", b"Foo");
	let foo = b.build();
	let attrs = DefAttrs {
		visibility: b"public",
		..DefAttrs::default()
	};
	// def starts at `class Foo` byte 22, class def is index 2 in graph
	g.add_def_attrs(foo.clone(), b"class", &module, Some((22, 35)), &attrs)
		.unwrap();
	let class_idx = g.defs().position(|d| d.moniker == foo).unwrap();

	// Emit @Decorator as an annotates ref starting at byte 11 (line 2)
	g.add_ref(
		&g.def_at(class_idx).moniker.clone(),
		module.clone(),
		b"annotates",
		Some((11, 21)),
	)
	.unwrap();

	let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).unwrap();
	assert!(
		v.is_empty(),
		"comment line 1 + annotation line 2 + class line 3: doc must attach via annotation anchor: {v:?}"
	);
}

// ─── booleans + implication semantics ───────────────────────────────

#[test]
fn or_passes_if_one_arm_passes() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "any-of"
		expr = "name = 'Foo' OR name = 'Bar'"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	g.add_def(
		child(&module, b"class", b"Foo"),
		b"class",
		&module,
		Some((0, 10)),
	)
	.unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert!(v.is_empty(), "Foo matches first arm: {v:?}");
}

#[test]
fn or_fails_when_all_arms_fail() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "any-of"
		expr = "name = 'Foo' OR name = 'Bar'"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	g.add_def(
		child(&module, b"class", b"Baz"),
		b"class",
		&module,
		Some((0, 10)),
	)
	.unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "Baz matches no arm: {v:?}");
}

#[test]
fn not_inverts_pass_and_fail() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "not-internal"
		expr = "NOT name = 'Internal'"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	g.add_def(
		child(&module, b"class", b"Internal"),
		b"class",
		&module,
		Some((0, 5)),
	)
	.unwrap();
	g.add_def(
		child(&module, b"class", b"Public"),
		b"class",
		&module,
		Some((6, 10)),
	)
	.unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "only `Internal` violates: {v:?}");
	assert!(v[0].moniker.contains("class:Internal"));
}

#[test]
fn implies_false_premise_is_pass() {
	// `name = 'Entity' => any(...)` should NOT flag classes that aren't Entities.
	// This is the bug that fix-by-implication addresses.
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "entity-implies-x"
		expr = "name =~ Entity$ => kind = 'class'"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	g.add_def(
		child(&module, b"class", b"NotAnEntity"),
		b"class",
		&module,
		Some((0, 10)),
	)
	.unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert!(
		v.is_empty(),
		"premise false (no `Entity` suffix) ⇒ implication trivially true: {v:?}"
	);
}

#[test]
fn implies_true_premise_evaluates_consequent() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "entity-must-be-class"
		expr = "name =~ Entity$ => kind = 'class'"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	// kind is 'class', so this should pass
	g.add_def(
		child(&module, b"class", b"UserEntity"),
		b"class",
		&module,
		Some((0, 10)),
	)
	.unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert!(v.is_empty(), "premise true + consequent true: {v:?}");
}

// ─── segment(K) projection ──────────────────────────────────────────

#[test]
fn segment_of_def_returns_first_match() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "must-be-in-domain-module"
		expr = "segment('module') = 'domain'"
		"#,
	);
	let module = build_module(b"app");
	let mut g = CodeGraph::new(module.clone(), b"module");
	g.add_def(
		child(&module, b"class", b"Foo"),
		b"class",
		&module,
		Some((0, 5)),
	)
	.unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(
		v.len(),
		1,
		"class lives in module:app, not module:domain: {v:?}"
	);
}

#[test]
fn source_and_target_segment_in_refs() {
	let cfg = cfg_from(
		r#"
		[[refs.where]]
		id   = "same-module-only"
		expr = "source.segment('module') != target.segment('module') => target.segment('module') = 'std'"
		"#,
	);
	let root = build_root();
	let mut g = CodeGraph::new(root.clone(), b"module");
	let billing = submodule(&root, b"billing");
	g.add_def(billing.clone(), b"module", &root, Some((0, 1)))
		.unwrap();
	let shipping = submodule(&root, b"shipping");
	g.add_def(shipping.clone(), b"module", &root, Some((2, 3)))
		.unwrap();
	let o = child(&billing, b"class", b"Order");
	g.add_def(o.clone(), b"class", &billing, Some((4, 5)))
		.unwrap();
	let p = child(&shipping, b"class", b"Pkg");
	g.add_def(p.clone(), b"class", &shipping, Some((6, 10)))
		.unwrap();
	g.add_ref(&o, p, b"uses_type", Some((4, 5))).unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "billing→shipping violation: {v:?}");
}

#[test]
fn per_lang_refs_section_is_evaluated() {
	let cfg = cfg_from(
		r#"
		[[ts.refs.where]]
		id   = "no-domain-import"
		expr = "source.segment('module') = 'domain' => NOT kind = 'imports'"
		"#,
	);
	let root = build_root();
	let mut g = CodeGraph::new(root.clone(), b"module");
	let domain = submodule(&root, b"domain");
	g.add_def(domain.clone(), b"module", &root, Some((0, 1)))
		.unwrap();
	let other = submodule(&root, b"infra");
	g.add_def(other.clone(), b"module", &root, Some((2, 3)))
		.unwrap();
	let order = child(&domain, b"class", b"Order");
	g.add_def(order.clone(), b"class", &domain, Some((4, 5)))
		.unwrap();
	let infra_cls = child(&other, b"class", b"X");
	g.add_def(infra_cls.clone(), b"class", &other, Some((6, 10)))
		.unwrap();
	g.add_ref(&order, infra_cls, b"imports", Some((4, 5)))
		.unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "per-lang refs rule fires: {v:?}");
	assert_eq!(v[0].rule_id, "ts.refs.no-domain-import");
}

// ─── quantifiers ────────────────────────────────────────────────────

#[test]
fn count_method_with_filter() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "few-getters"
		expr = "count(method, name =~ ^get) <= 1"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let cls = child(&module, b"class", b"Foo");
	g.add_def(cls.clone(), b"class", &module, Some((0, 50)))
		.unwrap();
	for name in [
		b"getFoo".as_slice(),
		b"getBar".as_slice(),
		b"setBaz".as_slice(),
	] {
		let m = child(&cls, b"method", name);
		g.add_def(m, b"method", &cls, Some((1, 5))).unwrap();
	}
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "2 getters > 1 limit: {v:?}");
}

#[test]
fn aggregate_cv_uses_each_binding() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "fanout-skew"
		expr = "count(method) >= 3 => cv(method, fan_out(each)) <= 0.1"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(foo.clone(), b"class", &module, Some((0, 50)))
		.unwrap();
	let m1 = child(&foo, b"method", b"m1");
	let m2 = child(&foo, b"method", b"m2");
	let m3 = child(&foo, b"method", b"m3");
	g.add_def(m1, b"method", &foo, Some((1, 5))).unwrap();
	g.add_def(m2, b"method", &foo, Some((6, 10))).unwrap();
	g.add_def(m3.clone(), b"method", &foo, Some((11, 15)))
		.unwrap();
	let bar = child(&module, b"class", b"Bar");
	g.add_def(bar.clone(), b"class", &module, Some((60, 90)))
		.unwrap();
	for name in [b"a", b"b", b"c"] {
		let target = child(&bar, b"method", name);
		g.add_def(target.clone(), b"method", &bar, Some((61, 62)))
			.unwrap();
		g.add_ref(&m3, target, b"method_call", Some((12, 13)))
			.unwrap();
	}
	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "fan-out distribution should be skewed: {v:?}");
	assert_eq!(v[0].rule_id, "ts.class.fanout-skew");
}

#[test]
fn mode_projection_compares_to_source_parent_alias() {
	let cfg = cfg_from(
		r#"
		[[ts.method.where]]
		id   = "feature-envy"
		expr = "count(out_refs) >= 2 => mode(out_refs, target.parent) = source.parent"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	let bar = child(&module, b"class", b"Bar");
	g.add_def(foo.clone(), b"class", &module, Some((0, 20)))
		.unwrap();
	g.add_def(bar.clone(), b"class", &module, Some((30, 60)))
		.unwrap();
	let method = child(&foo, b"method", b"work");
	g.add_def(method.clone(), b"method", &foo, Some((1, 10)))
		.unwrap();
	for name in [b"a", b"b"] {
		let target = child(&bar, b"method", name);
		g.add_def(target.clone(), b"method", &bar, Some((31, 32)))
			.unwrap();
		g.add_ref(&method, target, b"method_call", Some((2, 3)))
			.unwrap();
	}
	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(
		v.len(),
		1,
		"mode target parent should be Bar, not Foo: {v:?}"
	);
	assert_eq!(v[0].rule_id, "ts.method.feature-envy");
}

#[test]
fn average_field_entropy_uses_in_ref_sources() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "field-sharing"
		expr = "avg(field, entropy(in_refs, source)) >= 0.5"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(foo.clone(), b"class", &module, Some((0, 50)))
		.unwrap();
	let fields = [child(&foo, b"field", b"x"), child(&foo, b"field", b"y")];
	for field in &fields {
		g.add_def(field.clone(), b"field", &foo, Some((1, 2)))
			.unwrap();
	}
	let methods = [child(&foo, b"method", b"a"), child(&foo, b"method", b"b")];
	for method in &methods {
		g.add_def(method.clone(), b"method", &foo, Some((3, 8)))
			.unwrap();
		for field in &fields {
			g.add_ref(method, field.clone(), b"reads", Some((4, 5)))
				.unwrap();
		}
	}
	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert!(v.is_empty(), "both fields are read by both methods: {v:?}");
}

#[test]
fn gini_counts_filtered_in_refs_per_field() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "field-usage-skew"
		expr = "gini(field, count(in_refs, source.parent = target.parent)) <= 0.4"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(foo.clone(), b"class", &module, Some((0, 50)))
		.unwrap();
	let hot = child(&foo, b"field", b"hot");
	let cold = child(&foo, b"field", b"cold");
	g.add_def(hot.clone(), b"field", &foo, Some((1, 2)))
		.unwrap();
	g.add_def(cold, b"field", &foo, Some((3, 4))).unwrap();
	for name in [b"a", b"b", b"c"] {
		let method = child(&foo, b"method", name);
		g.add_def(method.clone(), b"method", &foo, Some((5, 10)))
			.unwrap();
		g.add_ref(&method, hot.clone(), b"reads", Some((6, 7)))
			.unwrap();
	}
	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(
		v.len(),
		1,
		"one hot field and one cold field are skewed: {v:?}"
	);
	assert_eq!(v[0].rule_id, "ts.class.field-usage-skew");
}

#[test]
fn any_quantifier_children() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "must-have-execute"
		expr = "name =~ UseCase$ => any(method, name = 'execute')"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	// MissingUC has no execute → violation
	let uc = child(&module, b"class", b"PayUseCase");
	g.add_def(uc.clone(), b"class", &module, Some((0, 50)))
		.unwrap();
	g.add_def(
		child(&uc, b"method", b"prepare"),
		b"method",
		&uc,
		Some((1, 5)),
	)
	.unwrap();
	// GoodUC has execute → no violation
	let good = child(&module, b"class", b"GoodUseCase");
	g.add_def(good.clone(), b"class", &module, Some((51, 100)))
		.unwrap();
	g.add_def(
		child(&good, b"method", b"execute"),
		b"method",
		&good,
		Some((52, 60)),
	)
	.unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "PayUseCase lacks execute: {v:?}");
	assert!(v[0].moniker.contains("PayUseCase"));
}

#[test]
fn same_class_call_to_proxy_advised_method_is_flagged() {
	let cfg = cfg_from(
		r#"
		[[ts.method.where]]
		id = "proxy-method-no-self-invocation"
		expr = """
		  any(out_refs, kind = 'annotates' AND target.name = 'Transactional')
		  => none(in_refs,
		       (kind = 'method_call' OR kind = 'calls')
		       AND source.parent = target.parent
		     )
		"""
		"#,
	);
	let module = build_module(b"billing");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let service = child(&module, b"class", b"InvoiceService");
	g.add_def(service.clone(), b"class", &module, Some((0, 100)))
		.unwrap();
	let caller = child(&service, b"method", b"createBatch()");
	g.add_def(caller.clone(), b"method", &service, Some((10, 30)))
		.unwrap();
	let target = child(&service, b"method", b"createInvoice()");
	g.add_def(target.clone(), b"method", &service, Some((40, 70)))
		.unwrap();
	let annotation = child(&module, b"path", b"Transactional");
	g.add_ref(&target, annotation, b"annotates", Some((40, 50)))
		.unwrap();
	g.add_ref(&caller, target, b"calls", Some((20, 25)))
		.unwrap();

	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "same-class proxy self-invocation: {v:?}");
	assert!(v[0].moniker.contains("createInvoice"));
}

#[test]
fn same_class_call_to_class_level_proxy_advised_method_is_flagged() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id = "proxy-class-no-self-invocation"
		expr = """
		  any(out_refs, kind = 'annotates' AND target.name = 'Transactional')
		  => none(method,
		       any(in_refs,
		         (kind = 'method_call' OR kind = 'calls')
		         AND source.parent = target.parent
		       )
		     )
		"""
		"#,
	);
	let module = build_module(b"billing");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let service = child(&module, b"class", b"InvoiceService");
	g.add_def(service.clone(), b"class", &module, Some((0, 100)))
		.unwrap();
	let annotation = child(&module, b"path", b"Transactional");
	g.add_ref(&service, annotation, b"annotates", Some((0, 10)))
		.unwrap();
	let caller = child(&service, b"method", b"createBatch()");
	g.add_def(caller.clone(), b"method", &service, Some((10, 30)))
		.unwrap();
	let target = child(&service, b"method", b"createInvoice()");
	g.add_def(target.clone(), b"method", &service, Some((40, 70)))
		.unwrap();
	g.add_ref(&caller, target, b"calls", Some((20, 25)))
		.unwrap();

	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "class-level proxy self-invocation: {v:?}");
	assert!(v[0].moniker.contains("InvoiceService"));
}

#[test]
fn all_quantifier_children() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "methods-short"
		expr = "all(method, lines <= 5)"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let cls = child(&module, b"class", b"Foo");
	g.add_def(cls.clone(), b"class", &module, Some((0, 100)))
		.unwrap();
	g.add_def(child(&cls, b"method", b"ok"), b"method", &cls, Some((0, 4)))
		.unwrap();
	g.add_def(
		child(&cls, b"method", b"long"),
		b"method",
		&cls,
		Some((0, 200)),
	)
	.unwrap();
	let source: String = (0..40).map(|_| "a\n").collect();
	let v = evaluate(&g, &source, Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "long method violates: {v:?}");
}

#[test]
fn none_quantifier_segments() {
	// "this def's moniker has no segment whose kind is 'class'"
	let cfg = cfg_from(
		r#"
		[[ts.function.where]]
		id   = "function-not-in-class"
		expr = "none(segment, segment.kind = 'class')"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let cls = child(&module, b"class", b"Foo");
	g.add_def(cls.clone(), b"class", &module, Some((0, 50)))
		.unwrap();
	// function nested inside class → has a class segment → violates
	let f = child(&cls, b"function", b"inner");
	g.add_def(f, b"function", &cls, Some((1, 5))).unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "function inside class violates: {v:?}");
}

#[test]
fn any_out_refs_must_implement_port() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "adapter-implements-port"
		expr = "name =~ Adapter$ => any(out_refs, kind = 'implements' AND target.name =~ Port$)"
		"#,
	);
	let root = build_root();
	let mut g = CodeGraph::new(root.clone(), b"module");
	let m = submodule(&root, b"adapters");
	g.add_def(m.clone(), b"module", &root, Some((0, 1)))
		.unwrap();
	let bad = child(&m, b"class", b"OrderAdapter");
	g.add_def(bad.clone(), b"class", &m, Some((2, 10))).unwrap();
	// No implements ref → adapter without port → violation
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "adapter with no implements: {v:?}");
}

// ─── projection extensions ──────────────────────────────────────────

#[test]
fn depth_projection() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "shallow"
		expr = "depth <= 3"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let cls = child(&module, b"class", b"DeepClass");
	g.add_def(cls.clone(), b"class", &module, Some((0, 5)))
		.unwrap();
	// depth = 3 (project segment doesn't count, segments: lang, module, class)
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert!(v.is_empty(), "depth = 3 is within limit: {v:?}");
}

#[test]
fn parent_name_projection() {
	let cfg = cfg_from(
		r#"
		[[ts.method.where]]
		id   = "no-name-clash"
		expr = "name != parent.name"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let cls = child(&module, b"class", b"Foo");
	g.add_def(cls.clone(), b"class", &module, Some((0, 50)))
		.unwrap();
	let m_ok = child(&cls, b"method", b"bar");
	g.add_def(m_ok, b"method", &cls, Some((1, 10))).unwrap();
	let m_bad = child(&cls, b"method", b"Foo");
	g.add_def(m_bad, b"method", &cls, Some((11, 20))).unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "method `Foo` shares parent name: {v:?}");
}

#[test]
fn parent_kind_projection() {
	let cfg = cfg_from(
		r#"
		[[ts.method.where]]
		id   = "method-in-class"
		expr = "parent.kind = 'class'"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	// method directly under module (no class parent) — violates
	let m = child(&module, b"method", b"loose");
	g.add_def(m, b"method", &module, Some((0, 5))).unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "parent is module, not class: {v:?}");
}

#[test]
fn source_and_target_kind_projection() {
	let cfg = cfg_from(
		r#"
		[[refs.where]]
		id   = "no-class-to-function-edge"
		expr = "source.kind = 'class' => NOT target.kind = 'function'"
		"#,
	);
	let root = build_root();
	let mut g = CodeGraph::new(root.clone(), b"module");
	let cls = child(&root, b"class", b"Foo");
	g.add_def(cls.clone(), b"class", &root, Some((0, 5)))
		.unwrap();
	let func = child(&root, b"function", b"bar");
	g.add_def(func.clone(), b"function", &root, Some((6, 10)))
		.unwrap();
	g.add_ref(&cls, func, b"calls", Some((0, 5))).unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "class→function edge flagged: {v:?}");
}

// ─── refs pipeline ──────────────────────────────────────────────────

fn build_root() -> Moniker {
	let mut b = MonikerBuilder::new();
	b.project(b".");
	b.segment(b"lang", b"ts");
	b.build()
}

fn submodule(root: &Moniker, name: &[u8]) -> Moniker {
	let mut b = MonikerBuilder::from_view(root.as_view());
	b.segment(b"module", name);
	b.build()
}

#[test]
fn refs_top_level_flags_cross_layer_dep() {
	let cfg = cfg_from(
		r#"
		[[refs.where]]
		id   = "domain-no-infra"
		expr = "source ~ '**/module:domain/**' => NOT target ~ '**/module:infrastructure/**'"
		"#,
	);
	let root = build_root();
	let mut g = CodeGraph::new(root.clone(), b"module");
	let domain = submodule(&root, b"domain");
	g.add_def(domain.clone(), b"module", &root, Some((0, 1)))
		.unwrap();
	let infra = submodule(&root, b"infrastructure");
	g.add_def(infra.clone(), b"module", &root, Some((2, 3)))
		.unwrap();
	let order = child(&domain, b"class", b"Order");
	g.add_def(order.clone(), b"class", &domain, Some((4, 5)))
		.unwrap();
	let repo = child(&infra, b"class", b"OrderRepoImpl");
	g.add_def(repo.clone(), b"class", &infra, Some((6, 10)))
		.unwrap();
	g.add_ref(&order, repo, b"uses_type", Some((4, 5))).unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "cross-layer ref must violate: {v:?}");
	assert_eq!(v[0].rule_id, "refs.domain-no-infra");
}

#[test]
fn refs_rule_message_templates_are_rendered() {
	let cfg = cfg_from(
		r#"
		[[refs.where]]
		id      = "domain-no-infra"
		expr    = "source ~ '**/module:domain/**' => NOT target ~ '**/module:infrastructure/**'"
		message = "{source.name} {kind} {target.name} ({source.shape}->{target.shape}) failed {atom}: {actual}/{expected}"
		"#,
	);
	let root = build_root();
	let mut g = CodeGraph::new(root.clone(), b"module");
	let domain = submodule(&root, b"domain");
	g.add_def(domain.clone(), b"module", &root, Some((0, 1)))
		.unwrap();
	let infra = submodule(&root, b"infrastructure");
	g.add_def(infra.clone(), b"module", &root, Some((2, 3)))
		.unwrap();
	let order = child(&domain, b"class", b"Order");
	g.add_def(order.clone(), b"class", &domain, Some((4, 5)))
		.unwrap();
	let repo = child(&infra, b"class", b"OrderRepo");
	g.add_def(repo.clone(), b"class", &infra, Some((6, 10)))
		.unwrap();
	g.add_ref(&order, repo, b"uses_type", Some((4, 5))).unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1);
	let explanation = v[0].explanation.as_deref().unwrap_or_default();
	assert!(explanation.contains("Order uses_type OrderRepo"), "{v:?}");
	assert!(explanation.contains("(type->type)"), "{v:?}");
	assert!(!explanation.contains("{source.name}"), "{v:?}");
}

#[test]
fn refs_implication_skips_unrelated_refs() {
	let cfg = cfg_from(
		r#"
		[[refs.where]]
		id   = "domain-only-self-or-std"
		expr = "source ~ '**/module:domain/**' => target ~ '**/module:domain/**' OR target ~ '**/module:std/**'"
		"#,
	);
	let root = build_root();
	let mut g = CodeGraph::new(root.clone(), b"module");
	let domain = submodule(&root, b"domain");
	g.add_def(domain.clone(), b"module", &root, Some((0, 1)))
		.unwrap();
	let std_mod = submodule(&root, b"std");
	g.add_def(std_mod.clone(), b"module", &root, Some((2, 3)))
		.unwrap();
	let order = child(&domain, b"class", b"Order");
	g.add_def(order.clone(), b"class", &domain, Some((4, 5)))
		.unwrap();
	let vec_class = child(&std_mod, b"class", b"Vec");
	g.add_def(vec_class.clone(), b"class", &std_mod, Some((6, 10)))
		.unwrap();
	g.add_ref(&order, vec_class, b"uses_type", Some((4, 5)))
		.unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert!(v.is_empty(), "domain → std is allowed: {v:?}");
}

#[test]
fn refs_filtered_by_kind() {
	let cfg = cfg_from(
		r#"
		[[refs.where]]
		id   = "no-domain-imports-framework"
		expr = "source ~ '**/module:domain/**' AND kind = 'imports' => NOT target.name =~ ^(express|nestjs)$"
		"#,
	);
	let root = build_root();
	let mut g = CodeGraph::new(root.clone(), b"module");
	let domain = submodule(&root, b"domain");
	g.add_def(domain.clone(), b"module", &root, Some((0, 1)))
		.unwrap();
	let ext = submodule(&root, b"extern");
	g.add_def(ext.clone(), b"module", &root, Some((2, 3)))
		.unwrap();
	let order = child(&domain, b"class", b"Order");
	g.add_def(order.clone(), b"class", &domain, Some((4, 5)))
		.unwrap();
	let express = child(&ext, b"class", b"express");
	g.add_def(express.clone(), b"class", &ext, Some((6, 10)))
		.unwrap();
	g.add_ref(&order, express, b"imports", Some((4, 5)))
		.unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "domain import of express must violate: {v:?}");
}

#[test]
fn alias_expands_in_rule_expr() {
	let cfg = cfg_from(
		r#"
		[aliases]
		domain = "moniker ~ '**/module:domain/**'"

		[[ts.class.where]]
		id   = "no-class-in-domain"
		expr = "NOT $domain"
		"#,
	);
	let module = build_module(b"domain");
	let mut g = CodeGraph::new(module.clone(), b"module");
	g.add_def(
		child(&module, b"class", b"Foo"),
		b"class",
		&module,
		Some((0, 5)),
	)
	.unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "class in module:domain violates: {v:?}");
}

#[test]
fn path_match_subtree_flags_domain_class() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "no-class-in-domain"
		expr = "NOT moniker ~ '**/module:domain/**'"
		"#,
	);
	let module = build_module(b"domain");
	let mut g = CodeGraph::new(module.clone(), b"module");
	g.add_def(
		child(&module, b"class", b"User"),
		b"class",
		&module,
		Some((0, 10)),
	)
	.unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "class lives in module:domain: {v:?}");
}

#[test]
fn has_segment_finds_module() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "must-be-in-app"
		expr = "has_segment('module', 'application')"
		"#,
	);
	let module = build_module(b"infrastructure");
	let mut g = CodeGraph::new(module.clone(), b"module");
	g.add_def(
		child(&module, b"class", b"Foo"),
		b"class",
		&module,
		Some((0, 5)),
	)
	.unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "Foo lives in infrastructure, not application");
}

#[test]
fn path_regex_step_on_class_name() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "ports-only-in-app"
		expr = "moniker ~ '**/class:/Port$/' => has_segment('module', 'application')"
		"#,
	);
	let module = build_module(b"domain");
	let mut g = CodeGraph::new(module.clone(), b"module");
	// A `Port` class living in `domain` (wrong place) — should flag.
	g.add_def(
		child(&module, b"class", b"UserPort"),
		b"class",
		&module,
		Some((0, 5)),
	)
	.unwrap();
	// A non-Port class in domain — premise false, should NOT flag.
	g.add_def(
		child(&module, b"class", b"Order"),
		b"class",
		&module,
		Some((6, 10)),
	)
	.unwrap();
	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "only `UserPort` violates: {v:?}");
	assert!(v[0].moniker.contains("UserPort"));
}

#[test]
fn implies_true_premise_failed_consequent_violates() {
	let cfg = cfg_from(
		r#"
		[[ts.function.where]]
		id   = "use-case-has-one-method"
		expr = "name =~ UseCase$ => lines <= 5"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	g.add_def(
		child(&module, b"function", b"CreateInvoiceUseCase"),
		b"function",
		&module,
		Some((0, 200)),
	)
	.unwrap();
	// 50 lines of source so lines > 5
	let source: String = (0..50).map(|_| "a\n").collect();
	let v = evaluate(&g, &source, Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(
		v.len(),
		1,
		"premise true, consequent false ⇒ violation: {v:?}"
	);
}
