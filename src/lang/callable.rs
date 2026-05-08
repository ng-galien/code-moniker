use crate::core::moniker::{Moniker, MonikerBuilder};

pub(crate) fn extend_segment(parent: &Moniker, kind: &[u8], name: &[u8]) -> Moniker {
	let mut b = MonikerBuilder::from_view(parent.as_view());
	b.segment(kind, name);
	b.build()
}

pub(crate) fn append_dir_module_segments(
	b: &mut MonikerBuilder,
	path: &str,
	dir_kind: &[u8],
	module_kind: &[u8],
) {
	let mut iter = path.split('/').filter(|s| !s.is_empty() && *s != ".");
	let Some(mut prev) = iter.next() else { return };
	for piece in iter {
		b.segment(dir_kind, prev.as_bytes());
		prev = piece;
	}
	b.segment(module_kind, prev.as_bytes());
}

pub(crate) fn normalize_type_text(text: &str) -> Vec<u8> {
	let bytes = text.as_bytes();
	if !bytes.iter().any(|b| matches!(*b, b' ' | b'\t' | b'\n' | b'\r')) {
		return bytes.to_vec();
	}
	let mut out = bytes.to_vec();
	out.retain(|b| !matches!(*b, b' ' | b'\t' | b'\n' | b'\r'));
	out
}

pub(crate) fn callable_segment_typed<T: AsRef<[u8]>>(name: &[u8], param_types: &[T]) -> Vec<u8> {
	let body_len: usize = param_types.iter().map(|t| t.as_ref().len() + 1).sum();
	let mut full = Vec::with_capacity(name.len() + 2 + body_len);
	full.extend_from_slice(name);
	full.push(b'(');
	full.extend_from_slice(&join_bytes_with_comma(param_types));
	full.push(b')');
	full
}

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

	#[test]
	fn normalize_type_text_collapses_inner_whitespace() {
		assert_eq!(normalize_type_text("Map<String, Integer>"), b"Map<String,Integer>".to_vec());
		assert_eq!(normalize_type_text("dict[str , int]"), b"dict[str,int]".to_vec());
		assert_eq!(normalize_type_text("string | number"), b"string|number".to_vec());
		assert_eq!(normalize_type_text("(x: number) => string"), b"(x:number)=>string".to_vec());
	}

	#[test]
	fn normalize_type_text_preserves_structural_punctuation() {
		assert_eq!(normalize_type_text("HashMap<String, u32>"), b"HashMap<String,u32>".to_vec());
		assert_eq!(normalize_type_text("\tFoo  "), b"Foo".to_vec());
	}
}
