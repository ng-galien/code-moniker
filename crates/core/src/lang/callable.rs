use crate::core::moniker::{Moniker, MonikerBuilder};

pub(crate) fn extend_segment(parent: &Moniker, kind: &[u8], name: &[u8]) -> Moniker {
	let mut b = MonikerBuilder::from_view(parent.as_view());
	b.segment(kind, name);
	b.build()
}

pub(crate) fn extend_segment_u32(parent: &Moniker, kind: &[u8], n: u32) -> Moniker {
	let mut buf = [0u8; 10];
	extend_segment(parent, kind, decimal_bytes(n as u64, &mut buf))
}

fn decimal_bytes(n: u64, buf: &mut [u8]) -> &[u8] {
	if n == 0 {
		buf[buf.len() - 1] = b'0';
		return &buf[buf.len() - 1..];
	}
	let mut i = buf.len();
	let mut x = n;
	while x > 0 {
		i -= 1;
		buf[i] = b'0' + (x % 10) as u8;
		x /= 10;
	}
	&buf[i..]
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
	if !bytes
		.iter()
		.any(|b| matches!(*b, b' ' | b'\t' | b'\n' | b'\r'))
	{
		return bytes.to_vec();
	}
	let mut out = bytes.to_vec();
	out.retain(|b| !matches!(*b, b' ' | b'\t' | b'\n' | b'\r'));
	out
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CallableSlot {
	pub name: Vec<u8>,
	pub r#type: Vec<u8>,
}

pub(crate) fn callable_segment_slots(name: &[u8], slots: &[CallableSlot]) -> Vec<u8> {
	let body_len: usize = slots
		.iter()
		.map(|s| s.name.len() + s.r#type.len() + 2)
		.sum();
	let mut full = Vec::with_capacity(name.len() + 2 + body_len);
	full.extend_from_slice(name);
	full.push(b'(');
	for (i, slot) in slots.iter().enumerate() {
		if i > 0 {
			full.push(b',');
		}
		match (slot.name.as_slice(), slot.r#type.as_slice()) {
			(b"", b"") => full.push(b'_'),
			(name, b"") => full.extend_from_slice(name),
			(b"", ty) => full.extend_from_slice(ty),
			(name, ty) => {
				full.extend_from_slice(name);
				full.push(b':');
				full.extend_from_slice(ty);
			}
		}
	}
	full.push(b')');
	full
}

pub(crate) fn join_bytes_with_comma<T: AsRef<[u8]>>(parts: &[T]) -> Vec<u8> {
	let body_len: usize = parts
		.iter()
		.map(|p| p.as_ref().len() + 1)
		.sum::<usize>()
		.saturating_sub(1);
	let mut out = Vec::with_capacity(body_len);
	for (i, p) in parts.iter().enumerate() {
		if i > 0 {
			out.push(b',');
		}
		out.extend_from_slice(p.as_ref());
	}
	out
}

pub(crate) fn extend_callable_slots(
	parent: &Moniker,
	kind: &[u8],
	name: &[u8],
	slots: &[CallableSlot],
) -> Moniker {
	extend_segment(parent, kind, &callable_segment_slots(name, slots))
}

pub(crate) fn slot_signature_bytes(slot: &CallableSlot) -> Vec<u8> {
	match (slot.name.as_slice(), slot.r#type.as_slice()) {
		(b"", b"") => b"_".to_vec(),
		(name, b"") => name.to_vec(),
		(b"", ty) => ty.to_vec(),
		(name, ty) => {
			let mut out = Vec::with_capacity(name.len() + 1 + ty.len());
			out.extend_from_slice(name);
			out.push(b':');
			out.extend_from_slice(ty);
			out
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn slots_segment_empty_args_emits_empty_parens() {
		assert_eq!(callable_segment_slots(b"f", &[]), b"f()".to_vec());
	}

	#[test]
	fn slots_segment_name_and_type_pairs() {
		let slots = vec![
			CallableSlot {
				name: b"id".to_vec(),
				r#type: b"int".to_vec(),
			},
			CallableSlot {
				name: b"label".to_vec(),
				r#type: b"String".to_vec(),
			},
		];
		assert_eq!(
			callable_segment_slots(b"findById", &slots),
			b"findById(id:int,label:String)".to_vec()
		);
	}

	#[test]
	fn slots_segment_name_only_when_type_absent() {
		let slots = vec![
			CallableSlot {
				name: b"x".to_vec(),
				r#type: Vec::new(),
			},
			CallableSlot {
				name: b"y".to_vec(),
				r#type: Vec::new(),
			},
		];
		assert_eq!(callable_segment_slots(b"f", &slots), b"f(x,y)".to_vec());
	}

	#[test]
	fn slots_segment_type_only_when_name_absent() {
		let slots = vec![
			CallableSlot {
				name: Vec::new(),
				r#type: b"int".to_vec(),
			},
			CallableSlot {
				name: Vec::new(),
				r#type: b"String".to_vec(),
			},
		];
		assert_eq!(
			callable_segment_slots(b"f", &slots),
			b"f(int,String)".to_vec()
		);
	}

	#[test]
	fn slots_segment_underscore_when_both_absent() {
		let slots = vec![
			CallableSlot::default(),
			CallableSlot::default(),
			CallableSlot::default(),
		];
		assert_eq!(callable_segment_slots(b"f", &slots), b"f(_,_,_)".to_vec());
	}

	#[test]
	fn slots_segment_mixed_per_slot() {
		let slots = vec![
			CallableSlot {
				name: b"id".to_vec(),
				r#type: b"int".to_vec(),
			},
			CallableSlot {
				name: Vec::new(),
				r#type: b"String".to_vec(),
			},
			CallableSlot::default(),
		];
		assert_eq!(
			callable_segment_slots(b"f", &slots),
			b"f(id:int,String,_)".to_vec()
		);
	}

	#[test]
	fn normalize_type_text_collapses_inner_whitespace() {
		assert_eq!(
			normalize_type_text("Map<String, Integer>"),
			b"Map<String,Integer>".to_vec()
		);
		assert_eq!(
			normalize_type_text("dict[str , int]"),
			b"dict[str,int]".to_vec()
		);
		assert_eq!(
			normalize_type_text("string | number"),
			b"string|number".to_vec()
		);
		assert_eq!(
			normalize_type_text("(x: number) => string"),
			b"(x:number)=>string".to_vec()
		);
	}

	#[test]
	fn normalize_type_text_preserves_structural_punctuation() {
		assert_eq!(
			normalize_type_text("HashMap<String, u32>"),
			b"HashMap<String,u32>".to_vec()
		);
		assert_eq!(normalize_type_text("\tFoo  "), b"Foo".to_vec());
	}
}
