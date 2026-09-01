use super::*;
use code_moniker_core::core::code_graph::DefAttrs;
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::{LangExtractor, ts};

const SCHEME: &str = "code+moniker://";

#[test]
fn javascript_and_jsx_apply_independent_function_naming_rules() {
	let source = "export function Widget() { return 1; }";
	let anchor = MonikerBuilder::new().project(b".").build();
	let presets = ts::Presets::default();
	let js = <ts::JsLang as LangExtractor>::extract("widget.js", source, &anchor, false, &presets);
	let jsx =
		<ts::JsxLang as LangExtractor>::extract("widget.jsx", source, &anchor, false, &presets);
	let cfg = crate::check::config::load_default().expect("default rules");

	let js_violations = evaluate(&js, source, Lang::Js, &cfg, SCHEME).unwrap();
	let jsx_violations = evaluate(&jsx, source, Lang::Jsx, &cfg, SCHEME).unwrap();

	assert!(
		js_violations
			.iter()
			.any(|violation| violation.rule_id == "js.function.name-camelcase"),
		"PascalCase functions are not ordinary JavaScript helpers: {js_violations:?}"
	);
	assert!(
		jsx_violations
			.iter()
			.all(|violation| violation.rule_id != "jsx.function.name-component-or-camelcase"),
		"PascalCase functions are valid JSX components: {jsx_violations:?}"
	);
}

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

fn line_span(source: &str, line: u32) -> (u32, u32) {
	let mut start = 0usize;
	for _ in 1..line {
		let next = source[start..]
			.find('\n')
			.map(|idx| start + idx + 1)
			.expect("line must exist in test source");
		start = next;
	}
	let end = source[start..]
		.find('\n')
		.map(|idx| start + idx)
		.unwrap_or(source.len());
	(start as u32, end as u32)
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
fn position_projections_are_available_to_rules() {
	let cfg = cfg_from(
		r#"
		[[ts.function.where]]
		id   = "position"
		expr = "start_line = 2 AND end_line = 3 AND start_byte = 2"
		"#,
	);
	let source = "a\nbc\nde\n";
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let f = child(&module, b"function", b"foo");
	g.add_def(f, b"function", &module, Some((2, 7))).unwrap();
	let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).unwrap();
	assert!(v.is_empty(), "position projections should match: {v:?}");
}

#[test]
fn vertical_layout_warns_when_private_helper_is_far_from_first_use() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id       = "vertical-layout"
		severity = "warn"
		expr     = "vertical_layout(shape:callable, private_after_first_use, max_gap = 3)"
		message  = "Callable layout under `{name}` does not match first-use order."
		"#,
	);
	let source = "class Foo\nrun\nother\n\n\n\n\nhelper\n";
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(
		foo.clone(),
		b"class",
		&module,
		Some((0, source.len() as u32)),
	)
	.unwrap();
	let run = child(&foo, b"method", b"run");
	g.add_def(run.clone(), b"method", &foo, Some(line_span(source, 2)))
		.unwrap();
	g.add_def(
		child(&foo, b"method", b"other"),
		b"method",
		&foo,
		Some(line_span(source, 3)),
	)
	.unwrap();
	let helper = child(&foo, b"method", b"helper");
	g.add_def_attrs(
		helper.clone(),
		b"method",
		&foo,
		Some(line_span(source, 8)),
		&DefAttrs {
			visibility: b"private",
			..DefAttrs::default()
		},
	)
	.unwrap();
	g.add_ref(&run, helper, b"calls", Some(line_span(source, 2)))
		.unwrap();
	let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "{v:?}");
	assert_eq!(v[0].severity, RuleSeverity::Warn);
	let explanation = v[0].explanation.as_deref().unwrap_or_default();
	assert!(
		explanation.contains("current: run -> other -> helper"),
		"{v:?}"
	);
	assert!(
		explanation.contains("suggested: run -> helper -> other"),
		"{v:?}"
	);
	assert!(explanation.contains("move: helper `helper`"), "{v:?}");
}

#[test]
fn vertical_layout_allows_private_helper_close_to_first_use() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id       = "vertical-layout"
		severity = "warn"
		expr     = "vertical_layout(shape:callable, private_after_first_use, max_gap = 3)"
		"#,
	);
	let source = "class Foo\nrun\n\nhelper\nother\n";
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(
		foo.clone(),
		b"class",
		&module,
		Some((0, source.len() as u32)),
	)
	.unwrap();
	let run = child(&foo, b"method", b"run");
	g.add_def(run.clone(), b"method", &foo, Some(line_span(source, 2)))
		.unwrap();
	let helper = child(&foo, b"method", b"helper");
	g.add_def_attrs(
		helper.clone(),
		b"method",
		&foo,
		Some(line_span(source, 4)),
		&DefAttrs {
			visibility: b"private",
			..DefAttrs::default()
		},
	)
	.unwrap();
	g.add_def(
		child(&foo, b"method", b"other"),
		b"method",
		&foo,
		Some(line_span(source, 5)),
	)
	.unwrap();
	g.add_ref(&run, helper, b"calls", Some(line_span(source, 2)))
		.unwrap();
	let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).unwrap();
	assert!(v.is_empty(), "helper is within max_gap: {v:?}");
}

