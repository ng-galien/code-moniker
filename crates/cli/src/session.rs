use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct SessionOptions {
	pub paths: Vec<PathBuf>,
	pub project: Option<String>,
	pub cache_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionStats {
	pub files: usize,
	pub defs: usize,
	pub refs: usize,
	pub by_lang: BTreeMap<&'static str, LangTotals>,
	pub by_shape: BTreeMap<&'static str, usize>,
	pub by_def_kind: BTreeMap<String, usize>,
	pub by_ref_kind: BTreeMap<String, usize>,
	pub scan_ms: u64,
	pub extract_ms: u64,
	pub index_ms: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LangTotals {
	pub files: usize,
	pub defs: usize,
	pub refs: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CheckSummary {
	pub files_scanned: usize,
	pub files_with_violations: usize,
	pub total_violations: usize,
	pub errors: Vec<CheckError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckError {
	pub path: PathBuf,
	pub error: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreWatchRoot {
	pub path: PathBuf,
	pub git_root: Option<PathBuf>,
	pub ignored_paths: Vec<PathBuf>,
}

pub fn watch_roots_for_options(opts: &SessionOptions) -> Vec<StoreWatchRoot> {
	let ignored_paths = opts
		.cache_dir
		.as_ref()
		.map(|path| vec![absolute_path(path)])
		.unwrap_or_default();
	opts.paths
		.iter()
		.map(|path| StoreWatchRoot {
			path: watch_path(path),
			git_root: None,
			ignored_paths: ignored_paths.clone(),
		})
		.collect()
}

fn watch_path(path: &std::path::Path) -> PathBuf {
	let path = absolute_path(path);
	if path.is_file() {
		path.parent()
			.map(std::path::Path::to_path_buf)
			.unwrap_or(path)
	} else {
		path
	}
}

fn absolute_path(path: &std::path::Path) -> PathBuf {
	if path.is_absolute() {
		path.to_path_buf()
	} else {
		std::env::current_dir()
			.map(|cwd| cwd.join(path))
			.unwrap_or_else(|_| path.to_path_buf())
	}
}
