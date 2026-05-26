use std::path::{Component, Path, PathBuf};

use notify::event::{AccessKind, AccessMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::session::StoreWatchRoot;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StoreEvent {
	GitOverlay,
	FullIndex,
}

impl StoreEvent {
	pub(super) fn coalesce(self, other: Self) -> Self {
		if matches!(self, Self::FullIndex) || matches!(other, Self::FullIndex) {
			Self::FullIndex
		} else {
			Self::GitOverlay
		}
	}
}

pub(super) struct LiveStoreWatcher {
	_watcher: RecommendedWatcher,
	watched_paths: usize,
	warnings: Vec<String>,
}

impl LiveStoreWatcher {
	pub(super) fn start<F>(roots: Vec<StoreWatchRoot>, publish: F) -> anyhow::Result<Self>
	where
		F: Fn(StoreEvent) + Send + 'static,
	{
		let classifier = EventClassifier::new(roots);
		let callback_classifier = classifier.clone();
		let mut watcher = RecommendedWatcher::new(
			move |event: notify::Result<Event>| {
				let Ok(event) = event else {
					return;
				};
				if let Some(store_event) = callback_classifier.classify_event(&event) {
					publish(store_event);
				}
			},
			Config::default(),
		)?;

		let mut warnings = Vec::new();
		let mut watched_paths = 0;
		for path in classifier.watch_paths() {
			match watcher.watch(&path, RecursiveMode::Recursive) {
				Ok(()) => watched_paths += 1,
				Err(error) => warnings.push(format!("{}: {error}", path.display())),
			}
		}

		Ok(Self {
			_watcher: watcher,
			watched_paths,
			warnings,
		})
	}

	pub(super) fn status(&self) -> Option<String> {
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

#[derive(Clone, Debug)]
struct EventClassifier {
	roots: Vec<StoreWatchRoot>,
}

impl EventClassifier {
	fn new(roots: Vec<StoreWatchRoot>) -> Self {
		Self { roots }
	}

	fn watch_paths(&self) -> Vec<PathBuf> {
		let mut paths = Vec::new();
		for root in &self.roots {
			push_unique(&mut paths, root.path.clone());
			if let Some(git_root) = &root.git_root {
				push_unique(&mut paths, git_root.join(".git"));
			}
		}
		paths
	}

	fn classify_event(&self, event: &Event) -> Option<StoreEvent> {
		if event.need_rescan() {
			return Some(StoreEvent::FullIndex);
		}
		match event.kind {
			EventKind::Access(AccessKind::Close(AccessMode::Write)) => {
				self.classify_paths_with_git_signals(&event.paths, false)
			}
			EventKind::Access(_) | EventKind::Other => None,
			EventKind::Any => self.classify_paths_with_git_signals(&event.paths, false),
			EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
				self.classify_paths_with_git_signals(&event.paths, true)
			}
		}
	}

	fn classify_paths_with_git_signals(
		&self,
		paths: &[PathBuf],
		allow_git_signals: bool,
	) -> Option<StoreEvent> {
		let mut event: Option<StoreEvent> = None;
		for path in paths {
			if allow_git_signals && self.is_git_signal_path(path) {
				event = Some(event.map_or(StoreEvent::GitOverlay, |current| {
					current.coalesce(StoreEvent::GitOverlay)
				}));
				continue;
			}
			if ignored_path(path) {
				continue;
			}
			if self.is_ignored_root(path) {
				continue;
			}
			if self.is_git_path(path) {
				continue;
			}
			if self.is_source_path(path) {
				return Some(StoreEvent::FullIndex);
			}
		}
		event
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
		self.roots.iter().any(|root| {
			root.git_root
				.as_ref()
				.map(|git_root| path.starts_with(git_root.join(".git")))
				.unwrap_or(false)
		})
	}

	fn is_source_path(&self, path: &Path) -> bool {
		self.roots.iter().any(|root| path.starts_with(&root.path))
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

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
	if !paths.iter().any(|existing| existing == &path) {
		paths.push(path);
	}
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

fn normalize_path(path: &Path) -> PathBuf {
	path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

// Disabled during the UI architecture rebuild; rewrite against the new store/watch contracts later.
#[cfg(any())]
mod tests {
	use super::*;
	use notify::event::{AccessKind, AccessMode, DataChange, ModifyKind};

	fn root(path: &str, git_root: Option<&str>) -> StoreWatchRoot {
		StoreWatchRoot {
			path: PathBuf::from(path),
			git_root: git_root.map(PathBuf::from),
			ignored_paths: Vec::new(),
		}
	}

	fn event(kind: EventKind, path: &str) -> Event {
		Event::new(kind).add_path(PathBuf::from(path))
	}

	#[test]
	fn classifies_source_changes_as_full_index_refresh() {
		let classifier = EventClassifier::new(vec![root("/repo/service", Some("/repo"))]);

		assert_eq!(
			classifier.classify_paths(&[PathBuf::from("/repo/service/src/App.java")]),
			Some(StoreEvent::FullIndex)
		);
	}

	#[test]
	fn classifies_git_overlays_as_git_overlay_refresh() {
		let classifier = EventClassifier::new(vec![root("/repo/service", Some("/repo"))]);

		assert_eq!(
			classifier.classify_paths(&[PathBuf::from("/repo/.git/HEAD")]),
			Some(StoreEvent::GitOverlay)
		);
		assert_eq!(
			classifier.classify_paths(&[PathBuf::from("/repo/.git/packed-refs")]),
			Some(StoreEvent::GitOverlay)
		);
		assert_eq!(
			classifier.classify_paths(&[PathBuf::from("/repo/.git/refs/heads/main")]),
			Some(StoreEvent::GitOverlay)
		);
	}

	#[test]
	fn ignores_git_access_events() {
		let classifier = EventClassifier::new(vec![root("/repo/service", Some("/repo"))]);

		assert_eq!(
			classifier.classify_event(&event(
				EventKind::Access(AccessKind::Open(AccessMode::Any)),
				"/repo/.git/HEAD"
			)),
			None
		);
		assert_eq!(
			classifier.classify_event(&event(
				EventKind::Access(AccessKind::Close(AccessMode::Write)),
				"/repo/.git/index"
			)),
			None
		);
	}

	#[test]
	fn classifies_source_close_write_as_full_index_refresh() {
		let classifier = EventClassifier::new(vec![root("/repo/service", Some("/repo"))]);

		assert_eq!(
			classifier.classify_event(&event(
				EventKind::Access(AccessKind::Close(AccessMode::Write)),
				"/repo/service/src/App.java"
			)),
			Some(StoreEvent::FullIndex)
		);
	}

	#[test]
	fn classifies_git_mutation_events() {
		let classifier = EventClassifier::new(vec![root("/repo/service", Some("/repo"))]);

		assert_eq!(
			classifier.classify_event(&event(
				EventKind::Modify(ModifyKind::Data(DataChange::Any)),
				"/repo/.git/HEAD"
			)),
			Some(StoreEvent::GitOverlay)
		);
	}

	#[test]
	fn ignores_noisy_git_internal_paths() {
		let classifier = EventClassifier::new(vec![root("/repo", Some("/repo"))]);

		assert_eq!(
			classifier.classify_paths(&[PathBuf::from("/repo/.git/index")]),
			None
		);
		assert_eq!(
			classifier.classify_paths(&[PathBuf::from("/repo/.git/logs/HEAD")]),
			None
		);
		assert_eq!(
			classifier.classify_paths(&[PathBuf::from("/repo/.git/index.lock")]),
			None
		);
		assert_eq!(
			classifier.classify_paths(&[PathBuf::from("/repo/.git/objects/info/commit-graph")]),
			None
		);
	}

	#[test]
	fn ignores_generated_cache_and_build_paths() {
		let classifier = EventClassifier::new(vec![root("/repo", Some("/repo"))]);

		assert_eq!(
			classifier.classify_paths(&[PathBuf::from("/repo/.code-moniker-cache/a")]),
			None
		);
		assert_eq!(
			classifier.classify_paths(&[PathBuf::from("/repo/target/debug/app")]),
			None
		);
		assert_eq!(
			classifier.classify_paths(&[PathBuf::from("/repo/build/classes/App.class")]),
			None
		);
	}

	#[test]
	fn ignores_custom_cache_path_inside_watched_root() {
		let mut watch_root = root("/repo", Some("/repo"));
		watch_root.ignored_paths = vec![PathBuf::from("/repo/.cm-cache")];
		let classifier = EventClassifier::new(vec![watch_root]);

		assert_eq!(
			classifier.classify_paths(&[PathBuf::from("/repo/.cm-cache/shard/graph")]),
			None
		);
	}

	#[test]
	fn coalesces_full_refresh_over_git_overlay_refresh() {
		assert_eq!(
			StoreEvent::GitOverlay.coalesce(StoreEvent::FullIndex),
			StoreEvent::FullIndex
		);
		assert_eq!(
			StoreEvent::GitOverlay.coalesce(StoreEvent::GitOverlay),
			StoreEvent::GitOverlay
		);
	}
}
