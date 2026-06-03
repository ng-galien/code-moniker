use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::app::View;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum UiMode {
	Normal,
	HeaderSearch(HeaderSearchFocus),
	Note,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(super) enum HeaderSearchFocus {
	#[default]
	Text,
	Lang,
	Kind,
}

impl HeaderSearchFocus {
	pub(super) fn next(self) -> Self {
		match self {
			Self::Text => Self::Lang,
			Self::Lang => Self::Kind,
			Self::Kind => Self::Text,
		}
	}

	pub(super) fn previous(self) -> Self {
		match self {
			Self::Text => Self::Kind,
			Self::Lang => Self::Text,
			Self::Kind => Self::Lang,
		}
	}
}

#[derive(Clone, Debug)]
pub(super) enum Msg {
	Quit,
	ShowView(View),
	ToggleHeaderSearch,
	FocusNextRegion,
	FocusPreviousRegion,
	HeaderSearchNextField,
	HeaderSearchPreviousField,
	HeaderSearchInput(FilterEdit),
	HeaderSearchSelectNext,
	HeaderSearchSelectPrevious,
	HeaderSearchToggleSelection,
	HeaderSearchApply,
	HeaderSearchReset,
	FocusUsages,
	Note(NoteMsg),
	ToggleChangeMode,
	ToggleViewRender,
	ResizeMainSplit(i8),
	ResetMainSplit,
	CopyPanelSnapshot,
	RunCheck,
	MoveDown,
	MoveUp,
	Home,
	End,
	PanelScrollDown,
	PanelScrollUp,
	ToggleNode,
	OpenNode,
	CloseNode,
	Help,
	Noop,
}

#[derive(Copy, Clone, Debug)]
pub(super) enum FilterEdit {
	Push(char),
	Backspace,
	Clear,
}

#[derive(Copy, Clone, Debug)]
pub(super) enum NoteMsg {
	ShowLens,
	OpenExisting,
	NewDraft,
	NextField,
	PreviousField,
	Input(KeyEvent),
	CycleKind,
	CycleStatus,
	PreviousStatus,
	Save,
	Delete,
	Close,
}

pub(super) fn key_to_msg(mode: UiMode, key: KeyEvent) -> Msg {
	if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
		return Msg::Quit;
	}
	match mode {
		UiMode::Note => note_key_to_msg(key),
		UiMode::HeaderSearch(HeaderSearchFocus::Text) => match key.code {
			KeyCode::Esc => Msg::ToggleHeaderSearch,
			KeyCode::Enter => Msg::HeaderSearchApply,
			KeyCode::Tab => Msg::HeaderSearchNextField,
			KeyCode::BackTab => Msg::HeaderSearchPreviousField,
			KeyCode::Backspace => Msg::HeaderSearchInput(FilterEdit::Backspace),
			KeyCode::Char('x') => Msg::HeaderSearchReset,
			KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
				Msg::HeaderSearchInput(FilterEdit::Clear)
			}
			KeyCode::Char(c)
				if !key.modifiers.contains(KeyModifiers::CONTROL)
					&& !key.modifiers.contains(KeyModifiers::ALT) =>
			{
				Msg::HeaderSearchInput(FilterEdit::Push(c))
			}
			_ => Msg::Noop,
		},
		UiMode::HeaderSearch(_) => match key.code {
			KeyCode::Esc | KeyCode::Char('s') => Msg::ToggleHeaderSearch,
			KeyCode::Enter => Msg::HeaderSearchApply,
			KeyCode::Char(' ') => Msg::HeaderSearchToggleSelection,
			KeyCode::Tab => Msg::HeaderSearchNextField,
			KeyCode::BackTab => Msg::HeaderSearchPreviousField,
			KeyCode::Char('x') => Msg::HeaderSearchReset,
			KeyCode::Down | KeyCode::Right | KeyCode::Char('j') => Msg::HeaderSearchSelectNext,
			KeyCode::Up | KeyCode::Left | KeyCode::Char('k') => Msg::HeaderSearchSelectPrevious,
			_ => Msg::Noop,
		},
		UiMode::Normal => normal_key_to_msg(key),
	}
}

