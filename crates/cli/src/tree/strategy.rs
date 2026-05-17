use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::core::shape::{Shape, shape_of};
use code_moniker_core::lang::Lang;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TreeStrategy {
	lang: Option<Lang>,
}

impl TreeStrategy {
	pub(crate) fn for_lang(lang: Lang) -> Self {
		Self { lang: Some(lang) }
	}

	pub(crate) fn from_moniker(moniker: &Moniker) -> Self {
		Self {
			lang: lang_from_moniker(moniker),
		}
	}

	pub(crate) fn unknown() -> Self {
		Self { lang: None }
	}

	pub(crate) fn definition_order(self, kind: &str) -> u16 {
		self.lang
			.and_then(|lang| lang.kind_spec(kind))
			.map(|spec| spec.order)
			.or_else(|| structural_kind_order(kind))
			.or_else(|| shape_of(kind.as_bytes()).map(fallback_order_for_shape))
			.unwrap_or(u16::MAX)
	}

	pub(crate) fn definition_shape(self, kind: &str) -> Option<Shape> {
		self.lang
			.and_then(|lang| lang.kind_spec(kind))
			.map(|spec| spec.shape)
			.or_else(|| structural_kind_shape(kind))
			.or_else(|| shape_of(kind.as_bytes()))
	}

	pub(crate) fn is_known_definition_kind(self, kind: &str) -> bool {
		self.lang.is_some_and(|lang| lang.kind_spec(kind).is_some())
	}

	pub(crate) fn collapse_separator(self, kind: &str) -> Option<&'static str> {
		match kind {
			"package" => Some("."),
			"dir" => Some("/"),
			_ => None,
		}
	}
}

fn lang_from_moniker(moniker: &Moniker) -> Option<Lang> {
	moniker.as_view().segments().find_map(|segment| {
		if segment.kind == b"lang" {
			std::str::from_utf8(segment.name)
				.ok()
				.and_then(Lang::from_tag)
		} else {
			None
		}
	})
}

fn structural_kind_order(kind: &str) -> Option<u16> {
	structural_kind_shape(kind).map(fallback_order_for_shape)
}

fn structural_kind_shape(kind: &str) -> Option<Shape> {
	match kind {
		"lang" | "dir" | "package" => Some(Shape::Namespace),
		_ => None,
	}
}

fn fallback_order_for_shape(shape: Shape) -> u16 {
	match shape {
		Shape::Namespace => 10,
		Shape::Type => 20,
		Shape::Callable => 40,
		Shape::Value => 60,
		Shape::Ref => 80,
		Shape::Annotation => 90,
	}
}
