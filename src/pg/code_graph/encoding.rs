//! Binary layout for the `code_graph` SQL type.
//!
//! ```text
//! [u16 version_le=2][u16 reserved=0]
//! [u32 def_count]   [u32 ref_count]
//! defs section, each def contiguously:
//!   [u32 moniker_len] [moniker_bytes]
//!   [u8  kind_len]    [kind_bytes]
//!   [u32 parent_or_MAX]
//!   [u32 start_or_MAX][u32 end_or_MAX]
//!   [u8  vis_len]     [vis_bytes]
//!   [u16 sig_len]     [sig_bytes]
//!   [u8  bind_len]    [bind_bytes]
//!   [u8  origin_len]  [origin_bytes]
//! refs section, each ref contiguously:
//!   [u32 source_idx]
//!   [u32 target_moniker_len] [target_moniker_bytes]
//!   [u8  kind_len]    [kind_bytes]
//!   [u32 start_or_MAX][u32 end_or_MAX]
//!   [u8  receiver_hint_len] [receiver_hint_bytes]
//!   [u8  alias_len]   [alias_bytes]
//!   [u8  conf_len]    [conf_bytes]
//!   [u8  bind_len]    [bind_bytes]
//! ```
//!
//! Sentinel `u32::MAX` encodes `Option::None` for parent / source / position.

use std::fmt;

use crate::core::code_graph::{CodeGraph, DefRecord, Position, RefRecord};
use crate::core::moniker::Moniker;

pub(super) const LAYOUT_VERSION: u16 = 2;
const VERSION_BYTES: usize = 2;
const RESERVED_BYTES: usize = 2;
const DEF_COUNT_BYTES: usize = 4;
const REF_COUNT_BYTES: usize = 4;
const HEADER_LEN: usize = VERSION_BYTES + RESERVED_BYTES + DEF_COUNT_BYTES + REF_COUNT_BYTES;
const NONE_U32: u32 = u32::MAX;

#[derive(Debug)]
pub enum EncodingError {
	Truncated(&'static str),
	UnknownVersion(u16),
	IndexOverflow,
	LengthOverflow(&'static str),
	InvalidIndex(&'static str),
}

impl fmt::Display for EncodingError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Truncated(what) => write!(f, "code_graph: buffer truncated reading {what}"),
			Self::UnknownVersion(v) => write!(f, "code_graph: unknown encoding version {v}"),
			Self::IndexOverflow => write!(f, "code_graph: parent or source index overflows u32"),
			Self::LengthOverflow(what) => write!(f, "code_graph: {what} length overflows its slot"),
			Self::InvalidIndex(what) => {
				write!(f, "code_graph: {what} points past the defs section")
			}
		}
	}
}

impl std::error::Error for EncodingError {}

pub(super) fn encode(graph: &CodeGraph) -> Result<Vec<u8>, EncodingError> {
	let defs: Vec<&DefRecord> = graph.defs().collect();
	let refs: Vec<&RefRecord> = graph.refs().collect();
	let def_count: u32 = defs
		.len()
		.try_into()
		.map_err(|_| EncodingError::IndexOverflow)?;
	let ref_count: u32 = refs
		.len()
		.try_into()
		.map_err(|_| EncodingError::IndexOverflow)?;

	let mut out = Vec::with_capacity(HEADER_LEN + 128 * defs.len() + 64 * refs.len());
	out.extend_from_slice(&LAYOUT_VERSION.to_le_bytes());
	out.extend_from_slice(&0u16.to_le_bytes());
	out.extend_from_slice(&def_count.to_le_bytes());
	out.extend_from_slice(&ref_count.to_le_bytes());

	for d in &defs {
		write_moniker(&mut out, &d.moniker)?;
		write_short_bytes(&mut out, &d.kind, "def kind")?;
		write_opt_idx(&mut out, d.parent)?;
		write_opt_pos(&mut out, d.position);
		write_short_bytes(&mut out, &d.visibility, "def visibility")?;
		write_medium_bytes(&mut out, &d.signature, "def signature")?;
		write_short_bytes(&mut out, &d.binding, "def binding")?;
		write_short_bytes(&mut out, &d.origin, "def origin")?;
	}

	for r in &refs {
		let source: u32 = r
			.source
			.try_into()
			.map_err(|_| EncodingError::IndexOverflow)?;
		out.extend_from_slice(&source.to_le_bytes());
		write_moniker(&mut out, &r.target)?;
		write_short_bytes(&mut out, &r.kind, "ref kind")?;
		write_opt_pos(&mut out, r.position);
		write_short_bytes(&mut out, &r.receiver_hint, "ref receiver_hint")?;
		write_short_bytes(&mut out, &r.alias, "ref alias")?;
		write_short_bytes(&mut out, &r.confidence, "ref confidence")?;
		write_short_bytes(&mut out, &r.binding, "ref binding")?;
	}

	Ok(out)
}