fn note_key_to_msg(key: KeyEvent) -> Msg {
	if key.modifiers.contains(KeyModifiers::CONTROL) {
		return match key.code {
			KeyCode::Char('s') => Msg::Note(NoteMsg::Save),
			KeyCode::Char('d') => Msg::Note(NoteMsg::Delete),
			KeyCode::Char('k') => Msg::Note(NoteMsg::CycleKind),
			KeyCode::Char('o') => Msg::Note(NoteMsg::CycleStatus),
			KeyCode::Char('p') => Msg::Note(NoteMsg::PreviousStatus),
			KeyCode::Char('u') => Msg::Note(NoteMsg::Input(key)),
			_ => Msg::Noop,
		};
	}
	if key.modifiers.contains(KeyModifiers::ALT) {
		return Msg::Noop;
	}
	match key.code {
		KeyCode::Esc => Msg::Note(NoteMsg::Close),
		KeyCode::Tab => Msg::Note(NoteMsg::NextField),
		KeyCode::BackTab => Msg::Note(NoteMsg::PreviousField),
		KeyCode::Enter
		| KeyCode::Backspace
		| KeyCode::Delete
		| KeyCode::Left
		| KeyCode::Right
		| KeyCode::Up
		| KeyCode::Down
		| KeyCode::Home
		| KeyCode::End
		| KeyCode::PageUp
		| KeyCode::PageDown
		| KeyCode::Char(_) => Msg::Note(NoteMsg::Input(key)),
		_ => Msg::Noop,
	}
}

fn normal_key_to_msg(key: KeyEvent) -> Msg {
	if key.modifiers.contains(KeyModifiers::CONTROL) {
		return match key.code {
			KeyCode::Left => Msg::ResizeMainSplit(-1),
			KeyCode::Right => Msg::ResizeMainSplit(1),
			KeyCode::Char('0') => Msg::ResetMainSplit,
			_ => Msg::Noop,
		};
	}
	if key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::ALT) {
		return Msg::Noop;
	}
	match key.code {
		KeyCode::Esc => Msg::CloseNode,
		KeyCode::Tab => Msg::FocusNextRegion,
		KeyCode::BackTab => Msg::FocusPreviousRegion,
		KeyCode::Char('1') => Msg::ShowView(View::Overview),
		KeyCode::Char('2') => Msg::ShowView(View::Tree),
		KeyCode::Char('3') | KeyCode::Char('r') => Msg::ShowView(View::Refs),
		KeyCode::Char('4') => Msg::ShowView(View::Unresolved),
		KeyCode::Char('5') => Msg::ShowView(View::Check),
		KeyCode::Char('6') => Msg::ShowView(View::Change),
		KeyCode::Char('7') | KeyCode::Char('v') => Msg::ShowView(View::Views),
		KeyCode::Char('8') | KeyCode::Char('m') => Msg::Note(NoteMsg::ShowLens),
		KeyCode::Char('q') => Msg::Quit,
		KeyCode::Char('/') => Msg::Noop,
		KeyCode::Char('s') => Msg::ToggleHeaderSearch,
		KeyCode::Char('x') => Msg::HeaderSearchReset,
		KeyCode::Char('u') => Msg::FocusUsages,
		KeyCode::Char('n') => Msg::Note(NoteMsg::OpenExisting),
		KeyCode::Char('N') => Msg::Note(NoteMsg::NewDraft),
		KeyCode::Char('d') => Msg::ToggleChangeMode,
		KeyCode::Char('a') => Msg::ToggleViewRender,
		KeyCode::Char('[') => Msg::ResizeMainSplit(-1),
		KeyCode::Char(']') => Msg::ResizeMainSplit(1),
		KeyCode::Char('<') => Msg::ResizeMainSplit(-1),
		KeyCode::Char('>') => Msg::ResizeMainSplit(1),
		KeyCode::Char('=') => Msg::ResetMainSplit,
		KeyCode::Char('y') => Msg::CopyPanelSnapshot,
		KeyCode::Char('c') => Msg::RunCheck,
		KeyCode::Down | KeyCode::Char('j') => Msg::MoveDown,
		KeyCode::Up | KeyCode::Char('k') => Msg::MoveUp,
		KeyCode::Home | KeyCode::Char('g') => Msg::Home,
		KeyCode::End | KeyCode::Char('G') => Msg::End,
		KeyCode::PageDown => Msg::PanelScrollDown,
		KeyCode::PageUp => Msg::PanelScrollUp,
		KeyCode::Enter => Msg::ToggleNode,
		KeyCode::Right => Msg::OpenNode,
		KeyCode::Left => Msg::CloseNode,
		KeyCode::Char('?') => Msg::Help,
		_ => Msg::Noop,
	}
}
