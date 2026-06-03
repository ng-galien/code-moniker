use std::time::{SystemTime, UNIX_EPOCH};

use code_moniker_workspace::notes::{Note, NoteAuthor, NoteChanges, NoteId, NoteKind, NoteStatus};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui_textarea::Input;

use crate::ui::app::{
	App, FocusRegion, NoteEditorState, PanelPolicy, ShellAction, View, dispatch_shell, selected,
	selected_nav_row, set_view, sync_contextual_view,
};

pub(in crate::ui) fn show_notes_lens(app: &mut App) {
	set_view(app, View::Notes, PanelPolicy::Manual);
	dispatch_shell(app, ShellAction::SetFocusRegion(FocusRegion::Panel));
	crate::ui::app::set_status(app, "notes lens focused; n edits selected note");
}

pub(in crate::ui) fn open_note_editor(app: &mut App, force_new: bool) {
	let Some(target) = current_note_target(app) else {
		crate::ui::app::set_status(
			app,
			"select a navigator row or note before opening note mode",
		);
		return;
	};
	let editor = if force_new {
		NoteEditorState::draft(target.moniker, target.label)
	} else if let Some(note) = selected_existing_note(app, &target.moniker) {
		NoteEditorState::existing(note, target.label)
	} else {
		NoteEditorState::draft(target.moniker, target.label)
	};
	dispatch_shell(app, ShellAction::SetNoteEditor(Some(editor)));
	crate::ui::app::set_status(app, "note editor: Tab field, Ctrl+s save, Ctrl+d delete");
}

pub(in crate::ui) fn edit_note_editor(app: &mut App, key: crossterm::event::KeyEvent) {
	let Some(mut editor) = crate::ui::app::note_editor(app).cloned() else {
		return;
	};
	editor.confirm_delete = false;
	let changed = edit_active_text_area(&mut editor, key);
	editor.dirty |= changed;
	dispatch_shell(app, ShellAction::SetNoteEditor(Some(editor)));
}

pub(in crate::ui) fn move_note_editor_field(app: &mut App, forward: bool) {
	let Some(mut editor) = crate::ui::app::note_editor(app).cloned() else {
		return;
	};
	editor.confirm_delete = false;
	editor.field = if forward {
		editor.field.next()
	} else {
		editor.field.previous()
	};
	let label = match editor.field {
		crate::ui::app::NoteEditorField::Kind => "kind",
		crate::ui::app::NoteEditorField::Title => "title",
		crate::ui::app::NoteEditorField::Body => "body",
	};
	dispatch_shell(app, ShellAction::SetNoteEditor(Some(editor)));
	crate::ui::app::set_status(app, format!("note field: {label}"));
}

pub(in crate::ui) fn cycle_note_editor_kind(app: &mut App) {
	let Some(mut editor) = crate::ui::app::note_editor(app).cloned() else {
		return;
	};
	editor.confirm_delete = false;
	editor.kind = next_note_kind(editor.kind);
	editor.dirty = true;
	let kind = editor.kind.as_str();
	dispatch_shell(app, ShellAction::SetNoteEditor(Some(editor)));
	crate::ui::app::set_status(app, format!("note kind: {kind}"));
}

pub(in crate::ui) fn cycle_note_editor_status(app: &mut App, forward: bool) {
	let Some(mut editor) = crate::ui::app::note_editor(app).cloned() else {
		return;
	};
	editor.confirm_delete = false;
	editor.status = match (editor.status, forward) {
		(NoteStatus::Pending, true) => NoteStatus::Ongoing,
		(NoteStatus::Ongoing, true) => NoteStatus::Done,
		(NoteStatus::Done, true) => NoteStatus::Ongoing,
		(NoteStatus::Done, false) => NoteStatus::Ongoing,
		(NoteStatus::Ongoing, false) => NoteStatus::Pending,
		(NoteStatus::Pending, false) => NoteStatus::Ongoing,
	};
	editor.dirty = true;
	let status = editor.status.as_str();
	dispatch_shell(app, ShellAction::SetNoteEditor(Some(editor)));
	crate::ui::app::set_status(app, format!("note status: {status}"));
}

pub(in crate::ui) fn save_note_from_editor(app: &mut App) {
	let Some(editor) = crate::ui::app::note_editor(app).cloned() else {
		return;
	};
	if editor.title_text().trim().is_empty() && editor.body_text().trim().is_empty() {
		crate::ui::app::set_status(app, "empty note not saved");
		return;
	}
	let now = current_timestamp();
	let result = if let Some(id) = editor.note_id.clone() {
		update_existing_note(app, &editor, id, &now)
	} else {
		create_note(app, &editor, &now)
	};
	match result {
		Ok(id) => {
			let _ = crate::ui::app::reload_notes(app);
			dispatch_shell(app, ShellAction::SetNoteEditor(None));
			sync_contextual_view(app);
			crate::ui::app::set_status(app, format!("saved note {}", id.as_str()));
		}
		Err(error) => crate::ui::app::set_status(app, format!("note save failed: {error:#}")),
	}
}