pub(super) fn decode_root(buf: &[u8]) -> Result<Moniker, EncodingError> {
	if buf.len() < HEADER_LEN {
		return Err(EncodingError::Truncated("header"));
	}
	let version = u16::from_le_bytes([buf[0], buf[1]]);
	if version != LAYOUT_VERSION {
		return Err(EncodingError::UnknownVersion(version));
	}
	let def_count = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
	if def_count == 0 {
		return Err(EncodingError::Truncated("root def"));
	}
	let mut cur = Cursor {
		buf,
		off: HEADER_LEN,
	};
	cur.read_moniker()
}

pub(super) fn decode(buf: &[u8]) -> Result<CodeGraph, EncodingError> {
	if buf.len() < HEADER_LEN {
		return Err(EncodingError::Truncated("header"));
	}
	let version = u16::from_le_bytes([buf[0], buf[1]]);
	if version != LAYOUT_VERSION {
		return Err(EncodingError::UnknownVersion(version));
	}
	let def_count = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
	let ref_count = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]) as usize;
	if def_count > buf.len() || ref_count > buf.len() {
		return Err(EncodingError::Truncated("counts exceed buffer"));
	}

	let mut cur = Cursor {
		buf,
		off: HEADER_LEN,
	};

	let mut def_records: Vec<DefRecord> = Vec::with_capacity(def_count);
	for _ in 0..def_count {
		let moniker = cur.read_moniker()?;
		let kind = cur.read_short_bytes("def kind")?.to_vec();
		let parent = cur.read_opt_idx()?;
		if let Some(p) = parent
			&& p >= def_count
		{
			return Err(EncodingError::InvalidIndex("def parent"));
		}
		let position = cur.read_opt_pos()?;
		let visibility = cur.read_short_bytes("def visibility")?.to_vec();
		let signature = cur.read_medium_bytes("def signature")?.to_vec();
		let binding = cur.read_short_bytes("def binding")?.to_vec();
		let origin = cur.read_short_bytes("def origin")?.to_vec();
		def_records.push(DefRecord {
			moniker,
			kind,
			parent,
			position,
			visibility,
			signature,
			binding,
			origin,
		});
	}

	let mut ref_records: Vec<RefRecord> = Vec::with_capacity(ref_count);
	for _ in 0..ref_count {
		let source = cur.read_u32("ref source")? as usize;
		if source >= def_count {
			return Err(EncodingError::InvalidIndex("ref source"));
		}
		let target = cur.read_moniker()?;
		let kind = cur.read_short_bytes("ref kind")?.to_vec();
		let position = cur.read_opt_pos()?;
		let receiver_hint = cur.read_short_bytes("ref receiver_hint")?.to_vec();
		let alias = cur.read_short_bytes("ref alias")?.to_vec();
		let confidence = cur.read_short_bytes("ref confidence")?.to_vec();
		let binding = cur.read_short_bytes("ref binding")?.to_vec();
		ref_records.push(RefRecord {
			source,
			target,
			kind,
			position,
			receiver_hint,
			alias,
			confidence,
			binding,
		});
	}

	Ok(CodeGraph::from_records(def_records, ref_records))
}

fn write_moniker(out: &mut Vec<u8>, m: &Moniker) -> Result<(), EncodingError> {
	let bytes = m.as_bytes();
	let len: u32 = bytes
		.len()
		.try_into()
		.map_err(|_| EncodingError::LengthOverflow("moniker"))?;
	out.extend_from_slice(&len.to_le_bytes());
	out.extend_from_slice(bytes);
	Ok(())
}

fn write_short_bytes(
	out: &mut Vec<u8>,
	bytes: &[u8],
	what: &'static str,
) -> Result<(), EncodingError> {
	if bytes.len() > u8::MAX as usize {
		return Err(EncodingError::LengthOverflow(what));
	}
	out.push(bytes.len() as u8);
	out.extend_from_slice(bytes);
	Ok(())
}

fn write_medium_bytes(
	out: &mut Vec<u8>,
	bytes: &[u8],
	what: &'static str,
) -> Result<(), EncodingError> {
	let len: u16 = bytes
		.len()
		.try_into()
		.map_err(|_| EncodingError::LengthOverflow(what))?;
	out.extend_from_slice(&len.to_le_bytes());
	out.extend_from_slice(bytes);
	Ok(())
}

