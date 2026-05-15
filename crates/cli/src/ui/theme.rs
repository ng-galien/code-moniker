use ratatui::style::Color;

use super::kinds::KindGroup;

#[derive(Clone, Copy, Debug)]
pub(super) struct UiTheme {
	pub(super) brand: Color,
	pub(super) section: Color,
	pub(super) status_label: Color,
	pub(super) component_marker: Color,
	pub(super) danger: Color,
	pub(super) kind: KindTheme,
	pub(super) nav: NavTheme,
	pub(super) source: SourceTheme,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct NavTheme {
	pub(super) selected_bg: Color,
	pub(super) marker: Color,
	pub(super) twisty: Color,
	pub(super) language: Color,
	pub(super) directory: Color,
	pub(super) file: Color,
	pub(super) symbol: Color,
	pub(super) meta: Color,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct KindTheme {
	pub(super) callable: Color,
	pub(super) type_like: Color,
	pub(super) value: Color,
	pub(super) module: Color,
	pub(super) reference: Color,
	pub(super) meta: Color,
	pub(super) fallback: Color,
}

impl KindTheme {
	pub(super) fn color_for_group(self, group: KindGroup) -> Color {
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
pub(super) struct SourceTheme {
	pub(super) active_fg: Color,
	pub(super) active_bg: Color,
	pub(super) active_indent_bg: Color,
	pub(super) context_fg: Color,
	pub(super) context_bg: Color,
	pub(super) context_indent_bg: Color,
	pub(super) active_number_fg: Color,
	pub(super) context_number_fg: Color,
	pub(super) gutter_fg: Color,
}

pub(super) const THEME: UiTheme = UiTheme {
	brand: Color::Cyan,
	section: Color::Cyan,
	status_label: Color::Yellow,
	component_marker: Color::Rgb(107, 114, 128),
	danger: Color::Red,
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
		meta: Color::Rgb(107, 114, 128),
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