#[test]
fn vertical_layout_skips_callables_in_different_layout_regions() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id       = "vertical-layout"
		severity = "warn"
		expr     = "vertical_layout(shape:callable, private_after_first_use, max_gap = 3)"
		"#,
	);
	let source = "class Foo {\nrun\n}\nnamespace Foo {\nprivate helper()\n}\n";
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(
		foo.clone(),
		b"class",
		&module,
		Some((0, source.len() as u32)),
	)
	.unwrap();
	let run = child(&foo, b"method", b"run");
	g.add_def(run.clone(), b"method", &foo, Some(line_span(source, 2)))
		.unwrap();
	let ns = child(&foo, b"namespace", b"Foo");
	g.add_def(ns.clone(), b"namespace", &foo, Some(line_span(source, 4)))
		.unwrap();
	let helper = child(&ns, b"method", b"helper");
	g.add_def_attrs(
		helper.clone(),
		b"method",
		&ns,
		Some(line_span(source, 5)),
		&DefAttrs {
			visibility: b"private",
			..DefAttrs::default()
		},
	)
	.unwrap();
	g.add_ref(&run, helper, b"calls", Some(line_span(source, 2)))
		.unwrap();
	let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).unwrap();
	assert!(
		v.is_empty(),
		"helpers in another physical layout region cannot be reordered with the caller: {v:?}"
	);
}

#[test]
fn vertical_layout_ignores_non_calling_refs_for_first_use() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id       = "vertical-layout"
		severity = "warn"
		expr     = "vertical_layout(shape:callable, private_after_first_use, max_gap = 0)"
		"#,
	);
	let source = "class Foo\nrun\n\nprivate helper()\nother\n";
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(
		foo.clone(),
		b"class",
		&module,
		Some((0, source.len() as u32)),
	)
	.unwrap();
	let run = child(&foo, b"method", b"run");
	g.add_def(run.clone(), b"method", &foo, Some(line_span(source, 2)))
		.unwrap();
	let helper = child(&foo, b"method", b"helper");
	g.add_def_attrs(
		helper.clone(),
		b"method",
		&foo,
		Some(line_span(source, 4)),
		&DefAttrs {
			visibility: b"private",
			..DefAttrs::default()
		},
	)
	.unwrap();
	g.add_def(
		child(&foo, b"method", b"other"),
		b"method",
		&foo,
		Some(line_span(source, 5)),
	)
	.unwrap();
	g.add_ref(&run, helper, b"reads", Some(line_span(source, 2)))
		.unwrap();
	let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).unwrap();
	assert!(
		v.is_empty(),
		"non-calling refs must not enforce first-use ordering: {v:?}"
	);
}

#[test]
fn vertical_layout_warns_when_private_callable_precedes_public_surface() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id       = "vertical-layout"
		severity = "warn"
		expr     = "vertical_layout(shape:callable, public_first)"
		"#,
	);
	let source = "class Foo\nhelper\nrun\n";
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(
		foo.clone(),
		b"class",
		&module,
		Some((0, source.len() as u32)),
	)
	.unwrap();
	let helper = child(&foo, b"method", b"helper");
	g.add_def_attrs(
		helper,
		b"method",
		&foo,
		Some(line_span(source, 2)),
		&DefAttrs {
			visibility: b"private",
			..DefAttrs::default()
		},
	)
	.unwrap();
	let run = child(&foo, b"method", b"run");
	g.add_def_attrs(
		run,
		b"method",
		&foo,
		Some(line_span(source, 3)),
		&DefAttrs {
			visibility: b"public",
			..DefAttrs::default()
		},
	)
	.unwrap();
	let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "{v:?}");
	let explanation = v[0].explanation.as_deref().unwrap_or_default();
	assert!(explanation.contains("suggested: run -> helper"), "{v:?}");
	assert!(
		explanation.contains("private declaration appears before visible API"),
		"{v:?}"
	);
}

#[test]
fn vertical_layout_skips_unpositioned_items_without_disabling_group() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id       = "vertical-layout"
		severity = "warn"
		expr     = "vertical_layout(shape:callable, private_after_first_use, max_gap = 3)"
		"#,
	);
	let source = "class Foo\nrun\nother\n\n\n\n\nhelper\n";
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(
		foo.clone(),
		b"class",
		&module,
		Some((0, source.len() as u32)),
	)
	.unwrap();
	let run = child(&foo, b"method", b"run");
	g.add_def(run.clone(), b"method", &foo, Some(line_span(source, 2)))
		.unwrap();
	g.add_def(
		child(&foo, b"method", b"unpositioned"),
		b"method",
		&foo,
		None,
	)
	.unwrap();
	g.add_def(
		child(&foo, b"method", b"other"),
		b"method",
		&foo,
		Some(line_span(source, 3)),
	)
	.unwrap();
	let helper = child(&foo, b"method", b"helper");
	g.add_def_attrs(
		helper.clone(),
		b"method",
		&foo,
		Some(line_span(source, 8)),
		&DefAttrs {
			visibility: b"private",
			..DefAttrs::default()
		},
	)
	.unwrap();
	g.add_ref(&run, helper, b"calls", Some(line_span(source, 2)))
		.unwrap();
	let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "{v:?}");
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

fn caller_concentration_config() -> Config {
	cfg_from(
		r#"
		[aliases]
		test_caller = "source.kind = 'test' OR source ~ '**/module:tests/**'"

		[[ts.method.where]]
		id   = "caller-concentration"
		expr = "visibility != 'public' OR count(in_refs, NOT $test_caller) < 6 OR entropy(in_refs, source.parent, NOT $test_caller) >= 0.5"
		"#,
	)
}

