use std::collections::{BTreeMap, BTreeSet};

use code_moniker_core::lang::Lang;
use code_moniker_workspace::notes::{Note, NoteId, NoteKind, NoteStatus};
use ratatui::style::{Color, Style};
use ratatui_textarea::{TextArea, WrapMode};

use crate::session::CheckSummary;
use crate::ui::app::action::ShellAction;
use crate::ui::app::{HeaderKindFilter, HeaderSearchState};
use crate::ui::async_task::{TaskId, TaskOutcome, TaskResult, WorkKind};
use crate::ui::events::{FilterEdit, HeaderSearchFocus, Msg, UiMode};
use crate::ui::explorer::HeaderSearchResults;
use crate::ui::render::component::ComponentId;
use crate::ui::store::navigation::NavigationState;
use crate::ui::store::reducer::Transition;
use crate::ui::workspace_read::UsageFocus;
use code_moniker_workspace::live::WorkspaceLiveEvent;

use super::Effect;

const PANEL_SCROLL_STEP: usize = 8;
const MAIN_SPLIT_DEFAULT_PERCENT: u16 = 42;
const MAIN_SPLIT_MIN_PERCENT: u16 = 24;
const MAIN_SPLIT_MAX_PERCENT: u16 = 70;
const MAIN_SPLIT_STEP_PERCENT: u16 = 4;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct WorkSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) epochs: BTreeMap<WorkKind, u64>,
	pub(in crate::ui) pending: BTreeSet<WorkKind>,
	pub(in crate::ui) running: BTreeMap<TaskId, RunningTask>,
}

impl WorkSlice {
	fn epoch(&self, work: WorkKind) -> u64 {
		self.epochs.get(&work).copied().unwrap_or(0)
	}

	fn bump_epochs(&mut self, works: &[WorkKind]) {
		self.generation += 1;
		for work in works {
			self.bump_epoch(*work);
		}
		self.pending.extend(works.iter().copied());
	}

