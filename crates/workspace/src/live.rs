use std::path::{Component, Path, PathBuf};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use notify::event::{AccessKind, AccessMode, ModifyKind};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::notes::notes_watch_targets_for_paths;
use code_moniker_core::lang::build_manifest::Manifest;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceWatchRoot {
	pub path: PathBuf,
	pub git_root: Option<PathBuf>,
	pub ignored_paths: Vec<PathBuf>,
	pub notes_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceLiveEvent {
	GitBaseChanged,
	Notes,
	GitBaseAndNotes,
	SourcesChanged(Vec<PathBuf>),
	SourcesAndNotes(Vec<PathBuf>),
	SourcesAndGitBase(Vec<PathBuf>),
	SourcesGitBaseAndNotes(Vec<PathBuf>),
	RescanRequired,
	RescanAndNotes,
	RescanAndGitBase,
	RescanGitBaseAndNotes,
}

impl WorkspaceLiveEvent {
	pub fn coalesce(self, other: Self) -> Self {
		WorkspaceLiveRefreshPlan::from_event(self)
			.coalesce(WorkspaceLiveRefreshPlan::from_event(other))
			.into_event()
	}

	pub fn source_paths(&self) -> Option<&[PathBuf]> {
		match self {
			Self::SourcesChanged(paths)
			| Self::SourcesAndNotes(paths)
			| Self::SourcesAndGitBase(paths)
			| Self::SourcesGitBaseAndNotes(paths) => Some(paths),
			Self::GitBaseChanged
			| Self::Notes
			| Self::GitBaseAndNotes
			| Self::RescanRequired
			| Self::RescanAndNotes
			| Self::RescanAndGitBase
			| Self::RescanGitBaseAndNotes => None,
		}
	}

	pub fn includes_notes(&self) -> bool {
		matches!(
			self,
			Self::Notes
				| Self::GitBaseAndNotes
				| Self::SourcesAndNotes(_)
				| Self::SourcesGitBaseAndNotes(_)
				| Self::RescanAndNotes
				| Self::RescanGitBaseAndNotes
		)
	}

	pub fn includes_git_base(&self) -> bool {
		matches!(
			self,
			Self::GitBaseChanged
				| Self::GitBaseAndNotes
				| Self::SourcesAndGitBase(_)
				| Self::SourcesGitBaseAndNotes(_)
				| Self::RescanAndGitBase
				| Self::RescanGitBaseAndNotes
		)
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceLiveRefreshPlan {
	rescan: bool,
	source_paths: Vec<PathBuf>,
	git_base: bool,
	notes: bool,
}

impl WorkspaceLiveRefreshPlan {
	pub fn from_event(event: WorkspaceLiveEvent) -> Self {
		refresh_plan_from_event(event)
	}

	pub fn requires_rescan(&self) -> bool {
		self.rescan
	}

	pub fn source_paths(&self) -> &[PathBuf] {
		&self.source_paths
	}

	pub fn includes_git_base(&self) -> bool {
		self.git_base
	}

	pub fn includes_notes(&self) -> bool {
		self.notes
	}

	pub fn coalesce(mut self, other: Self) -> Self {
		self.rescan |= other.rescan;
		self.git_base |= other.git_base;
		self.notes |= other.notes;
		for path in other.source_paths {
			push_unique(&mut self.source_paths, path);
		}
		self
	}

	pub fn into_event(self) -> WorkspaceLiveEvent {
		refresh_plan_into_event(self)
	}
}

fn refresh_plan_from_event(event: WorkspaceLiveEvent) -> WorkspaceLiveRefreshPlan {
	match event {
		WorkspaceLiveEvent::RescanRequired => WorkspaceLiveRefreshPlan {
			rescan: true,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::RescanAndNotes => WorkspaceLiveRefreshPlan {
			rescan: true,
			notes: true,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::RescanAndGitBase => WorkspaceLiveRefreshPlan {
			rescan: true,
			git_base: true,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::RescanGitBaseAndNotes => WorkspaceLiveRefreshPlan {
			rescan: true,
			git_base: true,
			notes: true,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::GitBaseChanged => WorkspaceLiveRefreshPlan {
			git_base: true,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::Notes => WorkspaceLiveRefreshPlan {
			notes: true,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::GitBaseAndNotes => WorkspaceLiveRefreshPlan {
			git_base: true,
			notes: true,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::SourcesChanged(source_paths) => WorkspaceLiveRefreshPlan {
			source_paths,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::SourcesAndNotes(source_paths) => WorkspaceLiveRefreshPlan {
			source_paths,
			notes: true,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::SourcesAndGitBase(source_paths) => WorkspaceLiveRefreshPlan {
			source_paths,
			git_base: true,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::SourcesGitBaseAndNotes(source_paths) => WorkspaceLiveRefreshPlan {
			source_paths,
			git_base: true,
			notes: true,
			..WorkspaceLiveRefreshPlan::default()
		},
	}
}

fn refresh_plan_into_event(plan: WorkspaceLiveRefreshPlan) -> WorkspaceLiveEvent {
	if plan.rescan {
		return match (plan.git_base, plan.notes) {
			(true, true) => WorkspaceLiveEvent::RescanGitBaseAndNotes,
			(true, false) => WorkspaceLiveEvent::RescanAndGitBase,
			(false, true) => WorkspaceLiveEvent::RescanAndNotes,
			(false, false) => WorkspaceLiveEvent::RescanRequired,
		};
	}
	match (plan.source_paths.is_empty(), plan.git_base, plan.notes) {
		(false, true, true) => WorkspaceLiveEvent::SourcesGitBaseAndNotes(plan.source_paths),
		(false, true, false) => WorkspaceLiveEvent::SourcesAndGitBase(plan.source_paths),
		(false, false, true) => WorkspaceLiveEvent::SourcesAndNotes(plan.source_paths),
		(false, false, false) => WorkspaceLiveEvent::SourcesChanged(plan.source_paths),
		(true, true, true) => WorkspaceLiveEvent::GitBaseAndNotes,
		(true, true, false) => WorkspaceLiveEvent::GitBaseChanged,
		(true, false, true) => WorkspaceLiveEvent::Notes,
		(true, false, false) => WorkspaceLiveEvent::RescanRequired,
	}
}

pub struct LiveWorkspaceWatcher {
	_watcher: RecommendedWatcher,
	_worker: JoinHandle<()>,
	watched_paths: usize,
	warnings: Vec<String>,
}

const LIVE_EVENT_DEBOUNCE: Duration = Duration::from_millis(50);

impl LiveWorkspaceWatcher {
	pub fn start<F>(roots: Vec<WorkspaceWatchRoot>, publish: F) -> anyhow::Result<Self>
	where
		F: Fn(WorkspaceLiveEvent) + Send + 'static,
	{
		let classifier = WorkspaceEventClassifier::new(roots);
		let (tx, worker) = watcher_event_channel(publish);
		let mut watcher = new_recommended_watcher(classifier.clone(), tx)?;
		let (watched_paths, warnings) = watch_classifier_paths(&mut watcher, &classifier);

		Ok(Self {
			_watcher: watcher,
			_worker: worker,
			watched_paths,
			warnings,
		})
	}

	pub fn status(&self) -> Option<String> {
		if self.watched_paths == 0 {
			return Some("live store disabled: no source path could be watched".to_string());
		}
		if self.warnings.is_empty() {
			return Some(format!(
				"live store watching {} path(s)",
				self.watched_paths
			));
		}
		Some(format!(
			"live store watching {} path(s), {} warning(s)",
			self.watched_paths,
			self.warnings.len()
		))
	}
}

fn watcher_event_channel<F>(publish: F) -> (mpsc::Sender<WorkspaceLiveEvent>, JoinHandle<()>)
where
	F: Fn(WorkspaceLiveEvent) + Send + 'static,
{
	let (tx, rx) = mpsc::channel();
	let worker = thread::spawn(move || publish_coalesced_events(rx, publish));
	(tx, worker)
}

fn new_recommended_watcher(
	classifier: WorkspaceEventClassifier,
	tx: mpsc::Sender<WorkspaceLiveEvent>,
) -> anyhow::Result<RecommendedWatcher> {
	Ok(RecommendedWatcher::new(
		move |event: notify::Result<Event>| {
			let Ok(event) = event else {
				return;
			};
			if let Some(store_event) = classifier.classify_event(&event) {
				let _ = tx.send(store_event);
			}
		},
		Config::default(),
	)?)
}

fn watch_classifier_paths(
	watcher: &mut RecommendedWatcher,
	classifier: &WorkspaceEventClassifier,
) -> (usize, Vec<String>) {
	let mut warnings = Vec::new();
	let mut watched_paths = 0;
	for path in classifier.watch_paths() {
		match watcher.watch(&path, RecursiveMode::NonRecursive) {
			Ok(()) => watched_paths += 1,
			Err(error) => warnings.push(format!("{}: {error}", path.display())),
		}
	}
	(watched_paths, warnings)
}

fn publish_coalesced_events<F>(rx: mpsc::Receiver<WorkspaceLiveEvent>, publish: F)
where
	F: Fn(WorkspaceLiveEvent),
{
	while let Ok(first) = rx.recv() {
		let mut event = first;
		while let Ok(next) = rx.recv_timeout(LIVE_EVENT_DEBOUNCE) {
			event = event.coalesce(next);
		}
		publish(event);
	}
}

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
	roots: Vec<WorkspaceWatchRoot>,
}

impl WorkspacePathClassifier {
	fn new(roots: Vec<WorkspaceWatchRoot>) -> Self {
		Self { roots }
	}

	fn watch_paths(&self) -> Vec<PathBuf> {
		let mut paths = Vec::new();
		for root in &self.roots {
			push_unique(&mut paths, root.path.clone());
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
		if allow_git_signals && self.is_git_signal_path(path) {
			return PathLiveSignal::GitBaseChanged;
		}
		if ignored_path(path) || self.is_ignored_root(path) || self.is_git_path(path) {
			return PathLiveSignal::Ignore;
		}
		if self.is_notes_path(path) {
			return PathLiveSignal::Notes;
		}
		if self.is_manifest_path(path) {
			return PathLiveSignal::Manifest;
		}
		if self.is_source_path(path) {
			return PathLiveSignal::Source;
		}
		PathLiveSignal::Ignore
	}

	fn is_git_signal_path(&self, path: &Path) -> bool {
		let path = normalize_path(path);
		self.roots.iter().any(|root| {
			let Some(git_root) = &root.git_root else {
				return false;
			};
			let git_dir = normalize_path(&git_root.join(".git"));
			let Ok(rel) = path.strip_prefix(&git_dir) else {
				return false;
			};
			rel == Path::new("HEAD") || rel == Path::new("packed-refs") || rel.starts_with("refs")
		})
	}

	fn is_git_path(&self, path: &Path) -> bool {
		let path = normalize_path(path);
		self.roots.iter().any(|root| {
			root.git_root
				.as_ref()
				.map(|git_root| path.starts_with(normalize_path(&git_root.join(".git"))))
				.unwrap_or(false)
		})
	}

	fn is_notes_path(&self, path: &Path) -> bool {
		let path = normalize_path(path);
		self.roots.iter().any(|root| {
			let Some(notes_path) = root.notes_path.as_ref().map(|path| normalize_path(path)) else {
				return false;
			};
			if path == notes_path {
				return true;
			}
			let Some(notes_dir) = notes_path.parent() else {
				return false;
			};
			path == notes_dir || path.parent().is_some_and(|parent| parent == notes_dir)
		})
	}

	fn is_source_path(&self, path: &Path) -> bool {
		let path = normalize_path(path);
		self.roots
			.iter()
			.any(|root| path.starts_with(normalize_path(&root.path)))
			&& (path.is_dir() || is_source_file(&path))
	}

	fn is_manifest_path(&self, path: &Path) -> bool {
		let path = normalize_path(path);
		self.roots
			.iter()
			.any(|root| path.starts_with(normalize_path(&root.path)))
			&& Manifest::for_filename(&path).is_some()
	}

	fn is_ignored_root(&self, path: &Path) -> bool {
		let path = normalize_path(path);
		self.roots.iter().any(|root| {
			root.ignored_paths
				.iter()
				.any(|ignored| path.starts_with(normalize_path(ignored)))
		})
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

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
	if !paths.iter().any(|existing| existing == &path) {
		paths.push(path);
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

fn absolute_path(path: &Path) -> PathBuf {
	let path = if path.is_absolute() {
		path.to_path_buf()
	} else {
		std::env::current_dir()
			.map(|cwd| cwd.join(path))
			.unwrap_or_else(|_| path.to_path_buf())
	};
	path.canonicalize().unwrap_or_else(|_| lexical_path(&path))
}

fn normalize_path(path: &Path) -> PathBuf {
	let path = if path.is_absolute() {
		path.to_path_buf()
	} else {
		std::env::current_dir()
			.map(|cwd| cwd.join(path))
			.unwrap_or_else(|_| path.to_path_buf())
	};
	path.canonicalize().unwrap_or_else(|_| lexical_path(&path))
}

fn lexical_path(path: &Path) -> PathBuf {
	let mut out = PathBuf::new();
	for component in path.components() {
		match component {
			Component::CurDir => {}
			Component::ParentDir => {
				out.pop();
			}
			_ => out.push(component.as_os_str()),
		}
	}
	out
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

#[cfg(test)]
mod tests {
	use std::path::PathBuf;
	use std::sync::mpsc;
	use std::time::Duration;

	use super::{
		LiveWorkspaceWatcher, WorkspaceEventClassifier, WorkspaceLiveEvent, WorkspaceWatchRoot,
		watch_roots_for_paths,
	};

	#[test]
	fn watcher_publishes_source_changes() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let source = temp.path().join("src").join("lib.rs");
		std::fs::create_dir_all(source.parent().expect("source parent")).expect("src dir");
		std::fs::write(&source, "pub fn before() {}\n").expect("seed source");
		let (tx, rx) = mpsc::channel();
		let _watcher = LiveWorkspaceWatcher::start(
			watch_roots_for_paths(&[temp.path().to_path_buf()], None),
			move |event| {
				let _ = tx.send(event);
			},
		)
		.expect("watcher starts");
		std::thread::sleep(Duration::from_millis(200));

		std::fs::write(&source, "pub fn before() {}\npub fn after() {}\n").expect("modify source");

		let event = rx
			.recv_timeout(Duration::from_secs(3))
			.expect("source change event");
		assert!(
			matches!(
				event,
				WorkspaceLiveEvent::SourcesChanged(_) | WorkspaceLiveEvent::RescanRequired
			),
			"unexpected event: {event:?}"
		);
	}

	#[test]
	fn watcher_publishes_atomic_source_replaces() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let source = temp.path().join("src").join("lib.rs");
		std::fs::create_dir_all(source.parent().expect("source parent")).expect("src dir");
		std::fs::write(&source, "pub fn before() {}\n").expect("seed source");
		let (tx, rx) = mpsc::channel();
		let _watcher = LiveWorkspaceWatcher::start(
			watch_roots_for_paths(&[temp.path().to_path_buf()], None),
			move |event| {
				let _ = tx.send(event);
			},
		)
		.expect("watcher starts");
		std::thread::sleep(Duration::from_millis(200));

		let replacement = source.with_extension("rs.tmp");
		std::fs::write(&replacement, "pub fn before() {}\npub fn after() {}\n")
			.expect("write replacement");
		std::fs::rename(&replacement, &source).expect("replace source");

		let event = rx
			.recv_timeout(Duration::from_secs(3))
			.expect("source replace event");
		assert!(
			matches!(
				event,
				WorkspaceLiveEvent::SourcesChanged(_) | WorkspaceLiveEvent::RescanRequired
			),
			"unexpected event: {event:?}"
		);
	}

	#[test]
	fn classifies_source_changes_with_changed_paths() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: None,
			ignored_paths: Vec::new(),
			notes_path: Some(PathBuf::from("/repo/.code-moniker/notes.toml")),
		}]);

		assert_eq!(
			classifier.classify_paths_with_git_signals(&[PathBuf::from("/repo/src/lib.rs")], true),
			Some(WorkspaceLiveEvent::SourcesChanged(vec![PathBuf::from(
				"/repo/src/lib.rs"
			)]))
		);
	}

	#[test]
	fn ignores_non_language_files_under_source_root() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: None,
			ignored_paths: Vec::new(),
			notes_path: None,
		}]);

		assert_eq!(
			classifier.classify_paths_with_git_signals(&[PathBuf::from("/repo/README.md")], true),
			None
		);
		assert_eq!(
			classifier.classify_event(
				&notify::Event::new(notify::EventKind::Create(notify::event::CreateKind::File))
					.add_path(PathBuf::from("/repo/README.md"))
			),
			None
		);
	}

	#[test]
	fn classifies_manifest_changes_as_live_path_refresh() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: None,
			ignored_paths: Vec::new(),
			notes_path: None,
		}]);

		assert_eq!(
			classifier
				.classify_paths_with_git_signals(&[PathBuf::from("/repo/package.json")], true),
			Some(WorkspaceLiveEvent::SourcesChanged(vec![PathBuf::from(
				"/repo/package.json"
			)]))
		);
	}

	#[test]
	fn classifies_source_create_remove_as_rescan_required() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: None,
			ignored_paths: Vec::new(),
			notes_path: None,
		}]);

		assert_eq!(
			classifier.classify_event(
				&notify::Event::new(notify::EventKind::Create(notify::event::CreateKind::File))
					.add_path(PathBuf::from("/repo/src/new.rs"))
			),
			Some(WorkspaceLiveEvent::RescanRequired)
		);
		assert_eq!(
			classifier.classify_event(
				&notify::Event::new(notify::EventKind::Remove(notify::event::RemoveKind::File))
					.add_path(PathBuf::from("/repo/src/old.rs"))
			),
			Some(WorkspaceLiveEvent::RescanRequired)
		);
	}

	#[test]
	fn classifies_source_rename_as_rescan_required() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: None,
			ignored_paths: Vec::new(),
			notes_path: None,
		}]);

		assert_eq!(
			classifier.classify_event(
				&notify::Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Name(
					notify::event::RenameMode::Both,
				)))
				.add_path(PathBuf::from("/repo/src/old.rs"))
				.add_path(PathBuf::from("/repo/src/new.rs"))
			),
			Some(WorkspaceLiveEvent::RescanRequired)
		);
	}

	#[test]
	fn classifies_missing_source_modify_as_rescan_required() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let missing = temp.path().join("src").join("deleted.rs");
		let classifier = WorkspaceEventClassifier::new(watch_roots_for_paths(
			&[temp.path().to_path_buf()],
			None,
		));

		assert_eq!(
			classifier.classify_event(
				&notify::Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Data(
					notify::event::DataChange::Content,
				)))
				.add_path(missing)
			),
			Some(WorkspaceLiveEvent::RescanRequired)
		);
	}

	#[test]
	fn classifies_source_directory_changes_as_rescan_required() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let src = temp.path().join("src");
		std::fs::create_dir_all(&src).expect("src dir");
		let classifier = WorkspaceEventClassifier::new(watch_roots_for_paths(
			&[temp.path().to_path_buf()],
			None,
		));

		assert_eq!(
			classifier.classify_paths_with_git_signals(&[src], true),
			Some(WorkspaceLiveEvent::RescanRequired)
		);
	}

	#[test]
	fn coalesces_source_with_notes_and_git_base_without_dropping_signals() {
		assert_eq!(
			WorkspaceLiveEvent::SourcesChanged(vec![PathBuf::from("/repo/src/lib.rs")])
				.coalesce(WorkspaceLiveEvent::Notes),
			WorkspaceLiveEvent::SourcesAndNotes(vec![PathBuf::from("/repo/src/lib.rs")])
		);
		assert_eq!(
			WorkspaceLiveEvent::SourcesAndNotes(vec![PathBuf::from("/repo/src/lib.rs")])
				.coalesce(WorkspaceLiveEvent::GitBaseChanged),
			WorkspaceLiveEvent::SourcesGitBaseAndNotes(vec![PathBuf::from("/repo/src/lib.rs")])
		);
	}

	#[test]
	fn classifies_atomic_notes_writes_as_notes_refresh() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: None,
			ignored_paths: Vec::new(),
			notes_path: Some(PathBuf::from("/repo/.code-moniker/notes.toml")),
		}]);

		assert_eq!(
			classifier.classify_paths_with_git_signals(
				&[PathBuf::from("/repo/.code-moniker/notes.toml.tmp")],
				true,
			),
			Some(WorkspaceLiveEvent::Notes)
		);
		assert_eq!(
			classifier.classify_paths_with_git_signals(
				&[PathBuf::from("/repo/.code-moniker/notes.toml")],
				false,
			),
			Some(WorkspaceLiveEvent::Notes)
		);
	}

	#[test]
	fn classifies_git_refs_as_git_base_changes() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: Some(PathBuf::from("/repo")),
			ignored_paths: Vec::new(),
			notes_path: None,
		}]);

		assert_eq!(
			classifier.classify_paths_with_git_signals(&[PathBuf::from("/repo/.git/HEAD")], true),
			Some(WorkspaceLiveEvent::GitBaseChanged)
		);
		assert_eq!(
			classifier.classify_paths_with_git_signals(
				&[PathBuf::from("/repo/.git/refs/heads/main")],
				true,
			),
			Some(WorkspaceLiveEvent::GitBaseChanged)
		);
		assert_eq!(
			classifier.classify_paths_with_git_signals(&[PathBuf::from("/repo/.git/index")], true),
			None
		);
	}

	#[test]
	fn coalesces_notes_and_git_base_without_dropping_either() {
		assert_eq!(
			WorkspaceLiveEvent::GitBaseChanged.coalesce(WorkspaceLiveEvent::Notes),
			WorkspaceLiveEvent::GitBaseAndNotes
		);
		assert_eq!(
			WorkspaceLiveEvent::GitBaseAndNotes.coalesce(WorkspaceLiveEvent::RescanRequired),
			WorkspaceLiveEvent::RescanGitBaseAndNotes
		);
	}
}
