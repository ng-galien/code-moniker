use ratatui::style::Color;

#[derive(Clone, Copy, Debug)]
pub(super) struct UiTheme {
	pub(super) brand: Color,
	pub(super) section: Color,
	pub(super) status_label: Color,
	pub(super) component_marker: Color,
	pub(super) danger: Color,
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
	pub(super) kind: Color,
	pub(super) symbol: Color,
	pub(super) meta: Color,
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
	nav: NavTheme {
		selected_bg: Color::Rgb(229, 231, 235),
		marker: Color::Yellow,
		twisty: Color::Rgb(107, 114, 128),
		language: Color::Cyan,
		directory: Color::Blue,
		file: Color::Rgb(17, 24, 39),
		kind: Color::Magenta,
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
