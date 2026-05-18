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

pub(super) fn reference_kind_group(_kind: &str) -> KindGroup {
	KindGroup::Reference
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