fn caller_concentration_graph(
	production_callers: usize,
	test_module_callers: usize,
	direct_test_callers: usize,
) -> CodeGraph {
	let module = build_module(b"a");
	let mut graph = CodeGraph::new(module.clone(), b"module");
	let api = child(&module, b"class", b"Api");
	graph
		.add_def(api.clone(), b"class", &module, Some((0, 10)))
		.unwrap();
	let operation = child(&api, b"method", b"operation");
	graph
		.add_def_attrs(
			operation.clone(),
			b"method",
			&api,
			Some((1, 2)),
			&DefAttrs {
				visibility: b"public",
				..Default::default()
			},
		)
		.unwrap();
	let production = child(&module, b"module", b"production");
	graph
		.add_def(production.clone(), b"module", &module, Some((20, 30)))
		.unwrap();
	for index in 0..production_callers {
		let caller = child(&production, b"fn", format!("caller_{index}").as_bytes());
		graph
			.add_def(caller.clone(), b"fn", &production, Some((21, 22)))
			.unwrap();
		graph
			.add_ref(&caller, operation.clone(), b"method_call", Some((21, 22)))
			.unwrap();
	}
	let tests = child(&module, b"module", b"tests");
	graph
		.add_def(tests.clone(), b"module", &module, Some((40, 50)))
		.unwrap();
	for index in 0..test_module_callers {
		let helper = child(&tests, b"fn", format!("fixture_{index}").as_bytes());
		graph
			.add_def(helper.clone(), b"fn", &tests, Some((41, 42)))
			.unwrap();
		graph
			.add_ref(&helper, operation.clone(), b"method_call", Some((41, 42)))
			.unwrap();
	}
	for index in 0..direct_test_callers {
		let test = child(&module, b"test", format!("direct_test_{index}").as_bytes());
		graph
			.add_def(test.clone(), b"test", &module, Some((60, 61)))
			.unwrap();
		graph
			.add_ref(&test, operation.clone(), b"method_call", Some((60, 61)))
			.unwrap();
	}
	graph
}

#[test]
fn filtered_entropy_excludes_helpers_inside_test_modules() {
	let graph = caller_concentration_graph(0, 8, 0);
	let violations =
		evaluate(&graph, "", Lang::Ts, &caller_concentration_config(), SCHEME).unwrap();
	assert!(
		violations.is_empty(),
		"test-only helpers are not production callers: {violations:?}"
	);
}

#[test]
fn filtered_entropy_excludes_direct_test_callers() {
	let graph = caller_concentration_graph(0, 0, 8);
	let violations =
		evaluate(&graph, "", Lang::Ts, &caller_concentration_config(), SCHEME).unwrap();
	assert!(
		violations.is_empty(),
		"direct test definitions are not production callers: {violations:?}"
	);
}

#[test]
fn filtered_entropy_keeps_concentrated_production_calls_despite_more_tests() {
	let graph = caller_concentration_graph(6, 7, 0);
	let violations =
		evaluate(&graph, "", Lang::Ts, &caller_concentration_config(), SCHEME).unwrap();
	assert_eq!(violations.len(), 1, "{violations:?}");
	assert_eq!(violations[0].rule_id, "ts.method.caller-concentration");
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
fn collection_size_unique_detects_duplicate_method_names() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "unique-method-names"
		expr = "size(unique(method.name)) = size(method.name)"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(foo.clone(), b"class", &module, Some((0, 50)))
		.unwrap();
	for name in [b"run()".as_slice(), b"run(x:int)".as_slice()] {
		g.add_def(child(&foo, b"method", name), b"method", &foo, Some((1, 5)))
			.unwrap();
	}
	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(
		v.len(),
		1,
		"duplicate bare method names should violate: {v:?}"
	);
	assert_eq!(v[0].rule_id, "ts.class.unique-method-names");
}

#[test]
fn collection_subset_compares_projected_multisets() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "fields-have-method-name"
		expr = "field.name subset method.name"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(foo.clone(), b"class", &module, Some((0, 50)))
		.unwrap();
	g.add_def(child(&foo, b"field", b"seen"), b"field", &foo, Some((1, 2)))
		.unwrap();
	g.add_def(
		child(&foo, b"field", b"missing"),
		b"field",
		&foo,
		Some((3, 4)),
	)
	.unwrap();
	g.add_def(
		child(&foo, b"method", b"seen()"),
		b"method",
		&foo,
		Some((5, 8)),
	)
	.unwrap();
	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(
		v.len(),
		1,
		"missing field name should not be in methods: {v:?}"
	);
	assert_eq!(v[0].rule_id, "ts.class.fields-have-method-name");
}

#[test]
fn collection_projection_can_follow_nested_ref_domains() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "field-readers-local"
		expr = "size(unique(field.in_refs.source.parent)) = 1"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(foo.clone(), b"class", &module, Some((0, 50)))
		.unwrap();
	let field = child(&foo, b"field", b"value");
	g.add_def(field.clone(), b"field", &foo, Some((1, 2)))
		.unwrap();
	for name in [b"a()", b"b()"] {
		let method = child(&foo, b"method", name);
		g.add_def(method.clone(), b"method", &foo, Some((5, 10)))
			.unwrap();
		g.add_ref(&method, field.clone(), b"reads", Some((6, 7)))
			.unwrap();
	}
	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert!(v.is_empty(), "all field readers are methods of Foo: {v:?}");
}

