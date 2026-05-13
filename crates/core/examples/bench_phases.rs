use std::time::Instant;

use code_moniker_core::core::moniker::MonikerBuilder;
use code_moniker_core::lang::ts;

const ITERS: usize = 50;

fn main() {
	let path = std::env::args().nth(1).unwrap_or_else(|| {
		"/Users/alexandreboyer/dev/projects/pg_code_moniker/dogfood/ts/zod/src/types.ts".to_string()
	});
	let src = std::fs::read_to_string(&path).expect("read");
	let anchor = MonikerBuilder::new().project(b".").build();
	let presets = ts::Presets::default();

	let mut parse_ns = 0u128;
	let mut extract_ns = 0u128;
	let mut total_ns = 0u128;

	for _ in 0..ITERS {
		let t_total = Instant::now();

		let t = Instant::now();
		let _tree = ts::parse(&src);
		parse_ns += t.elapsed().as_nanos();

		let t = Instant::now();
		let _g = ts::extract(&path, &src, &anchor, true, &presets);
		extract_ns += t.elapsed().as_nanos();

		total_ns += t_total.elapsed().as_nanos();
	}

	let total_ms = total_ns as f64 / 1_000_000.0;
	let parse_ms = parse_ns as f64 / 1_000_000.0;
	let extract_ms = extract_ns as f64 / 1_000_000.0;
	println!("file:        {} ({} bytes)", path, src.len());
	println!("iterations:  {}", ITERS);
	println!("--- per-run averages ---");
	println!("parse-only:  {:.2} ms", parse_ms / ITERS as f64);
	println!("full extract:{:.2} ms", extract_ms / ITERS as f64);
	println!(
		"walk+emit:   {:.2} ms (extract - parse)",
		(extract_ms - parse_ms) / ITERS as f64
	);
	println!("--- totals (both phases run each iter) ---");
	println!("total wall:  {:.2} ms", total_ms / ITERS as f64);
	println!();
	let parse_share = (parse_ms / extract_ms) * 100.0;
	let walk_share = ((extract_ms - parse_ms) / extract_ms) * 100.0;
	println!("Parse share of extract: {:.1}%", parse_share);
	println!("Walk+emit share:        {:.1}%", walk_share);
}
