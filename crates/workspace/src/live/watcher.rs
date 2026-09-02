// code-moniker: ignore-file[smell-harmonious-method-size]
// Watcher startup owns registration while runtime-state accessors stay intentionally small.
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[cfg(target_os = "macos")]
use notify::RecursiveMode;
use notify::{Config, ErrorKind, Event, PathsMut, Watcher};

use super::model::{WorkspaceLiveEvent, WorkspaceWatchRoot};
use super::roots::{
	WorkspaceEventClassifier, WorkspaceWatchTarget, resolve_git_metadata_dirs,
	watch_targets_for_resolved_git_dirs,
};
#[cfg(target_os = "macos")]
use super::roots::{git_watch_targets_for_resolved_git_dirs, notes_watch_targets_for};

pub struct LiveWorkspaceWatcher {
	_watcher: WorkspaceWatcherBackend,
	_worker: JoinHandle<()>,
	watched_paths: usize,
	warnings: Vec<String>,
	runtime_error_reported: Arc<AtomicBool>,
	polling: bool,
	runtime_fallback_allowed: bool,
}

const LIVE_EVENT_DEBOUNCE: Duration = Duration::from_millis(50);

impl LiveWorkspaceWatcher {
	pub fn start<F>(roots: Vec<WorkspaceWatchRoot>, publish: F) -> anyhow::Result<Self>
	where
		F: Fn(WorkspaceLiveEvent) + Send + 'static,
	{
		Self::start_with_backend(roots, configured_watcher_backend()?, publish)
	}

	pub fn start_polling<F>(roots: Vec<WorkspaceWatchRoot>, publish: F) -> anyhow::Result<Self>
	where
		F: Fn(WorkspaceLiveEvent) + Send + 'static,
	{
		Self::start_with_backend(roots, WorkspaceWatcherBackendKind::Polling, publish)
	}

	pub fn start_production_polling<F>(
		roots: Vec<WorkspaceWatchRoot>,
		publish: F,
	) -> anyhow::Result<Self>
	where
		F: Fn(WorkspaceLiveEvent) + Send + 'static,
	{
		Self::start_with_backend(
			roots,
			WorkspaceWatcherBackendKind::PollingProduction,
			publish,
		)
	}

	fn start_with_backend<F>(
		roots: Vec<WorkspaceWatchRoot>,
		backend: WorkspaceWatcherBackendKind,
		publish: F,
	) -> anyhow::Result<Self>
	where
		F: Fn(WorkspaceLiveEvent) + Send + 'static,
	{
		let git_dirs = resolve_git_metadata_dirs(&roots)?;
		let watch_plan = WatchRegistrationPlan::new(&roots, backend, &git_dirs)?;
		anyhow::ensure!(
			!watch_plan.primary.is_empty(),
			"live watcher registration failed: no non-ignored directory to watch"
		);
		let classifier = WorkspaceEventClassifier::new_with_resolved_git_dirs(
			roots,
			watch_plan.fallback_targets(),
			git_dirs,
		);
		let (tx, worker) = watcher_event_channel(publish);
		let runtime_error_reported = Arc::new(AtomicBool::new(false));
		let registration = register_watcher(
			backend,
			&classifier,
			&tx,
			&watch_plan.primary,
			watch_plan.fallback_targets(),
			&runtime_error_reported,
		)?;

		Ok(Self {
			_watcher: registration.watcher,
			_worker: worker,
			watched_paths: registration.watched_paths,
			warnings: registration.warnings,
			runtime_error_reported,
			polling: registration.polling,
			runtime_fallback_allowed: backend == WorkspaceWatcherBackendKind::Recommended,
		})
	}

	pub fn runtime_failed(&self) -> bool {
		self.runtime_error_reported.load(Ordering::Acquire)
	}

	pub fn uses_polling(&self) -> bool {
		self.polling
	}

	pub fn runtime_fallback_allowed(&self) -> bool {
		self.runtime_fallback_allowed
	}

	pub fn status(&self) -> Option<String> {
		if self.warnings.is_empty() {
			return Some(format!(
				"live store watching {} path(s)",
				self.watched_paths
			));
		}
		Some(format!(
			"live store watching {} path(s), {} warning(s): {}",
			self.watched_paths,
			self.warnings.len(),
			self.warnings.join("; ")
		))
	}
}

enum WorkspaceWatcherBackend {
	Recommended(notify::RecommendedWatcher),
	Polling(notify::PollWatcher),
}

