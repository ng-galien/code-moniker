// code-moniker: ignore-file[smell-clone-reflex]
// Runtime task wiring clones handles and snapshots across async UI boundaries.
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use crossterm::event::KeyEvent;

use crate::ui::app::App;
use crate::ui::app::{
	AppAction, CheckState, Effect, FocusCycle, FocusRegion, PanelPolicy, ShellAction,
	TaskCompletion, View, apply_file_catalog_store, apply_navigation, apply_reloaded_store,
	close_note_editor, close_panel_tree_node, close_selected_nav, copy_panel_snapshot,
	cycle_focus_region, cycle_note_editor_kind, cycle_note_editor_status, delete_note_from_editor,
	dispatch_shell, edit_note_editor, ensure_active_panel_selection, handle_store_event,
	handle_store_event_sync, has_clearable_scope, move_note_editor_field, move_panel_selection,
	move_panel_to_edge, open_note_editor, open_panel_tree_node, open_selected_nav,
	refresh_workspace_on_demand, save_note_from_editor, set_view, show_notes_lens,
	sync_contextual_view, toggle_panel_tree_node, toggle_selected_nav,
};
use crate::ui::async_task::{TaskOutcome, TaskResult, TaskRunner, TaskSpec};
use crate::ui::clipboard;
use crate::ui::events::{HeaderSearchFocus, Msg, NoteMsg, UiMode, key_to_msg};
use crate::ui::shell::ShellEvent;
use crate::ui::store::navigation::{NavigationAction, NavigationPane};
use crate::ui::store::tree_pane_action::TreePaneAction;
use code_moniker_workspace::live::{WorkspaceLiveEvent, WorkspaceWatchRoot};

const HEADER_SEARCH_DEBOUNCE_MS: u64 = 180;
const USAGE_LENS_DEBOUNCE_MS: u64 = 120;

pub(in crate::ui) fn run_check(app: &mut App) {
	set_view(app, View::Check, PanelPolicy::Manual);
	if let Ok(context) = crate::ui::app::store_check_context(app) {
		let task = TaskSpec::run_check(
			context,
			app.config.rules.clone(),
			app.config.profile.clone(),
			app.config.scheme.clone(),
		);
		if queue_task(app, task) {
			crate::ui::app::set_status(app, "check queued in background");
			return;
		}
	}
	match crate::ui::app::store_check_context(app).and_then(|context| {
		context.check_summary(
			&app.config.rules,
			app.config.profile.as_deref(),
			&app.config.scheme,
		)
	}) {
		Ok(summary) => {
			crate::ui::app::set_status(
				app,
				format!(
					"check complete: {} violation(s) across {} file(s)",
					summary.total_violations, summary.files_with_violations
				),
			);
			crate::ui::app::set_check_state(app, CheckState::Ready(summary));
		}
		Err(e) => {
			crate::ui::app::set_status(app, "check failed");
			crate::ui::app::set_check_state(app, CheckState::Error(e.to_string()));
		}
	}
}

pub(in crate::ui) fn set_event_sender(app: &mut App, tx: Sender<ShellEvent>) {
	app.runtime.event_tx = Some(tx);
}

pub(in crate::ui) fn queue_startup_load(app: &mut App) {
	if !app.runtime.startup_load_pending {
		return;
	}
	app.runtime.startup_load_pending = false;
	if queue_task(
		app,
		TaskSpec::load_file_catalog(crate::ui::app::store_options(app)),
	) {
		crate::ui::app::set_status(app, "loading file tree in background");
	} else {
		handle_store_event_sync(app, WorkspaceLiveEvent::RescanRequired);
	}
}

pub(in crate::ui) fn take_watch_roots_update(app: &mut App) -> Option<Vec<WorkspaceWatchRoot>> {
	app.runtime.watch_roots_update.take()
}

fn handle_clipboard_result(app: &mut App, result: clipboard::ClipboardResult) {
	match result.result {
		Ok(()) => {
			crate::ui::app::set_status(
				app,
				format!("copied {} snapshot to clipboard", result.component),
			);
		}
		Err(error) => {
			crate::ui::app::set_status(
				app,
				format!("clipboard copy failed for {}: {error}", result.component),
			);
		}
	}
}