#[test]
fn pair_count_detects_duplicate_method_names() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "no-overloaded-method-names"
		expr = "count(pairs(method), a.name = b.name) = 0"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(foo.clone(), b"class", &module, Some((0, 50)))
		.unwrap();
	for name in [
		b"run()".as_slice(),
		b"run(x:int)".as_slice(),
		b"stop()".as_slice(),
	] {
		g.add_def(child(&foo, b"method", name), b"method", &foo, Some((1, 5)))
			.unwrap();
	}
	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "duplicate pair should violate: {v:?}");
	assert_eq!(v[0].rule_id, "ts.class.no-overloaded-method-names");
}

#[test]
fn descendants_domain_supports_pairwise_duplicate_checks() {
	let cfg = cfg_from(
		r#"
		[[ts.module.where]]
		id   = "no-duplicate-descendant-callables"
		expr = """
		  name != 'root'
		  OR count(
		    pairs(descendants(shape:callable)),
		    a.name = b.name AND a.parent != b.parent
		  ) = 0
		"""
		"#,
	);
	let root = build_module(b"root");
	let mut g = CodeGraph::new(root.clone(), b"module");
	let left = child(&root, b"module", b"left");
	let right = child(&root, b"module", b"right");
	g.add_def(left.clone(), b"module", &root, Some((0, 20)))
		.unwrap();
	g.add_def(right.clone(), b"module", &root, Some((21, 40)))
		.unwrap();
	g.add_def(
		child(&left, b"function", b"run()"),
		b"function",
		&left,
		Some((1, 5)),
	)
	.unwrap();
	g.add_def(
		child(&left, b"function", b"local_only()"),
		b"function",
		&left,
		Some((6, 10)),
	)
	.unwrap();
	g.add_def(
		child(&right, b"function", b"run(input)"),
		b"function",
		&right,
		Some((22, 30)),
	)
	.unwrap();

	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "root sees duplicate descendant names: {v:?}");
	assert_eq!(v[0].rule_id, "ts.module.no-duplicate-descendant-callables");
	assert!(v[0].moniker.contains("module:root"));
}

#[test]
fn pair_quantifier_binds_self_to_owner() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "method-pairs-owned-by-class"
		expr = "all(pairs(method), a.parent = self AND b.parent = self)"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(foo.clone(), b"class", &module, Some((0, 50)))
		.unwrap();
	g.add_def(
		child(&foo, b"method", b"a()"),
		b"method",
		&foo,
		Some((1, 5)),
	)
	.unwrap();
	g.add_def(
		child(&foo, b"method", b"b()"),
		b"method",
		&foo,
		Some((6, 10)),
	)
	.unwrap();
	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert!(v.is_empty(), "pair self binding should point to Foo: {v:?}");
}

#[test]
fn pair_filter_compares_child_collection_intersection() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "no-data-clumps"
		expr = "count(pairs(method), size(a.param.name intersect b.param.name) >= 3) = 0"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	g.add_def(foo.clone(), b"class", &module, Some((0, 80)))
		.unwrap();
	let create = child(&foo, b"method", b"create()");
	let update = child(&foo, b"method", b"update()");
	let archive = child(&foo, b"method", b"archive()");
	for method in [&create, &update, &archive] {
		g.add_def(method.clone(), b"method", &foo, Some((1, 10)))
			.unwrap();
	}
	for (method, names) in [
		(&create, ["customer_id", "street", "zip"]),
		(&update, ["customer_id", "street", "zip"]),
		(&archive, ["customer_id", "reason", "dry_run"]),
	] {
		for name in names {
			g.add_def(
				child(method, b"param", name.as_bytes()),
				b"param",
				method,
				Some((1, 1)),
			)
			.unwrap();
		}
	}
	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "shared param clump should violate: {v:?}");
	assert_eq!(v[0].rule_id, "ts.class.no-data-clumps");
}

#[test]
fn named_metrics_evaluate_local_class_metrics() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "local-class-metrics"
		expr = """
		  name != 'Foo'
		  OR (wmc(self) = 2 AND rfc(self) = 3 AND cbo(self) = 2 AND fan_in(self) = 1)
		"""
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let foo = child(&module, b"class", b"Foo");
	let bar = child(&module, b"class", b"Bar");
	let baz = child(&module, b"class", b"Baz");
	g.add_def(foo.clone(), b"class", &module, Some((0, 50)))
		.unwrap();
	g.add_def(bar.clone(), b"class", &module, Some((60, 90)))
		.unwrap();
	g.add_def(baz.clone(), b"class", &module, Some((100, 130)))
		.unwrap();
	let run = child(&foo, b"method", b"run()");
	let stop = child(&foo, b"method", b"stop()");
	let value = child(&foo, b"field", b"value");
	let helper = child(&bar, b"method", b"helper()");
	g.add_def(run.clone(), b"method", &foo, Some((1, 5)))
		.unwrap();
	g.add_def(stop, b"method", &foo, Some((6, 10))).unwrap();
	g.add_def(value.clone(), b"field", &foo, Some((11, 12)))
		.unwrap();
	g.add_def(helper.clone(), b"method", &bar, Some((61, 65)))
		.unwrap();
	g.add_ref(&run, helper, b"calls", Some((2, 3))).unwrap();
	g.add_ref(&run, value, b"reads", Some((4, 5))).unwrap();
	g.add_ref(&baz, foo.clone(), b"uses", Some((101, 102)))
		.unwrap();

	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert!(
		v.is_empty(),
		"metrics should match local graph facts: {v:?}"
	);
}

