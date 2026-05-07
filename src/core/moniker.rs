//! Moniker — byte-compact native representation of a node identity in
//! the canonical project tree.
//!
//! The format is **SCIP-inspired**: the URI representation
//! ([`crate::core::uri`]) follows SCIP descriptor conventions
//! (`Foo#`, `bar().`, `field.`, …) and the binary representation
//! mirrors that structure as a sequence of `(kind, arity, name)`
//! segments.
//!
//! # Binary layout (version 1)
//!
//! ```text
//! [version u8 = 1]
//! [project_len u16 LE] [project bytes]
//! [seg_count u16 LE]
//!   segment[i] := [kind u16 LE] [arity u16 LE] [seg_len u16 LE] [seg bytes]
//! ```
//!
//! All multi-byte integers are little-endian. The format is
//! **canonical**: two monikers logically equal have byte-identical
//! encodings, so equality is a slice compare and the GiST opclass
//! (Phase 6) can index the buffer directly.
//!
//! `arity` is meaningful only for segments whose kind has
//! [`PunctClass::Method`]; it is `0` for the arity-less form
//! (`bar().`) and `N` for an arity disambiguator (`bar(N).`). For
//! other punct classes it is required to be `0`.

use std::fmt;

use crate::core::kind_registry::KindId;

const VERSION: u8 = 1;
const HEADER_FIXED_LEN: usize =
	1   /* version     */
	+ 2 /* project_len */
	+ 2 /* seg_count   */;
const SEG_HEADER_LEN: usize = 2 /* kind */ + 2 /* arity */ + 2 /* seg_len */;

/// Errors raised by parsing an encoded moniker buffer.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum EncodingError {
	Truncated,
	UnknownVersion(u8),
	ProjectOverflow,
	SegmentOverflow,
	TrailingBytes,
}

impl fmt::Display for EncodingError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Truncated => write!(f, "buffer too short for header"),
			Self::UnknownVersion(v) => write!(f, "unknown encoding version: {v}"),
			Self::ProjectOverflow => write!(f, "project bytes extend past buffer"),
			Self::SegmentOverflow => write!(f, "segment extends past buffer"),
			Self::TrailingBytes => write!(f, "trailing bytes after declared segments"),
		}
	}
}

impl std::error::Error for EncodingError {}

/// One segment of a moniker, as observed through a [`MonikerView`].
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct Segment<'a> {
	pub kind: KindId,
	/// Arity disambiguator, meaningful for [`crate::core::kind_registry::PunctClass::Method`].
	/// `0` indicates no disambiguator.
	pub arity: u16,
	pub bytes: &'a [u8],
}

// -----------------------------------------------------------------------------
// Moniker (owned)
// -----------------------------------------------------------------------------

/// Owned encoded moniker. Wraps the canonical byte layout.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct Moniker {
	bytes: Vec<u8>,
}

impl Moniker {
	pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, EncodingError> {
		MonikerView::from_bytes(&bytes)?;
		Ok(Self { bytes })
	}

	pub fn as_view(&self) -> MonikerView<'_> {
		MonikerView::from_bytes(&self.bytes)
			.expect("Moniker maintains a valid encoding invariant")
	}

	pub fn as_bytes(&self) -> &[u8] {
		&self.bytes
	}

	pub fn into_bytes(self) -> Vec<u8> {
		self.bytes
	}
}

// -----------------------------------------------------------------------------
// MonikerView (borrowed)
// -----------------------------------------------------------------------------

/// Read-only borrowed view over an encoded moniker. Cheap to copy.
#[derive(Copy, Clone, Debug)]
pub struct MonikerView<'a> {
	bytes: &'a [u8],
	project_off: usize,
	project_len: usize,
	seg_count: u16,
	segs_off: usize,
}

impl<'a> MonikerView<'a> {
	pub fn from_bytes(bytes: &'a [u8]) -> Result<Self, EncodingError> {
		if bytes.len() < HEADER_FIXED_LEN {
			return Err(EncodingError::Truncated);
		}
		let version = bytes[0];
		if version != VERSION {
			return Err(EncodingError::UnknownVersion(version));
		}
		let project_len = read_u16(bytes, 1) as usize;
		let project_off = 3;
		if bytes.len() < project_off + project_len + 2 {
			return Err(EncodingError::ProjectOverflow);
		}
		let seg_count = read_u16(bytes, project_off + project_len);
		let segs_off = project_off + project_len + 2;

		let mut cursor = segs_off;
		for _ in 0..seg_count {
			if bytes.len() < cursor + SEG_HEADER_LEN {
				return Err(EncodingError::SegmentOverflow);
			}
			let seg_len = read_u16(bytes, cursor + 4) as usize;
			cursor += SEG_HEADER_LEN + seg_len;
			if bytes.len() < cursor {
				return Err(EncodingError::SegmentOverflow);
			}
		}
		if cursor != bytes.len() {
			return Err(EncodingError::TrailingBytes);
		}

		Ok(Self {
			bytes,
			project_off,
			project_len,
			seg_count,
			segs_off,
		})
	}

