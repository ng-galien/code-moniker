use std::path::{Path, PathBuf};

use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::rs;

fn fixture_root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("tests/fixtures/projects/rust/multiproject")
		.canonicalize()
		.expect("fixture root")
}

fn anchor() -> Moniker {
	MonikerBuilder::new().project(b".").build()
}

#[test]
fn rust_sdk_extracts_multiproject_fixture() {
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
		let graph = rs::extract(&rel, &source, &anchor, true, &rs::Presets::default());
		code_moniker_core::lang::assert_conformance::<rs::Lang>(&graph, &anchor);
		assert!(
			graph.def_count() > 0,
			"SDK extraction should emit defs for {rel}"
		);
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