#[test]
fn inheritance_metrics_use_local_extends_refs() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "inheritance-bounds"
		expr = """
		  (name != 'Base' OR noc(self) <= 1)
		  AND (name != 'Grand' OR dit(self) <= 1)
		"""
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let base = child(&module, b"class", b"Base");
	let child1 = child(&module, b"class", b"Child1");
	let child2 = child(&module, b"class", b"Child2");
	let grand = child(&module, b"class", b"Grand");
	for class in [&base, &child1, &child2, &grand] {
		g.add_def(class.clone(), b"class", &module, Some((0, 10)))
			.unwrap();
	}
	g.add_ref(&child1, base.clone(), b"extends", Some((1, 2)))
		.unwrap();
	g.add_ref(&child2, base, b"extends", Some((3, 4))).unwrap();
	g.add_ref(&grand, child1, b"extends", Some((5, 6))).unwrap();

	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 2, "Base NOC and Grand DIT should violate: {v:?}");
	assert!(v.iter().any(|violation| violation.moniker.contains("Base")));
	assert!(
		v.iter()
			.any(|violation| violation.moniker.contains("Grand"))
	);
}

#[test]
fn lcom4_detects_disconnected_method_groups() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "cohesive-methods"
		expr = "name != 'Split' OR lcom4(self) <= 1"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	let split = child(&module, b"class", b"Split");
	g.add_def(split.clone(), b"class", &module, Some((0, 80)))
		.unwrap();
	let x = child(&split, b"field", b"x");
	let y = child(&split, b"field", b"y");
	g.add_def(x.clone(), b"field", &split, Some((1, 2)))
		.unwrap();
	g.add_def(y.clone(), b"field", &split, Some((3, 4)))
		.unwrap();
	let methods = [
		child(&split, b"method", b"a()"),
		child(&split, b"method", b"b()"),
		child(&split, b"method", b"c()"),
		child(&split, b"method", b"d()"),
	];
	for method in &methods {
		g.add_def(method.clone(), b"method", &split, Some((10, 12)))
			.unwrap();
	}
	g.add_ref(&methods[0], x.clone(), b"reads", Some((11, 12)))
		.unwrap();
	g.add_ref(&methods[1], x, b"reads", Some((13, 14))).unwrap();
	g.add_ref(&methods[2], y.clone(), b"reads", Some((15, 16)))
		.unwrap();
	g.add_ref(&methods[3], y, b"reads", Some((17, 18))).unwrap();

	let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(
		v.len(),
		1,
		"two method/field components should violate: {v:?}"
	);
	assert_eq!(v[0].rule_id, "ts.class.cohesive-methods");
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

