use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use code_moniker_core::core::moniker::MonikerBuilder;
use code_moniker_core::lang::rs;

struct SourceFile {
	rel: String,
	source: String,
}

fn main() {
	let iters: usize = env::args()
		.nth(1)
		.and_then(|value| value.parse().ok())
		.unwrap_or(200);
	let root = env::args().nth(2).unwrap_or_else(|| {
		"crates/workspace/tests/fixtures/projects/rust/multiproject".to_string()
	});
	let mode = env::args().nth(3).unwrap_or_else(|| "both".to_string());
	let root = PathBuf::from(root)
		.canonicalize()
		.expect("fixture root should exist");
	let files = read_sources(&root);
	assert!(!files.is_empty(), "fixture should contain Rust files");

	let anchor = MonikerBuilder::new().project(b".").build();
	let presets = rs::Presets::default();

	for file in &files {
		let _ = rs::extract(&file.rel, &file.source, &anchor, true, &presets);
		let _ = rs::extract_sdk(&file.rel, &file.source, &anchor, true, &presets);
	}

	println!("root:              {}", root.display());
	println!("files:             {}", files.len());
	println!("iterations:        {iters}");
	println!(
		"source:            {} KiB",
		files.iter().map(|file| file.source.len()).sum::<usize>() / 1024
	);

	match mode.as_str() {
		"legacy" => {
			let legacy = time_extract(iters, &files, |file| {
				rs::extract(&file.rel, &file.source, &anchor, true, &presets)
			});
			print_result("legacy", &legacy, iters, files.len());
		}
		"sdk" => {
			let sdk = time_extract(iters, &files, |file| {
				rs::extract_sdk(&file.rel, &file.source, &anchor, true, &presets)
			});
			print_result("sdk", &sdk, iters, files.len());
		}
		"both" => {
			let legacy = time_extract(iters, &files, |file| {
				rs::extract(&file.rel, &file.source, &anchor, true, &presets)
			});
			let sdk = time_extract(iters, &files, |file| {
				rs::extract_sdk(&file.rel, &file.source, &anchor, true, &presets)
			});
			print_result("legacy", &legacy, iters, files.len());
			print_result("sdk", &sdk, iters, files.len());
			println!(
				"ratio sdk/legacy:  {:.2}x",
				sdk.elapsed.as_secs_f64() / legacy.elapsed.as_secs_f64()
			);
		}
		other => panic!("unknown mode {other}; expected legacy, sdk, or both"),
	}
}

fn read_sources(root: &Path) -> Vec<SourceFile> {
	let mut paths = Vec::new();
	collect_rust_files(root, &mut paths);
	paths.sort();
	paths
		.into_iter()
		.map(|path| {
			let rel = path
				.strip_prefix(root)
				.expect("fixture file under root")
				.to_string_lossy()
				.replace('\\', "/");
			let source = fs::read_to_string(&path).unwrap_or_else(|err| {
				panic!("read {}: {err}", path.display());
			});
			SourceFile { rel, source }
		})
		.collect()
}

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
	for entry in fs::read_dir(dir).expect("fixture dir") {
		let path = entry.expect("fixture entry").path();
		if path.is_dir() {
			collect_rust_files(&path, out);
		} else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
			out.push(path);
		}
	}
}

struct BenchResult {
	elapsed: Duration,
	defs: usize,
	refs: usize,
}

fn time_extract<F>(iters: usize, files: &[SourceFile], mut extract: F) -> BenchResult
where
	F: FnMut(&SourceFile) -> code_moniker_core::core::code_graph::CodeGraph,
{
	let started = Instant::now();
	let mut defs = 0usize;
	let mut refs = 0usize;
	for _ in 0..iters {
		for file in files {
			let graph = extract(file);
			defs += graph.def_count();
			refs += graph.ref_count();
		}
	}
	BenchResult {
		elapsed: started.elapsed(),
		defs,
		refs,
	}
}

fn print_result(label: &str, result: &BenchResult, iters: usize, file_count: usize) {
	let runs = iters * file_count;
	println!("{label} total:      {:?}", result.elapsed);
	println!(
		"{label} per file:   {:?}",
		result.elapsed / runs.try_into().expect("runs should fit in u32")
	);
	println!(
		"{label} files/sec:  {:.0}",
		runs as f64 / result.elapsed.as_secs_f64()
	);
	println!("{label} defs/run:   {}", result.defs / iters);
	println!("{label} refs/run:   {}", result.refs / iters);
}