pub(in crate::ui) fn delete_note_from_editor(app: &mut App) {
	let Some(mut editor) = crate::ui::app::note_editor(app).cloned() else {
		return;
	};
	let Some(id) = editor.note_id.clone() else {
		dispatch_shell(app, ShellAction::SetNoteEditor(None));
		crate::ui::app::set_status(app, "discarded unsaved note");
		return;
	};
	if !editor.confirm_delete {
		editor.confirm_delete = true;
		dispatch_shell(app, ShellAction::SetNoteEditor(Some(editor)));
		crate::ui::app::set_status(app, "press Ctrl+d again to delete note");
		return;
	}
	match crate::ui::app::mutate_notes(app, |document| document.delete(id.as_str()).map(|_| ())) {
		Ok(()) => {
			let _ = crate::ui::app::reload_notes(app);
			dispatch_shell(app, ShellAction::SetNoteEditor(None));
			sync_contextual_view(app);
			crate::ui::app::set_status(app, format!("deleted note {}", id.as_str()));
		}
		Err(error) => crate::ui::app::set_status(app, format!("note delete failed: {error:#}")),
	}
}

pub(in crate::ui) fn close_note_editor(app: &mut App) {
	let Some(editor) = crate::ui::app::note_editor(app).cloned() else {
		return;
	};
	let status = if editor.is_empty_draft() {
		"discarded empty note draft"
	} else if editor.dirty {
		"closed note editor without saving"
	} else {
		"closed note editor"
	};
	dispatch_shell(app, ShellAction::SetNoteEditor(None));
	sync_contextual_view(app);
	crate::ui::app::set_status(app, status);
}

fn edit_active_text_area(editor: &mut NoteEditorState, key: crossterm::event::KeyEvent) -> bool {
	if key.code == KeyCode::Char('u') && key.modifiers.contains(KeyModifiers::CONTROL) {
		return clear_active_note_field(editor);
	}
	match editor.field {
		crate::ui::app::NoteEditorField::Kind => {
			return edit_note_kind_selector(editor, key);
		}
		crate::ui::app::NoteEditorField::Title if key.code == KeyCode::Down => {
			editor.field = crate::ui::app::NoteEditorField::Body;
			return false;
		}
		crate::ui::app::NoteEditorField::Title if key.code == KeyCode::Up => {
			editor.field = crate::ui::app::NoteEditorField::Kind;
			return false;
		}
		crate::ui::app::NoteEditorField::Body
			if key.code == KeyCode::Up && body_at_first_line(editor) =>
		{
			editor.field = crate::ui::app::NoteEditorField::Title;
			return false;
		}
		_ => {}
	}
	if key.code == KeyCode::Enter && editor.field == crate::ui::app::NoteEditorField::Title {
		return false;
	}
	let input = Input::from(key);
	match editor.field {
		crate::ui::app::NoteEditorField::Kind => false,
		crate::ui::app::NoteEditorField::Title => editor.title.input(input),
		crate::ui::app::NoteEditorField::Body => editor.body.input(input),
	}
}

fn clear_active_note_field(editor: &mut NoteEditorState) -> bool {
	match editor.field {
		crate::ui::app::NoteEditorField::Kind => false,
		crate::ui::app::NoteEditorField::Title => {
			editor.clear_title();
			true
		}
		crate::ui::app::NoteEditorField::Body => {
			editor.clear_body();
			true
		}
	}
}

fn edit_note_kind_selector(editor: &mut NoteEditorState, key: crossterm::event::KeyEvent) -> bool {
	match key.code {
		KeyCode::Left => {
			editor.kind = previous_note_kind(editor.kind);
			true
		}
		KeyCode::Right => {
			editor.kind = next_note_kind(editor.kind);
			true
		}
		KeyCode::Down | KeyCode::Enter => {
			editor.field = crate::ui::app::NoteEditorField::Title;
			false
		}
		_ => false,
	}
}

fn body_at_first_line(editor: &NoteEditorState) -> bool {
	editor.body.cursor().0 == 0
}

fn next_note_kind(kind: NoteKind) -> NoteKind {
	match kind {
		NoteKind::Note => NoteKind::Todo,
		NoteKind::Todo => NoteKind::Gotcha,
		NoteKind::Gotcha => NoteKind::Request,
		NoteKind::Request => NoteKind::Note,
	}
}