	pub fn project(&self) -> &'a [u8] {
		&self.bytes[self.project_off..self.project_off + self.project_len]
	}

	pub fn segment_count(&self) -> u16 {
		self.seg_count
	}

	pub fn segments(&self) -> SegmentIter<'a> {
		SegmentIter {
			bytes: self.bytes,
			cursor: self.segs_off,
			remaining: self.seg_count,
		}
	}

	pub fn as_bytes(&self) -> &'a [u8] {
		self.bytes
	}
}

#[derive(Clone, Debug)]
pub struct SegmentIter<'a> {
	bytes: &'a [u8],
	cursor: usize,
	remaining: u16,
}

impl<'a> Iterator for SegmentIter<'a> {
	type Item = Segment<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		if self.remaining == 0 {
			return None;
		}
		let kind = KindId::from_raw(read_u16(self.bytes, self.cursor));
		let arity = read_u16(self.bytes, self.cursor + 2);
		let len = read_u16(self.bytes, self.cursor + 4) as usize;
		let start = self.cursor + SEG_HEADER_LEN;
		let bytes = &self.bytes[start..start + len];
		self.cursor = start + len;
		self.remaining -= 1;
		Some(Segment { kind, arity, bytes })
	}

	fn size_hint(&self) -> (usize, Option<usize>) {
		let n = self.remaining as usize;
		(n, Some(n))
	}
}

impl<'a> ExactSizeIterator for SegmentIter<'a> {}

// -----------------------------------------------------------------------------
// MonikerBuilder
// -----------------------------------------------------------------------------

/// Builder for [`Moniker`]. Constructs canonical bytes step-by-step.
#[derive(Default, Debug)]
pub struct MonikerBuilder {
	project: Vec<u8>,
	segments: Vec<(KindId, u16, Vec<u8>)>,
}

impl MonikerBuilder {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn project(&mut self, project: &[u8]) -> &mut Self {
		self.project.clear();
		self.project.extend_from_slice(project);
		self
	}

	/// Append a segment with no arity (default for non-method kinds).
	pub fn segment(&mut self, kind: KindId, bytes: &[u8]) -> &mut Self {
		self.segments.push((kind, 0, bytes.to_vec()));
		self
	}

	/// Append a method segment with an arity disambiguator. Arity `0`
	/// indicates the arity-less form (`bar().`); `N` produces `bar(N).`.
	pub fn method(&mut self, kind: KindId, bytes: &[u8], arity: u16) -> &mut Self {
		self.segments.push((kind, arity, bytes.to_vec()));
		self
	}

	pub fn build(&self) -> Moniker {
		let mut buf = Vec::with_capacity(self.estimated_size());
		buf.push(VERSION);
		write_u16(&mut buf, self.project.len() as u16);
		buf.extend_from_slice(&self.project);
		write_u16(&mut buf, self.segments.len() as u16);
		for (k, arity, s) in &self.segments {
			write_u16(&mut buf, k.as_u16());
			write_u16(&mut buf, *arity);
			write_u16(&mut buf, s.len() as u16);
			buf.extend_from_slice(s);
		}
		Moniker { bytes: buf }
	}

