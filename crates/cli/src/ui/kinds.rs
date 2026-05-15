use code_moniker_core::core::shape::{Shape, shape_of};
use code_moniker_core::lang::Lang;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum KindGroup {
	Namespace,
	Type,
	Callable,
	Value,
	Reference,
	Meta,
	Unknown,
}

pub(super) fn definition_kind_group(lang: Lang, kind: &str) -> KindGroup {
	lang.kind_spec(kind)
		.map(|spec| group_for_shape(spec.shape))
		.unwrap_or_else(|| group_for_kind(kind))
}

pub(super) fn definition_kind_order(lang: Lang, kind: &str) -> u16 {
	lang.kind_spec(kind)
		.map(|spec| spec.order)
		.unwrap_or_else(|| fallback_order(group_for_kind(kind)))
}

pub(super) fn is_navigable_definition(lang: Lang, kind: &str) -> bool {
	lang.kind_spec(kind).is_some()
}

pub(super) fn reference_kind_group(_kind: &str) -> KindGroup {
	KindGroup::Reference
}

pub(super) fn sort_reference_kinds(kinds: &mut [String]) {
	kinds.sort_by(|left, right| {
		reference_kind_order(left)
			.cmp(&reference_kind_order(right))
			.then_with(|| left.cmp(right))
	});
}

fn group_for_kind(kind: &str) -> KindGroup {
	match shape_of(kind.as_bytes()).map(group_for_shape) {
		Some(group) => group,
		None => KindGroup::Unknown,
	}
}

fn group_for_shape(shape: Shape) -> KindGroup {
	match shape {
		Shape::Namespace => KindGroup::Namespace,
		Shape::Type => KindGroup::Type,
		Shape::Callable => KindGroup::Callable,
		Shape::Value => KindGroup::Value,
		Shape::Annotation => KindGroup::Meta,
		Shape::Ref => KindGroup::Reference,
	}
}

fn fallback_order(group: KindGroup) -> u16 {
	match group {
		KindGroup::Namespace => 10,
		KindGroup::Type => 20,
		KindGroup::Callable => 40,
		KindGroup::Value => 60,
		KindGroup::Reference => 80,
		KindGroup::Meta => 90,
		KindGroup::Unknown => u16::MAX,
	}
}

fn reference_kind_order(kind: &str) -> u16 {
	match kind {
		"extends" | "implements" => 10,
		"instantiates" => 20,
		"uses_type" | "annotates" => 30,
		"calls" | "method_call" => 40,
		"reads" => 50,
		"imports_symbol" | "imports_module" | "reexports" => 60,
		"di_register" | "di_require" => 70,
		_ => 90,
	}
}
