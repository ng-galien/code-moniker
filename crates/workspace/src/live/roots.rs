// code-moniker: ignore-file[smell-clone-reflex, smell-feature-envy-local]
// Watch root planning clones normalized paths into durable live-refresh state.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use notify::event::{AccessKind, AccessMode, CreateKind, ModifyKind, RemoveKind};
use notify::{Event, EventKind, RecursiveMode};

use super::model::{WorkspaceLiveEvent, WorkspaceWatchRoot, push_unique};
use crate::git_runtime::git_fast_text;
use crate::notes::{notes_watch_path, notes_watch_targets_for_paths};
use crate::path_util::{absolute_path, normalize_path};
use crate::walk::{walk_non_ignored_directories, workspace_ignore_matcher};
use code_moniker_core::lang::build_manifest::Manifest;

#[derive(Clone, Debug)]
pub(crate) struct WorkspaceEventClassifier {
	paths: WorkspacePathClassifier,
}

impl WorkspaceEventClassifier {
	#[cfg(test)]
	pub(crate) fn new(roots: Vec<WorkspaceWatchRoot>) -> Self {
		Self {
			paths: WorkspacePathClassifier::new(roots),
		}
	}

	#[cfg(test)]
	pub(super) fn new_with_watch_targets(
		roots: Vec<WorkspaceWatchRoot>,
		targets: &[WorkspaceWatchTarget],
	) -> anyhow::Result<Self> {
		let git_dirs = resolve_git_metadata_dirs(&roots)?;
		Ok(Self::new_with_resolved_git_dirs(roots, targets, git_dirs))
	}

	pub(super) fn new_with_resolved_git_dirs(
		roots: Vec<WorkspaceWatchRoot>,
		targets: &[WorkspaceWatchTarget],
		git_dirs: BTreeSet<PathBuf>,
	) -> Self {
		Self {
			paths: WorkspacePathClassifier::new_with_watch_targets(roots, targets, git_dirs),
		}
	}

	pub(crate) fn classify_event(&self, event: &Event) -> Option<WorkspaceLiveEvent> {
		if event.need_rescan() {
			return Some(WorkspaceLiveEvent::RescanRequired);
		}
		self.classify_event_paths(event_path_policy(&event.kind), &event.paths)
	}

	fn classify_event_paths(
		&self,
		policy: EventPathPolicy,
		paths: &[PathBuf],
	) -> Option<WorkspaceLiveEvent> {
		match policy {
			EventPathPolicy::Ignore => None,
			EventPathPolicy::Classify { allow_git_signals } => {
				self.classify_paths_with_git_signals(paths, allow_git_signals)
			}
			EventPathPolicy::RescanSourceChange { assume_directory } => {
				if self
					.paths
					.requires_directory_rescan(paths, assume_directory)
				{
					return Some(WorkspaceLiveEvent::RescanRequired);
				}
				self.classify_paths_with_git_signals(paths, true)
			}
			EventPathPolicy::RescanMissingSource => {
				self.classify_paths_with_git_signals(paths, true)
			}
			EventPathPolicy::RescanRename => {
				if self.paths.requires_rename_rescan(paths) {
					return Some(WorkspaceLiveEvent::RescanRequired);
				}
				self.classify_paths_with_git_signals(paths, true)
			}
		}
	}

