use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_core::core::moniker::MonikerBuilder;
use code_moniker_core::core::uri::to_uri;
use code_moniker_core::lang::{LangExtractor, assert_conformance, json, markdown, yaml};

fn extract<E: LangExtractor<Presets = ()>>(path: &str, source: &str) -> CodeGraph {
	let anchor = MonikerBuilder::new().project(b"test").build();
	let graph = E::extract(path, source, &anchor, false, &());
	assert_conformance::<E>(&graph, &anchor);
	for def in graph.defs().skip(1) {
		let (start, end) = def.position.expect("source position");
		assert!(source.get(start as usize..end as usize).is_some());
	}
	graph
}

fn uris(graph: &CodeGraph) -> Vec<String> {
	graph
		.defs()
		.map(|d| to_uri(&d.moniker, &Default::default()))
		.collect()
}

#[test]
fn json_keys_arrays_escapes_and_duplicate_identity() {
	let source = r#"{"a":{"b":[{"c":true},null]},"a":2,"a~2":3,"\u0061":4,"é/:":5,"":6}"#;
	let graph = extract::<json::Lang>("data/config.json", source);
	let names = uris(&graph);
	for suffix in [
		"/module:config.json/key:a/key:b/item:0/key:c",
		"/key:a~2",
		"/key:a~02",
		"/key:a~3",
		"/key:``",
	] {
		assert!(
			names.iter().any(|n| n.ends_with(suffix)),
			"missing {suffix}: {names:?}"
		);
	}
	assert_eq!(graph.defs().filter(|d| d.kind == b"key").count(), 8);
	let c = graph
		.defs()
		.find(|d| to_uri(&d.moniker, &Default::default()).ends_with("/key:c"))
		.unwrap();
	let (start, end) = c.position.unwrap();
	assert_eq!(&source[start as usize..end as usize], "\"c\":true");
	assert_eq!(
		uris(&extract::<json::Lang>("data/config.json", source)),
		names
	);
}

#[test]
fn markdown_sections_follow_heading_levels_and_ignore_code_contents() {
	let source = "# Guide\nIntro\n\n## Setup\n```yaml\n# not a heading\nx: 1\n```\n\n[site]: https://example.org\n\n## Setup\nAgain\n\nOther\n=====\n\n### Deep\n";
	let graph = extract::<markdown::Lang>("README.md", source);
	let names = uris(&graph);
	for suffix in [
		"/section:Guide/section:Setup",
		"/section:Guide/section:Setup/code_block:yaml",
		"/section:Guide/section:Setup~2",
		"/section:Other/section:Deep",
	] {
		assert!(
			names.iter().any(|n| n.ends_with(suffix)),
			"missing {suffix}: {names:?}"
		);
	}
	assert_eq!(graph.defs().filter(|d| d.kind == b"section").count(), 5);
	assert_eq!(
		graph
			.defs()
			.filter(|d| d.kind == b"link_definition")
			.count(),
		1
	);
	assert!(!names.iter().any(|n| n.contains("not a heading")));
	let setup = graph
		.defs()
		.find(|d| to_uri(&d.moniker, &Default::default()).ends_with("/section:Setup"))
		.unwrap();
	let (start, end) = setup.position.unwrap();
	let section = &source[start as usize..end as usize];
	assert!(section.starts_with("## Setup\n"));
	assert!(section.contains("[site]:"));
	assert!(!section.contains("Again"));
}

#[test]
fn yaml_documents_flow_and_block_sequences_preserve_source_keys() {
	let source = "---\ndefaults: &defaults\n  name: Café\nservices:\n  - {port: 80, names: [a, b]}\n  - *defaults\nmessage: |\n  hi\n  there\n'x:y': true\n? [a, b]\n: complex\n---\ndefaults: 2\ndefaults: 3\n";
	let graph = extract::<yaml::Lang>("config.yaml", source);
	let names = uris(&graph);
	for suffix in [
		"/document:0/key:defaults/key:name",
		"/document:0/key:services/item:0/key:port",
		"/document:0/key:services/item:0/key:names/item:1",
		"/document:0/key:services/item:1",
		"/document:1/key:defaults~2",
	] {
		assert!(
			names.iter().any(|n| n.ends_with(suffix)),
			"missing {suffix}: {names:?}"
		);
	}
	assert_eq!(graph.defs().filter(|d| d.kind == b"document").count(), 2);
	assert!(
		!names.iter().any(|n| n.contains("item:1/key:name")),
		"aliases must not expand"
	);
	let message = graph
		.defs()
		.find(|d| to_uri(&d.moniker, &Default::default()).ends_with("/key:message"))
		.unwrap();
	let (start, end) = message.position.unwrap();
	assert!(source[start as usize..end as usize].contains("  there"));
}

#[test]
fn documents_handle_empty_scalar_malformed_and_unicode_inputs() {
	for source in [
		"",
		"null",
		"[]",
		"{}",
		"[1,{\"x\":2}]",
		"{\"x\":",
		"{\"é\":true}",
	] {
		extract::<json::Lang>("x.json", source);
	}
	for source in [
		"",
		"---",
		"42",
		"-\n- x",
		"---\n...\n---\n...",
		"x: [",
		"é: oui",
		"[a, {b: c}]",
	] {
		extract::<yaml::Lang>("x.yml", source);
	}
	for source in [
		"",
		"#",
		"# é\n# é~2\n# é\n",
		"```\n# unfinished\n",
		"    # indented code\n",
	] {
		extract::<markdown::Lang>("x.md", source);
	}
	assert!(
		<json::Lang as LangExtractor>::parse("x.json", "{\"x\":")
			.primary()
			.root_node()
			.has_error()
	);
	assert!(
		<yaml::Lang as LangExtractor>::parse("x.yaml", "x: [")
			.primary()
			.root_node()
			.has_error()
	);
}

#[test]
fn format_alias_files_have_distinct_roots() {
	let first = extract::<yaml::Lang>("dir/config.yaml", "x: 1");
	let second = extract::<yaml::Lang>("dir/config.yml", "x: 1");
	assert_ne!(first.root(), second.root());
}

#[test]
fn yaml_flow_mapping_preserves_keys_with_implicit_null_values() {
	let source = "{foo, bar, baz: 1}\n";
	let document = yaml::Lang::parse("flow.yml", source);
	assert!(!document.primary().root_node().has_error());
	let graph = extract::<yaml::Lang>("flow.yml", source);
	let root = to_uri(graph.root(), &Default::default());
	let actual: Vec<_> = graph
		.defs()
		.filter(|def| def.kind == b"key")
		.map(|def| (to_uri(&def.moniker, &Default::default()), def.position))
		.collect();
	assert_eq!(
		actual,
		vec![
			(format!("{root}/document:0/key:foo"), Some((1, 4))),
			(format!("{root}/document:0/key:bar"), Some((6, 9))),
			(format!("{root}/document:0/key:baz"), Some((11, 17))),
		],
		"implicit null members must retain their own monikers and source positions"
	);
}

proptest::proptest! {
	#![proptest_config(proptest::test_runner::Config::with_cases(64))]
	#[test]
	fn arbitrary_document_text_preserves_graph_conformance(source in ".{0,256}") {
		extract::<json::Lang>("random.json", &source);
		extract::<yaml::Lang>("random.yaml", &source);
		extract::<markdown::Lang>("random.md", &source);
	}
}