fn write_opt_idx(out: &mut Vec<u8>, idx: Option<usize>) -> Result<(), EncodingError> {
	let v = match idx {
		None => NONE_U32,
		Some(i) => i.try_into().map_err(|_| EncodingError::IndexOverflow)?,
	};
	out.extend_from_slice(&v.to_le_bytes());
	Ok(())
}

fn write_opt_pos(out: &mut Vec<u8>, pos: Option<Position>) {
	let (s, e) = match pos {
		None => (NONE_U32, NONE_U32),
		Some((s, e)) => (s, e),
	};
	out.extend_from_slice(&s.to_le_bytes());
	out.extend_from_slice(&e.to_le_bytes());
}

struct Cursor<'a> {
	buf: &'a [u8],
	off: usize,
}

impl<'a> Cursor<'a> {
	fn need(&self, n: usize, what: &'static str) -> Result<(), EncodingError> {
		if self.off + n > self.buf.len() {
			Err(EncodingError::Truncated(what))
		} else {
			Ok(())
		}
	}

	fn read_u8(&mut self, what: &'static str) -> Result<u8, EncodingError> {
		self.need(1, what)?;
		let v = self.buf[self.off];
		self.off += 1;
		Ok(v)
	}

	fn read_u16(&mut self, what: &'static str) -> Result<u16, EncodingError> {
		self.need(2, what)?;
		let v = u16::from_le_bytes([self.buf[self.off], self.buf[self.off + 1]]);
		self.off += 2;
		Ok(v)
	}

	fn read_u32(&mut self, what: &'static str) -> Result<u32, EncodingError> {
		self.need(4, what)?;
		let v = u32::from_le_bytes([
			self.buf[self.off],
			self.buf[self.off + 1],
			self.buf[self.off + 2],
			self.buf[self.off + 3],
		]);
		self.off += 4;
		Ok(v)
	}

	fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8], EncodingError> {
		self.need(n, what)?;
		let s = &self.buf[self.off..self.off + n];
		self.off += n;
		Ok(s)
	}

	fn read_moniker(&mut self) -> Result<Moniker, EncodingError> {
		let len = self.read_u32("moniker len")? as usize;
		let bytes = self.take(len, "moniker bytes")?;
		Ok(Moniker::from_canonical_bytes(bytes.to_vec()))
	}

	fn read_short_bytes(&mut self, what: &'static str) -> Result<&'a [u8], EncodingError> {
		let len = self.read_u8(what)? as usize;
		self.take(len, what)
	}

	fn read_medium_bytes(&mut self, what: &'static str) -> Result<&'a [u8], EncodingError> {
		let len = self.read_u16(what)? as usize;
		self.take(len, what)
	}

	fn read_opt_idx(&mut self) -> Result<Option<usize>, EncodingError> {
		let v = self.read_u32("opt idx")?;
		Ok(if v == NONE_U32 {
			None
		} else {
			Some(v as usize)
		})
	}

	fn read_opt_pos(&mut self) -> Result<Option<Position>, EncodingError> {
		let s = self.read_u32("position start")?;
		let e = self.read_u32("position end")?;
		Ok(if s == NONE_U32 && e == NONE_U32 {
			None
		} else {
			Some((s, e))
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::code_graph::{CodeGraph, DefAttrs, RefAttrs};
	use crate::core::moniker::MonikerBuilder;

	fn mk(seg: &[u8]) -> Moniker {
		MonikerBuilder::new()
			.project(b"app")
			.segment(b"path", seg)
			.build()
	}

	fn mk_under(parent: &Moniker, kind: &[u8], name: &[u8]) -> Moniker {
		let mut b = MonikerBuilder::from_view(parent.as_view());
		b.segment(kind, name);
		b.build()
	}

	#[test]
	fn roundtrip_empty_graph() {
		let g = CodeGraph::new(mk(b"util"), b"module");
		let bytes = encode(&g).unwrap();
		let g2 = decode(&bytes).unwrap();
		assert_eq!(g, g2);
	}

	#[test]
	fn roundtrip_with_defs_and_refs() {
		let root = mk(b"util");
		let foo = mk_under(&root, b"path", b"foo");
		let mut g = CodeGraph::new(root.clone(), b"module");
		let attrs = DefAttrs {
			visibility: b"public",
			signature: b"fn(x: i32, y: String) -> Vec<u8>",
			..DefAttrs::default()
		};
		g.add_def_attrs(foo.clone(), b"function", &root, Some((10, 20)), &attrs)
			.unwrap();
		let rattrs = RefAttrs {
			receiver_hint: b"self",
			alias: b"f",
			confidence: b"local",
			..RefAttrs::default()
		};
		g.add_ref_attrs(&foo, mk(b"ext"), b"calls", Some((15, 18)), &rattrs)
			.unwrap();

		let bytes = encode(&g).unwrap();
		let g2 = decode(&bytes).unwrap();
		assert_eq!(g, g2);
	}

	#[test]
	fn roundtrip_exercises_none_sentinels() {
		let root = mk(b"util");
		let foo = mk_under(&root, b"path", b"foo");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(foo.clone(), b"function", &root, None).unwrap();
		g.add_ref(&foo, mk(b"ext"), b"calls", None).unwrap();

		let bytes = encode(&g).unwrap();
		let g2 = decode(&bytes).unwrap();
		assert_eq!(g, g2);
		let foo_def = g2.defs().find(|d| d.moniker == foo).unwrap();
		assert_eq!(foo_def.position, None);
		assert_eq!(g2.refs().next().unwrap().position, None);
	}

	#[test]
	fn decode_root_skips_def_and_ref_sections() {
		let root = mk(b"util");
		let mut g = CodeGraph::new(root.clone(), b"module");
		for i in 0..8 {
			let m = mk_under(&root, b"path", format!("c_{i}").as_bytes());
			g.add_def(m.clone(), b"class", &root, None).unwrap();
			g.add_ref(&m, mk(b"ext"), b"calls", None).unwrap();
		}
		let bytes = encode(&g).unwrap();
		assert_eq!(decode_root(&bytes).unwrap(), root);
	}

	#[test]
	fn version_mismatch_errors() {
		let mut bytes = encode(&CodeGraph::new(mk(b"a"), b"module")).unwrap();
		bytes[0] = 99;
		bytes[1] = 0;
		assert!(matches!(
			decode(&bytes),
			Err(EncodingError::UnknownVersion(99))
		));
	}

	#[test]
	fn truncated_buffer_errors() {
		let bytes = encode(&CodeGraph::new(mk(b"a"), b"module")).unwrap();
		let truncated = &bytes[..bytes.len() - 4];
		assert!(matches!(
			decode(truncated),
			Err(EncodingError::Truncated(_))
		));
	}

	#[test]
	fn position_just_below_u32_max_round_trips() {
		let root = mk(b"util");
		let foo = mk_under(&root, b"path", b"foo");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(
			foo.clone(),
			b"class",
			&root,
			Some((u32::MAX - 1, u32::MAX - 1)),
		)
		.unwrap();
		let g2 = decode(&encode(&g).unwrap()).unwrap();
		let foo_def = g2.defs().find(|d| d.moniker == foo).unwrap();
		assert_eq!(foo_def.position, Some((u32::MAX - 1, u32::MAX - 1)));
	}

	#[test]
	fn position_both_at_u32_max_collides_with_none_sentinel() {
		let root = mk(b"util");
		let foo = mk_under(&root, b"path", b"foo");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(foo.clone(), b"class", &root, Some((u32::MAX, u32::MAX)))
			.unwrap();
		let g2 = decode(&encode(&g).unwrap()).unwrap();
		let foo_def = g2.defs().find(|d| d.moniker == foo).unwrap();
		assert_eq!(foo_def.position, None);
	}

	#[test]
	fn position_one_at_u32_max_other_zero_round_trips() {
		let root = mk(b"util");
		let foo = mk_under(&root, b"path", b"foo");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(foo.clone(), b"class", &root, Some((u32::MAX, 0)))
			.unwrap();
		let g2 = decode(&encode(&g).unwrap()).unwrap();
		let foo_def = g2.defs().find(|d| d.moniker == foo).unwrap();
		assert_eq!(foo_def.position, Some((u32::MAX, 0)));
	}

	#[test]
	fn moniker_with_max_u16_project_round_trips_through_code_graph() {
		let big = vec![b'a'; u16::MAX as usize];
		let root = MonikerBuilder::new()
			.project(&big)
			.segment(b"path", b"r")
			.build();
		let child = MonikerBuilder::new()
			.project(&big)
			.segment(b"path", b"r")
			.segment(b"path", b"c")
			.build();
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(child.clone(), b"class", &root, None).unwrap();
		let g2 = decode(&encode(&g).unwrap()).unwrap();
		assert_eq!(g, g2);
	}

	#[test]
	fn decode_rejects_parent_index_past_def_count() {
		let root = mk(b"util");
		let a = mk_under(&root, b"path", b"a");
		let b = mk_under(&a, b"path", b"b");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(a.clone(), b"class", &root, None).unwrap();
		g.add_def(b.clone(), b"class", &a, None).unwrap();
		let mut bytes = encode(&g).unwrap();
		let needle = 1u32.to_le_bytes();
		let pos = bytes
			.windows(4)
			.rposition(|w| w == needle)
			.expect("parent idx u32 must be present");
		bytes[pos..pos + 4].copy_from_slice(&99u32.to_le_bytes());
		assert!(matches!(
			decode(&bytes),
			Err(EncodingError::InvalidIndex("def parent"))
		));
	}

	#[test]
	fn decode_rejects_def_count_exceeding_buffer() {
		let mut bytes = vec![0u8; HEADER_LEN];
		bytes[0..2].copy_from_slice(&LAYOUT_VERSION.to_le_bytes());
		bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
		assert!(matches!(
			decode(&bytes),
			Err(EncodingError::Truncated("counts exceed buffer"))
		));
	}

	#[test]
	fn decode_rejects_ref_count_exceeding_buffer() {
		let mut bytes = vec![0u8; HEADER_LEN];
		bytes[0..2].copy_from_slice(&LAYOUT_VERSION.to_le_bytes());
		bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
		assert!(matches!(
			decode(&bytes),
			Err(EncodingError::Truncated("counts exceed buffer"))
		));
	}

	#[test]
	fn decode_rejects_source_index_past_def_count() {
		let root = mk(b"util");
		let foo = mk_under(&root, b"path", b"foo");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(foo.clone(), b"class", &root, None).unwrap();
		g.add_ref(&foo, mk(b"ext"), b"call", None).unwrap();
		let mut bytes = encode(&g).unwrap();
		let needle = 1u32.to_le_bytes();
		let pos = bytes
			.windows(4)
			.rposition(|w| w == needle)
			.expect("source idx u32 must be present");
		bytes[pos..pos + 4].copy_from_slice(&99u32.to_le_bytes());
		assert!(matches!(
			decode(&bytes),
			Err(EncodingError::InvalidIndex(_))
		));
	}

	#[cfg(feature = "serde")]
	#[test]
	fn custom_layout_is_smaller_than_cbor() {
		let root = mk(b"util");
		let mut g = CodeGraph::new(root.clone(), b"module");
		for i in 0..16 {
			let m = mk_under(&root, b"path", format!("class_{i}").as_bytes());
			let attrs = DefAttrs {
				visibility: b"public",
				signature: b"fn(x: i32, y: String) -> Vec<u8>",
				..DefAttrs::default()
			};
			g.add_def_attrs(
				m.clone(),
				b"function",
				&root,
				Some((10 * i, 10 * i + 8)),
				&attrs,
			)
			.unwrap();
			let rattrs = RefAttrs {
				receiver_hint: b"self",
				confidence: b"local",
				..RefAttrs::default()
			};
			g.add_ref_attrs(
				&m,
				mk(format!("ext_{i}").as_bytes()),
				b"calls",
				Some((10 * i + 2, 10 * i + 6)),
				&rattrs,
			)
			.unwrap();
		}
		let custom = encode(&g).unwrap();
		let cbor = serde_cbor::to_vec(&g).expect("cbor");
		eprintln!(
			"storage compare: custom={} bytes, cbor={} bytes ({:.0}% of cbor)",
			custom.len(),
			cbor.len(),
			100.0 * custom.len() as f64 / cbor.len() as f64
		);
		assert!(
			custom.len() * 2 < cbor.len() * 3,
			"custom layout {} bytes is not meaningfully smaller than cbor {} bytes",
			custom.len(),
			cbor.len()
		);
	}

	use proptest::prelude::*;

	proptest! {
		#![proptest_config(ProptestConfig {
			cases: 256,
			..ProptestConfig::default()
		})]

		#[test]
		fn decode_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
			let _ = decode(&bytes);
		}

		#[test]
		fn decode_root_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
			let _ = decode_root(&bytes);
		}

		#[test]
		fn decode_after_single_byte_flip_never_panics(
			flip_offset in 0usize..512,
			flip_xor in 1u8..=255,
		) {
			let mut g = CodeGraph::new(mk(b"util"), b"module");
			let foo = mk_under(&g.root().clone(), b"path", b"foo");
			let _ = g.add_def(foo.clone(), b"class", &g.root().clone(), None);
			let _ = g.add_ref(&foo, mk(b"ext"), b"call", None);
			let mut bytes = encode(&g).unwrap();
			if flip_offset < bytes.len() {
				bytes[flip_offset] ^= flip_xor;
			}
			let _ = decode(&bytes);
			let _ = decode_root(&bytes);
		}
	}
}
