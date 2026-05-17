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
			namespace_kind: Style::new(),
			type_kind: Style::new(),
			callable_kind: Style::new(),
			value_kind: Style::new(),
			meta_kind: Style::new(),
			unknown_kind: Style::new(),
			name: Style::new(),
			range: Style::new(),
			arrow: Style::new(),
			ref_kind: Style::new(),
			dim: Style::new(),
			punct: Style::new(),
			arg_name: Style::new(),
			arg_type: Style::new(),
		}
	}

	fn ansi() -> Self {
		Self {
			namespace_kind: Style::new().fg_color(Some(AnsiColor::Cyan.into())),
			type_kind: Style::new().fg_color(Some(AnsiColor::Blue.into())).bold(),
			callable_kind: Style::new().fg_color(Some(AnsiColor::Green.into())),
			value_kind: Style::new().fg_color(Some(AnsiColor::Yellow.into())),
			meta_kind: Style::new()
				.fg_color(Some(AnsiColor::BrightBlack.into()))
				.dimmed(),
			unknown_kind: Style::new().fg_color(Some(AnsiColor::Cyan.into())),
			name: Style::new().bold(),
			range: Style::new().fg_color(Some(AnsiColor::Green.into())),
			arrow: Style::new()
				.fg_color(Some(AnsiColor::BrightBlack.into()))
				.dimmed(),
			ref_kind: Style::new().fg_color(Some(AnsiColor::Magenta.into())),
			dim: Style::new()
				.fg_color(Some(AnsiColor::BrightBlack.into()))
				.dimmed(),
			punct: Style::new().fg_color(Some(AnsiColor::BrightBlack.into())),
			arg_name: Style::new().fg_color(Some(AnsiColor::Yellow.into())),
			arg_type: Style::new().fg_color(Some(AnsiColor::Blue.into())),
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
