use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use notify::{Config, Event, RecursiveMode, Watcher};

use super::model::{WorkspaceLiveEvent, WorkspaceWatchRoot};
use super::roots::{WorkspaceEventClassifier, watch_paths_for};

pub struct LiveWorkspaceWatcher {
	_watchers: Vec<WorkspaceWatcherBackend>,
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
		Self::start_with_backend(roots, default_watcher_backend(), publish)
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
		let (watchers, warnings) = watch_target_paths(backend, &classifier, &tx, &watch_targets);
		let watched_paths = if watchers.is_empty() { 0 } else { watch_targets.len() - warnings.len() };
		ensure_all_watch_targets_registered(watch_targets.len(), watched_paths, &warnings)?;

		Ok(Self {
			_watchers: watchers,
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
	PollingProduction,
}

fn default_watcher_backend() -> WorkspaceWatcherBackendKind {
	WorkspaceWatcherBackendKind::Recommended
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
		WorkspaceWatcherBackendKind::Recommended => {
			let classifier_clone = classifier.clone();
			let tx_clone = tx.clone();
			match notify::RecommendedWatcher::new(
				move |event| publish_classified_event(&classifier_clone, &tx_clone, event),
				Config::default(),
			) {
				Ok(watcher) => Ok(WorkspaceWatcherBackend::Recommended(watcher)),
				Err(_) => {
					Ok(WorkspaceWatcherBackend::Polling(notify::PollWatcher::new(
						move |event| publish_classified_event(&classifier, &tx, event),
						Config::default().with_poll_interval(Duration::from_secs(1)),
					)?))
				}
			}
		}
		WorkspaceWatcherBackendKind::Polling => {
			Ok(WorkspaceWatcherBackend::Polling(notify::PollWatcher::new(
				move |event| publish_classified_event(&classifier, &tx, event),
				polling_watcher_config(),
			)?))
		}
		WorkspaceWatcherBackendKind::PollingProduction => {
			Ok(WorkspaceWatcherBackend::Polling(notify::PollWatcher::new(
				move |event| publish_classified_event(&classifier, &tx, event),
				Config::default().with_poll_interval(Duration::from_secs(1)),
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
	backend: WorkspaceWatcherBackendKind,
	classifier: &WorkspaceEventClassifier,
	tx: &mpsc::Sender<WorkspaceLiveEvent>,
	targets: &[PathBuf],
) -> (Vec<WorkspaceWatcherBackend>, Vec<String>) {
	let mut warnings = Vec::new();
	let mut watchers = Vec::new();
	
	if targets.is_empty() {
		return (watchers, warnings);
	}

	match new_watcher(backend, classifier.clone(), tx.clone()) {
		Ok(mut watcher) => {
			for path in targets {
				if let Err(error) = watcher.watch(path.as_path(), RecursiveMode::Recursive) {
					warnings.push(format!("{}: {error}", path.display()));
				}
			}
			watchers.push(watcher);
		}
		Err(error) => {
			warnings.push(format!("failed to initialize watcher: {error}"));
		}
	}

	(watchers, warnings)
}

fn ensure_all_watch_targets_registered(
	target_count: usize,
	watched_paths: usize,
	warnings: &[String],
) -> anyhow::Result<()> {
	anyhow::ensure!(
		target_count > 0 && watched_paths == target_count && warnings.is_empty(),
		"live watcher registration failed: {}",
		if warnings.is_empty() {
			format!("watched {watched_paths} of {target_count} source paths")
		} else {
			warnings.join("; ")
		}
	);
	Ok(())
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn partial_multi_root_registration_fails_closed() {
		let warnings = vec!["C:\\missing: access denied".to_string()];
		let error = ensure_all_watch_targets_registered(2, 1, &warnings)
			.expect_err("one missing root must fail the complete registration");

		assert!(error.to_string().contains("access denied"));
	}
}