	fn bump_epoch(&mut self, work: WorkKind) {
		*self.epochs.entry(work).or_default() += 1;
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct RunningTask {
	pub(in crate::ui) kind: WorkKind,
	pub(in crate::ui) generation: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum View {
	Overview,
	Tree,
	Refs,
	Unresolved,
	Check,
	Change,
	Views,
	Notes,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum VisualizationMode {
	Explorer,
	Search,
	Change,
}

impl VisualizationMode {
	pub(in crate::ui) fn label(self) -> &'static str {
		match self {
			Self::Explorer => "explorer",
			Self::Search => "search",
			Self::Change => "change",
		}
	}
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum ChangePanelMode {
	Diff,
	Usages,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum PanelPolicy {
	Contextual,
	Manual,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) enum FocusRegion {
	#[default]
	Navigator,
	UsageLens,
	Panel,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct PanelNavigationState {
	pub(in crate::ui) component: Option<ComponentId>,
	pub(in crate::ui) selected: Option<usize>,
	pub(in crate::ui) scroll: usize,
	pub(in crate::ui) expanded: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) enum NoteEditorField {
	#[default]
	Kind,
	Title,
	Body,
}

impl NoteEditorField {
	pub(in crate::ui) fn next(self) -> Self {
		match self {
			Self::Kind => Self::Title,
			Self::Title => Self::Body,
			Self::Body => Self::Kind,
		}
	}

	pub(in crate::ui) fn previous(self) -> Self {
		match self {
			Self::Kind => Self::Body,
			Self::Title => Self::Kind,
			Self::Body => Self::Title,
		}
	}
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct NoteEditorState {
	pub(in crate::ui) note_id: Option<NoteId>,
	pub(in crate::ui) target_moniker: String,
	pub(in crate::ui) target_label: String,
	pub(in crate::ui) kind: NoteKind,
	pub(in crate::ui) status: NoteStatus,
	pub(in crate::ui) title: TextArea<'static>,
	pub(in crate::ui) body: TextArea<'static>,
	pub(in crate::ui) field: NoteEditorField,
	pub(in crate::ui) dirty: bool,
	pub(in crate::ui) confirm_delete: bool,
}

impl NoteEditorState {
	pub(in crate::ui) fn draft(target_moniker: String, target_label: String) -> Self {
		Self {
			note_id: None,
			target_moniker,
			target_label,
			kind: NoteKind::Todo,
			status: NoteStatus::Pending,
			title: title_editor(""),
			body: body_editor(""),
			field: NoteEditorField::Kind,
			dirty: false,
			confirm_delete: false,
		}
	}

	pub(in crate::ui) fn existing(note: Note, target_label: String) -> Self {
		Self {
			note_id: Some(note.id),
			target_moniker: note.moniker,
			target_label,
			kind: note.kind,
			status: note.status,
			title: title_editor(&note.title),
			body: body_editor(&note.body),
			field: NoteEditorField::Kind,
			dirty: false,
			confirm_delete: false,
		}
	}

	pub(in crate::ui) fn is_empty_draft(&self) -> bool {
		self.note_id.is_none()
			&& self.title_text().trim().is_empty()
			&& self.body_text().trim().is_empty()
	}

	pub(in crate::ui) fn title_text(&self) -> String {
		textarea_text(&self.title)
	}

	pub(in crate::ui) fn body_text(&self) -> String {
		textarea_text(&self.body)
	}

	pub(in crate::ui) fn clear_title(&mut self) {
		self.title = title_editor("");
	}

	pub(in crate::ui) fn clear_body(&mut self) {
		self.body = body_editor("");
	}
}

fn title_editor(text: &str) -> TextArea<'static> {
	let line = text.lines().next().unwrap_or("").to_string();
	let mut editor = TextArea::new(vec![line]);
	style_note_editor(&mut editor, "title");
	editor
}

fn body_editor(text: &str) -> TextArea<'static> {
	let mut editor = TextArea::new(text.lines().map(ToOwned::to_owned).collect());
	style_note_editor(&mut editor, "body");
	editor
}

fn style_note_editor(editor: &mut TextArea<'static>, placeholder: &'static str) {
	editor.set_wrap_mode(WrapMode::WordOrGlyph);
	editor.set_placeholder_text(placeholder);
	editor.set_placeholder_style(Style::default().fg(Color::Rgb(156, 163, 175)));
	editor.set_cursor_line_style(Style::default());
	editor.set_cursor_style(
		Style::default()
			.fg(Color::Rgb(17, 24, 39))
			.bg(Color::Rgb(219, 234, 254)),
	);
}

fn textarea_text(editor: &TextArea<'_>) -> String {
	editor.lines().join("\n")
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) enum ActiveFilter {
	#[default]
	None,
	HeaderSearch(HeaderSearchResults),
	Change,
}

impl ActiveFilter {
	pub(in crate::ui) fn filters_navigator(&self) -> bool {
		matches!(self, Self::HeaderSearch(_) | Self::Change)
	}
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct ShellSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) status: String,
	pub(in crate::ui) view: View,
	pub(in crate::ui) view_mode: VisualizationMode,
	pub(in crate::ui) panel_policy: PanelPolicy,
	pub(in crate::ui) change_panel: ChangePanelMode,
	pub(in crate::ui) mode: UiMode,
	pub(in crate::ui) focus_region: FocusRegion,
	pub(in crate::ui) active_filter: ActiveFilter,
	pub(in crate::ui) usage_lens: Option<UsageFocus>,
	pub(in crate::ui) views_show_all: bool,
	pub(in crate::ui) note_editor: Option<NoteEditorState>,
	pub(in crate::ui) notes_error: Option<String>,
	pub(in crate::ui) main_split_percent: u16,
	pub(in crate::ui) header_search: HeaderSearchState,
	pub(in crate::ui) panel_navigation: PanelNavigationState,
}

impl Default for ShellSlice {
	fn default() -> Self {
		Self {
			generation: 0,
			status: String::new(),
			view: View::Overview,
			view_mode: VisualizationMode::Explorer,
			panel_policy: PanelPolicy::Contextual,
			change_panel: ChangePanelMode::Diff,
			mode: UiMode::Normal,
			focus_region: FocusRegion::Navigator,
			active_filter: ActiveFilter::None,
			usage_lens: None,
			views_show_all: false,
			note_editor: None,
			notes_error: None,
			main_split_percent: MAIN_SPLIT_DEFAULT_PERCENT,
			header_search: HeaderSearchState::default(),
			panel_navigation: PanelNavigationState::default(),
		}
	}
}

struct HeaderSearchOptionsUpdate<'a> {
	langs: &'a [Lang],
	kind_filters: &'a [HeaderKindFilter],
	available_langs: &'a [Lang],
	available_kind_filters: &'a [HeaderKindFilter],
	lang_cursor: usize,
	kind_cursor: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) enum CheckState {
	#[default]
	Pending,
	Ready(CheckSummary),
	Error(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct CheckSlice {
	pub(in crate::ui) state: CheckState,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct NavigationSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) state: Option<NavigationState>,
}

#[derive(Clone, Debug, Default)]
pub(in crate::ui) struct AppState {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) shell: ShellSlice,
	pub(in crate::ui) check: CheckSlice,
	pub(in crate::ui) navigation: NavigationSlice,
	pub(in crate::ui) work: WorkSlice,
	pub(in crate::ui) last_task: Option<TaskSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct TaskSummary {
	pub(in crate::ui) id: TaskId,
	pub(in crate::ui) label: String,
	pub(in crate::ui) status: TaskStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum TaskStatus {
	Completed,
	Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum TaskCompletion {
	Accepted,
	Ignored,
}

pub(in crate::ui) fn status(state: &AppState) -> &str {
	&state.shell.status
}

pub(in crate::ui) fn set_status(state: &mut AppState, status: impl Into<String>) {
	bump(state);
	state.shell.generation += 1;
	state.shell.status = status.into();
}

pub(in crate::ui) fn set_notes_error(state: &mut AppState, error: Option<String>) {
	bump(state);
	state.shell.generation += 1;
	state.shell.notes_error = error;
}

pub(in crate::ui) fn append_status(state: &mut AppState, suffix: impl AsRef<str>) {
	let suffix = suffix.as_ref();
	bump(state);
	state.shell.generation += 1;
	if state.shell.status.is_empty() {
		state.shell.status = suffix.to_string();
	} else {
		state.shell.status = format!("{}; {suffix}", state.shell.status);
	}
}

pub(in crate::ui) fn check_state(state: &AppState) -> &CheckState {
	&state.check.state
}

pub(in crate::ui) fn set_check_state(state: &mut AppState, check: CheckState) {
	bump(state);
	state.work.bump_epoch(WorkKind::CheckPanel);
	state.check.state = check;
}

pub(in crate::ui) fn set_navigation(state: &mut AppState, navigation: NavigationState) {
	bump(state);
	state.navigation.generation += 1;
	state.navigation.state = Some(navigation);
}

pub(in crate::ui) fn reduce_header_search_debounced(
	_state: &mut AppState,
	_generation: u64,
) -> Transition {
	Transition::unchanged()
}

pub(in crate::ui) fn generation_for_work(state: &AppState, work: WorkKind) -> u64 {
	state.work.epoch(work)
}

pub(in crate::ui) fn start_task(state: &mut AppState, id: TaskId, kind: WorkKind, generation: u64) {
	bump(state);
	state.work.pending.remove(&kind);
	state
		.work
		.running
		.insert(id, RunningTask { kind, generation });
	if kind == WorkKind::CheckPanel {
		state.check.state = CheckState::Pending;
	}
}

pub(in crate::ui) fn invalidate_for_store_event(state: &mut AppState, event: &WorkspaceLiveEvent) {
	bump(state);
	match event {
		WorkspaceLiveEvent::RescanRequired
		| WorkspaceLiveEvent::RescanAndNotes
		| WorkspaceLiveEvent::RescanAndGitBase
		| WorkspaceLiveEvent::RescanGitBaseAndNotes
		| WorkspaceLiveEvent::SourcesChanged(_)
		| WorkspaceLiveEvent::SourcesAndNotes(_)
		| WorkspaceLiveEvent::SourcesAndGitBase(_)
		| WorkspaceLiveEvent::SourcesGitBaseAndNotes(_) => invalidate_full_index(state),
		WorkspaceLiveEvent::GitBaseChanged => invalidate_git_overlay(state),
		WorkspaceLiveEvent::GitBaseAndNotes => invalidate_git_overlay(state),
		WorkspaceLiveEvent::Notes => {}
	}
}

pub(in crate::ui) fn complete_task(state: &mut AppState, result: &TaskResult) -> TaskCompletion {
	let accepted = accepts_task_result(state, result);
	bump(state);
	state.work.running.remove(&result.id);
	if !accepted {
		return TaskCompletion::Ignored;
	}
	match &result.outcome {
		TaskOutcome::FileCatalogLoaded(_) => {
			state.work.pending.remove(&WorkKind::ProjectLoad);
			state.work.pending.remove(&WorkKind::FileCatalog);
		}
		TaskOutcome::SymbolIndexLoaded { .. } => {
			state.work.pending.remove(&WorkKind::GraphIndex);
			state.work.pending.insert(WorkKind::LinkageSnapshot);
		}
		TaskOutcome::LinkageResolved(_) | TaskOutcome::LiveWorkspaceRefreshed { .. } => {
			state.work.pending.remove(&WorkKind::LinkageSnapshot);
			state.work.pending.remove(&WorkKind::GitOverlay);
			state.work.pending.remove(&WorkKind::ImpactIndex);
			state.work.pending.remove(&WorkKind::PanelData);
		}
		TaskOutcome::CheckCompleted(summary) => {
			state.check.state = CheckState::Ready((**summary).clone());
			state.work.pending.remove(&WorkKind::CheckPanel);
		}
		TaskOutcome::Failed(error) => {
			mark_failed(state, result.work, error.clone());
		}
	}
	state.last_task = Some(TaskSummary {
		id: result.id,
		label: result.label.clone(),
		status: match &result.outcome {
			TaskOutcome::FileCatalogLoaded(_) => TaskStatus::Completed,
			TaskOutcome::SymbolIndexLoaded { .. } => TaskStatus::Completed,
			TaskOutcome::LinkageResolved(_) => TaskStatus::Completed,
			TaskOutcome::LiveWorkspaceRefreshed { .. } => TaskStatus::Completed,
			TaskOutcome::CheckCompleted(_) => TaskStatus::Completed,
			TaskOutcome::Failed(_) => TaskStatus::Failed,
		},
	});
	TaskCompletion::Accepted
}

fn scroll_panel(state: &mut AppState, direction: i8) -> Transition {
	update_shell(state, |shell| shell_scroll_panel(shell, direction));
	Transition::changed()
}

fn accepts_task_result(state: &AppState, result: &TaskResult) -> bool {
	state.work.running.get(&result.id).is_some_and(|running| {
		running.kind == result.work
			&& running.generation == result.generation
			&& generation_for_work(state, result.work) == result.generation
	})
}

fn invalidate_full_index(state: &mut AppState) {
	state.check.state = CheckState::Pending;
	state.work.bump_epochs(&[
		WorkKind::ProjectLoad,
		WorkKind::FileCatalog,
		WorkKind::GraphIndex,
		WorkKind::LinkageSnapshot,
		WorkKind::SearchIndex,
		WorkKind::GitOverlay,
		WorkKind::LinkageSnapshot,
		WorkKind::ImpactIndex,
		WorkKind::PanelData,
		WorkKind::CheckPanel,
		WorkKind::CoverageIndex,
	]);
}

fn invalidate_git_overlay(state: &mut AppState) {
	state.work.bump_epochs(&[
		WorkKind::GitOverlay,
		WorkKind::ImpactIndex,
		WorkKind::PanelData,
	]);
}

fn bump(state: &mut AppState) {
	state.generation += 1;
}

fn mark_failed(state: &mut AppState, kind: WorkKind, error: String) {
	if kind == WorkKind::CheckPanel {
		state.check.state = CheckState::Error(error);
	}
}

fn update_shell(state: &mut AppState, update: impl FnOnce(&mut ShellSlice)) {
	bump(state);
	state.shell.generation += 1;
	update(&mut state.shell);
}

fn display_filter_text(filter: &str) -> &str {
	if filter.is_empty() { "<empty>" } else { filter }
}

fn shell_set_view(shell: &mut ShellSlice, view: View, policy: PanelPolicy) {
	if shell.view != view {
		shell.panel_navigation = PanelNavigationState::default();
	}
	shell.view = view;
	shell.panel_policy = policy;
	if matches!(view, View::Views | View::Notes) {
		shell.mode = UiMode::Normal;
		shell.focus_region = FocusRegion::Navigator;
		shell.usage_lens = None;
	}
}

fn shell_clear_filter(shell: &mut ShellSlice, return_focus: bool) {
	if return_focus {
		shell.mode = UiMode::Normal;
	}
	shell.focus_region = FocusRegion::Navigator;
	shell.active_filter = ActiveFilter::None;
	shell.view_mode = VisualizationMode::Explorer;
	shell.panel_policy = PanelPolicy::Contextual;
	shell.change_panel = ChangePanelMode::Diff;
	shell.panel_navigation = PanelNavigationState::default();
	shell.header_search.reset();
	shell.header_search.pending_generation = None;
}

fn shell_enter_change_mode(shell: &mut ShellSlice) {
	shell.mode = UiMode::Normal;
	shell.focus_region = FocusRegion::Navigator;
	shell.active_filter = ActiveFilter::Change;
	shell.usage_lens = None;
	shell.view_mode = VisualizationMode::Change;
	shell.panel_policy = PanelPolicy::Contextual;
	shell.change_panel = ChangePanelMode::Diff;
	shell.panel_navigation = PanelNavigationState::default();
	shell.header_search.reset();
	shell.header_search.pending_generation = None;
}

fn shell_set_usage_lens(shell: &mut ShellSlice, focus: Option<UsageFocus>) {
	shell.mode = UiMode::Normal;
	shell.focus_region = if focus.is_some() {
		FocusRegion::UsageLens
	} else {
		FocusRegion::Navigator
	};
	shell.usage_lens = focus;
	shell.panel_policy = PanelPolicy::Contextual;
	shell.panel_navigation = PanelNavigationState::default();
}

fn shell_replace_usage_lens(shell: &mut ShellSlice, focus: UsageFocus) {
	shell.usage_lens = Some(focus);
	shell.panel_policy = PanelPolicy::Contextual;
	shell.panel_navigation = PanelNavigationState::default();
}

fn shell_set_change_panel(shell: &mut ShellSlice, change_panel: ChangePanelMode) {
	if shell.change_panel != change_panel {
		shell.panel_navigation = PanelNavigationState::default();
	}
	shell.change_panel = change_panel;
}

fn shell_set_focus_region(shell: &mut ShellSlice, region: FocusRegion) {
	shell.mode = UiMode::Normal;
	shell.focus_region = region;
}

fn shell_set_note_editor(shell: &mut ShellSlice, editor: Option<NoteEditorState>) {
	let opening = editor.is_some();
	shell.mode = if opening {
		UiMode::Note
	} else {
		UiMode::Normal
	};
	if opening {
		shell.focus_region = FocusRegion::Panel;
		shell.panel_policy = PanelPolicy::Manual;
	}
	shell.note_editor = editor;
	shell.panel_navigation = PanelNavigationState::default();
}

fn shell_toggle_views_show_all(shell: &mut ShellSlice) {
	if shell.view == View::Views {
		shell.views_show_all = !shell.views_show_all;
		shell.panel_navigation.scroll = 0;
	}
}

fn shell_scroll_panel(shell: &mut ShellSlice, direction: i8) {
	if direction > 0 {
		shell.panel_navigation.scroll = shell
			.panel_navigation
			.scroll
			.saturating_add(PANEL_SCROLL_STEP);
	} else {
		shell.panel_navigation.scroll = shell
			.panel_navigation
			.scroll
			.saturating_sub(PANEL_SCROLL_STEP);
	}
}

fn shell_toggle_header_search(shell: &mut ShellSlice) {
	let next = match shell.mode {
		UiMode::Normal => UiMode::HeaderSearch(shell.header_search.focus),
		UiMode::HeaderSearch(_) => UiMode::Normal,
		UiMode::Note => UiMode::Note,
	};
	shell.mode = next;
	shell.header_search.combo_open = false;
	if matches!(next, UiMode::Normal) {
		shell.focus_region = FocusRegion::Navigator;
	}
	shell.status = match next {
		UiMode::Normal => "search focus returned to navigator".to_string(),
		UiMode::HeaderSearch(HeaderSearchFocus::Text) => {
			"type to search; Tab selects lang, Shift+Tab selects kind".to_string()
		}
		UiMode::HeaderSearch(HeaderSearchFocus::Lang) => {
			"select language; Tab selects kind, Shift+Tab returns to text".to_string()
		}
		UiMode::HeaderSearch(HeaderSearchFocus::Kind) => {
			"select kind; Tab returns to text, Shift+Tab selects lang".to_string()
		}
		UiMode::Note => "note editor active".to_string(),
	};
}

fn shell_focus_header_search_field(shell: &mut ShellSlice, forward: bool) {
	let focus = match shell.mode {
		UiMode::HeaderSearch(focus) if forward => focus.next(),
		UiMode::HeaderSearch(focus) => focus.previous(),
		UiMode::Normal => HeaderSearchFocus::Text,
		UiMode::Note => HeaderSearchFocus::Text,
	};
	shell.header_search.focus = focus;
	shell.header_search.combo_open = false;
	shell.mode = UiMode::HeaderSearch(focus);
	shell.status = match focus {
		HeaderSearchFocus::Text => "search text focused".to_string(),
		HeaderSearchFocus::Lang => "language selector focused".to_string(),
		HeaderSearchFocus::Kind => "kind selector focused".to_string(),
	};
}

fn shell_edit_header_search_input(shell: &mut ShellSlice, edit: FilterEdit) -> u64 {
	match edit {
		FilterEdit::Push(c) => shell.header_search.text.push(c),
		FilterEdit::Backspace => {
			shell.header_search.text.pop();
		}
		FilterEdit::Clear => shell.header_search.text.clear(),
	}
	let generation = shell.header_search.bump_pending();
	let text = display_filter_text(&shell.header_search.text);
	shell.status = format!("search draft: {text}");
	generation
}

fn shell_reset_header_search(shell: &mut ShellSlice) {
	shell.header_search.reset();
	shell.header_search.bump_pending();
	shell.status = "search filters reset".to_string();
}

fn shell_apply_header_search(
	shell: &mut ShellSlice,
	results: &HeaderSearchResults,
	return_focus: bool,
) {
	if return_focus {
		shell.mode = UiMode::Normal;
		shell.header_search.combo_open = false;
		shell.focus_region = FocusRegion::Navigator;
	}
	shell.active_filter = ActiveFilter::HeaderSearch(results.clone());
	shell.view_mode = VisualizationMode::Search;
	shell.panel_policy = PanelPolicy::Contextual;
	shell.panel_navigation = PanelNavigationState::default();
	shell.header_search.text = results.text.clone();
	shell.header_search.langs = results.langs.clone();
	shell.header_search.kind_filters = results.kind_filters.clone();
	shell.header_search.pending_generation = None;
}

fn shell_set_header_search_filters(
	shell: &mut ShellSlice,
	langs: &[Lang],
	kind_filters: &[HeaderKindFilter],
) -> u64 {
	shell.header_search.langs = langs.to_vec();
	shell.header_search.kind_filters = kind_filters.to_vec();
	shell.header_search.bump_pending()
}

fn shell_set_header_search_options(shell: &mut ShellSlice, options: HeaderSearchOptionsUpdate<'_>) {
	shell.header_search.langs = options.langs.to_vec();
	shell.header_search.kind_filters = options.kind_filters.to_vec();
	shell.header_search.available_langs = options.available_langs.to_vec();
	shell.header_search.available_kind_filters = options.available_kind_filters.to_vec();
	shell.header_search.lang_cursor = options.lang_cursor;
	shell.header_search.kind_cursor = options.kind_cursor;
}

fn shell_set_header_search_cursor(shell: &mut ShellSlice, focus: HeaderSearchFocus, cursor: usize) {
	match focus {
		HeaderSearchFocus::Text => {}
		HeaderSearchFocus::Lang => shell.header_search.lang_cursor = cursor,
		HeaderSearchFocus::Kind => shell.header_search.kind_cursor = cursor,
	}
}

fn shell_toggle_header_search_combo(shell: &mut ShellSlice) {
	match shell.mode {
		UiMode::HeaderSearch(HeaderSearchFocus::Text) | UiMode::Normal => {}
		UiMode::Note => {}
		UiMode::HeaderSearch(HeaderSearchFocus::Lang | HeaderSearchFocus::Kind)
			if shell.header_search.combo_open =>
		{
			shell.header_search.combo_open = false;
		}
		UiMode::HeaderSearch(HeaderSearchFocus::Lang | HeaderSearchFocus::Kind) => {
			shell.header_search.combo_open = true;
		}
	}
}

pub(in crate::ui) fn reduce_shell_action(state: &mut AppState, action: &ShellAction) -> Transition {
	match action {
		ShellAction::SetStatus(status) => {
			set_status(state, status.clone());
			Transition::changed()
		}
		ShellAction::AppendStatus(status) => {
			append_status(state, status);
			Transition::changed()
		}
		ShellAction::SetNotesError(error) => {
			set_notes_error(state, error.clone());
			Transition::changed()
		}
		ShellAction::SetCheckState(check_state) => {
			set_check_state(state, check_state.clone());
			Transition::changed()
		}
		ShellAction::SetView { view, policy } => {
			update_shell(state, |shell| shell_set_view(shell, *view, *policy));
			Transition::changed()
		}
		ShellAction::ApplyHeaderSearch {
			results,
			return_focus,
		} => {
			update_shell(state, |shell| {
				shell_apply_header_search(shell, results, *return_focus);
			});
			Transition::changed()
		}
		ShellAction::SetHeaderSearchFilters {
			langs,
			kind_filters,
		} => {
			let mut generation = 0;
			update_shell(state, |shell| {
				generation = shell_set_header_search_filters(shell, langs, kind_filters);
			});
			Transition::changed().with_effect(Effect::DebounceHeaderSearch(generation))
		}
		ShellAction::SetHeaderSearchOptions {
			langs,
			kind_filters,
			available_langs,
			available_kind_filters,
			lang_cursor,
			kind_cursor,
		} => {
			let options = HeaderSearchOptionsUpdate {
				langs,
				kind_filters,
				available_langs,
				available_kind_filters,
				lang_cursor: *lang_cursor,
				kind_cursor: *kind_cursor,
			};
			update_shell(state, |shell| {
				shell_set_header_search_options(shell, options)
			});
			Transition::changed()
		}
		ShellAction::SetHeaderSearchCursor { focus, cursor } => {
			update_shell(state, |shell| {
				shell_set_header_search_cursor(shell, *focus, *cursor);
			});
			Transition::changed()
		}
		ShellAction::ClearFilter { return_focus } => {
			update_shell(state, |shell| shell_clear_filter(shell, *return_focus));
			Transition::changed()
		}
		ShellAction::SetUsageLens(focus) => {
			update_shell(state, |shell| shell_set_usage_lens(shell, focus.clone()));
			Transition::changed()
		}
		ShellAction::ReplaceUsageLens(focus) => {
			update_shell(state, |shell| {
				shell_replace_usage_lens(shell, focus.clone())
			});
			Transition::changed()
		}
		ShellAction::SetNoteEditor(editor) => {
			update_shell(state, |shell| shell_set_note_editor(shell, editor.clone()));
			Transition::changed()
		}
		ShellAction::EnterChangeMode => {
			update_shell(state, shell_enter_change_mode);
			Transition::changed()
		}
		ShellAction::ReplaceActiveFilter(active_filter) => {
			update_shell(state, |shell| shell.active_filter = active_filter.clone());
			Transition::changed()
		}
		ShellAction::SetChangePanel(change_panel) => {
			update_shell(state, |shell| shell_set_change_panel(shell, *change_panel));
			Transition::changed()
		}
		ShellAction::SetFocusRegion(region) => {
			update_shell(state, |shell| shell_set_focus_region(shell, *region));
			Transition::changed()
		}
		ShellAction::ToggleViewsShowAll => {
			update_shell(state, shell_toggle_views_show_all);
			Transition::changed()
		}
		ShellAction::SetPanelScroll(offset) => {
			update_shell(state, |shell| shell.panel_navigation.scroll = *offset);
			Transition::changed()
		}
		ShellAction::SetPanelNavigation(panel_state) => {
			update_shell(state, |shell| shell.panel_navigation = panel_state.clone());
			Transition::changed()
		}
	}
}

pub(in crate::ui) fn reduce_ui_msg(state: &mut AppState, msg: &Msg) -> Transition {
	match msg {
		Msg::Quit => Transition::unchanged().with_effect(Effect::Quit),
		Msg::ShowView(view) => Transition::unchanged().with_effect(Effect::ShowView(*view)),
		Msg::ToggleHeaderSearch => {
			update_shell(state, shell_toggle_header_search);
			Transition::changed()
		}
		Msg::FocusNextRegion | Msg::FocusPreviousRegion => Transition::unchanged(),
		Msg::HeaderSearchNextField => {
			update_shell(state, |shell| shell_focus_header_search_field(shell, true));
			Transition::changed()
		}
		Msg::HeaderSearchPreviousField => {
			update_shell(state, |shell| shell_focus_header_search_field(shell, false));
			Transition::changed()
		}
		Msg::HeaderSearchInput(edit) => {
			let mut generation = 0;
			update_shell(state, |shell| {
				generation = shell_edit_header_search_input(shell, *edit);
			});
			Transition::changed().with_effect(Effect::DebounceHeaderSearch(generation))
		}
		Msg::HeaderSearchSelectNext => Transition::unchanged(),
		Msg::HeaderSearchSelectPrevious => Transition::unchanged(),
		Msg::HeaderSearchToggleSelection => Transition::unchanged(),
		Msg::HeaderSearchReset => {
			update_shell(state, shell_reset_header_search);
			Transition::changed()
		}
		Msg::HeaderSearchApply => match state.shell.mode {
			UiMode::HeaderSearch(HeaderSearchFocus::Text) | UiMode::Normal => {
				Transition::unchanged()
			}
			UiMode::HeaderSearch(HeaderSearchFocus::Lang | HeaderSearchFocus::Kind) => {
				update_shell(state, shell_toggle_header_search_combo);
				Transition::changed()
			}
			UiMode::Note => Transition::unchanged(),
		},
		Msg::Help => {
			set_status(
				state,
				"keys: s search, Tab/Shift+Tab focus, Enter/right open, Esc/left close, n note, 8 notes, d changes, u usages, y copy panel, 1-8 panels, c check, q quit",
			);
			Transition::changed()
		}
		Msg::FocusUsages => Transition::unchanged(),
		Msg::Note(_) => Transition::unchanged(),
		Msg::ToggleChangeMode => Transition::unchanged(),
		Msg::ToggleViewRender => Transition::unchanged(),
		Msg::ResizeMainSplit(direction) => {
			let mut percent = 0;
			update_shell(state, |shell| {
				shell_resize_main_split(shell, *direction);
				percent = shell.main_split_percent;
			});
			set_status(state, main_split_status(percent));
			Transition::changed()
		}
		Msg::ResetMainSplit => {
			update_shell(state, shell_reset_main_split);
			set_status(state, main_split_status(MAIN_SPLIT_DEFAULT_PERCENT));
			Transition::changed()
		}
		Msg::CopyPanelSnapshot => emit_effect(Effect::CopyPanelSnapshot),
		Msg::RunCheck => emit_effect(Effect::RunCheck),
		Msg::RefreshWorkspace => emit_effect(Effect::RefreshWorkspace),
		Msg::MoveDown => Transition::unchanged(),
		Msg::MoveUp => Transition::unchanged(),
		Msg::Home => Transition::unchanged(),
		Msg::End => Transition::unchanged(),
		Msg::PanelScrollDown => scroll_panel(state, 1),
		Msg::PanelScrollUp => scroll_panel(state, -1),
		Msg::ToggleNode | Msg::OpenNode if state.shell.focus_region == FocusRegion::Panel => {
			Transition::unchanged()
		}
		Msg::ToggleNode => Transition::unchanged(),
		Msg::OpenNode => Transition::unchanged(),
		Msg::CloseNode if state.shell.focus_region == FocusRegion::UsageLens => {
			Transition::unchanged()
		}
		Msg::CloseNode if state.shell.focus_region == FocusRegion::Panel => Transition::unchanged(),
		Msg::CloseNode => Transition::unchanged(),
		Msg::Noop => Transition::unchanged(),
	}
}

fn emit_effect(effect: Effect) -> Transition {
	Transition::unchanged().with_effect(effect)
}

fn shell_resize_main_split(shell: &mut ShellSlice, direction: i8) {
	let step = MAIN_SPLIT_STEP_PERCENT.saturating_mul(direction.unsigned_abs() as u16);
	let next = if direction.is_negative() {
		shell.main_split_percent.saturating_sub(step)
	} else {
		shell.main_split_percent.saturating_add(step)
	};
	shell.main_split_percent = next.clamp(MAIN_SPLIT_MIN_PERCENT, MAIN_SPLIT_MAX_PERCENT);
}

fn shell_reset_main_split(shell: &mut ShellSlice) {
	shell.main_split_percent = MAIN_SPLIT_DEFAULT_PERCENT;
}

fn main_split_status(left_percent: u16) -> String {
	format!(
		"layout split: {left_percent}% navigator / {}% panel",
		100u16.saturating_sub(left_percent)
	)
}
