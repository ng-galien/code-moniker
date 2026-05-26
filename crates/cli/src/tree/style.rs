use anstyle::{AnsiColor, Style};
use code_moniker_core::core::shape::Shape;

use crate::args::{Charset, ExtractArgs};
use crate::color::resolve_color;

pub(super) struct TreeOpts {
	pub(super) glyph: Glyphs,
	pub(super) palette: Palette,
}

impl TreeOpts {
	pub(super) fn from_args(args: &ExtractArgs) -> Self {
		let glyph = match args.charset {
			Charset::Utf8 => Glyphs::utf8(),
			Charset::Ascii => Glyphs::ascii(),
		};
		let palette = if resolve_color(args.color) {
			Palette::ansi()
		} else {
			Palette::none()
		};
		Self { glyph, palette }
	}
}

pub(super) struct Glyphs {
	pub(super) tee: &'static str,
	pub(super) last: &'static str,
	pub(super) skip_mid: &'static str,
	pub(super) skip_last: &'static str,
	pub(super) arrow: &'static str,
}

impl Glyphs {
	fn utf8() -> Self {
		Self {
			tee: "├──",
			last: "└──",
			skip_mid: "│   ",
			skip_last: "    ",
			arrow: "→",
		}
	}

	fn ascii() -> Self {
		Self {
			tee: "+--",
			last: "+--",
			skip_mid: "|   ",
			skip_last: "    ",
			arrow: "->",
		}
	}
}

pub(super) struct Palette {
	pub(super) namespace_kind: Style,
	pub(super) type_kind: Style,
	pub(super) callable_kind: Style,
	pub(super) value_kind: Style,
	pub(super) meta_kind: Style,
	pub(super) unknown_kind: Style,
	pub(super) name: Style,
	pub(super) range: Style,
	pub(super) arrow: Style,
	pub(super) ref_kind: Style,
	pub(super) dim: Style,
	pub(super) punct: Style,
	pub(super) arg_name: Style,
	pub(super) arg_type: Style,
}

impl Palette {
	fn none() -> Self {
		Self {
			namespace_kind: plain(),
			type_kind: plain(),
			callable_kind: plain(),
			value_kind: plain(),
			meta_kind: plain(),
			unknown_kind: plain(),
			name: plain(),
			range: plain(),
			arrow: plain(),
			ref_kind: plain(),
			dim: plain(),
			punct: plain(),
			arg_name: plain(),
			arg_type: plain(),
		}
	}

	fn ansi() -> Self {
		Self {
			namespace_kind: fg(AnsiColor::Cyan),
			type_kind: bold_fg(AnsiColor::Blue),
			callable_kind: fg(AnsiColor::Green),
			value_kind: fg(AnsiColor::Yellow),
			meta_kind: dim_fg(AnsiColor::BrightBlack),
			unknown_kind: fg(AnsiColor::Cyan),
			name: bold(),
			range: fg(AnsiColor::Green),
			arrow: dim_fg(AnsiColor::BrightBlack),
			ref_kind: fg(AnsiColor::Magenta),
			dim: dim_fg(AnsiColor::BrightBlack),
			punct: fg(AnsiColor::BrightBlack),
			arg_name: fg(AnsiColor::Yellow),
			arg_type: fg(AnsiColor::Blue),
		}
	}

	pub(super) fn kind_style(&self, shape: Option<Shape>) -> Style {
		match shape {
			Some(Shape::Namespace) => self.namespace_kind,
			Some(Shape::Type) => self.type_kind,
			Some(Shape::Callable) => self.callable_kind,
			Some(Shape::Value) => self.value_kind,
			Some(Shape::Annotation) => self.meta_kind,
			Some(Shape::Ref) => self.ref_kind,
			None => self.unknown_kind,
		}
	}
}

fn plain() -> Style {
	Style::new()
}

fn bold() -> Style {
	plain().bold()
}

fn fg(color: AnsiColor) -> Style {
	plain().fg_color(Some(color.into()))
}

fn bold_fg(color: AnsiColor) -> Style {
	fg(color).bold()
}

fn dim_fg(color: AnsiColor) -> Style {
	fg(color).dimmed()
}