	fn estimated_size(&self) -> usize {
		HEADER_FIXED_LEN
			+ self.project.len()
			+ self
				.segments
				.iter()
				.map(|(_, _, s)| SEG_HEADER_LEN + s.len())
				.sum::<usize>()
	}
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn read_u16(buf: &[u8], off: usize) -> u16 {
	u16::from_le_bytes([buf[off], buf[off + 1]])
}

fn write_u16(buf: &mut Vec<u8>, value: u16) {
	buf.extend_from_slice(&value.to_le_bytes());
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
	use super::*;

	fn kid(n: u16) -> KindId {
		KindId::from_raw(n)
	}

	#[test]
	fn builder_empty() {
		let m = MonikerBuilder::new().build();
		let v = m.as_view();
		assert_eq!(v.project(), b"");
		assert_eq!(v.segment_count(), 0);
		assert_eq!(v.segments().count(), 0);
	}

	#[test]
	fn builder_with_project_no_segments() {
		let m = MonikerBuilder::new().project(b"my-app").build();
		let v = m.as_view();
		assert_eq!(v.project(), b"my-app");
		assert_eq!(v.segment_count(), 0);
	}

	#[test]
	fn builder_with_segments() {
		let m = MonikerBuilder::new()
			.project(b"my-app")
			.segment(kid(10), b"main")
			.segment(kid(11), b"com")
			.segment(kid(11), b"acme")
			.segment(kid(20), b"Foo")
			.build();
		let v = m.as_view();
		assert_eq!(v.segment_count(), 4);
		let segs: Vec<_> = v.segments().collect();
		assert_eq!(segs[0], Segment { kind: kid(10), arity: 0, bytes: b"main" });
		assert_eq!(segs[3], Segment { kind: kid(20), arity: 0, bytes: b"Foo" });
	}

	#[test]
	fn builder_method_with_arity() {
		let m = MonikerBuilder::new()
			.project(b"app")
			.segment(kid(10), b"Foo")
			.method(kid(30), b"bar", 0)   // bar()
			.method(kid(30), b"bar", 2)   // bar(2)
			.build();
		let segs: Vec<_> = m.as_view().segments().collect();
		assert_eq!(segs[1].arity, 0);
		assert_eq!(segs[2].arity, 2);
	}

	#[test]
	fn view_rejects_truncated_buffer() {
		assert_eq!(
			MonikerView::from_bytes(&[1, 0]).unwrap_err(),
			EncodingError::Truncated
		);
	}

	#[test]
	fn view_rejects_unknown_version() {
		let buf = [99, 0, 0, 0, 0];
		assert_eq!(
			MonikerView::from_bytes(&buf).unwrap_err(),
			EncodingError::UnknownVersion(99)
		);
	}

	#[test]
	fn view_rejects_project_overflow() {
		// project_len = 10 but only 0 bytes follow.
		let buf: Vec<u8> = vec![1, 10, 0, 0, 0];
		assert_eq!(
			MonikerView::from_bytes(&buf).unwrap_err(),
			EncodingError::ProjectOverflow
		);
	}

	#[test]
	fn view_rejects_segment_overflow() {
		let buf: Vec<u8> = vec![
			1,         // version
			0, 0,      // project_len = 0
			1, 0,      // seg_count = 1
			0, 0,      // kind
			0, 0,      // arity
			10, 0,     // seg_len = 10 (no bytes follow)
		];
		assert_eq!(
			MonikerView::from_bytes(&buf).unwrap_err(),
			EncodingError::SegmentOverflow
		);
	}

	#[test]
	fn view_rejects_trailing_bytes() {
		// 0 segments declared but one extra byte follows.
		let buf: Vec<u8> = vec![1, 0, 0, 0, 0, 0xff];
		assert_eq!(
			MonikerView::from_bytes(&buf).unwrap_err(),
			EncodingError::TrailingBytes
		);
	}

	#[test]
	fn roundtrip_canonicality() {
		let m1 = MonikerBuilder::new()
			.project(b"my-app")
			.segment(kid(10), b"main")
			.segment(kid(20), b"Foo")
			.method(kid(30), b"bar", 2)
			.build();

		let v = m1.as_view();
		let mut b2 = MonikerBuilder::new();
		b2.project(v.project());
		for seg in v.segments() {
			if seg.arity != 0 {
				b2.method(seg.kind, seg.bytes, seg.arity);
			} else {
				b2.segment(seg.kind, seg.bytes);
			}
		}
		let m2 = b2.build();

		assert_eq!(m1.as_bytes(), m2.as_bytes());
		assert_eq!(m1, m2);
	}

	#[test]
	fn eq_via_bytes() {
		let a = MonikerBuilder::new()
			.project(b"x")
			.segment(kid(1), b"a")
			.build();
		let b = MonikerBuilder::new()
			.project(b"x")
			.segment(kid(1), b"a")
			.build();
		let c = MonikerBuilder::new()
			.project(b"x")
			.segment(kid(1), b"b")
			.build();
		assert_eq!(a, b);
		assert_ne!(a, c);
	}

	#[test]
	fn from_bytes_roundtrip() {
		let m = MonikerBuilder::new().project(b"pj").segment(kid(7), b"foo").build();
		let bytes = m.clone().into_bytes();
		let m2 = Moniker::from_bytes(bytes).unwrap();
		assert_eq!(m, m2);
		assert!(Moniker::from_bytes(vec![99u8; 5]).is_err());
	}
}
