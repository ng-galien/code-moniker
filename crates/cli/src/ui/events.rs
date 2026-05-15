use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::View;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum UiMode {
	Normal,
	EditingFilter,
}

#[derive(Clone, Debug)]
pub(super) enum Msg {
	Key(KeyEvent),
	Quit,
	CycleView,
	ShowView(View),
	StartFilterEdit,
	FilterInput(FilterEdit),
	ApplyFilter,
	CancelInput,
	ClearFilter,
	FocusUsages,
	RunCheck,
	MoveDown,
	MoveUp,
	Home,
	End,
	ToggleNode,
	OpenNode,
	CloseNode,
	Help,
	Noop,
}

#[derive(Clone, Debug)]
pub(super) enum FilterEdit {
	Push(char),
	Backspace,
	Clear,
}

pub(super) fn key_to_msg(mode: UiMode, key: KeyEvent) -> Msg {
	match mode {
		UiMode::EditingFilter => match key.code {
			KeyCode::Esc => Msg::CancelInput,
			KeyCode::Enter => Msg::ApplyFilter,
			KeyCode::Backspace => Msg::FilterInput(FilterEdit::Backspace),
			KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
				Msg::FilterInput(FilterEdit::Clear)
			}
			KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
				Msg::FilterInput(FilterEdit::Push(c))
			}
			_ => Msg::Noop,
		},
		UiMode::Normal => normal_key_to_msg(key),
	}
}

fn normal_key_to_msg(key: KeyEvent) -> Msg {
	if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
		return Msg::Quit;
	}
	if key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::ALT) {
		return Msg::Noop;
	}
	match key.code {
		KeyCode::Esc => Msg::CloseNode,
		KeyCode::Tab => Msg::CycleView,
		KeyCode::Char('1') => Msg::ShowView(View::Overview),
		KeyCode::Char('2') => Msg::ShowView(View::Tree),
		KeyCode::Char('3') | KeyCode::Char('r') => Msg::ShowView(View::Refs),
		KeyCode::Char('4') => Msg::ShowView(View::Check),
		KeyCode::Char('q') => Msg::Quit,
		KeyCode::Char('/') => Msg::StartFilterEdit,
		KeyCode::Char('x') => Msg::ClearFilter,
		KeyCode::Char('u') => Msg::FocusUsages,
		KeyCode::Char('c') => Msg::RunCheck,
		KeyCode::Down | KeyCode::Char('j') => Msg::MoveDown,
		KeyCode::Up | KeyCode::Char('k') => Msg::MoveUp,
		KeyCode::Home | KeyCode::Char('g') => Msg::Home,
		KeyCode::End | KeyCode::Char('G') => Msg::End,
		KeyCode::Enter => Msg::ToggleNode,
		KeyCode::Right => Msg::OpenNode,
		KeyCode::Left => Msg::CloseNode,
		KeyCode::Char('?') => Msg::Help,
		_ => Msg::Noop,
	}
}
