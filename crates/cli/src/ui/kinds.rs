use code_moniker_core::core::shape::{Shape, shape_of};
use code_moniker_core::lang::Lang;

use crate::tree::strategy::TreeStrategy;

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
	TreeStrategy::for_lang(lang)
		.definition_shape(kind)
		.map(group_for_shape)
		.unwrap_or_else(|| group_for_kind(kind))
}

pub(super) fn definition_kind_order(lang: Lang, kind: &str) -> u16 {
	TreeStrategy::for_lang(lang).definition_order(kind)
}

pub(super) fn is_navigable_definition(lang: Lang, kind: &str) -> bool {
	TreeStrategy::for_lang(lang).is_known_definition_kind(kind)
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
