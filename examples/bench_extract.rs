use std::env;
use std::fs;
use std::time::Instant;

use code_moniker::core::moniker::MonikerBuilder;
use code_moniker::lang::ts;

fn main() {
	let iters: usize = env::args()
		.nth(1)
		.and_then(|s| s.parse().ok())
		.unwrap_or(50);
	let path = env::args()
		.nth(2)
		.unwrap_or_else(|| "dogfood/ts/zod/src/types.ts".to_string());

	let source = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
	let anchor = MonikerBuilder::new().project(b"zod").build();
	let presets = ts::Presets::default();

	let _ = ts::extract("types.ts", &source, &anchor, false, &presets);

	let t0 = Instant::now();
	let mut total_defs = 0usize;
	let mut total_refs = 0usize;
	for _ in 0..iters {
		let g = ts::extract("types.ts", &source, &anchor, false, &presets);
		total_defs += g.def_count();
		total_refs += g.ref_count();
	}
	let elapsed = t0.elapsed();
	let per_run = elapsed / iters as u32;

	let defs_per_run = total_defs / iters;
	let refs_per_run = total_refs / iters;
	let kb = source.len() / 1024;

	println!("file:        {path} ({kb} KiB)");
	println!("iterations:  {iters}");
	println!("defs/run:    {defs_per_run}");
	println!("refs/run:    {refs_per_run}");
	println!("total:       {elapsed:?}");
	println!("per-run:     {per_run:?}");
	println!(
		"throughput:  {:.0} files/sec, {:.0} refs/sec",
		iters as f64 / elapsed.as_secs_f64(),
		(refs_per_run * iters) as f64 / elapsed.as_secs_f64()
	);
}
