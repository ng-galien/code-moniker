use std::path::{Component, Path, PathBuf};

use notify::event::{AccessKind, AccessMode, ModifyKind};
use notify::{Event, EventKind};

use super::model::{WorkspaceLiveEvent, WorkspaceWatchRoot, push_unique};
use crate::notes::notes_watch_targets_for_paths;
use crate::path_util::{absolute_path, normalize_path};
use code_moniker_core::lang::build_manifest::Manifest;

#[derive(Clone, Debug)]
pub(crate) struct WorkspaceEventClassifier {
	paths: WorkspacePathClassifier,
}

impl WorkspaceEventClassifier {
	pub(crate) fn new(roots: Vec<WorkspaceWatchRoot>) -> Self {
		Self {
			paths: WorkspacePathClassifier::new(roots),
		}
	}

	pub(crate) fn watch_paths(&self) -> Vec<PathBuf> {
		self.paths.watch_paths()
	}

	pub(crate) fn classify_event(&self, event: &Event) -> Option<WorkspaceLiveEvent> {
		if event.need_rescan() {
			return Some(WorkspaceLiveEvent::RescanRequired);
		}
		self.classify_event_kind(&event.kind, &event.paths)
	}

	fn classify_event_kind(
		&self,
		kind: &EventKind,
		paths: &[PathBuf],
	) -> Option<WorkspaceLiveEvent> {
		match kind {
			EventKind::Access(AccessKind::Close(AccessMode::Write)) => {
				self.classify_paths_with_git_signals(paths, false)
			}
			EventKind::Access(_) | EventKind::Other => None,
			EventKind::Any => self.classify_paths_with_git_signals(paths, false),
			EventKind::Create(_) | EventKind::Remove(_) => {
				if self.paths.requires_source_rescan(paths) {
					return Some(WorkspaceLiveEvent::RescanRequired);
				}
				self.classify_paths_with_git_signals(paths, true)
			}
			EventKind::Modify(ModifyKind::Name(_)) => {
				if self.paths.requires_source_rescan(paths) {
					return Some(WorkspaceLiveEvent::RescanRequired);
				}
				self.classify_paths_with_git_signals(paths, true)
			}
			EventKind::Modify(_) => {
				if self.paths.includes_missing_source(paths) {
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

#[derive(Clone, Debug)]
struct WorkspacePathClassifier {
	roots: Vec<WatchedPathRoot>,
}

impl WorkspacePathClassifier {
	fn new(roots: Vec<WorkspaceWatchRoot>) -> Self {
		Self {
			roots: roots.into_iter().map(WatchedPathRoot::new).collect(),
		}
	}

	fn watch_paths(&self) -> Vec<PathBuf> {
		let mut paths = Vec::new();
		for root in &self.roots {
			push_unique(&mut paths, root.watch.path.clone());
		}
		paths
	}

	fn requires_source_rescan(&self, paths: &[PathBuf]) -> bool {
		paths
			.iter()
			.any(|path| matches!(self.classify(path, true), PathLiveSignal::Source))
	}

	fn includes_missing_source(&self, paths: &[PathBuf]) -> bool {
		paths.iter().any(|path| {
			matches!(self.classify(path, true), PathLiveSignal::Source) && !path.exists()
		})
	}

	fn classify(&self, path: &Path, allow_git_signals: bool) -> PathLiveSignal {
		let path = normalize_path(path);
		if allow_git_signals && self.is_git_signal_path(&path) {
			return PathLiveSignal::GitBaseChanged;
		}
		if ignored_path(&path) || self.is_ignored_root(&path) || self.is_git_path(&path) {
			return PathLiveSignal::Ignore;
		}
		if self.is_notes_path(&path) {
			return PathLiveSignal::Notes;
		}
		if self.is_manifest_path(&path) {
			return PathLiveSignal::Manifest;
		}
		if self.is_source_path(&path) {
			return PathLiveSignal::Source;
		}
		PathLiveSignal::Ignore
	}

	fn is_git_signal_path(&self, path: &Path) -> bool {
		self.roots.iter().any(|root| {
			let Some(git_dir) = &root.git_dir else {
				return false;
			};
			let Ok(rel) = path.strip_prefix(git_dir) else {
				return false;
			};
			rel == Path::new("HEAD") || rel == Path::new("packed-refs") || rel.starts_with("refs")
		})
	}

	fn is_git_path(&self, path: &Path) -> bool {
		self.roots.iter().any(|root| {
			root.git_dir
				.as_ref()
				.is_some_and(|git_dir| path.starts_with(git_dir))
		})
	}

	fn is_notes_path(&self, path: &Path) -> bool {
		self.roots.iter().any(|root| {
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

	fn is_source_path(&self, path: &Path) -> bool {
		self.roots.iter().any(|root| path.starts_with(&root.path))
			&& (path.is_dir() || is_source_file(path))
	}

	fn is_manifest_path(&self, path: &Path) -> bool {
		self.roots.iter().any(|root| path.starts_with(&root.path))
			&& Manifest::for_filename(path).is_some()
	}

	fn is_ignored_root(&self, path: &Path) -> bool {
		self.roots.iter().any(|root| {
			root.ignored_paths
				.iter()
				.any(|ignored| path.starts_with(ignored))
		})
	}
}

#[derive(Clone, Debug)]
struct WatchedPathRoot {
	watch: WorkspaceWatchRoot,
	path: PathBuf,
	git_dir: Option<PathBuf>,
	ignored_paths: Vec<PathBuf>,
	notes_path: Option<PathBuf>,
	notes_dir: Option<PathBuf>,
}

impl WatchedPathRoot {
	fn new(watch: WorkspaceWatchRoot) -> Self {
		let path = normalize_path(&watch.path);
		let git_dir = watch
			.git_root
			.as_ref()
			.map(|git_root| normalize_path(&git_root.join(".git")));
		let ignored_paths = watch
			.ignored_paths
			.iter()
			.map(|path| normalize_path(path))
			.collect();
		let notes_path = watch.notes_path.as_ref().map(|path| normalize_path(path));
		let notes_dir = notes_path
			.as_ref()
			.and_then(|path| path.parent().map(Path::to_path_buf));
		Self {
			watch,
			path,
			git_dir,
			ignored_paths,
			notes_path,
			notes_dir,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PathLiveSignal {
	Ignore,
	GitBaseChanged,
	Notes,
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
		for dir in watch_paths_for_path(path, &ignored_paths) {
			push_watch_root(
				&mut roots,
				dir,
				git_root.clone(),
				ignored_paths.clone(),
				workspace_notes_path.clone(),
			);
		}
		if let Some(git_root) = git_root {
			for dir in git_watch_dirs(&git_root) {
				push_git_watch_root(
					&mut roots,
					dir,
					ignored_paths.clone(),
					workspace_notes_path.clone(),
					git_root.clone(),
				);
			}
		}
	}
	for target in notes_watch_targets {
		let git_root = nearest_git_root(&target.path);
		push_watch_root(
			&mut roots,
			watch_path(&target.path),
			git_root,
			ignored_paths.clone(),
			Some(target.notes_path),
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
	if ignored_path(&path)
		|| ignored_paths
			.iter()
			.any(|ignored| normalize_path(&path).starts_with(normalize_path(ignored)))
	{
		return;
	}
	if let Some(existing) = roots
		.iter_mut()
		.find(|root| normalize_path(&root.path) == normalize_path(&path))
	{
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

fn push_git_watch_root(
	roots: &mut Vec<WorkspaceWatchRoot>,
	path: PathBuf,
	ignored_paths: Vec<PathBuf>,
	notes_path: Option<PathBuf>,
	git_root: PathBuf,
) {
	let path = absolute_path(&path);
	if let Some(existing) = roots
		.iter_mut()
		.find(|root| normalize_path(&root.path) == normalize_path(&path))
	{
		existing.git_root = Some(git_root);
		if existing.notes_path.is_none() {
			existing.notes_path = notes_path;
		}
		return;
	}
	roots.push(WorkspaceWatchRoot {
		path,
		git_root: Some(git_root),
		ignored_paths,
		notes_path,
	});
}

fn watch_paths_for_path(path: &Path, ignored_paths: &[PathBuf]) -> Vec<PathBuf> {
	let path = absolute_path(path);
	let mut paths = Vec::new();
	collect_watch_paths(&path, ignored_paths, &mut paths);
	paths
}

fn collect_watch_paths(path: &Path, ignored_paths: &[PathBuf], paths: &mut Vec<PathBuf>) {
	if ignored_path(path)
		|| ignored_paths
			.iter()
			.any(|ignored| normalize_path(path).starts_with(normalize_path(ignored)))
	{
		return;
	}
	let path = absolute_path(path);
	if path.is_file() {
		if is_source_file(&path) {
			push_unique(paths, path.clone());
		}
		if let Some(parent) = path.parent() {
			push_unique(paths, parent.to_path_buf());
		}
		return;
	}
	if !path.is_dir() {
		push_unique(paths, path);
		return;
	}
	push_unique(paths, path.clone());
	let Ok(entries) = std::fs::read_dir(&path) else {
		return;
	};
	for entry in entries.flatten() {
		let child = entry.path();
		if child.is_dir() {
			collect_watch_paths(&child, ignored_paths, paths);
		} else if child.is_file() && is_source_file(&child) {
			push_unique(paths, absolute_path(&child));
		}
	}
}

fn is_source_file(path: &Path) -> bool {
	crate::environment::language_for_path(path).is_ok()
}

fn git_watch_dirs(git_root: &Path) -> Vec<PathBuf> {
	let git_dir = git_root.join(".git");
	let mut dirs = Vec::new();
	collect_git_watch_dirs(&git_dir, &mut dirs);
	dirs
}

fn collect_git_watch_dirs(path: &Path, dirs: &mut Vec<PathBuf>) {
	let path = absolute_path(path);
	if path.is_file() {
		if let Some(parent) = path.parent() {
			push_unique(dirs, parent.to_path_buf());
		}
		return;
	}
	if !path.is_dir() {
		return;
	}
	push_unique(dirs, path.clone());
	let refs = path.join("refs");
	collect_git_refs_dirs(&refs, dirs);
}

fn collect_git_refs_dirs(path: &Path, dirs: &mut Vec<PathBuf>) {
	if !path.is_dir() {
		return;
	}
	push_unique(dirs, absolute_path(path));
	let Ok(entries) = std::fs::read_dir(path) else {
		return;
	};
	for entry in entries.flatten() {
		let child = entry.path();
		if child.is_dir() {
			collect_git_refs_dirs(&child, dirs);
		}
	}
}

fn coalesce_optional(
	current: Option<WorkspaceLiveEvent>,
	next: WorkspaceLiveEvent,
) -> Option<WorkspaceLiveEvent> {
	Some(current.map_or(next.clone(), |current| current.coalesce(next)))
}

fn ignored_path(path: &Path) -> bool {
	path.components().any(|component| {
		matches!(
			component,
			Component::Normal(name)
				if name == ".code-moniker-cache"
					|| name == ".git"
					|| name == ".gradle"
					|| name == "target"
					|| name == "node_modules"
					|| name == "build"
					|| name == "dist"
		)
	})
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
