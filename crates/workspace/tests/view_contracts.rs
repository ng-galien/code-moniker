//! Golden behavior tests for the snapshot read model.
//!
//! These freeze the observable output of `SymbolView`, `ReferenceView` and
//! `SearchView` on a small fixture, so index-side refactors (record shards,
//! nav/search indexes) must preserve view behavior bit-for-bit. The camelCase
//! search golden documents today's miss and is flipped intentionally when the
//! camelCase-aware search index lands.

use std::fs;
use std::path::Path;

use code_moniker_workspace::registry::{LocalWorkspaceOptions, LocalWorkspaceRegistry};
use code_moniker_workspace::snapshot::{
	SymbolId, WorkspaceRequest, WorkspaceSnapshot, WorkspaceView,
};

const LIB_RS: &str = "pub mod probe;\npub mod caller;\n";
const PROBE_RS: &str = "pub struct WidgetProbe {\n\tpub value: u32,\n}\n\nimpl WidgetProbe {\n\tpub fn compute(&self) -> u32 {\n\t\tself.value\n\t}\n\n\tpub fn reset(&mut self) {\n\t\tself.value = 0;\n\t}\n}\n\npub fn widget_probe() -> u32 {\n\t7\n}\n";
const CALLER_RS: &str = "use crate::probe::{WidgetProbe, widget_probe};\n\npub fn drive() -> u32 {\n\tlet mut probe = WidgetProbe { value: widget_probe() };\n\tprobe.reset();\n\tprobe.compute()\n}\n";

fn seed(dir: &Path) {
	let src = dir.join("src");
	fs::create_dir_all(&src).expect("src dir");
	fs::write(src.join("lib.rs"), LIB_RS).expect("lib");
	fs::write(src.join("probe.rs"), PROBE_RS).expect("probe");
	fs::write(src.join("caller.rs"), CALLER_RS).expect("caller");
}

fn indexed_snapshot(dir: &Path) -> WorkspaceSnapshot {
	let mut registry =
		LocalWorkspaceRegistry::local(LocalWorkspaceOptions::new(vec![dir.to_path_buf()], None));
	registry
		.commands()
		.refresh(WorkspaceRequest::new("view-contracts"));
	registry
		.queries()
		.snapshot()
		.expect("workspace snapshot")
		.clone()
}

fn symbol_id_by_name(snapshot: &WorkspaceSnapshot, name: &str) -> SymbolId {
	snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.name == name)
		.map(|symbol| symbol.id.clone())
		.unwrap_or_else(|| panic!("symbol named {name} in fixture"))
}

#[test]
fn symbol_view_all_is_ordered_with_child_counts() {
	let temp = tempfile::tempdir().expect("tempdir");
	seed(temp.path());
	let snapshot = indexed_snapshot(temp.path());
	let listed: Vec<(String, String, usize)> = WorkspaceView::new(&snapshot)
		.symbols()
		.all()
		.into_iter()
		.map(|summary| (summary.name, summary.kind, summary.child_count))
		.collect();
	assert_eq!(
		listed,
		vec![
			("drive()".to_string(), "fn".to_string(), 0),
			("widget_probe()".to_string(), "fn".to_string(), 0),
			("WidgetProbe".to_string(), "struct".to_string(), 2),
			("compute()".to_string(), "method".to_string(), 0),
			("reset()".to_string(), "method".to_string(), 0),
		],
		"SymbolView::all ordering or child counts drifted"
	);
}

#[test]
fn symbol_view_children_lists_struct_methods() {
	let temp = tempfile::tempdir().expect("tempdir");
	seed(temp.path());
	let snapshot = indexed_snapshot(temp.path());
	let parent = symbol_id_by_name(&snapshot, "WidgetProbe");
	let mut children: Vec<String> = WorkspaceView::new(&snapshot)
		.symbols()
		.children(&parent)
		.into_iter()
		.map(|summary| summary.name)
		.collect();
	children.sort();
	assert_eq!(children, vec!["compute()", "reset()"]);
}

#[test]
fn reference_view_reports_incoming_and_outgoing_for_hot_symbol() {
	let temp = tempfile::tempdir().expect("tempdir");
	seed(temp.path());
	let snapshot = indexed_snapshot(temp.path());
	let view = WorkspaceView::new(&snapshot);
	let target = symbol_id_by_name(&snapshot, "widget_probe()");
	let incoming = view.references().incoming_ids(&target);
	assert_eq!(
		incoming.len(),
		2,
		"widget_probe() expects the import edge and the call edge, got {incoming:?}"
	);
	let driver = symbol_id_by_name(&snapshot, "drive()");
	let outgoing = view.references().outgoing_ids(&driver);
	let mut outgoing_kinds: Vec<String> = outgoing
		.iter()
		.filter_map(|id| {
			snapshot
				.index
				.references
				.iter()
				.find(|reference| &reference.id == id)
				.map(|reference| reference.kind.clone())
		})
		.collect();
	outgoing_kinds.sort();
	assert_eq!(
		outgoing_kinds,
		vec![
			"calls",
			"instantiates",
			"method_call",
			"method_call",
			"reads",
			"reads"
		],
		"drive() outgoing reference kinds drifted"
	);
}

#[test]
fn multiword_search_finds_camelcase_and_snake_case_symbols() {
	let temp = tempfile::tempdir().expect("tempdir");
	seed(temp.path());
	let snapshot = indexed_snapshot(temp.path());
	let hits = WorkspaceView::new(&snapshot)
		.search()
		.search_symbols("widget probe", 10);
	let mut names: Vec<String> = hits
		.iter()
		.filter_map(|hit| {
			snapshot
				.index
				.symbols
				.iter()
				.find(|symbol| symbol.id == hit.symbol)
				.map(|symbol| symbol.name.clone())
		})
		.collect();
	names.sort();
	assert_eq!(
		names,
		vec!["WidgetProbe", "compute()", "reset()", "widget_probe()"],
		"multiword search results drifted"
	);
}

#[test]
fn camelcase_query_currently_misses_snake_case_symbols() {
	let temp = tempfile::tempdir().expect("tempdir");
	seed(temp.path());
	let snapshot = indexed_snapshot(temp.path());
	let hits = WorkspaceView::new(&snapshot)
		.search()
		.search_symbols("WidgetProbe", 10);
	let names: Vec<String> = hits
		.iter()
		.filter_map(|hit| {
			snapshot
				.index
				.symbols
				.iter()
				.find(|symbol| symbol.id == hit.symbol)
				.map(|symbol| symbol.name.clone())
		})
		.collect();
	assert!(
		names.contains(&"WidgetProbe".to_string()),
		"exact camelCase name should match, got {names:?}"
	);
	assert!(
		!names.contains(&"widget_probe()".to_string()),
		"documents today's gap: a concatenated camelCase query does not match \
		 snake_case symbols; flip this assertion when the camelCase-aware \
		 search index lands"
	);
}
