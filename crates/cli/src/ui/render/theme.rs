use ratatui::style::Color;

use super::kinds::KindGroup;

#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct UiTheme {
	pub(in crate::ui) brand: Color,
	pub(in crate::ui) section: Color,
	pub(in crate::ui) status_label: Color,
	pub(in crate::ui) component_marker: Color,
	pub(in crate::ui) danger: Color,
	pub(in crate::ui) change_added: Color,
	pub(in crate::ui) change_modified: Color,
	pub(in crate::ui) focus: FocusTheme,
	pub(in crate::ui) kind: KindTheme,
	pub(in crate::ui) nav: NavTheme,
	pub(in crate::ui) panel: PanelTheme,
	pub(in crate::ui) scrollbar: ScrollbarTheme,
	pub(in crate::ui) search: SearchTheme,
	pub(in crate::ui) source: SourceTheme,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct FocusTheme {
	pub(in crate::ui) title: Color,
	pub(in crate::ui) border: Color,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct NavTheme {
	pub(in crate::ui) selected_bg: Color,
	pub(in crate::ui) marker: Color,
	pub(in crate::ui) twisty: Color,
	pub(in crate::ui) language: Color,
	pub(in crate::ui) directory: Color,
	pub(in crate::ui) file: Color,
	pub(in crate::ui) symbol: Color,
	pub(in crate::ui) visibility: Color,
	pub(in crate::ui) meta: Color,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct KindTheme {
	pub(in crate::ui) callable: Color,
	pub(in crate::ui) type_like: Color,
	pub(in crate::ui) value: Color,
	pub(in crate::ui) module: Color,
	pub(in crate::ui) reference: Color,
	pub(in crate::ui) meta: Color,
	pub(in crate::ui) fallback: Color,
}

impl KindTheme {
	pub(in crate::ui) fn color_for_group(self, group: KindGroup) -> Color {
		match group {
			KindGroup::Callable => self.callable,
			KindGroup::Type => self.type_like,
			KindGroup::Value => self.value,
			KindGroup::Namespace => self.module,
			KindGroup::Reference => self.reference,
			KindGroup::Meta => self.meta,
			KindGroup::Unknown => self.fallback,
		}
	}
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct PanelTheme {
	pub(in crate::ui) section: Color,
	pub(in crate::ui) label: Color,
	pub(in crate::ui) value: Color,
	pub(in crate::ui) header: Color,
	pub(in crate::ui) muted: Color,
	pub(in crate::ui) separator: Color,
	pub(in crate::ui) selected_bg: Color,
	pub(in crate::ui) selected_focus_bg: Color,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct ScrollbarTheme {
	pub(in crate::ui) thumb: Color,
	pub(in crate::ui) track: Color,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct SearchTheme {
	pub(in crate::ui) background: Color,
	pub(in crate::ui) focus_bg: Color,
	pub(in crate::ui) label: Color,
	pub(in crate::ui) value: Color,
	pub(in crate::ui) muted: Color,
	pub(in crate::ui) active: Color,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct SourceTheme {
	pub(in crate::ui) active_fg: Color,
	pub(in crate::ui) active_bg: Color,
	pub(in crate::ui) active_indent_bg: Color,
	pub(in crate::ui) context_fg: Color,
	pub(in crate::ui) context_bg: Color,
	pub(in crate::ui) context_indent_bg: Color,
	pub(in crate::ui) active_number_fg: Color,
	pub(in crate::ui) context_number_fg: Color,
	pub(in crate::ui) gutter_fg: Color,
}

pub(in crate::ui) const THEME: UiTheme = UiTheme {
	brand: Color::Cyan,
	section: Color::Cyan,
	status_label: Color::Yellow,
	component_marker: Color::Rgb(107, 114, 128),
	danger: Color::Red,
	change_added: Color::Rgb(5, 150, 105),
	change_modified: Color::Rgb(217, 119, 6),
	focus: FocusTheme {
		title: Color::Rgb(37, 99, 235),
		border: Color::Rgb(37, 99, 235),
	},
	kind: KindTheme {
		callable: Color::Rgb(37, 99, 235),
		type_like: Color::Rgb(126, 34, 206),
		value: Color::Rgb(4, 120, 87),
		module: Color::Rgb(2, 132, 199),
		reference: Color::Rgb(194, 65, 12),
		meta: Color::Rgb(107, 114, 128),
		fallback: Color::Rgb(147, 51, 234),
	},
	nav: NavTheme {
		selected_bg: Color::Rgb(229, 231, 235),
		marker: Color::Yellow,
		twisty: Color::Rgb(107, 114, 128),
		language: Color::Cyan,
		directory: Color::Blue,
		file: Color::Rgb(17, 24, 39),
		symbol: Color::Rgb(17, 24, 39),
		visibility: Color::Rgb(180, 83, 9),
		meta: Color::Rgb(107, 114, 128),
	},
	panel: PanelTheme {
		section: Color::Rgb(8, 145, 178),
		label: Color::Rgb(107, 114, 128),
		value: Color::Rgb(17, 24, 39),
		header: Color::Rgb(55, 65, 81),
		muted: Color::Rgb(107, 114, 128),
		separator: Color::Rgb(209, 213, 219),
		selected_bg: Color::Rgb(243, 244, 246),
		selected_focus_bg: Color::Rgb(219, 234, 254),
	},
	scrollbar: ScrollbarTheme {
		thumb: Color::Rgb(156, 163, 175),
		track: Color::Rgb(229, 231, 235),
	},
	search: SearchTheme {
		background: Color::Rgb(243, 244, 246),
		focus_bg: Color::Rgb(219, 234, 254),
		label: Color::Rgb(75, 85, 99),
		value: Color::Rgb(17, 24, 39),
		muted: Color::Rgb(107, 114, 128),
		active: Color::Rgb(37, 99, 235),
	},
	source: SourceTheme {
		active_fg: Color::Rgb(31, 41, 55),
		active_bg: Color::Rgb(232, 240, 254),
		active_indent_bg: Color::Rgb(219, 234, 254),
		context_fg: Color::Rgb(75, 85, 99),
		context_bg: Color::Rgb(249, 250, 251),
		context_indent_bg: Color::Rgb(243, 244, 246),
		active_number_fg: Color::Rgb(37, 99, 235),
		context_number_fg: Color::Rgb(156, 163, 175),
		gutter_fg: Color::Rgb(209, 213, 219),
	},
};
