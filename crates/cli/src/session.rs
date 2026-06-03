#[cfg(feature = "tui")]
use std::collections::BTreeMap;
use std::path::PathBuf;

#[cfg(feature = "tui")]
use code_moniker_workspace::notes::notes_watch_targets_for_paths;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SessionOptions {
	pub paths: Vec<PathBuf>,
	pub project: Option<String>,
	pub cache_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg(feature = "tui")]
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
	pub linkage_ms: u64,
	pub changes_ms: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg(feature = "tui")]
pub(crate) struct LangTotals {
	pub files: usize,
	pub defs: usize,
	pub refs: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg(feature = "tui")]
pub struct CheckSummary {
	pub files_scanned: usize,
	pub files_with_violations: usize,
	pub total_violations: usize,
	pub errors: Vec<CheckError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg(feature = "tui")]
pub(crate) struct CheckError {
	pub path: PathBuf,
	pub error: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg(feature = "tui")]
pub(crate) struct StoreWatchRoot {
	pub path: PathBuf,
	pub git_root: Option<PathBuf>,
	pub ignored_paths: Vec<PathBuf>,
	pub notes_path: Option<PathBuf>,
}

#[cfg(feature = "tui")]
pub(crate) fn watch_roots_for_options(opts: &SessionOptions) -> Vec<StoreWatchRoot> {
	let ignored_paths = opts
		.cache_dir
		.as_ref()
		.map(|path| vec![absolute_path(path)])
		.unwrap_or_default();
	let notes_watch_targets =
		notes_watch_targets_for_paths(&opts.paths).unwrap_or_else(|_| Vec::new());
	let workspace_notes_path = notes_watch_targets
		.first()
		.map(|target| target.notes_path.clone());
	let mut roots = opts
		.paths
		.iter()
		.map(|path| StoreWatchRoot {
			path: watch_path(path),
			git_root: None,
			ignored_paths: ignored_paths.clone(),
			notes_path: workspace_notes_path.clone(),
		})
		.collect::<Vec<_>>();
	for target in notes_watch_targets {
		if !roots
			.iter()
			.any(|root| target.notes_path.starts_with(&root.path))
		{
			roots.push(StoreWatchRoot {
				path: target.path,
				git_root: None,
				ignored_paths: ignored_paths.clone(),
				notes_path: Some(target.notes_path),
			});
		}
	}
	roots
}

#[cfg(feature = "tui")]
pub(crate) fn root_label_for_options(opts: &SessionOptions) -> String {
	match opts.paths.as_slice() {
		[] => "<empty>".to_string(),
		[path] => path.display().to_string(),
		paths => sibling_parent(paths)
			.map(|parent| parent.display().to_string())
			.unwrap_or_else(|| {
				paths
					.iter()
					.map(|path| path.display().to_string())
					.collect::<Vec<_>>()
					.join(", ")
			}),
	}
}

#[cfg(feature = "tui")]
fn sibling_parent(paths: &[PathBuf]) -> Option<&std::path::Path> {
	let mut parents = paths.iter().map(|path| path.parent());
	let first = parents.next()??;
	parents.all(|parent| parent == Some(first)).then_some(first)
}

#[cfg(feature = "tui")]
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

#[cfg(feature = "tui")]
fn absolute_path(path: &std::path::Path) -> PathBuf {
	if path.is_absolute() {
		path.to_path_buf()
	} else {
		std::env::current_dir()
			.map(|cwd| cwd.join(path))
			.unwrap_or_else(|_| path.to_path_buf())
	}
}