fn handle_task_result(app: &mut App, result: TaskResult) {
	match result.outcome {
		TaskOutcome::FileCatalogLoaded(store) => {
			let (store, cache, options) = *store;
			let symbol_cache = cache.clone();
			let symbol_options = options.clone();
			let catalog_seed = store.queries().snapshot_arc();
			crate::ui::app::replace_store(app, store, cache, options);
			let _ = crate::ui::app::reload_notes(app);
			apply_file_catalog_store(app, "file tree ready".to_string());
			let task = catalog_seed.map_or_else(
				|| TaskSpec::load_symbol_index(crate::ui::app::store_options(app)),
				|snapshot| {
					TaskSpec::load_symbol_index_from_catalog(symbol_options, symbol_cache, snapshot)
				},
			);
			if queue_task(app, task) {
				crate::ui::app::set_status(app, "file tree ready; loading symbols in background");
			}
		}
		TaskOutcome::SymbolIndexLoaded {
			workspace,
			linkage_seed,
		} => {
			let (store, cache, options) = *workspace;
			let linkage_cache = cache.clone();
			let linkage_options = options.clone();
			crate::ui::app::replace_store(app, store, cache, options);
			let _ = crate::ui::app::reload_notes(app);
			apply_reloaded_store(app, "symbols ready; linkage pending".to_string());
			if queue_task(
				app,
				TaskSpec::resolve_linkage(linkage_options, linkage_cache, linkage_seed),
			) {
				crate::ui::app::set_status(app, "symbols ready; resolving linkage in background");
			}
		}
		TaskOutcome::LinkageResolved(store) => {
			let (store, cache, options) = *store;
			crate::ui::app::replace_store(app, store, cache, options);
			let _ = crate::ui::app::reload_notes(app);
			apply_reloaded_store(app, format!("{} completed", result.label));
		}
		TaskOutcome::LiveWorkspaceRefreshed {
			workspace,
			reload_notes,
		} => {
			let (store, cache, options) = *workspace;
			crate::ui::app::replace_store(app, store, cache, options);
			if reload_notes {
				let _ = crate::ui::app::reload_notes(app);
			}
			apply_reloaded_store(app, format!("{} completed", result.label));
		}
		TaskOutcome::CheckCompleted(summary) => {
			crate::ui::app::set_status(
				app,
				format!(
					"check complete: {} violation(s) across {} file(s)",
					summary.total_violations, summary.files_with_violations
				),
			);
		}
		TaskOutcome::Failed(error) => {
			crate::ui::app::set_status(app, format!("{} failed: {error}", result.label));
		}
	}
}

pub(in crate::ui) fn handle_key(app: &mut App, key: KeyEvent) -> anyhow::Result<bool> {
	Ok(update(
		app,
		AppAction::Ui(key_to_msg(crate::ui::app::mode(app), key)),
	))
}

pub(in crate::ui) fn update(app: &mut App, action: AppAction) -> bool {
	let action = match action {
		AppAction::TaskCompleted(result) => {
			match app.app_store.complete_task(&result) {
				TaskCompletion::Accepted => handle_task_result(app, result),
				TaskCompletion::Ignored => {
					crate::ui::app::set_status(
						app,
						format!("ignored stale task result: {}", result.label),
					);
				}
			}
			return false;
		}
		AppAction::HeaderSearchDebounced(generation) => {
			if crate::ui::app::header_search(app).pending_generation == Some(generation) {
				app.apply_header_search(Some(generation), false);
			}
			return false;
		}
		AppAction::UsageLensDebounced(generation) => {
			if app.runtime.usage_lens_generation == generation {
				app.refresh_usage_lens_for_primary_selection();
				sync_contextual_view(app);
			}
			return false;
		}
		AppAction::Ui(msg) if handle_ui_transition_msg(app, &msg) => {
			return false;
		}
		action => action,
	};
	if dispatch_and_apply(app, &action) {
		return true;
	}
	match action {
		AppAction::Ui(_) => false,
		AppAction::HeaderSearchDebounced(_) => false,
		AppAction::UsageLensDebounced(_) => false,
		AppAction::Shell(_) => false,
		AppAction::Store(event) => {
			handle_store_event(app, event);
			false
		}
		AppAction::TaskStarted { .. } => false,
		AppAction::TaskCompleted(_) => unreachable!("task completion handled before dispatch"),
		AppAction::Clipboard(result) => {
			handle_clipboard_result(app, result);
			false
		}
	}
}

