//! Read-model query benchmarks: usages of a hot symbol, symbol search
//! (multi-word and camelCase), and full symbol listing (children counts).

mod support;

use code_moniker_workspace::registry::{LocalWorkspaceOptions, LocalWorkspaceRegistry};
use code_moniker_workspace::snapshot::{SymbolId, WorkspaceRequest, WorkspaceSnapshot};
use criterion::{Criterion, criterion_group, criterion_main};
use std::sync::Arc;

const MODULES: usize = 120;
const FNS_PER_MODULE: usize = 10;

fn indexed_snapshot() -> Arc<WorkspaceSnapshot> {
	let workspace = support::generate(MODULES, FNS_PER_MODULE);
	let mut registry = LocalWorkspaceRegistry::local(LocalWorkspaceOptions::new(
		vec![workspace.root().to_path_buf()],
		None,
	));
	registry
		.commands()
		.refresh(WorkspaceRequest::new("bench-queries"));
	registry
		.queries()
		.snapshot_arc()
		.expect("snapshot for query benches")
}

fn hot_symbol(snapshot: &WorkspaceSnapshot) -> SymbolId {
	snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.name == "f5_0()")
		.map(|symbol| symbol.id.clone())
		.expect("hot symbol f5_0 present")
}

fn usages_of_hot_symbol(c: &mut Criterion) {
	let snapshot = indexed_snapshot();
	let symbol = hot_symbol(&snapshot);
	let view = code_moniker_workspace::snapshot::WorkspaceView::new(&snapshot);
	let mut group = c.benchmark_group("queries");
	group.bench_function("usages_of_hot_symbol", |b| {
		b.iter(|| std::hint::black_box(view.references().for_symbol(&symbol)));
	});
	group.finish();
}

fn search_symbols(c: &mut Criterion) {
	let snapshot = indexed_snapshot();
	let view = code_moniker_workspace::snapshot::WorkspaceView::new(&snapshot);
	let mut group = c.benchmark_group("queries");
	group.bench_function("search_multiword", |b| {
		b.iter(|| std::hint::black_box(view.search().search_symbols("widget compute", 20)));
	});
	group.bench_function("search_camelcase", |b| {
		b.iter(|| std::hint::black_box(view.search().search_symbols("Widget5", 20)));
	});
	group.finish();
}

fn symbol_children(c: &mut Criterion) {
	let snapshot = indexed_snapshot();
	let view = code_moniker_workspace::snapshot::WorkspaceView::new(&snapshot);
	let mut group = c.benchmark_group("queries");
	group.sample_size(10);
	group.bench_function("symbol_children_all", |b| {
		b.iter(|| std::hint::black_box(view.symbols().all()));
	});
	group.finish();
}

criterion_group!(
	benches,
	usages_of_hot_symbol,
	search_symbols,
	symbol_children
);
criterion_main!(benches);
