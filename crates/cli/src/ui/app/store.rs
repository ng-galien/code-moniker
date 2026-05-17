use crate::ui::app::action::{AppAction, ShellAction};
use crate::ui::app::state::{AppState, CheckState, ShellSlice};
use crate::ui::live::StoreEvent;
use crate::ui::reactive::{ReactiveStore, Reduce, Transition};
use crate::ui::runtime::{TaskResult, TaskSpec};
use crate::ui::store::navigation::{NavigationAction, NavigationState};
use crate::workspace::WorkspaceStore;

pub(in crate::ui) struct AppStore {
	inner: ReactiveStore<AppState>,
	workspace: Option<WorkspaceStore>,
}

impl AppStore {
	pub(in crate::ui) fn new() -> Self {
		Self {
			inner: ReactiveStore::new(AppState::new()),
			workspace: None,
		}
	}

	pub(in crate::ui) fn from_workspace_store(store: WorkspaceStore) -> Self {
		let mut app_store = Self::new();
		app_store.workspace = Some(store);
		app_store
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

	pub(in crate::ui) fn workspace(&self) -> &WorkspaceStore {
		self.workspace
			.as_ref()
			.expect("workspace store initialized")
	}

	pub(in crate::ui) fn workspace_mut(&mut self) -> &mut WorkspaceStore {
		self.workspace
			.as_mut()
			.expect("workspace store initialized")
	}

	pub(in crate::ui) fn replace_workspace(&mut self, store: WorkspaceStore) {
		self.workspace = Some(store);
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

	pub(in crate::ui) fn shell(&self) -> &ShellSlice {
		&self.inner.state().shell
	}

	pub(in crate::ui) fn set_status(&mut self, status: impl Into<String>) {
		self.dispatch(&AppAction::Shell(ShellAction::SetStatus(status.into())));
	}

	pub(in crate::ui) fn append_status(&mut self, suffix: impl AsRef<str>) {
		self.dispatch(&AppAction::Shell(ShellAction::AppendStatus(
			suffix.as_ref().to_string(),
		)));
	}

	pub(in crate::ui) fn check_state(&self) -> &CheckState {
		self.inner.state().check_state()
	}

	pub(in crate::ui) fn set_check_state(&mut self, check: CheckState) {
		self.dispatch(&AppAction::Shell(ShellAction::SetCheckState(check)));
	}

	pub(in crate::ui) fn set_navigation(&mut self, navigation: NavigationState) {
		self.inner.reduce_with(|state| {
			state.set_navigation(navigation);
			Transition::changed("navigation-init")
		});
	}

	pub(in crate::ui) fn navigation(&self) -> &NavigationState {
		self.inner
			.state()
			.navigation
			.state
			.as_ref()
			.expect("navigation initialized")
	}

	pub(in crate::ui) fn dispatch_navigation(
		&mut self,
		action: NavigationAction,
	) -> &mut Transition {
		self.inner.reduce_with(|state| {
			let Some(navigation) = state.navigation.state.as_mut() else {
				return Transition::unchanged("navigation-missing");
			};
			let transition = navigation.reduce(action);
			if transition.changed {
				state.generation += 1;
				state.navigation.generation += 1;
			}
			transition
		})
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
			AppAction::Ui(msg) => self.reduce_ui_msg(msg),
			AppAction::Shell(action) => self.reduce_shell_action(action),
			AppAction::Store(event) => {
				self.invalidate_for_store_event(*event);
				match event {
					StoreEvent::FullIndex => Transition::changed("full-index-invalidated"),
					StoreEvent::GitOverlay => Transition::changed("git-overlay-invalidated"),
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
			AppAction::Clipboard(_) => Transition::unchanged("ui-local"),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ui::app::{AppCommand, Effect, View};
	use crate::ui::events::Msg;
	use crate::ui::features::explorer::{ExplorerFeature, ROUTE_REFS};
	use crate::ui::navigator::{NavNode, NavNodeKind};
	use crate::ui::runtime::{TaskOutcome, TaskSpec, WorkKind};
	use crate::ui::store::ids::NodeId;

	#[test]
	fn full_index_event_invalidates_workspace_epochs() {
		let mut store = AppStore::new();

		let transition = store.dispatch(&AppAction::Store(StoreEvent::FullIndex));

		assert!(transition.changed);
		assert_eq!(transition.reason, "full-index-invalidated");
		assert_eq!(store.state().generation, 1);
		assert_eq!(store.state().generation_for_work(WorkKind::GraphIndex), 1);
		assert_eq!(store.state().generation_for_work(WorkKind::GitOverlay), 1);
		assert_eq!(store.state().generation_for_work(WorkKind::ImpactIndex), 1);
		assert_eq!(
			store.state().generation_for_work(WorkKind::CoverageIndex),
			1
		);
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
	fn git_overlay_event_only_invalidates_git_and_panels() {
		let mut store = AppStore::new();

		store.dispatch(&AppAction::Store(StoreEvent::GitOverlay));

		assert_eq!(store.state().generation, 1);
		assert_eq!(store.state().generation_for_work(WorkKind::GraphIndex), 0);
		assert_eq!(store.state().generation_for_work(WorkKind::GitOverlay), 1);
		assert_eq!(store.state().generation_for_work(WorkKind::ImpactIndex), 1);
		assert_eq!(store.state().generation_for_work(WorkKind::PanelData), 1);
		assert!(store.state().work.pending.contains(&WorkKind::GitOverlay));
		assert!(store.state().work.pending.contains(&WorkKind::ImpactIndex));
		assert!(store.state().work.pending.contains(&WorkKind::PanelData));
		assert!(!store.state().work.pending.contains(&WorkKind::GraphIndex));
	}

	#[test]
	fn app_store_keeps_workspace_data_outside_app_state() {
		let store = AppStore::new();

		assert_eq!(store.state().generation_for_work(WorkKind::GraphIndex), 0);
		assert!(store.state().work.pending.is_empty());
	}

	#[test]
	fn ui_messages_emit_commands_from_the_reducer() {
		let mut store = AppStore::new();

		let transition = store.dispatch(&AppAction::Ui(Msg::ApplySearch));

		assert!(!transition.changed);
		assert!(matches!(
			&transition.effects[0],
			Effect::RunCommand(AppCommand::ApplySearch)
		));
	}

	#[test]
	fn ui_view_messages_emit_navigation_effects_from_the_reducer() {
		let mut store = AppStore::new();

		let transition = store.dispatch(&AppAction::Ui(Msg::ShowView(View::Refs)));

		assert!(!transition.changed);
		assert!(matches!(
			&transition.effects[0],
			Effect::Navigate(route) if *route == ExplorerFeature::route(ROUTE_REFS)
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

		store.set_check_state(CheckState::Ready(crate::workspace::CheckSummary {
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
		assert_eq!(store.state().generation_for_work(WorkKind::CheckPanel), 1);
	}

	#[test]
	fn navigation_state_is_owned_by_app_store() {
		let mut store = AppStore::new();
		let mut root = NavNode::new(NodeId::root("test"), "root", NavNodeKind::Root);
		root.push_child(NavNode::new(
			NodeId::lang("test", "rs"),
			"rs",
			NavNodeKind::Lang,
		));
		root.push_child(NavNode::new(
			NodeId::lang("test", "ts"),
			"ts",
			NavNodeKind::Lang,
		));
		let change = NavNode::new(NodeId::root("change"), "root", NavNodeKind::Root);

		store.set_navigation(NavigationState::new(root, change));
		store.dispatch_navigation(NavigationAction::MoveDown);

		assert_eq!(store.navigation().selection(), 1);
		assert_eq!(store.state().navigation.generation, 2);
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