fn previous_note_kind(kind: NoteKind) -> NoteKind {
	match kind {
		NoteKind::Note => NoteKind::Request,
		NoteKind::Todo => NoteKind::Note,
		NoteKind::Gotcha => NoteKind::Todo,
		NoteKind::Request => NoteKind::Gotcha,
	}
}

fn create_note(app: &App, editor: &NoteEditorState, now: &str) -> anyhow::Result<NoteId> {
	crate::ui::app::mutate_notes(app, |document| {
		let id = generated_note_id(document);
		document.insert(Note {
			id: id.clone(),
			moniker: editor.target_moniker.clone(),
			kind: editor.kind,
			status: editor.status,
			title: editor.title_text(),
			body: editor.body_text(),
			created_by: NoteAuthor::User,
			created_at: now.to_string(),
			updated_at: now.to_string(),
		})?;
		Ok(id)
	})
}

fn update_existing_note(
	app: &App,
	editor: &NoteEditorState,
	id: NoteId,
	now: &str,
) -> anyhow::Result<NoteId> {
	crate::ui::app::mutate_notes(app, |document| {
		if document
			.get(id.as_str())
			.map(|note| note.status != editor.status)
			.unwrap_or(false)
		{
			document.transition(id.as_str(), editor.status, now.to_string())?;
		}
		document.update(
			id.as_str(),
			NoteChanges {
				moniker: Some(editor.target_moniker.clone()),
				kind: Some(editor.kind),
				title: Some(editor.title_text()),
				body: Some(editor.body_text()),
			},
			now.to_string(),
		)?;
		Ok(id)
	})
}

fn selected_existing_note(app: &App, moniker: &str) -> Option<Note> {
	if crate::ui::app::view(app) == View::Notes {
		return selected_note_from_lens(app);
	}
	let mut notes = crate::ui::app::notes(app)
		.notes()
		.iter()
		.filter(|note| note.moniker == moniker)
		.cloned()
		.collect::<Vec<_>>();
	sort_notes_for_editing(&mut notes);
	notes.into_iter().next()
}

fn selected_note_from_lens(app: &App) -> Option<Note> {
	let selected = app.app_store.shell().panel_navigation.selected.unwrap_or(0);
	let mut notes = crate::ui::app::notes(app).notes().to_vec();
	sort_notes_for_lens(&mut notes);
	notes.into_iter().nth(selected)
}

fn sort_notes_for_editing(notes: &mut [Note]) {
	notes.sort_by(|left, right| {
		note_status_rank(left.status)
			.cmp(&note_status_rank(right.status))
			.then_with(|| left.updated_at.cmp(&right.updated_at).reverse())
			.then_with(|| left.id.cmp(&right.id))
	});
}

pub(in crate::ui) fn sort_notes_for_lens(notes: &mut [Note]) {
	notes.sort_by(|left, right| {
		note_status_rank(left.status)
			.cmp(&note_status_rank(right.status))
			.then_with(|| left.updated_at.cmp(&right.updated_at).reverse())
			.then_with(|| left.id.cmp(&right.id))
	});
}

fn note_status_rank(status: NoteStatus) -> u8 {
	match status {
		NoteStatus::Pending => 0,
		NoteStatus::Ongoing => 1,
		NoteStatus::Done => 3,
	}
}

struct NoteTarget {
	moniker: String,
	label: String,
}

fn current_note_target(app: &App) -> Option<NoteTarget> {
	if crate::ui::app::view(app) == View::Notes {
		if let Some(note) = selected_note_from_lens(app) {
			return Some(NoteTarget {
				moniker: note.moniker,
				label: "selected note".to_string(),
			});
		}
	}
	if let Some(loc) = selected(app) {
		let summary = crate::ui::workspace_read::symbol_summary(crate::ui::app::store(app), &loc);
		if !summary.identity.is_empty() {
			return Some(NoteTarget {
				moniker: summary.identity,
				label: format!("{} {}", summary.kind, summary.name),
			});
		}
	}
	let row = selected_nav_row(app)?;
	crate::ui::nav_notes::nav_row_note_target(crate::ui::app::store(app), row, &app.config.scheme)
		.map(|target| NoteTarget {
			moniker: target.moniker,
			label: target.label,
		})
}

fn generated_note_id(document: &code_moniker_workspace::notes::NotesDocument) -> NoteId {
	for attempt in 0..1000_u32 {
		let nanos = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map(|duration| duration.as_nanos())
			.unwrap_or_default();
		let id = if attempt == 0 {
			format!("note_{nanos:x}")
		} else {
			format!("note_{nanos:x}_{attempt}")
		};
		if document.get(&id).is_none() {
			return NoteId::new(id);
		}
	}
	NoteId::new("note_fallback")
}

fn current_timestamp() -> String {
	let seconds = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|duration| duration.as_secs())
		.unwrap_or_default();
	format!("unix:{seconds}")
}
