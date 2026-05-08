//! Cross-language helpers for building moniker segments. Centralized
//! so every extractor uses the same wire shape for callable
//! identities and the same `extend_segment` plumbing.

use crate::core::moniker::{Moniker, MonikerBuilder};

/// Append one typed segment to the parent.
pub(crate) fn extend_segment(parent: &Moniker, kind: &[u8], name: &[u8]) -> Moniker {
	let mut b = MonikerBuilder::from_view(parent.as_view());
	b.segment(kind, name);
	b.build()
}

/// Build the segment-name bytes for a callable definition with known
/// parameter types: `name(t1,t2,...)` or `name()` for arity 0. Generic
/// over the slot type so callers with `Vec<u8>` (SQL via PG parser)
/// and `&str` (tree-sitter languages) share one implementation.
pub(crate) fn callable_segment_typed<T: AsRef<[u8]>>(name: &[u8], param_types: &[T]) -> Vec<u8> {
	let body_len: usize = param_types.iter().map(|t| t.as_ref().len() + 1).sum();
	let mut full = Vec::with_capacity(name.len() + 2 + body_len);
	full.extend_from_slice(name);
	full.push(b'(');
	full.extend_from_slice(&join_bytes_with_comma(param_types));
	full.push(b')');
	full
}

/// Concatenate `[a, b, c]` into `a,b,c`. `Vec<u8>` doesn't have a
/// built-in `.join(",")` against a `str` separator, so consumers
/// (e.g. the python signature column) call this directly.
pub(crate) fn join_bytes_with_comma<T: AsRef<[u8]>>(parts: &[T]) -> Vec<u8> {
	let body_len: usize = parts.iter().map(|p| p.as_ref().len() + 1).sum::<usize>().saturating_sub(1);
	let mut out = Vec::with_capacity(body_len);
	for (i, p) in parts.iter().enumerate() {
		if i > 0 {
			out.push(b',');
		}
		out.extend_from_slice(p.as_ref());
	}
	out
}

/// Build the segment-name bytes for a call site where only arity is
/// statically known: `name()` for arity 0, `name(N)` otherwise. Refs
/// using this shape will not match a typed def via `=`; emit them
/// with `confidence: unresolved` so consumers project on name+arity.
pub(crate) fn callable_segment_arity(name: &[u8], arity: u16) -> Vec<u8> {
	let mut full = Vec::with_capacity(name.len() + 6);
	full.extend_from_slice(name);
	full.push(b'(');
	if arity != 0 {
		full.extend_from_slice(arity.to_string().as_bytes());
	}
	full.push(b')');
	full
}

pub(crate) fn extend_callable_typed<T: AsRef<[u8]>>(
	parent: &Moniker,
	kind: &[u8],
	name: &[u8],
	param_types: &[T],
) -> Moniker {
	extend_segment(parent, kind, &callable_segment_typed(name, param_types))
}

pub(crate) fn extend_callable_arity(
	parent: &Moniker,
	kind: &[u8],
	name: &[u8],
	arity: u16,
) -> Moniker {
	extend_segment(parent, kind, &callable_segment_arity(name, arity))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn typed_segment_no_params_emits_empty_parens() {
		assert_eq!(callable_segment_typed(b"bar", &[] as &[&[u8]]), b"bar()".to_vec());
	}

	#[test]
	fn typed_segment_joins_str_slots_with_commas() {
		assert_eq!(
			callable_segment_typed(b"findById", &["int", "String"]),
			b"findById(int,String)".to_vec()
		);
	}

	#[test]
	fn typed_segment_accepts_byte_vec_slots() {
		let types = vec![b"int4".to_vec(), b"text".to_vec()];
		assert_eq!(
			callable_segment_typed(b"bar", &types),
			b"bar(int4,text)".to_vec()
		);
	}

	#[test]
	fn typed_segment_underscore_for_untyped_slot() {
		assert_eq!(
			callable_segment_typed(b"f", &["_", "_"]),
			b"f(_,_)".to_vec()
		);
	}

	#[test]
	fn arity_segment_zero_drops_number() {
		assert_eq!(callable_segment_arity(b"bar", 0), b"bar()".to_vec());
	}

	#[test]
	fn arity_segment_keeps_count() {
		assert_eq!(callable_segment_arity(b"bar", 3), b"bar(3)".to_vec());
	}
}
