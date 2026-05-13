use std::time::Instant;

use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};

fn def_at(i: usize) -> Moniker {
	MonikerBuilder::new()
		.project(b"bench")
		.segment(b"path", b"root")
		.segment(b"path", format!("d{i:06}").as_bytes())
		.build()
}

fn ref_target(i: usize) -> Moniker {
	MonikerBuilder::new()
		.project(b"bench")
		.segment(b"path", b"ext")
		.segment(b"path", format!("t{i:06}").as_bytes())
		.build()
}

fn run(n_defs: usize, n_refs: usize) -> (usize, usize, std::time::Duration) {
	let root = MonikerBuilder::new()
		.project(b"bench")
		.segment(b"path", b"root")
		.build();
	let t0 = Instant::now();
	let mut g = CodeGraph::new(root.clone(), b"module");
	for i in 0..n_defs {
		let m = def_at(i);
		g.add_def(m, b"class", &root, None).unwrap();
	}
	for i in 0..n_refs {
		let src = def_at(i % n_defs.max(1));
		let target = ref_target(i);
		g.add_ref(&src, target, b"calls", None).unwrap();
	}
	(g.def_count(), g.ref_count(), t0.elapsed())
}

fn main() {
	let scales: &[(usize, usize)] = &[
		(100, 1_000),
		(500, 5_000),
		(2_000, 20_000),
		(5_000, 50_000),
		(10_000, 100_000),
	];

	println!(
		"{:>8}  {:>8}  {:>10}  {:>14}",
		"defs", "refs", "elapsed", "ns/op"
	);
	for &(d, r) in scales {
		let (dc, rc, dt) = run(d, r);
		let ops = dc + rc;
		let ns_per_op = dt.as_nanos() as f64 / ops as f64;
		println!(
			"{:>8}  {:>8}  {:>10.1?}  {:>11.0} ns",
			dc, rc, dt, ns_per_op
		);
	}
}