pub(in crate::ui) fn dispatch_and_apply(app: &mut App, action: &AppAction) -> bool {
	let effects = {
		let transition = app.app_store.dispatch(action);
		transition.take_effects()
	};
	apply_effects(app, effects)
}

pub(in crate::ui) fn apply_effects(app: &mut App, effects: Vec<Effect>) -> bool {
	for effect in effects {
		if apply_effect(app, effect) {
			return true;
		}
	}
	false
}

fn apply_effect(app: &mut App, effect: Effect) -> bool {
	match effect {
		Effect::ShowView(View::Views) if crate::ui::app::view(app) == View::Views => {
			set_view(app, View::Overview, PanelPolicy::Contextual)
		}
		Effect::ShowView(view) => set_view(app, view, PanelPolicy::Manual),
		Effect::Quit => return true,
		Effect::DebounceHeaderSearch(generation) => {
			queue_header_search_debounce(app, generation);
		}
		Effect::CopyPanelSnapshot => copy_panel_snapshot(app),
		Effect::RunCheck => run_check(app),
		Effect::RefreshWorkspace => refresh_workspace_on_demand(app),
	}
	false
}

fn handle_ui_transition_msg(app: &mut App, msg: &Msg) -> bool {
	match msg {
		Msg::FocusNextRegion => cycle_focus_region(app, FocusCycle::Forward),
		Msg::FocusPreviousRegion => cycle_focus_region(app, FocusCycle::Backward),
		Msg::HeaderSearchSelectNext => app.cycle_header_search_selector(1),
		Msg::HeaderSearchSelectPrevious => app.cycle_header_search_selector(-1),
		Msg::HeaderSearchToggleSelection => app.toggle_header_search_selection(),
		Msg::HeaderSearchReset => {
			let return_focus = matches!(crate::ui::app::mode(app), UiMode::Normal);
			dispatch_and_apply(app, &AppAction::Ui(msg.clone()));
			app.apply_header_search(None, return_focus);
		}
		Msg::HeaderSearchApply => {
			let should_apply = matches!(
				crate::ui::app::mode(app),
				UiMode::HeaderSearch(HeaderSearchFocus::Text) | UiMode::Normal
			) || matches!(
				crate::ui::app::mode(app),
				UiMode::HeaderSearch(HeaderSearchFocus::Lang | HeaderSearchFocus::Kind)
			) && crate::ui::app::header_search(app).combo_open;
			let return_focus = matches!(
				crate::ui::app::mode(app),
				UiMode::HeaderSearch(HeaderSearchFocus::Text) | UiMode::Normal
			);
			dispatch_and_apply(app, &AppAction::Ui(msg.clone()));
			if should_apply {
				app.apply_header_search(None, return_focus);
			}
		}
		Msg::FocusUsages => app.focus_usages_of_selected(),
		Msg::Note(note_msg) => handle_note_msg(app, *note_msg),
		Msg::ToggleChangeMode => app.toggle_change_mode(),
		Msg::ToggleViewRender => toggle_view_render(app),
		Msg::MoveDown => apply_vertical_navigation(app, 1),
		Msg::MoveUp => apply_vertical_navigation(app, -1),
		Msg::Home => apply_positional_navigation(app, true),
		Msg::End => apply_positional_navigation(app, false),
		Msg::ToggleNode if crate::ui::app::focus_region(app) == FocusRegion::Panel => {
			ensure_active_panel_selection(app);
			toggle_panel_tree_node(app);
		}
		Msg::ToggleNode => toggle_selected_nav(app),
		Msg::OpenNode if crate::ui::app::focus_region(app) == FocusRegion::Panel => {
			ensure_active_panel_selection(app);
			open_panel_tree_node(app);
		}
		Msg::OpenNode => open_selected_nav(app),
		Msg::CloseNode if crate::ui::app::focus_region(app) == FocusRegion::Panel => {
			ensure_active_panel_selection(app);
			close_panel_tree_node(app);
		}
		Msg::CloseNode => {
			if !close_selected_nav(app) && has_clearable_scope(app) {
				app.clear_filter();
			}
		}
		_ => return false,
	}
	true
}

