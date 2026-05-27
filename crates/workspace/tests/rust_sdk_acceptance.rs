use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::core::uri::{UriConfig, to_uri};
use code_moniker_core::lang::rs;

fn fixture_root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("tests/fixtures/rust/multiproject")
		.canonicalize()
		.expect("fixture root")
}

fn anchor() -> Moniker {
	MonikerBuilder::new().project(b".").build()
}

#[test]
fn rust_sdk_matches_legacy_on_multiproject_fixture() {
	let root = fixture_root();
	let mut files = rust_fixture_files(&root);
	assert!(!files.is_empty(), "fixture should contain Rust files");

	for path in files.drain(..) {
		let rel = path
			.strip_prefix(&root)
			.expect("fixture file under root")
			.to_string_lossy()
			.replace('\\', "/");
		let source = std::fs::read_to_string(&path).expect("fixture source");
		let anchor = anchor();
		let legacy = rs::extract(&rel, &source, &anchor, true, &rs::Presets::default());
		let sdk = rs::extract_sdk(&rel, &source, &anchor, true, &rs::Presets::default());
		assert_graph_eq(&rel, &legacy, &sdk);
	}
}

fn rust_fixture_files(root: &Path) -> Vec<PathBuf> {
	let mut files = Vec::new();
	collect_rust_files(root, &mut files);
	files.sort();
	files
}

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
	for entry in std::fs::read_dir(dir).expect("fixture dir") {
		let path = entry.expect("fixture entry").path();
		if path.is_dir() {
			collect_rust_files(&path, out);
		} else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
			out.push(path);
		}
	}
}

fn assert_graph_eq(rel: &str, legacy: &CodeGraph, sdk: &CodeGraph) {
	assert_rows_eq(rel, "defs", def_rows(legacy), def_rows(sdk));
	assert_rows_eq(rel, "refs", ref_rows(legacy), ref_rows(sdk));
}

fn assert_rows_eq(rel: &str, label: &str, legacy: Vec<String>, sdk: Vec<String>) {
	if legacy == sdk {
		return;
	}
	let legacy_set = legacy.into_iter().collect::<BTreeSet<_>>();
	let sdk_set = sdk.into_iter().collect::<BTreeSet<_>>();
	let missing = legacy_set
		.difference(&sdk_set)
		.take(20)
		.cloned()
		.collect::<Vec<_>>();
	let extra = sdk_set
		.difference(&legacy_set)
		.take(20)
		.cloned()
		.collect::<Vec<_>>();
	panic!(
		"SDK {label} diverged from legacy for {rel}\nmissing from SDK:\n{}\nextra in SDK:\n{}",
		missing.join("\n"),
		extra.join("\n")
	);
}

fn def_rows(graph: &CodeGraph) -> Vec<String> {
	let mut rows = graph
		.defs()
		.map(|def| {
			format!(
				"{} kind={} parent={:?} vis={} sig={} call={}/{}",
				uri(&def.moniker),
				String::from_utf8_lossy(&def.kind),
				def.parent
					.map(|parent| uri(&graph.def_at(parent).moniker))
					.unwrap_or_else(|| "-".to_string()),
				String::from_utf8_lossy(&def.visibility),
				String::from_utf8_lossy(&def.signature),
				String::from_utf8_lossy(&def.call_name),
				def.call_arity
					.map(|arity| arity.to_string())
					.unwrap_or_else(|| "-".to_string())
			)
		})
		.collect::<Vec<_>>();
	rows.sort();
	rows
}

fn ref_rows(graph: &CodeGraph) -> Vec<String> {
	let mut rows = graph
		.refs()
		.map(|reference| {
			format!(
				"source={} target={} kind={} conf={} recv={} alias={} call={}/{}",
				uri(&graph.def_at(reference.source).moniker),
				uri(&reference.target),
				String::from_utf8_lossy(&reference.kind),
				String::from_utf8_lossy(&reference.confidence),
				String::from_utf8_lossy(&reference.receiver_hint),
				String::from_utf8_lossy(&reference.alias),
				String::from_utf8_lossy(&reference.call_name),
				reference
					.call_arity
					.map(|arity| arity.to_string())
					.unwrap_or_else(|| "-".to_string())
			)
		})
		.collect::<Vec<_>>();
	rows.sort();
	rows
}

fn uri(moniker: &Moniker) -> String {
	to_uri(moniker, &UriConfig::default()).expect("moniker uri")
}