	pub(crate) fn classify_paths_with_git_signals(
		&self,
		paths: &[PathBuf],
		allow_git_signals: bool,
	) -> Option<WorkspaceLiveEvent> {
		let mut event: Option<WorkspaceLiveEvent> = None;
		let mut source_paths = Vec::new();
		for path in paths {
			if matches!(
				collect_path_live_signal(
					self.paths.classify(path, allow_git_signals),
					path,
					&mut event,
					&mut source_paths,
				),
				PathCollection::RescanRequired
			) {
				return Some(WorkspaceLiveEvent::RescanRequired);
			}
		}
		if !source_paths.is_empty() {
			event = coalesce_optional(event, WorkspaceLiveEvent::SourcesChanged(source_paths));
		}
		event
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct WorkspaceWatchTarget {
	pub path: PathBuf,
	pub mode: RecursiveMode,
}

#[cfg(test)]
pub(super) fn watch_targets_for(
	roots: &[WorkspaceWatchRoot],
) -> anyhow::Result<Vec<WorkspaceWatchTarget>> {
	let git_dirs = resolve_git_metadata_dirs(roots)?;
	Ok(watch_targets_for_resolved_git_dirs(roots, &git_dirs))
}

pub(super) fn watch_targets_for_resolved_git_dirs(
	roots: &[WorkspaceWatchRoot],
	git_dirs: &BTreeSet<PathBuf>,
) -> Vec<WorkspaceWatchTarget> {
	let mut targets = BTreeSet::new();
	for root in roots {
		for path in walk_non_ignored_directories(&root.path) {
			if !root
				.ignored_paths
				.iter()
				.any(|ignored| path.starts_with(ignored))
			{
				insert_watch_target(&mut targets, path);
			}
		}
		if let Some(notes_path) = root.notes_path.as_ref() {
			insert_notes_watch_paths(&mut targets, notes_path);
		}
	}
	insert_git_watch_paths(&mut targets, git_dirs);
	watch_targets_from_paths(targets)
}

#[cfg(target_os = "macos")]
pub(super) fn notes_watch_targets_for(roots: &[WorkspaceWatchRoot]) -> Vec<WorkspaceWatchTarget> {
	let mut targets = BTreeSet::new();
	for notes_path in roots.iter().filter_map(|root| root.notes_path.as_ref()) {
		insert_notes_watch_paths(&mut targets, notes_path);
	}
	watch_targets_from_paths(targets)
}

fn insert_notes_watch_paths(targets: &mut BTreeSet<PathBuf>, notes_path: &Path) {
	let watch_path = notes_watch_path(notes_path);
	insert_watch_target(targets, watch_path);
	if let Some(notes_dir) = notes_path.parent().filter(|path| path.is_dir())
		&& let Some(parent) = notes_dir.parent()
	{
		insert_watch_target(targets, parent.to_path_buf());
	}
}

#[cfg(target_os = "macos")]
pub(super) fn git_watch_targets_for_resolved_git_dirs(
	git_dirs: &BTreeSet<PathBuf>,
) -> Vec<WorkspaceWatchTarget> {
	let mut targets = BTreeSet::new();
	insert_git_watch_paths(&mut targets, git_dirs);
	watch_targets_from_paths(targets)
}

fn insert_git_watch_paths(targets: &mut BTreeSet<PathBuf>, git_dirs: &BTreeSet<PathBuf>) {
	for git_dir in git_dirs {
		if !git_dir.is_dir() {
			continue;
		}
		insert_watch_target(targets, git_dir.clone());
		let refs = git_dir.join("refs");
		if refs.is_dir() {
			for path in walk_non_ignored_directories(&refs) {
				insert_watch_target(targets, path);
			}
		}
	}
}

pub(super) fn resolve_git_metadata_dirs(
	roots: &[WorkspaceWatchRoot],
) -> anyhow::Result<BTreeSet<PathBuf>> {
	let mut dirs = BTreeSet::new();
	let git_roots = roots
		.iter()
		.filter_map(|root| root.git_root.as_ref())
		.map(|root| normalize_path(root))
		.collect::<BTreeSet<_>>();
	for git_root in git_roots {
		dirs.extend(git_metadata_dirs(&git_root)?);
	}
	Ok(dirs)
}

pub(super) fn git_metadata_dirs(git_root: &Path) -> anyhow::Result<Vec<PathBuf>> {
	let dot_git = git_root.join(".git");
	if dot_git.is_dir() {
		return Ok(vec![normalize_path(&dot_git)]);
	}
	if !dot_git.is_file() {
		return Ok(vec![normalize_path(&dot_git)]);
	}
	let output = git_fast_text(
		git_root,
		&["rev-parse", "--absolute-git-dir", "--git-common-dir"],
	)
	.map_err(|error| {
		anyhow::anyhow!(
			"cannot resolve Git metadata directories for linked worktree {}: {error}",
			git_root.display()
		)
	})?;
	Ok(output
		.lines()
		.map(str::trim)
		.filter(|path| !path.is_empty())
		.map(|path| {
			let path = Path::new(path);
			if path.is_absolute() {
				normalize_path(path)
			} else {
				normalize_path(&git_root.join(path))
			}
		})
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect())
}

fn watch_targets_from_paths(targets: BTreeSet<PathBuf>) -> Vec<WorkspaceWatchTarget> {
	targets
		.into_iter()
		.map(|path| WorkspaceWatchTarget {
			path,
			mode: RecursiveMode::NonRecursive,
		})
		.collect()
}

fn insert_watch_target(targets: &mut BTreeSet<PathBuf>, path: PathBuf) {
	targets.insert(normalize_path(&path));
}

#[derive(Clone, Debug)]
struct WorkspacePathClassifier {
	roots: Vec<WatchedPathRoot>,
	watched_directories: BTreeSet<PathBuf>,
	git_dirs: BTreeSet<PathBuf>,
}

impl WorkspacePathClassifier {
	#[cfg(test)]
	fn new(roots: Vec<WorkspaceWatchRoot>) -> Self {
		let git_dirs = resolve_git_metadata_dirs(&roots).expect("test watch roots resolve");
		Self::new_with_watch_targets(roots, &[], git_dirs)
	}

	fn new_with_watch_targets(
		roots: Vec<WorkspaceWatchRoot>,
		targets: &[WorkspaceWatchTarget],
		git_dirs: BTreeSet<PathBuf>,
	) -> Self {
		Self {
			roots: roots.into_iter().map(WatchedPathRoot::new).collect(),
			watched_directories: targets
				.iter()
				.map(|target| normalize_path(&target.path))
				.collect(),
			git_dirs,
		}
	}

	fn requires_directory_rescan(&self, paths: &[PathBuf], assume_directory: bool) -> bool {
		paths.iter().any(|path| {
			(path.is_dir() && self.classify(path, true) == PathLiveSignal::Source)
				|| (assume_directory && self.should_watch_directory(path))
		})
	}

	fn should_watch_directory(&self, path: &Path) -> bool {
		let path = normalize_path(path);
		match self.classify_control_path(&path, true, true) {
			Some(PathLiveSignal::Ignore | PathLiveSignal::BuildContext) => false,
			Some(PathLiveSignal::GitBaseChanged | PathLiveSignal::Notes) => true,
			Some(PathLiveSignal::Manifest | PathLiveSignal::Source) => true,
			None => self.roots.iter().any(|root| path.starts_with(&root.path)),
		}
	}

	fn requires_rename_rescan(&self, paths: &[PathBuf]) -> bool {
		paths.iter().any(|path| {
			let path = normalize_path(path);
			self.watched_directories.contains(&path)
				|| (path.is_dir() && self.should_watch_directory(&path))
		})
	}

	fn classify(&self, path: &Path, allow_git_signals: bool) -> PathLiveSignal {
		let path = normalize_path(path);
		if let Some(signal) = self.classify_control_path(&path, allow_git_signals, path.is_dir()) {
			return signal;
		}
		classify_workspace_path(&self.roots, &path)
	}

	fn classify_control_path(
		&self,
		path: &Path,
		allow_git_signals: bool,
		is_dir: bool,
	) -> Option<PathLiveSignal> {
		if is_ignore_rules_path(&self.roots, path) {
			return Some(PathLiveSignal::BuildContext);
		}
		if allow_git_signals && is_git_signal_path(&self.git_dirs, path) {
			return Some(PathLiveSignal::GitBaseChanged);
		}
		if is_dir && is_git_topology_path(&self.git_dirs, path) {
			return Some(PathLiveSignal::GitBaseChanged);
		}
		if is_workspace_config_path(&self.roots, path) {
			return Some(PathLiveSignal::BuildContext);
		}
		if is_notes_path(&self.roots, path) {
			return Some(PathLiveSignal::Notes);
		}
		if is_ignored_root(&self.roots, path)
			|| is_ignored_by_gitignore(&self.roots, path, is_dir)
			|| is_git_path(&self.git_dirs, path)
		{
			return Some(PathLiveSignal::Ignore);
		}
		None
	}
}

fn classify_workspace_path(roots: &[WatchedPathRoot], path: &Path) -> PathLiveSignal {
	if is_notes_path(roots, path) {
		return PathLiveSignal::Notes;
	}
	if is_build_context_path(roots, path) {
		return PathLiveSignal::BuildContext;
	}
	if is_manifest_path(roots, path) {
		return PathLiveSignal::Manifest;
	}
	if is_source_path(roots, path) {
		return PathLiveSignal::Source;
	}
	PathLiveSignal::Ignore
}

fn is_manifest_path(roots: &[WatchedPathRoot], path: &Path) -> bool {
	roots.iter().any(|root| path.starts_with(&root.path)) && is_manifest_file(path)
}

fn is_build_context_path(roots: &[WatchedPathRoot], path: &Path) -> bool {
	roots.iter().any(|root| path.starts_with(&root.path)) && is_build_context_file(path)
}

fn is_workspace_config_path(roots: &[WatchedPathRoot], path: &Path) -> bool {
	roots.iter().any(|root| path.starts_with(&root.path))
		&& path.file_name() == Some(std::ffi::OsStr::new(".code-moniker.toml"))
}

fn is_ignore_rules_path(roots: &[WatchedPathRoot], path: &Path) -> bool {
	let is_workspace_ignore = matches!(
		path.file_name().and_then(|name| name.to_str()),
		Some(".gitignore" | ".ignore")
	) && path.parent().is_some_and(|parent| {
		roots
			.iter()
			.any(|root| root.accepts_workspace_path(parent, true))
	});
	if is_workspace_ignore {
		return true;
	}
	false
}

fn is_ignored_root(roots: &[WatchedPathRoot], path: &Path) -> bool {
	roots.iter().any(|root| {
		root.ignored_paths
			.iter()
			.any(|ignored| path.starts_with(ignored))
	})
}

fn is_ignored_by_gitignore(roots: &[WatchedPathRoot], path: &Path, is_dir: bool) -> bool {
	let mut covering = roots
		.iter()
		.filter(|root| path.starts_with(&root.path))
		.peekable();
	covering.peek().is_some() && covering.all(|root| root.matches_gitignore(path, is_dir))
}

fn is_git_signal_path(git_dirs: &BTreeSet<PathBuf>, path: &Path) -> bool {
	git_dirs.iter().any(|git_dir| {
		let Ok(rel) = path.strip_prefix(git_dir) else {
			return false;
		};
		rel == Path::new("HEAD") || rel == Path::new("packed-refs") || rel.starts_with("refs")
	})
}

fn is_git_topology_path(git_dirs: &BTreeSet<PathBuf>, path: &Path) -> bool {
	git_dirs.iter().any(|git_dir| {
		let Ok(rel) = path.strip_prefix(git_dir) else {
			return false;
		};
		rel == Path::new("refs") || rel.starts_with("refs")
	})
}

fn is_git_path(git_dirs: &BTreeSet<PathBuf>, path: &Path) -> bool {
	git_dirs.iter().any(|git_dir| path.starts_with(git_dir))
}

fn is_notes_path(roots: &[WatchedPathRoot], path: &Path) -> bool {
	roots.iter().any(|root| {
		let Some(notes_path) = root.notes_path.as_ref() else {
			return false;
		};
		if path == notes_path {
			return true;
		}
		root.notes_dir
			.as_ref()
			.is_some_and(|notes_dir| path == notes_dir || path.parent() == Some(notes_dir))
	})
}

fn is_source_path(roots: &[WatchedPathRoot], path: &Path) -> bool {
	roots.iter().any(|root| path.starts_with(&root.path)) && (path.is_dir() || is_source_file(path))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EventPathPolicy {
	Ignore,
	Classify { allow_git_signals: bool },
	RescanSourceChange { assume_directory: bool },
	RescanMissingSource,
	RescanRename,
}

fn event_path_policy(kind: &EventKind) -> EventPathPolicy {
	match kind {
		EventKind::Access(AccessKind::Close(AccessMode::Write)) => EventPathPolicy::Classify {
			allow_git_signals: false,
		},
		EventKind::Access(_) => EventPathPolicy::Ignore,
		EventKind::Other => EventPathPolicy::Classify {
			allow_git_signals: true,
		},
		EventKind::Any => EventPathPolicy::Classify {
			allow_git_signals: false,
		},
		EventKind::Create(kind) => EventPathPolicy::RescanSourceChange {
			assume_directory: *kind == CreateKind::Folder,
		},
		EventKind::Remove(kind) => EventPathPolicy::RescanSourceChange {
			assume_directory: !matches!(kind, RemoveKind::File),
		},
		EventKind::Modify(ModifyKind::Name(_)) => EventPathPolicy::RescanRename,
		EventKind::Modify(_) => EventPathPolicy::RescanMissingSource,
	}
}

#[derive(Clone, Debug)]
struct WatchedPathRoot {
	path: PathBuf,
	ignored_paths: Vec<PathBuf>,
	notes_path: Option<PathBuf>,
	notes_dir: Option<PathBuf>,
	ignore: Arc<Mutex<ignore::IncrementalIgnore>>,
}

impl WatchedPathRoot {
	fn new(watch: WorkspaceWatchRoot) -> Self {
		let WorkspaceWatchRoot {
			path,
			git_root: _,
			ignored_paths,
			notes_path,
		} = watch;
		let path = normalize_path(&path);
		let ignored_paths = ignored_paths
			.iter()
			.map(|path| normalize_path(path))
			.collect();
		let notes_path = notes_path.as_ref().map(|path| normalize_path(path));
		let notes_dir = notes_path
			.as_ref()
			.and_then(|path| path.parent().map(Path::to_path_buf));
		let ignore = Arc::new(Mutex::new(workspace_ignore_matcher(&path)));

		Self {
			path,
			ignored_paths,
			notes_path,
			notes_dir,
			ignore,
		}
	}

	fn matches_gitignore(&self, path: &Path, is_dir: bool) -> bool {
		let Ok(relative) = path.strip_prefix(&self.path) else {
			return false;
		};
		self.ignore
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
			.matched(relative, is_dir)
			.is_ignore()
	}

	fn accepts_workspace_path(&self, path: &Path, is_dir: bool) -> bool {
		path.starts_with(&self.path) && !self.matches_gitignore(path, is_dir)
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PathLiveSignal {
	Ignore,
	GitBaseChanged,
	Notes,
	BuildContext,
	Manifest,
	Source,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PathCollection {
	Collected,
	RescanRequired,
}

fn collect_path_live_signal(
	signal: PathLiveSignal,
	path: &Path,
	event: &mut Option<WorkspaceLiveEvent>,
	source_paths: &mut Vec<PathBuf>,
) -> PathCollection {
	match signal {
		PathLiveSignal::Ignore => {}
		PathLiveSignal::GitBaseChanged => {
			*event = coalesce_optional(event.take(), WorkspaceLiveEvent::GitBaseChanged);
		}
		PathLiveSignal::Notes => {
			*event = coalesce_optional(event.take(), WorkspaceLiveEvent::Notes);
		}
		PathLiveSignal::BuildContext => return PathCollection::RescanRequired,
		PathLiveSignal::Manifest => {
			push_unique(source_paths, normalize_path(path));
		}
		PathLiveSignal::Source => {
			let path = normalize_path(path);
			if path.is_dir() {
				return PathCollection::RescanRequired;
			}
			push_unique(source_paths, path);
		}
	}
	PathCollection::Collected
}

pub(crate) fn watch_roots_for_paths(
	paths: &[PathBuf],
	cache_dir: Option<&Path>,
) -> Vec<WorkspaceWatchRoot> {
	let ignored_paths = cache_dir
		.map(|path| vec![absolute_path(path)])
		.unwrap_or_default();
	let notes_watch_targets = notes_watch_targets_for_paths(paths).unwrap_or_else(|_| Vec::new());
	let workspace_notes_path = notes_watch_targets
		.first()
		.map(|target| target.notes_path.clone());
	let mut roots = Vec::new();
	for path in paths {
		let watched_path = watch_path(path);
		let git_root = nearest_git_root(&watched_path);
		push_watch_root(
			&mut roots,
			watched_path,
			git_root,
			ignored_paths.clone(),
			workspace_notes_path.clone(),
		);
	}
	roots
}

fn push_watch_root(
	roots: &mut Vec<WorkspaceWatchRoot>,
	path: PathBuf,
	git_root: Option<PathBuf>,
	ignored_paths: Vec<PathBuf>,
	notes_path: Option<PathBuf>,
) {
	let path = absolute_path(&path);
	if ignored_paths
		.iter()
		.any(|ignored| path.starts_with(ignored))
	{
		return;
	}
	if let Some(existing) = roots.iter_mut().find(|root| root.path == path) {
		if existing.git_root.is_none() {
			existing.git_root = git_root;
		}
		if existing.notes_path.is_none() {
			existing.notes_path = notes_path;
		}
		return;
	}
	roots.push(WorkspaceWatchRoot {
		path,
		git_root,
		ignored_paths,
		notes_path,
	});
}

fn is_source_file(path: &Path) -> bool {
	crate::environment::language_for_path(path).is_ok()
}

fn is_build_context_file(path: &Path) -> bool {
	is_c_build_context_file(path) || crate::tsconfig::is_tsconfig_path(path)
}

fn is_c_build_context_file(path: &Path) -> bool {
	let filename = path.file_name().and_then(|name| name.to_str());
	if filename.is_some_and(|name| {
		matches!(
			name,
			"Makefile" | "makefile" | "GNUmakefile" | "compile_commands.json"
		)
	}) {
		return true;
	}
	path.extension()
		.and_then(|extension| extension.to_str())
		.is_some_and(|extension| {
			matches!(
				extension.to_ascii_lowercase().as_str(),
				"c" | "h" | "cc" | "cpp" | "cxx" | "c++" | "hh" | "hpp" | "hxx" | "h++"
			)
		})
}

fn is_manifest_file(path: &Path) -> bool {
	Manifest::for_filename(path).is_some()
}

fn coalesce_optional(
	current: Option<WorkspaceLiveEvent>,
	next: WorkspaceLiveEvent,
) -> Option<WorkspaceLiveEvent> {
	Some(current.map_or(next.clone(), |current| current.coalesce(next)))
}

fn watch_path(path: &Path) -> PathBuf {
	let path = absolute_path(path);
	if path.is_file() {
		path.parent().map(Path::to_path_buf).unwrap_or(path)
	} else {
		path
	}
}

fn nearest_git_root(path: &Path) -> Option<PathBuf> {
	let mut cursor = if path.is_file() {
		path.parent()?.to_path_buf()
	} else {
		path.to_path_buf()
	};
	loop {
		if cursor.join(".git").exists() {
			return Some(cursor);
		}
		if !cursor.pop() {
			return None;
		}
	}
}
