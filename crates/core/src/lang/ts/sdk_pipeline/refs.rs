use crate::core::code_graph::RefAttrs;
use crate::core::moniker::{Moniker, MonikerBuilder};
use crate::lang::sdk::Namespace;

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
	let mut builder = MonikerBuilder::new();
	builder.project(module.as_view().project());
	builder.segment(kinds::EXTERNAL_PKG, b"code-moniker-ts-runtime");
	builder.segment(kind, name);
	builder.build()
}

pub(super) fn is_global_value(name: &[u8]) -> bool {
	matches!(
		name,
		b"AbortController"
			| b"Array"
			| b"Blob" | b"Boolean"
			| b"Date" | b"Error"
			| b"File" | b"FormData"
			| b"JSON" | b"Map"
			| b"Math" | b"Number"
			| b"Object"
			| b"Promise"
			| b"RegExp"
			| b"ResizeObserver"
			| b"Response"
			| b"Set" | b"String"
			| b"URL" | b"WebSocket"
			| b"cancelAnimationFrame"
			| b"clearInterval"
			| b"clearTimeout"
			| b"console"
			| b"crypto"
			| b"decodeURIComponent"
			| b"document"
			| b"encodeURIComponent"
			| b"fetch"
			| b"localStorage"
			| b"navigator"
			| b"parseFloat"
			| b"parseInt"
			| b"process"
			| b"requestAnimationFrame"
			| b"setInterval"
			| b"setTimeout"
			| b"structuredClone"
			| b"window"
	)
}

pub(super) fn is_global_type(name: &[u8]) -> bool {
	is_global_value(name)
		|| matches!(
			name,
			b"Awaited"
				| b"CanvasRenderingContext2D"
				| b"ClipboardEvent"
				| b"Element" | b"Event"
				| b"HTMLElement"
				| b"KeyboardEvent"
				| b"MouseEvent"
				| b"Partial" | b"Pick"
				| b"Record" | b"Required"
				| b"ReturnType"
				| b"Storage" | b"Timeout"
				| b"Window"
		)
}