fn handle_note_msg(app: &mut App, msg: NoteMsg) {
	match msg {
		NoteMsg::ShowLens => show_notes_lens(app),
		NoteMsg::OpenExisting => open_note_editor(app, false),
		NoteMsg::NewDraft => open_note_editor(app, true),
		NoteMsg::NextField => move_note_editor_field(app, true),
		NoteMsg::PreviousField => move_note_editor_field(app, false),
		NoteMsg::Input(edit) => edit_note_editor(app, edit),
		NoteMsg::CycleKind => cycle_note_editor_kind(app),
		NoteMsg::CycleStatus => cycle_note_editor_status(app, true),
		NoteMsg::PreviousStatus => cycle_note_editor_status(app, false),
		NoteMsg::Save => save_note_from_editor(app),
		NoteMsg::Delete => delete_note_from_editor(app),
		NoteMsg::Close => close_note_editor(app),
	}
}

fn toggle_view_render(app: &mut App) {
	if crate::ui::app::view(app) != View::Views {
		crate::ui::app::set_status(app, "view render toggle is available in views panel");
		return;
	}
	dispatch_shell(app, ShellAction::ToggleViewsShowAll);
	let mode = if crate::ui::app::views_show_all(app) {
		"all"
	} else {
		"summary"
	};
	crate::ui::app::set_status(app, format!("views render: {mode}"));
}

fn apply_vertical_navigation(app: &mut App, direction: i8) {
	match crate::ui::app::focus_region(app) {
		FocusRegion::Navigator => apply_navigation(
			app,
			NavigationAction::Pane {
				pane: NavigationPane::Primary,
				action: if direction > 0 {
					TreePaneAction::MoveDown
				} else {
					TreePaneAction::MoveUp
				},
			},
		),
		FocusRegion::UsageLens => apply_navigation(
			app,
			NavigationAction::Pane {
				pane: NavigationPane::UsageLens,
				action: if direction > 0 {
					TreePaneAction::MoveDown
				} else {
					TreePaneAction::MoveUp
				},
			},
		),
		FocusRegion::Panel => {
			ensure_active_panel_selection(app);
			move_panel_selection(app, direction);
		}
	}
}

fn apply_positional_navigation(app: &mut App, home: bool) {
	match crate::ui::app::focus_region(app) {
		FocusRegion::Navigator => apply_navigation(
			app,
			NavigationAction::Pane {
				pane: NavigationPane::Primary,
				action: if home {
					TreePaneAction::Home
				} else {
					TreePaneAction::End
				},
			},
		),
		FocusRegion::UsageLens => apply_navigation(
			app,
			NavigationAction::Pane {
				pane: NavigationPane::UsageLens,
				action: if home {
					TreePaneAction::Home
				} else {
					TreePaneAction::End
				},
			},
		),
		FocusRegion::Panel => {
			ensure_active_panel_selection(app);
			move_panel_to_edge(app, home);
		}
	}
}

pub(in crate::ui) fn queue_task(app: &mut App, task: TaskSpec) -> bool {
	let Some(tx) = app.runtime.event_tx.clone() else {
		let label = task.label().to_string();
		crate::ui::app::set_status(app, format!("task runtime unavailable for {label}"));
		return false;
	};
	let task = app.app_store.register_task(task);
	let label = task.label().to_string();
	let id = task.id();
	TaskRunner::spawn(task, move |result| {
		let _ = tx.send(ShellEvent::TaskCompleted(result));
	});
	crate::ui::app::set_status(app, format!("task queued: {label} ({id})"));
	true
}

fn queue_header_search_debounce(app: &mut App, generation: u64) {
	let Some(tx) = app.runtime.event_tx.clone() else {
		return;
	};
	thread::spawn(move || {
		thread::sleep(Duration::from_millis(HEADER_SEARCH_DEBOUNCE_MS));
		let _ = tx.send(ShellEvent::HeaderSearchDebounced(generation));
	});
}

pub(in crate::ui) fn queue_usage_lens_refresh(app: &mut App) {
	app.runtime.usage_lens_generation += 1;
	let generation = app.runtime.usage_lens_generation;
	let Some(tx) = app.runtime.event_tx.clone() else {
		app.refresh_usage_lens_for_primary_selection();
		sync_contextual_view(app);
		return;
	};
	thread::spawn(move || {
		thread::sleep(Duration::from_millis(USAGE_LENS_DEBOUNCE_MS));
		let _ = tx.send(ShellEvent::UsageLensDebounced(generation));
	});
}