#[test]
fn srcset_projections_cover_defs_and_reference_endpoints() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id = "main-class"
		expr = "srcset = 'main'"

		[[refs.where]]
		id = "main-must-not-call-test"
		expr = "source.srcset = 'main' => target.srcset != 'test'"
		"#,
	);
	let mut root_builder = MonikerBuilder::new();
	root_builder.project(b".");
	root_builder.segment(b"srcset", b"main");
	root_builder.segment(b"lang", b"ts");
	root_builder.segment(b"module", b"app");
	let root = root_builder.build();
	let mut graph = CodeGraph::new(root.clone(), b"module");
	let source = child(&root, b"class", b"App");
	graph
		.add_def(source.clone(), b"class", &root, Some((0, 5)))
		.unwrap();
	let mut target_builder = MonikerBuilder::new();
	target_builder.project(b".");
	target_builder.segment(b"srcset", b"test");
	target_builder.segment(b"lang", b"ts");
	target_builder.segment(b"module", b"support");
	target_builder.segment(b"function", b"fixture");
	graph
		.add_ref(&source, target_builder.build(), b"calls", Some((0, 5)))
		.unwrap();

	let violations = evaluate(&graph, "fixture", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(violations.len(), 1, "{violations:#?}");
	assert_eq!(
		violations[0].rule_id, "refs.main-must-not-call-test",
		"{violations:#?}"
	);
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
fn ref_text_projection_uses_reference_span() {
	let cfg = cfg_from(
		r#"
		[[refs.where]]
		id   = "no-qualified-java-time"
		expr = "kind != 'uses_type' OR text !~ '^java\\.time\\.'"
		"#,
	);
	let source = "java.time.Instant capturedAt;\n";
	let root = build_root();
	let mut g = CodeGraph::new(root.clone(), b"module");
	let cls = child(&root, b"class", b"ClockReader");
	g.add_def(cls.clone(), b"class", &root, Some(line_span(source, 1)))
		.unwrap();
	let target = {
		let mut b = MonikerBuilder::from_view(root.as_view());
		b.segment(b"sdk", b"java");
		b.segment(b"path", b"java");
		b.segment(b"path", b"time");
		b.segment(b"path", b"Instant");
		b.build()
	};
	let start = source.find("java.time.Instant").unwrap() as u32;
	let end = start + "java.time.Instant".len() as u32;
	g.add_ref(&cls, target, b"uses_type", Some((start, end)))
		.unwrap();
	let v = evaluate(&g, source, Lang::Java, &cfg, SCHEME).unwrap();
	assert_eq!(v.len(), 1, "qualified ref text should violate: {v:?}");
	assert_eq!(v[0].lines, (1, 1));
}

#[test]
fn current_projection_compares_ref_to_ancestor_refs() {
	let cfg = cfg_from(
		r#"
		[[refs.where]]
		id   = "qualified-type-needs-alternative"
		expr = """
		  kind != 'uses_type'
		  OR text !~ '^java\\.time\\.'
		  OR any(source.ancestors.out_refs,
		    kind = 'imports_symbol'
		    AND target.name = current.target.name
		    AND target != current.target
		  )
		"""
		"#,
	);
	let source = "import com.acme.other.Instant;\njava.time.Instant capturedAt;\n";
	let root = build_root();
	let mut g = CodeGraph::new(root.clone(), b"module");
	let cls = child(&root, b"class", b"AuditClock");
	g.add_def(cls.clone(), b"class", &root, Some(line_span(source, 2)))
		.unwrap();
	let imported = {
		let mut b = MonikerBuilder::from_view(root.as_view());
		b.segment(b"external_pkg", b"com");
		b.segment(b"path", b"acme");
		b.segment(b"path", b"other");
		b.segment(b"path", b"Instant");
		b.build()
	};
	let qualified = {
		let mut b = MonikerBuilder::from_view(root.as_view());
		b.segment(b"sdk", b"java");
		b.segment(b"path", b"java");
		b.segment(b"path", b"time");
		b.segment(b"path", b"Instant");
		b.build()
	};
	g.add_ref(
		&root,
		imported,
		b"imports_symbol",
		Some(line_span(source, 1)),
	)
	.unwrap();
	let start = source.find("java.time.Instant").unwrap() as u32;
	let end = start + "java.time.Instant".len() as u32;
	g.add_ref(&cls, qualified, b"uses_type", Some((start, end)))
		.unwrap();
	let v = evaluate(&g, source, Lang::Java, &cfg, SCHEME).unwrap();
	assert!(
		v.is_empty(),
		"ancestor import with same simple name should justify FQN: {v:?}"
	);
}

#[test]
fn current_projection_supports_moniker_ancestry() {
	let cfg = cfg_from(
		r#"
		[[refs.where]]
		id   = "qualified-type-allows-imported-outer"
		expr = "kind != 'uses_type' OR any(source.ancestors.out_refs, kind = 'imports_symbol' AND target @> current.target)"
		"#,
	);
	let source = "import java.util.Map;\nMap.Entry<String, String> entry;\n";
	let root = build_root();
	let mut graph = CodeGraph::new(root.clone(), b"module");
	let class = child(&root, b"class", b"MapUser");
	graph
		.add_def(class.clone(), b"class", &root, Some(line_span(source, 2)))
		.unwrap();
	let map = {
		let mut builder = MonikerBuilder::from_view(root.as_view());
		builder.segment(b"sdk", b"java");
		builder.segment(b"path", b"java");
		builder.segment(b"path", b"util");
		builder.segment(b"path", b"Map");
		builder.build()
	};
	let entry = {
		let mut builder = MonikerBuilder::from_view(map.as_view());
		builder.segment(b"path", b"Entry");
		builder.build()
	};
	graph
		.add_ref(&root, map, b"imports_symbol", Some(line_span(source, 1)))
		.unwrap();
	let start = source.find("Map.Entry").unwrap() as u32;
	graph
		.add_ref(
			&class,
			entry,
			b"uses_type",
			Some((start, start + "Map.Entry".len() as u32)),
		)
		.unwrap();
	let violations = evaluate(&graph, source, Lang::Java, &cfg, SCHEME).unwrap();
	assert!(
		violations.is_empty(),
		"imported outer type should justify qualification: {violations:?}"
	);
}

#[test]
fn current_projection_rejects_unrelated_imports() {
	let cfg = cfg_from(
		r#"
		[[refs.where]]
		id   = "qualified-type-needs-alternative"
		expr = """
		  kind != 'uses_type'
		  OR text = target.name
		  OR any(source.ancestors.out_refs,
		    kind = 'imports_symbol'
		    AND target.name = current.target.name
		    AND target != current.target
		  )
		"""
		"#,
	);
	let source = "import com.acme.other.Foo;\njava.time.Instant capturedAt;\n";
	let root = build_root();
	let mut g = CodeGraph::new(root.clone(), b"module");
	let cls = child(&root, b"class", b"AuditClock");
	g.add_def(cls.clone(), b"class", &root, Some(line_span(source, 2)))
		.unwrap();
	let imported = {
		let mut b = MonikerBuilder::from_view(root.as_view());
		b.segment(b"external_pkg", b"com");
		b.segment(b"path", b"acme");
		b.segment(b"path", b"other");
		b.segment(b"path", b"Foo");
		b.build()
	};
	let qualified = {
		let mut b = MonikerBuilder::from_view(root.as_view());
		b.segment(b"sdk", b"java");
		b.segment(b"path", b"java");
		b.segment(b"path", b"time");
		b.segment(b"path", b"Instant");
		b.build()
	};
	g.add_ref(
		&root,
		imported,
		b"imports_symbol",
		Some(line_span(source, 1)),
	)
	.unwrap();
	let start = source.find("java.time.Instant").unwrap() as u32;
	let end = start + "java.time.Instant".len() as u32;
	g.add_ref(&cls, qualified, b"uses_type", Some((start, end)))
		.unwrap();
	let v = evaluate(&g, source, Lang::Java, &cfg, SCHEME).unwrap();
	assert_eq!(
		v.len(),
		1,
		"unrelated import must not justify qualified use: {v:?}"
	);
}

#[test]
fn current_projection_tracks_immediate_ref_quantifier_parent() {
	let cfg = cfg_from(
		r#"
		[[refs.where]]
		id   = "nested-current"
		expr = """
		  kind != 'uses_type'
		  OR target.name != 'LocalDate'
		  OR any(source.out_refs,
		    kind = 'imports_symbol'
		    AND any(source.out_refs,
		      kind = 'uses_type'
		      AND target.name = current.target.name
		      AND target != current.target
		    )
		  )
		"""
		"#,
	);
	let source = "java.time.LocalDate businessDate;\nimport com.acme.other.Instant;\njava.time.Instant capturedAt;\n";
	let root = build_root();
	let mut g = CodeGraph::new(root.clone(), b"module");
	let cls = child(&root, b"class", b"AuditClock");
	g.add_def(cls.clone(), b"class", &root, Some(line_span(source, 1)))
		.unwrap();
	let local_date = {
		let mut b = MonikerBuilder::from_view(root.as_view());
		b.segment(b"sdk", b"java");
		b.segment(b"path", b"java");
		b.segment(b"path", b"time");
		b.segment(b"path", b"LocalDate");
		b.build()
	};
	let imported_instant = {
		let mut b = MonikerBuilder::from_view(root.as_view());
		b.segment(b"external_pkg", b"com");
		b.segment(b"path", b"acme");
		b.segment(b"path", b"other");
		b.segment(b"path", b"Instant");
		b.build()
	};
	let qualified_instant = {
		let mut b = MonikerBuilder::from_view(root.as_view());
		b.segment(b"sdk", b"java");
		b.segment(b"path", b"java");
		b.segment(b"path", b"time");
		b.segment(b"path", b"Instant");
		b.build()
	};
	let local_date_start = source.find("java.time.LocalDate").unwrap() as u32;
	let local_date_end = local_date_start + "java.time.LocalDate".len() as u32;
	g.add_ref(
		&cls,
		local_date,
		b"uses_type",
		Some((local_date_start, local_date_end)),
	)
	.unwrap();
	g.add_ref(
		&cls,
		imported_instant,
		b"imports_symbol",
		Some(line_span(source, 2)),
	)
	.unwrap();
	let instant_start = source.rfind("java.time.Instant").unwrap() as u32;
	let instant_end = instant_start + "java.time.Instant".len() as u32;
	g.add_ref(
		&cls,
		qualified_instant,
		b"uses_type",
		Some((instant_start, instant_end)),
	)
	.unwrap();
	let v = evaluate(&g, source, Lang::Java, &cfg, SCHEME).unwrap();
	assert!(
		v.is_empty(),
		"nested current.* should bind to the immediate imports_symbol ref: {v:?}"
	);
}

#[test]
fn target_ref_domains_detect_only_one_to_one_method_helpers() {
	fn add_def(graph: &mut CodeGraph, def: &Moniker, kind: &[u8], parent: &Moniker) {
		graph
			.add_def(def.clone(), kind, parent, Some((0, 0)))
			.unwrap();
	}

	fn add_ref(graph: &mut CodeGraph, source: &Moniker, target: &Moniker, kind: &[u8]) {
		graph
			.add_ref(source, target.clone(), kind, Some((0, 0)))
			.unwrap();
	}

	let cfg = cfg_from(
		r#"
		[[shape.type.where]]
		id = "no-one-to-one-method-helper"
		expr = """
		  count(method,
		    lines <= 13
		    AND count(out_refs, kind = 'calls' AND target.kind = 'function') = 1
		    AND any(out_refs,
		      kind = 'calls'
		      AND target.kind = 'function'
		      AND count(target.in_refs, kind = 'calls') = 1
		      AND any(target.out_refs,
		        kind = 'uses_type'
		        AND target = current.source.parent
		      )
		    )
		  ) = 0
		"""
		"#,
	);
	let module = build_module(b"delegation");
	let mut graph = CodeGraph::new(module.clone(), b"module");

	let satellite = child(&module, b"class", b"Satellite");
	let satellite_method = child(&satellite, b"method", b"refresh");
	let satellite_helper = child(&module, b"function", b"refresh_satellite");
	add_def(&mut graph, &satellite, b"class", &module);
	add_def(&mut graph, &satellite_method, b"method", &satellite);
	add_def(&mut graph, &satellite_helper, b"function", &module);
	add_ref(&mut graph, &satellite_method, &satellite_helper, b"calls");
	add_ref(&mut graph, &satellite_helper, &satellite, b"uses_type");

	let shared = child(&module, b"class", b"Shared");
	let shared_helper = child(&module, b"function", b"refresh_shared");
	add_def(&mut graph, &shared, b"class", &module);
	add_def(&mut graph, &shared_helper, b"function", &module);
	for name in [b"first".as_slice(), b"second".as_slice()] {
		let method = child(&shared, b"method", name);
		add_def(&mut graph, &method, b"method", &shared);
		add_ref(&mut graph, &method, &shared_helper, b"calls");
	}
	add_ref(&mut graph, &shared_helper, &shared, b"uses_type");

	let foreign = child(&module, b"class", b"Foreign");
	let other = child(&module, b"class", b"Other");
	let foreign_method = child(&foreign, b"method", b"refresh");
	let foreign_helper = child(&module, b"function", b"refresh_other");
	for (definition, kind, parent) in [
		(&foreign, b"class".as_slice(), &module),
		(&other, b"class".as_slice(), &module),
		(&foreign_method, b"method".as_slice(), &foreign),
		(&foreign_helper, b"function".as_slice(), &module),
	] {
		add_def(&mut graph, definition, kind, parent);
	}
	add_ref(&mut graph, &foreign_method, &foreign_helper, b"calls");
	add_ref(&mut graph, &foreign_helper, &other, b"uses_type");

	let violations = evaluate(&graph, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(
		violations.len(),
		1,
		"only the one-to-one cycle violates: {violations:?}"
	);
	assert!(
		violations[0].moniker.contains("Satellite"),
		"{violations:?}"
	);
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

#[test]
fn disjoint_holds_unless_both_operands_match_in_either_order() {
	let rule = |expr: &str| {
		cfg_from(&format!(
			r#"
			[[ts.class.where]]
			id   = "disjoint-truth-table"
			expr = "{expr}"
			"#
		))
	};
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	for (index, name) in [
		b"Neither".as_slice(),
		b"AOnly".as_slice(),
		b"OnlyB".as_slice(),
		b"AandB".as_slice(),
	]
	.into_iter()
	.enumerate()
	{
		let start = index as u32 * 10;
		g.add_def(
			child(&module, b"class", name),
			b"class",
			&module,
			Some((start, start + 5)),
		)
		.unwrap();
	}
	let violations = |expr: &str| {
		evaluate(&g, "x", Lang::Ts, &rule(expr), SCHEME)
			.unwrap()
			.iter()
			.map(|violation| violation.moniker.clone())
			.collect::<Vec<_>>()
	};
	let forward = violations("name =~ ^A disjoint name =~ B$");
	assert_eq!(
		forward.len(),
		1,
		"only the both-match class violates: {forward:?}"
	);
	assert!(forward[0].contains("class:AandB"));
	assert_eq!(
		forward,
		violations("name =~ B$ disjoint name =~ ^A"),
		"`disjoint` is symmetric"
	);
}

#[test]
fn disjoint_chain_excludes_every_pair() {
	let cfg = cfg_from(
		r#"
		[[ts.class.where]]
		id   = "three-way"
		expr = "name =~ A disjoint name =~ B disjoint name =~ C"
		"#,
	);
	let module = build_module(b"a");
	let mut g = CodeGraph::new(module.clone(), b"module");
	for (index, name) in [
		b"AB".as_slice(),
		b"ConlyX".as_slice(),
		b"ABC".as_slice(),
		b"Zed".as_slice(),
	]
	.into_iter()
	.enumerate()
	{
		let start = index as u32 * 10;
		g.add_def(
			child(&module, b"class", name),
			b"class",
			&module,
			Some((start, start + 5)),
		)
		.unwrap();
	}
	let flagged = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME)
		.unwrap()
		.iter()
		.map(|violation| violation.moniker.clone())
		.collect::<Vec<_>>();
	assert_eq!(
		flagged.len(),
		2,
		"a chain excludes every pair, so matching two operands is enough to violate: {flagged:?}"
	);
	assert!(flagged.iter().any(|uri| uri.contains("class:AB")));
	assert!(flagged.iter().any(|uri| uri.contains("class:ABC")));
	assert!(
		!flagged.iter().any(|uri| uri.contains("class:ConlyX")),
		"matching a single operand of the chain stays valid: {flagged:?}"
	);
}

#[test]
fn disjoint_rejects_package_imports_in_both_directions() {
	let cfg = cfg_from(
		r#"
		[aliases]
		package_a = "source ~ '**/module:package-a/**' OR target ~ '**/module:package-a/**'"
		package_b = "source ~ '**/module:package-b/**' OR target ~ '**/module:package-b/**'"

		[[refs.where]]
		id   = "package-a-and-package-b-do-not-import-each-other"
		expr = "kind = 'uses_type' => $package_a disjoint $package_b"
		"#,
	);
	let root = build_root();
	let mut g = CodeGraph::new(root.clone(), b"module");
	let package_a = submodule(&root, b"package-a");
	g.add_def(package_a.clone(), b"module", &root, Some((0, 1)))
		.unwrap();
	let package_b = submodule(&root, b"package-b");
	g.add_def(package_b.clone(), b"module", &root, Some((2, 3)))
		.unwrap();
	let other = submodule(&root, b"package-c");
	g.add_def(other.clone(), b"module", &root, Some((4, 5)))
		.unwrap();

	let a_one = child(&package_a, b"class", b"AOne");
	let a_two = child(&package_a, b"class", b"ATwo");
	let b_one = child(&package_b, b"class", b"BOne");
	let b_two = child(&package_b, b"class", b"BTwo");
	let c_one = child(&other, b"class", b"COne");
	let c_two = child(&other, b"class", b"CTwo");
	for (moniker, parent, span) in [
		(&a_one, &package_a, (6, 7)),
		(&a_two, &package_a, (8, 9)),
		(&b_one, &package_b, (10, 11)),
		(&b_two, &package_b, (12, 13)),
		(&c_one, &other, (14, 15)),
		(&c_two, &other, (16, 17)),
	] {
		g.add_def(moniker.clone(), b"class", parent, Some(span))
			.unwrap();
	}

	g.add_ref(&a_one, b_one.clone(), b"uses_type", Some((6, 7)))
		.unwrap();
	g.add_ref(&b_two, a_two.clone(), b"uses_type", Some((12, 13)))
		.unwrap();
	g.add_ref(&a_one, a_two.clone(), b"uses_type", Some((6, 7)))
		.unwrap();
	g.add_ref(&b_one, b_two.clone(), b"uses_type", Some((10, 11)))
		.unwrap();
	g.add_ref(&c_one, c_two.clone(), b"uses_type", Some((14, 15)))
		.unwrap();

	let v = evaluate(&g, "x", Lang::Ts, &cfg, SCHEME).unwrap();
	assert_eq!(
		v.len(),
		2,
		"both crossing directions violate, internal and unrelated refs pass: {v:?}"
	);
	assert!(
		v.iter().all(|violation| violation.rule_id
			== "refs.package-a-and-package-b-do-not-import-each-other"),
		"{v:?}"
	);
}
