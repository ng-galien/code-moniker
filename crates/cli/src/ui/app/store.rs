use crate::ui::app::action::AppAction;
#[cfg(test)]
use crate::ui::app::state::LoadState;
use crate::ui::app::state::{AppState, CheckState};
use crate::ui::live::StoreEvent;
use crate::ui::reactive::{ReactiveStore, Reduce, Transition};
use crate::ui::runtime::{TaskResult, TaskSpec};
use crate::ui::store::IndexStore;

#[derive(Debug)]
pub(in crate::ui) struct AppStore {
	inner: ReactiveStore<AppState>,
}

impl AppStore {
	pub(in crate::ui) fn new() -> Self {
		Self {
			inner: ReactiveStore::new(AppState::new()),
		}
	}

	pub(in crate::ui) fn from_index_store(store: &impl IndexStore) -> Self {
		Self::from_stats(store.stats())
	}

	fn from_stats(stats: &crate::inspect::SessionStats) -> Self {
		let mut state = AppState::new();
		state.set_index_ready(stats.files, stats.defs, stats.refs);
		Self {
			inner: ReactiveStore::new(state),
		}
	}

	#[cfg(test)]
	fn state(&self) -> &AppState {
		self.inner.state()
	}

	pub(in crate::ui) fn register_task(&mut self, task: TaskSpec) -> TaskSpec {
		let work = task.work_kind();
		let generation = self.inner.select(|state| state.generation_for_work(work));
		let task = task.with_generation(generation);
		self.dispatch(&AppAction::TaskStarted {
			id: task.id(),
			work,
			generation,
		});
		task
	}

	pub(in crate::ui) fn complete_task(&mut self, result: &TaskResult) -> bool {
		let mut accepted = false;
		self.inner.reduce_with(|state| {
			accepted = state.complete_task(result);
			if accepted {
				Transition::changed("task-completed")
			} else {
				Transition::changed("task-ignored")
			}
		});
		accepted
	}

	pub(in crate::ui) fn status(&self) -> &str {
		self.inner.state().status()
	}

	pub(in crate::ui) fn set_status(&mut self, status: impl Into<String>) {
		let status = status.into();
		self.inner.reduce_with(|state| {
			state.set_status(status);
			Transition::changed("shell-status")
		});
	}

	pub(in crate::ui) fn append_status(&mut self, suffix: impl AsRef<str>) {
		self.inner.reduce_with(|state| {
			state.append_status(suffix);
			Transition::changed("shell-status-appended")
		});
	}

	pub(in crate::ui) fn check_state(&self) -> &CheckState {
		self.inner.state().check_state()
	}

	pub(in crate::ui) fn set_check_state(&mut self, check: CheckState) {
		self.inner.reduce_with(|state| {
			state.set_check_state(check);
			Transition::changed("check-state")
		});
	}

	pub(in crate::ui) fn dispatch(&mut self, action: &AppAction) -> &mut Transition {
		self.inner.dispatch(action)
	}
}

impl Default for AppStore {
	fn default() -> Self {
		Self::new()
	}
}

