// code-moniker: ignore-file[smell-god-type-local-metrics]
// TODO(smell): split AppStore shell-state reduction from workspace ownership and transition application before enabling this guardrail here.
use crate::ui::app::action::AppAction;
use crate::ui::app::state::{AppState, CheckState, ShellSlice, TaskCompletion};
use crate::ui::async_task::{TaskResult, TaskSpec};
use crate::ui::live::StoreEvent;
use crate::ui::store::navigation::{NavigationAction, NavigationState};
use crate::ui::store::reducer::{Reduce, ReducerStore, Transition};
use crate::workspace::WorkspaceStore;

pub(in crate::ui) struct AppStore {
	inner: ReducerStore<AppState>,
	workspace: Option<WorkspaceStore>,
}

impl AppStore {
	pub(in crate::ui) fn new() -> Self {
		Self {
			inner: ReducerStore::new(AppState::new()),
			workspace: None,
		}
	}

	pub(in crate::ui) fn from_workspace_store(store: WorkspaceStore) -> Self {
		let mut app_store = Self::new();
		app_store.workspace = Some(store);
		app_store
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

	pub(in crate::ui) fn complete_task(&mut self, result: &TaskResult) -> TaskCompletion {
		let (_, completion) = self.inner.reduce_with_outcome(|state| {
			let completion = state.complete_task(result);
			Transition::changed().with_outcome(completion)
		});
		completion
	}

	pub(in crate::ui) fn status(&self) -> &str {
		self.inner.state().status()
	}

	pub(in crate::ui) fn shell(&self) -> &ShellSlice {
		&self.inner.state().shell
	}

	pub(in crate::ui) fn check_state(&self) -> &CheckState {
		self.inner.state().check_state()
	}

	pub(in crate::ui) fn set_navigation(&mut self, navigation: NavigationState) {
		self.inner.reduce_with(|state| {
			state.set_navigation(navigation);
			Transition::changed()
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
				return Transition::unchanged();
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
			AppAction::HeaderSearchDebounced(generation) => {
				self.reduce_header_search_debounced(*generation)
			}
			AppAction::Shell(action) => self.reduce_shell_action(action),
			AppAction::Store(event) => {
				self.invalidate_for_store_event(*event);
				match event {
					StoreEvent::FullIndex => Transition::changed(),
					StoreEvent::GitOverlay => Transition::changed(),
				}
			}
			AppAction::TaskStarted {
				id,
				work,
				generation,
			} => {
				self.start_task(*id, *work, *generation);
				Transition::changed()
			}
			AppAction::TaskCompleted(result) => {
				self.complete_task(result);
				Transition::changed()
			}
			AppAction::Clipboard(_) => Transition::unchanged(),
		}
	}
}
