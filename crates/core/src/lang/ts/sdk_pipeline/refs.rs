use crate::core::code_graph::RefAttrs;
use crate::core::moniker::Moniker;
use crate::lang::sdk::Namespace;

use super::super::TsSdkProfile;
use super::super::kinds;
use super::defs::callable_arity;

pub(super) fn namespace_for_ref(_kind: &'static [u8]) -> Namespace {
	Namespace::Unified
}

pub(super) fn confidence_attr(value: &[u8]) -> &'static [u8] {
	match value {
		b"resolved" => kinds::CONF_RESOLVED,
		b"local" => kinds::CONF_LOCAL,
		b"imported" => kinds::CONF_IMPORTED,
		b"external" => kinds::CONF_EXTERNAL,
		b"name_match" => kinds::CONF_NAME_MATCH,
		b"" => kinds::CONF_RESOLVED,
		_ => crate::lang::kinds::CONF_UNRESOLVED,
	}
}

pub(super) fn ref_call_metadata(
	kind: &'static [u8],
	target: &Moniker,
	attrs: &RefAttrs<'_>,
) -> (Vec<u8>, Option<usize>) {
	if !attrs.call_name.is_empty() || attrs.call_arity.is_some() {
		return (attrs.call_name.to_vec(), attrs.call_arity);
	}
	if !matches!(
		kind,
		kinds::CALLS | kinds::METHOD_CALL | kinds::INSTANTIATES
	) {
		return (Vec::new(), None);
	}
	let Some(last) = target.as_view().segments().last() else {
		return (Vec::new(), None);
	};
	let name = crate::core::moniker::query::bare_callable_name(last.name).to_vec();
	(name, callable_arity(last.name))
}

pub(super) fn external_runtime_target(
	module: &Moniker,
	kind: &'static [u8],
	name: &[u8],
) -> Moniker {
	let mut builder = crate::lang::sdk::sdk_target_builder(module.as_view().project(), b"ts");
	builder.segment(kinds::PATH, b"runtime");
	builder.segment(kind, name);
	builder.build()
}

pub(super) fn external_runtime_member_target(
	module: &Moniker,
	owner: &[u8],
	kind: &'static [u8],
	name: &[u8],
) -> Moniker {
	let mut builder = crate::lang::sdk::sdk_target_builder(module.as_view().project(), b"ts");
	builder.segment(kinds::PATH, b"runtime");
	builder.segment(kinds::PATH, owner);
	builder.segment(kind, name);
	builder.build()
}

pub(super) fn is_global_value(profile: &TsSdkProfile, name: &[u8]) -> bool {
	profile.is_global_value(name)
}

pub(super) fn is_global_type(profile: &TsSdkProfile, name: &[u8]) -> bool {
	profile.is_global_type(name)
}
