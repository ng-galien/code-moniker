use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};

use super::model::{WorkspaceLiveEvent, WorkspaceWatchRoot};
use super::roots::WorkspaceEventClassifier;

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