impl Reduce<&AppAction> for AppState {
	fn reduce(&mut self, action: &AppAction) -> Transition {
		match action {
			AppAction::Store(event) => {
				self.invalidate_for_store_event(*event);
				match event {
					StoreEvent::FullIndex => Transition::changed("full-index-invalidated"),
					StoreEvent::ChangeIndex => Transition::changed("git-change-invalidated"),
				}
			}
			AppAction::TaskStarted {
				id,
				work,
				generation,
			} => {
				self.start_task(*id, *work, *generation);
				Transition::changed("task-started")
			}
			AppAction::TaskCompleted(result) => {
				if self.complete_task(result) {
					Transition::changed("task-completed")
				} else {
					Transition::changed("task-ignored")
				}
			}
			AppAction::Ui(_) | AppAction::Clipboard(_) => Transition::unchanged("ui-local"),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ui::runtime::{TaskOutcome, TaskSpec, WorkKind};

	#[test]
	fn full_index_event_invalidates_domain_slices() {
		let mut store = AppStore::new();

		let transition = store.dispatch(&AppAction::Store(StoreEvent::FullIndex));

		assert!(transition.changed);
		assert_eq!(transition.reason, "full-index-invalidated");
		assert_eq!(store.state().generation, 1);
		assert_eq!(store.state().index.generation, 1);
		assert_eq!(store.state().git.generation, 1);
		assert_eq!(store.state().impact.generation, 1);
		assert_eq!(store.state().coverage.generation, 1);
		assert!(store.state().work.pending.contains(&WorkKind::ProjectLoad));
		assert!(store.state().work.pending.contains(&WorkKind::GraphIndex));
		assert!(store.state().work.pending.contains(&WorkKind::ImpactIndex));
		assert!(
			store
				.state()
				.work
				.pending
				.contains(&WorkKind::CoverageIndex)
		);
	}

	#[test]
	fn change_index_event_only_invalidates_git_and_panels() {
		let mut store = AppStore::new();

		store.dispatch(&AppAction::Store(StoreEvent::ChangeIndex));

		assert_eq!(store.state().generation, 1);
		assert_eq!(store.state().index.generation, 0);
		assert_eq!(store.state().git.generation, 1);
		assert_eq!(store.state().impact.generation, 1);
		assert_eq!(store.state().panels.generation, 1);
		assert!(
			store
				.state()
				.work
				.pending
				.contains(&WorkKind::GitChangeIndex)
		);
		assert!(store.state().work.pending.contains(&WorkKind::ImpactIndex));
		assert!(store.state().work.pending.contains(&WorkKind::PanelData));
		assert!(!store.state().work.pending.contains(&WorkKind::GraphIndex));
	}

	#[test]
	fn app_store_can_be_seeded_from_loaded_index() {
		let store = AppStore::from_stats(&crate::inspect::SessionStats {
			files: 3,
			defs: 5,
			refs: 8,
			..Default::default()
		});

		assert!(matches!(
			store.state().index.status,
			LoadState::Ready(ref summary)
				if summary.files == 3 && summary.defs == 5 && summary.refs == 8
		));
	}

	#[test]
	fn task_completion_is_recorded_in_state() {
		let mut store = AppStore::new();
		let spec = store.register_task(TaskSpec::noop("coverage lookup"));
		let id = spec.id();

		assert!(store.complete_task(&crate::ui::runtime::TaskResult {
			id,
			work: spec.work_kind(),
			generation: spec.generation(),
			label: "coverage lookup".to_string(),
			outcome: TaskOutcome::Completed("ok".to_string()),
		}));

		let task = store.state().last_task.as_ref().expect("task summary");
		assert_eq!(task.id, id);
		assert_eq!(task.label, "coverage lookup");
	}

	#[test]
	fn shell_status_is_owned_by_app_store() {
		let mut store = AppStore::new();

		store.set_status("loading index");
		store.append_status("watching git");

		assert_eq!(store.status(), "loading index; watching git");
		assert_eq!(store.state().shell.generation, 2);
	}

	#[test]
	fn check_state_is_owned_by_app_store() {
		let mut store = AppStore::new();

		store.set_check_state(CheckState::Ready(crate::inspect::CheckSummary {
			files_scanned: 7,
			files_with_violations: 1,
			total_violations: 3,
			errors: Vec::new(),
		}));

		assert!(matches!(
			store.check_state(),
			CheckState::Ready(summary)
				if summary.files_scanned == 7 && summary.total_violations == 3
		));
		assert_eq!(store.state().check.generation, 1);
	}

	#[test]
	fn stale_task_completion_is_ignored_after_generation_change() {
		let mut store = AppStore::new();
		let spec = store.register_task(TaskSpec::noop("panel lookup"));

		store.dispatch(&AppAction::Store(StoreEvent::FullIndex));

		let accepted = store.complete_task(&crate::ui::runtime::TaskResult {
			id: spec.id(),
			work: spec.work_kind(),
			generation: spec.generation(),
			label: "panel lookup".to_string(),
			outcome: TaskOutcome::Completed("late".to_string()),
		});

		assert!(!accepted);
		assert!(store.state().last_task.is_none());
		assert!(!store.state().work.running.contains_key(&spec.id()));
	}
}
