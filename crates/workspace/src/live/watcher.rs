use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use notify::{Config, Event, RecursiveMode, Watcher};

use super::model::{WorkspaceLiveEvent, WorkspaceWatchRoot};
use super::roots::{WorkspaceEventClassifier, watch_paths_for};

pub struct LiveWorkspaceWatcher {
	_watcher: WorkspaceWatcherBackend,
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
		Self::start_with_backend(roots, WorkspaceWatcherBackendKind::Recommended, publish)
	}

	pub fn start_polling<F>(roots: Vec<WorkspaceWatchRoot>, publish: F) -> anyhow::Result<Self>
	where
		F: Fn(WorkspaceLiveEvent) + Send + 'static,
	{
		Self::start_with_backend(roots, WorkspaceWatcherBackendKind::Polling, publish)
	}

	fn start_with_backend<F>(
		roots: Vec<WorkspaceWatchRoot>,
		backend: WorkspaceWatcherBackendKind,
		publish: F,
	) -> anyhow::Result<Self>
	where
		F: Fn(WorkspaceLiveEvent) + Send + 'static,
	{
		let watch_targets = watch_paths_for(&roots);
		let classifier = WorkspaceEventClassifier::new(roots);
		let (tx, worker) = watcher_event_channel(publish);
		let mut watcher = new_watcher(backend, classifier.clone(), tx)?;
		let (watched_paths, warnings) = watch_target_paths(&mut watcher, &watch_targets);

		Ok(Self {
			_watcher: watcher,
			_worker: worker,
			watched_paths,
			warnings,
		})
	}

	pub fn status(&self) -> Option<String> {
		if self.watched_paths == 0 {
			if !self.warnings.is_empty() {
				return Some(format!(
					"live store disabled: no source path could be watched ({})",
					self.warnings.join("; ")
				));
			}
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

enum WorkspaceWatcherBackend {
	Recommended(notify::RecommendedWatcher),
	Polling(notify::PollWatcher),
}

impl WorkspaceWatcherBackend {
	fn watch(&mut self, path: &std::path::Path, mode: RecursiveMode) -> notify::Result<()> {
		match self {
			Self::Recommended(watcher) => watcher.watch(path, mode),
			Self::Polling(watcher) => watcher.watch(path, mode),
		}
	}
}

#[derive(Clone, Copy)]
enum WorkspaceWatcherBackendKind {
	Recommended,
	Polling,
}

fn watcher_event_channel<F>(publish: F) -> (mpsc::Sender<WorkspaceLiveEvent>, JoinHandle<()>)
where
	F: Fn(WorkspaceLiveEvent) + Send + 'static,
{
	let (tx, rx) = mpsc::channel();
	let worker = thread::spawn(move || publish_coalesced_events(rx, publish));
	(tx, worker)
}

fn new_watcher(
	backend: WorkspaceWatcherBackendKind,
	classifier: WorkspaceEventClassifier,
	tx: mpsc::Sender<WorkspaceLiveEvent>,
) -> anyhow::Result<WorkspaceWatcherBackend> {
	match backend {
		WorkspaceWatcherBackendKind::Recommended => Ok(WorkspaceWatcherBackend::Recommended(
			notify::RecommendedWatcher::new(
				move |event| publish_classified_event(&classifier, &tx, event),
				Config::default(),
			)?,
		)),
		WorkspaceWatcherBackendKind::Polling => {
			Ok(WorkspaceWatcherBackend::Polling(notify::PollWatcher::new(
				move |event| publish_classified_event(&classifier, &tx, event),
				polling_watcher_config(),
			)?))
		}
	}
}

fn publish_classified_event(
	classifier: &WorkspaceEventClassifier,
	tx: &mpsc::Sender<WorkspaceLiveEvent>,
	event: notify::Result<Event>,
) {
	let Ok(event) = event else {
		return;
	};
	if let Some(store_event) = classifier.classify_event(&event) {
		let _ = tx.send(store_event);
	}
}

fn polling_watcher_config() -> Config {
	Config::default()
		.with_poll_interval(Duration::from_millis(50))
		.with_compare_contents(true)
}

fn watch_target_paths(
	watcher: &mut WorkspaceWatcherBackend,
	targets: &[PathBuf],
) -> (usize, Vec<String>) {
	let mut warnings = Vec::new();
	let mut watched_paths = 0;
	for path in targets {
		match watcher.watch(path.as_path(), RecursiveMode::Recursive) {
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