impl WorkspaceWatcherBackend {
	fn register(&mut self, targets: &[WorkspaceWatchTarget]) -> notify::Result<()> {
		match self {
			Self::Recommended(watcher) => register_paths(watcher.paths_mut(), targets),
			Self::Polling(watcher) => register_paths(watcher.paths_mut(), targets),
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkspaceWatcherBackendKind {
	Recommended,
	Native,
	Polling,
	PollingProduction,
}

struct WatchRegistrationPlan {
	primary: Vec<WorkspaceWatchTarget>,
	polling_fallback: Vec<WorkspaceWatchTarget>,
}

impl WatchRegistrationPlan {
	fn new(
		roots: &[WorkspaceWatchRoot],
		backend: WorkspaceWatcherBackendKind,
		git_dirs: &BTreeSet<std::path::PathBuf>,
	) -> anyhow::Result<Self> {
		let primary = watch_targets_for_backend(roots, backend, git_dirs)?;
		let polling_fallback =
			if cfg!(target_os = "macos") && backend == WorkspaceWatcherBackendKind::Recommended {
				watch_targets_for_resolved_git_dirs(roots, git_dirs)
			} else {
				Vec::new()
			};
		Ok(Self {
			primary,
			polling_fallback,
		})
	}

	fn fallback_targets(&self) -> &[WorkspaceWatchTarget] {
		if self.polling_fallback.is_empty() {
			&self.primary
		} else {
			&self.polling_fallback
		}
	}
}

fn configured_watcher_backend() -> anyhow::Result<WorkspaceWatcherBackendKind> {
	watcher_backend_from_value(
		std::env::var("CODE_MONIKER_WATCHER_BACKEND")
			.ok()
			.as_deref(),
	)
}

fn watcher_backend_from_value(value: Option<&str>) -> anyhow::Result<WorkspaceWatcherBackendKind> {
	match value.unwrap_or("auto") {
		"auto" => Ok(WorkspaceWatcherBackendKind::Recommended),
		"native" => Ok(WorkspaceWatcherBackendKind::Native),
		"poll" | "polling" => Ok(WorkspaceWatcherBackendKind::PollingProduction),
		other => anyhow::bail!(
			"unknown CODE_MONIKER_WATCHER_BACKEND value `{other}`; expected auto, native, or poll"
		),
	}
}

#[cfg(target_os = "macos")]
fn watch_targets_for_backend(
	roots: &[WorkspaceWatchRoot],
	backend: WorkspaceWatcherBackendKind,
	git_dirs: &BTreeSet<std::path::PathBuf>,
) -> anyhow::Result<Vec<WorkspaceWatchTarget>> {
	if matches!(
		backend,
		WorkspaceWatcherBackendKind::Recommended | WorkspaceWatcherBackendKind::Native
	) {
		let mut paths = std::collections::BTreeSet::new();
		let mut targets: Vec<_> = roots
			.iter()
			.filter_map(|root| {
				let path = root.path.clone();
				paths.insert(path.clone()).then_some(WorkspaceWatchTarget {
					path,
					mode: RecursiveMode::Recursive,
				})
			})
			.collect();
		for target in git_watch_targets_for_resolved_git_dirs(git_dirs)
			.into_iter()
			.chain(notes_watch_targets_for(roots))
		{
			if roots.iter().any(|root| target.path.starts_with(&root.path)) {
				continue;
			}
			if paths.insert(target.path.clone()) {
				targets.push(target);
			}
		}
		return Ok(targets);
	}
	Ok(watch_targets_for_resolved_git_dirs(roots, git_dirs))
}

#[cfg(not(target_os = "macos"))]
fn watch_targets_for_backend(
	roots: &[WorkspaceWatchRoot],
	_backend: WorkspaceWatcherBackendKind,
	git_dirs: &BTreeSet<std::path::PathBuf>,
) -> anyhow::Result<Vec<WorkspaceWatchTarget>> {
	Ok(watch_targets_for_resolved_git_dirs(roots, git_dirs))
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
	runtime_error_reported: Arc<AtomicBool>,
) -> notify::Result<WorkspaceWatcherBackend> {
	match backend {
		WorkspaceWatcherBackendKind::Recommended | WorkspaceWatcherBackendKind::Native => Ok(
			WorkspaceWatcherBackend::Recommended(notify::RecommendedWatcher::new(
				watcher_event_handler(classifier, tx, runtime_error_reported),
				Config::default(),
			)?),
		),
		WorkspaceWatcherBackendKind::Polling => {
			Ok(WorkspaceWatcherBackend::Polling(notify::PollWatcher::new(
				watcher_event_handler(classifier, tx, runtime_error_reported),
				polling_watcher_config(),
			)?))
		}
		WorkspaceWatcherBackendKind::PollingProduction => {
			Ok(WorkspaceWatcherBackend::Polling(notify::PollWatcher::new(
				watcher_event_handler(classifier, tx, runtime_error_reported),
				production_polling_watcher_config(),
			)?))
		}
	}
}

fn watcher_event_handler(
	classifier: WorkspaceEventClassifier,
	tx: mpsc::Sender<WorkspaceLiveEvent>,
	runtime_error_reported: Arc<AtomicBool>,
) -> impl FnMut(notify::Result<Event>) + Send + 'static {
	move |event| publish_classified_event(&classifier, &tx, &runtime_error_reported, event)
}

struct WatcherRegistration {
	watcher: WorkspaceWatcherBackend,
	watched_paths: usize,
	warnings: Vec<String>,
	polling: bool,
}

enum WatcherStartError {
	Initialize(notify::Error),
	Register(notify::Error),
}

fn register_watcher(
	backend: WorkspaceWatcherBackendKind,
	classifier: &WorkspaceEventClassifier,
	tx: &mpsc::Sender<WorkspaceLiveEvent>,
	targets: &[WorkspaceWatchTarget],
	fallback_targets: &[WorkspaceWatchTarget],
	runtime_error_reported: &Arc<AtomicBool>,
) -> anyhow::Result<WatcherRegistration> {
	match initialize_and_register(backend, classifier, tx, targets, runtime_error_reported) {
		Ok(watcher) => {
			let warnings = if backend == WorkspaceWatcherBackendKind::PollingProduction {
				let warning =
					"polling watcher explicitly selected with a five-second interval".to_string();
				eprintln!("live watcher warning: {warning}");
				vec![warning]
			} else {
				Vec::new()
			};
			Ok(WatcherRegistration {
				watcher,
				watched_paths: targets.len(),
				warnings,
				polling: matches!(
					backend,
					WorkspaceWatcherBackendKind::Polling
						| WorkspaceWatcherBackendKind::PollingProduction
				),
			})
		}
		Err(primary) if should_fallback_to_polling(backend, &primary) => {
			let warning = format!(
				"native watcher unavailable ({}); using five-second polling fallback",
				start_error_message(&primary)
			);
			eprintln!("live watcher warning: {warning}");
			let watcher = initialize_and_register(
				WorkspaceWatcherBackendKind::PollingProduction,
				classifier,
				tx,
				fallback_targets,
				runtime_error_reported,
			)
			.map_err(|fallback| {
				anyhow::anyhow!(
					"live watcher registration failed: {warning}; polling fallback also failed ({})",
					start_error_message(&fallback)
				)
			})?;
			Ok(WatcherRegistration {
				watcher,
				watched_paths: fallback_targets.len(),
				warnings: vec![warning],
				polling: true,
			})
		}
		Err(error) => Err(anyhow::anyhow!(
			"live watcher registration failed: {}",
			start_error_message(&error)
		)),
	}
}

fn initialize_and_register(
	backend: WorkspaceWatcherBackendKind,
	classifier: &WorkspaceEventClassifier,
	tx: &mpsc::Sender<WorkspaceLiveEvent>,
	targets: &[WorkspaceWatchTarget],
	runtime_error_reported: &Arc<AtomicBool>,
) -> Result<WorkspaceWatcherBackend, WatcherStartError> {
	let mut watcher = new_watcher(
		backend,
		classifier.clone(),
		tx.clone(),
		runtime_error_reported.clone(),
	)
	.map_err(WatcherStartError::Initialize)?;
	watcher
		.register(targets)
		.map_err(WatcherStartError::Register)?;
	Ok(watcher)
}

fn should_fallback_to_polling(
	backend: WorkspaceWatcherBackendKind,
	error: &WatcherStartError,
) -> bool {
	backend == WorkspaceWatcherBackendKind::Recommended
		&& matches!(
			error,
			WatcherStartError::Initialize(_)
				| WatcherStartError::Register(notify::Error {
					kind: ErrorKind::MaxFilesWatch,
					..
				})
		)
}

fn start_error_message(error: &WatcherStartError) -> String {
	match error {
		WatcherStartError::Initialize(error) => format!("initialization: {error}"),
		WatcherStartError::Register(error) => format!("path registration: {error}"),
	}
}

fn register_paths(
	mut paths: Box<dyn PathsMut + '_>,
	targets: &[WorkspaceWatchTarget],
) -> notify::Result<()> {
	let target_paths = targets
		.iter()
		.map(|target| target.path.as_path())
		.collect::<std::collections::BTreeSet<_>>();
	for target in targets {
		if let Err(error) = paths.add(&target.path, target.mode) {
			let vanished_child = matches!(error.kind, ErrorKind::PathNotFound)
				&& !target.path.exists()
				&& target
					.path
					.parent()
					.is_some_and(|parent| target_paths.contains(parent));
			if vanished_child {
				continue;
			}
			return Err(if error.paths.is_empty() {
				error.add_path(target.path.clone())
			} else {
				error
			});
		}
	}
	paths.commit()
}

fn publish_classified_event(
	classifier: &WorkspaceEventClassifier,
	tx: &mpsc::Sender<WorkspaceLiveEvent>,
	runtime_error_reported: &AtomicBool,
	event: notify::Result<Event>,
) {
	match event {
		Ok(event) => {
			if let Some(store_event) = classifier.classify_event(&event) {
				let _ = tx.send(store_event);
			}
		}
		Err(error) => {
			eprintln!("live watcher error: {error}");
			if !runtime_error_reported.swap(true, Ordering::AcqRel) {
				let _ = tx.send(WorkspaceLiveEvent::RescanRequired);
			}
		}
	}
}

fn polling_watcher_config() -> Config {
	Config::default()
		.with_poll_interval(Duration::from_millis(50))
		.with_compare_contents(true)
}

fn production_polling_watcher_config() -> Config {
	Config::default().with_poll_interval(Duration::from_secs(5))
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
	use std::path::PathBuf;

	use super::*;

	#[test]
	fn native_watch_limit_failure_uses_polling_fallback() {
		let error = WatcherStartError::Register(notify::Error::new(ErrorKind::MaxFilesWatch));

		assert!(should_fallback_to_polling(
			WorkspaceWatcherBackendKind::Recommended,
			&error
		));
		assert!(!should_fallback_to_polling(
			WorkspaceWatcherBackendKind::Polling,
			&error
		));
	}

	#[test]
	fn missing_path_does_not_hide_coverage_failure_behind_polling() {
		let error = WatcherStartError::Register(
			notify::Error::path_not_found().add_path(PathBuf::from("/missing")),
		);

		assert!(!should_fallback_to_polling(
			WorkspaceWatcherBackendKind::Recommended,
			&error
		));
	}

	#[test]
	fn watcher_errors_request_a_rescan_when_the_native_limit_is_reached() {
		let classifier = WorkspaceEventClassifier::new(Vec::new());
		let (tx, rx) = mpsc::channel();
		let runtime_error_reported = AtomicBool::new(false);

		publish_classified_event(
			&classifier,
			&tx,
			&runtime_error_reported,
			Err(notify::Error::new(ErrorKind::MaxFilesWatch)),
		);

		assert_eq!(rx.recv().unwrap(), WorkspaceLiveEvent::RescanRequired);
	}

	#[test]
	fn watcher_runtime_errors_request_only_one_rebuild_per_backend_instance() {
		let classifier = WorkspaceEventClassifier::new(Vec::new());
		let (tx, rx) = mpsc::channel();
		let runtime_error_reported = AtomicBool::new(false);

		for _ in 0..2 {
			publish_classified_event(
				&classifier,
				&tx,
				&runtime_error_reported,
				Err(notify::Error::generic("backend failed")),
			);
		}

		assert_eq!(rx.recv().unwrap(), WorkspaceLiveEvent::RescanRequired);
		assert!(rx.try_recv().is_err());
	}

	#[test]
	fn watcher_backend_configuration_selects_existing_backends() {
		assert_eq!(
			watcher_backend_from_value(None).unwrap(),
			WorkspaceWatcherBackendKind::Recommended
		);
		assert_eq!(
			watcher_backend_from_value(Some("native")).unwrap(),
			WorkspaceWatcherBackendKind::Native
		);
		assert_eq!(
			watcher_backend_from_value(Some("poll")).unwrap(),
			WorkspaceWatcherBackendKind::PollingProduction
		);
		assert!(watcher_backend_from_value(Some("other")).is_err());
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn fsevents_adds_bounded_git_targets_when_the_workspace_is_a_subdirectory() {
		let temp = tempfile::tempdir().unwrap();
		let workspace = temp.path().join("module");
		std::fs::create_dir_all(temp.path().join(".git/refs/heads")).unwrap();
		std::fs::create_dir(&workspace).unwrap();
		let roots = vec![WorkspaceWatchRoot {
			path: workspace.clone(),
			git_root: Some(temp.path().to_path_buf()),
			ignored_paths: Vec::new(),
			notes_path: None,
		}];

		let git_dirs = resolve_git_metadata_dirs(&roots).unwrap();
		let targets =
			watch_targets_for_backend(&roots, WorkspaceWatcherBackendKind::Recommended, &git_dirs)
				.unwrap();

		assert!(
			targets.iter().any(|target| {
				target.path == workspace && target.mode == RecursiveMode::Recursive
			})
		);
		assert!(
			targets.iter().any(|target| {
				target.path == temp.path().join(".git").canonicalize().unwrap()
					&& target.mode == RecursiveMode::NonRecursive
			}),
			"targets: {targets:?}"
		);
		assert!(targets.iter().any(|target| {
			target.path == temp.path().join(".git/refs/heads").canonicalize().unwrap()
				&& target.mode == RecursiveMode::NonRecursive
		}));
	}
}
