//! Deterministic synthetic workspace generator for benchmarks.
//!
//! Layout: `src/lib.rs` + `src/m{i}.rs`, each module with one struct (method
//! calls exercise the semantic indexer), a chain of intra-module calls, and a
//! cross-module call into the next module, so linkage resolution and
//! invalidation both have real work per file.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

pub struct SyntheticWorkspace {
	pub dir: tempfile::TempDir,
	pub modules: usize,
	pub fns_per_module: usize,
}

impl SyntheticWorkspace {
	pub fn root(&self) -> &Path {
		self.dir.path()
	}

	pub fn module_path(&self, module: usize) -> PathBuf {
		self.dir.path().join(format!("src/m{module}.rs"))
	}

	pub fn rewrite_module(&self, module: usize, salt: usize) {
		fs::write(
			self.module_path(module),
			module_source(module, self.modules, self.fns_per_module, salt),
		)
		.expect("rewrite module");
	}
}

pub fn generate(modules: usize, fns_per_module: usize) -> SyntheticWorkspace {
	let dir = tempfile::tempdir().expect("tempdir");
	let src = dir.path().join("src");
	fs::create_dir_all(&src).expect("src dir");
	let lib = (0..modules)
		.map(|module| format!("pub mod m{module};\n"))
		.collect::<String>();
	fs::write(src.join("lib.rs"), lib).expect("lib.rs");
	for module in 0..modules {
		fs::write(
			src.join(format!("m{module}.rs")),
			module_source(module, modules, fns_per_module, 0),
		)
		.expect("module");
	}
	SyntheticWorkspace {
		dir,
		modules,
		fns_per_module,
	}
}

fn module_source(module: usize, modules: usize, fns_per_module: usize, salt: usize) -> String {
	let next = (module + 1) % modules;
	let mut out = String::new();
	out.push_str(&format!("use crate::m{next}::f{next}_0;\n\n"));
	out.push_str(&format!(
		"pub struct Widget{module} {{\n\tpub value: u32,\n}}\n\n"
	));
	out.push_str(&format!(
		"impl Widget{module} {{\n\tpub fn compute(&self) -> u32 {{\n\t\tself.value + {salt}\n\t}}\n}}\n\n"
	));
	out.push_str(&format!("pub fn f{module}_0() -> u32 {{\n\t{salt}\n}}\n\n"));
	for idx in 1..fns_per_module {
		let prev = idx - 1;
		out.push_str(&format!(
			"pub fn f{module}_{idx}() -> u32 {{\n\tlet widget = Widget{module} {{ value: {idx} }};\n\tf{module}_{prev}() + f{next}_0() + widget.compute()\n}}\n\n"
		));
	}
	out
}
