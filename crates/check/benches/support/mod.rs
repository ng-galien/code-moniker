//! Deterministic workspace used by the workspace-rule benchmarks.

use std::fs;
use std::path::{Path, PathBuf};

pub struct SyntheticWorkspace {
	dir: tempfile::TempDir,
	modules: usize,
	symbols_per_module: usize,
}

impl SyntheticWorkspace {
	pub fn root(&self) -> &Path {
		self.dir.path()
	}

	pub fn rewrite_module(&self, module: usize, salt: usize) {
		fs::write(
			module_path(self.root(), module),
			module_source(module, self.symbols_per_module, salt),
		)
		.expect("rewrite synthetic module");
	}

	pub fn changed_module(&self) -> usize {
		self.modules / 2
	}
}

pub fn generate(modules: usize, symbols_per_module: usize) -> SyntheticWorkspace {
	let dir = tempfile::tempdir().expect("synthetic workspace");
	for module in 0..modules {
		let path = module_path(dir.path(), module);
		fs::create_dir_all(path.parent().expect("module parent")).expect("module directory");
		fs::write(&path, module_source(module, symbols_per_module, 0)).expect("synthetic module");
	}
	SyntheticWorkspace {
		dir,
		modules,
		symbols_per_module,
	}
}

fn module_path(root: &Path, module: usize) -> PathBuf {
	let layer = if module % 2 == 0 { "infra" } else { "domain" };
	root.join(format!("src/{layer}/m{module}.rs"))
}

fn module_source(module: usize, symbols_per_module: usize, salt: usize) -> String {
	let mut source = format!("pub const REFRESH_SALT: usize = {salt};\n\n");
	for symbol in 0..symbols_per_module {
		let suffix = if symbol % 4 == 0 {
			"Repository"
		} else {
			"Service"
		};
		let name = if symbol % 10 == 0 {
			format!("Shared{symbol}{suffix}")
		} else {
			format!("Entity{module}_{symbol}{suffix}")
		};
		source.push_str(&format!("pub struct {name};\n"));
	}
	source
}
