use crate::ui::app::action::AppAction;
use crate::ui::app::state::{
	AppState, CheckState, FocusRegion, PanelNavigationState, ShellSlice, TaskCompletion,
	check_state, complete_task, generation_for_work, invalidate_for_store_event,
	reduce_header_search_debounced, reduce_shell_action, reduce_ui_msg, set_navigation, start_task,
	status,
};
use crate::ui::async_task::{TaskResult, TaskSpec};
use crate::ui::store::ids::NodeId;
use crate::ui::store::navigation::{
	NavigationAction, NavigationPane, NavigationState, navigation_last_notice,
	navigation_pane_view, navigation_primary_view,
};
use crate::ui::store::reducer::{Reduce, ReducerStore, Reduction, Transition};
use crate::ui::store::tree_pane_action::TreePaneNotice;
use code_moniker_workspace::live::WorkspaceLiveEvent;

pub(in crate::ui) struct AppStore {
	inner: ReducerStore<AppState>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct NavigationDispatchOutcome {
	pub(in crate::ui) changed: bool,
	pub(in crate::ui) selection_changed: bool,
	pub(in crate::ui) primary_selection_changed: bool,
	pub(in crate::ui) notice: TreePaneNotice,
}

impl AppStore {
	pub(in crate::ui) fn new() -> Self {
		Self {
			inner: ReducerStore::new(AppState::default()),
		}
	}

	pub(in crate::ui) fn register_task(&mut self, task: TaskSpec) -> TaskSpec {
		let work = task.work_kind();
		let generation = self.inner.select(|state| generation_for_work(state, work));
		let task = task.with_generation(generation);
		self.dispatch(&AppAction::TaskStarted {
			id: task.id(),
			work,
			generation,
		});
		task
	}

	pub(in crate::ui) fn dispatch(&mut self, action: &AppAction) -> &mut Transition {
		self.inner.dispatch(action)
	}

	pub(in crate::ui) fn complete_task(&mut self, result: &TaskResult) -> TaskCompletion {
		let (_, completion) = self.inner.reduce_with_outcome(|state| {
			let completion = complete_task(state, result);
			Transition::changed().with_outcome(completion)
		});
		completion
	}

	pub(in crate::ui) fn status(&self) -> &str {
		status(self.inner.state())
	}

	pub(in crate::ui) fn shell(&self) -> &ShellSlice {
		&self.inner.state().shell
	}

	pub(in crate::ui) fn check_state(&self) -> &CheckState {
		check_state(self.inner.state())
	}

	pub(in crate::ui) fn set_navigation(&mut self, navigation: NavigationState) {
		self.inner.reduce_with(|state| {
			set_navigation(state, navigation);
			Transition::changed()
		});
	}

	pub(in crate::ui) fn navigation(&self) -> &NavigationState {
		self.inner
			.state()
			.navigation
			.state
			.as_ref()
			.unwrap_or_else(|| {
				panic!("navigation initialized");
			})
	}

	pub(in crate::ui) fn dispatch_navigation(
		&mut self,
		action: NavigationAction,
	) -> NavigationDispatchOutcome {
		let (_, outcome) = self
			.inner
			.reduce_with_outcome(|state| reduce_navigation_action(state, action));
		outcome
	}
}

fn reduce_navigation_action(
	state: &mut AppState,
	action: NavigationAction,
) -> Reduction<NavigationDispatchOutcome> {
	let Some(navigation) = state.navigation.state.as_mut() else {
		return Transition::unchanged().with_outcome(NavigationDispatchOutcome {
			changed: false,
			selection_changed: false,
			primary_selection_changed: false,
			notice: TreePaneNotice::Noop,
		});
	};
	let active_pane = active_navigation_pane(state.shell.focus_region);
	let before = selected_nav_key(navigation, active_pane);
	let before_primary = navigation_primary_view(navigation)
		.selected_row()
		.map(|row| row.key.clone());
	let transition = navigation.reduce(action);
	let changed = transition.changed;
	if changed {
		state.generation += 1;
		state.navigation.generation += 1;
	}
	let selection_changed = changed && before != selected_nav_key(navigation, active_pane);
	if selection_changed {
		state.shell.panel_navigation = PanelNavigationState::default();
		state.shell.generation += 1;
	}
	transition.with_outcome(NavigationDispatchOutcome {
		changed,
		selection_changed,
		primary_selection_changed: changed
			&& before_primary
				!= navigation_primary_view(navigation)
					.selected_row()
					.map(|row| row.key.clone()),
		notice: navigation_last_notice(navigation).clone(),
	})
}

fn active_navigation_pane(focus: FocusRegion) -> NavigationPane {
	if focus == FocusRegion::UsageLens {
		NavigationPane::UsageLens
	} else {
		NavigationPane::Primary
	}
}

fn selected_nav_key(navigation: &NavigationState, pane: NavigationPane) -> Option<NodeId> {
	navigation_pane_view(navigation, pane)
		.and_then(|pane| pane.selected_row())
		.map(|row| row.key.clone())
}

impl Default for AppStore {
	fn default() -> Self {
		Self::new()
	}
}

impl Reduce<&AppAction> for AppState {
	fn reduce(&mut self, action: &AppAction) -> Transition {
		match action {
			AppAction::Ui(msg) => reduce_ui_msg(self, msg),
			AppAction::HeaderSearchDebounced(generation) => {
				reduce_header_search_debounced(self, *generation)
			}
			AppAction::UsageLensDebounced(_) => Transition::unchanged(),
			AppAction::Shell(action) => reduce_shell_action(self, action),
			AppAction::Store(event) => {
				invalidate_for_store_event(self, event);
				store_event_transition(event)
			}
			AppAction::TaskStarted {
				id,
				work,
				generation,
			} => {
				start_task(self, *id, *work, *generation);
				Transition::changed()
			}
			AppAction::TaskCompleted(result) => {
				complete_task(self, result);
				Transition::changed()
			}
			AppAction::Clipboard(_) => Transition::unchanged(),
		}
	}
}

fn store_event_transition(_event: &WorkspaceLiveEvent) -> Transition {
	Transition::changed()
}
