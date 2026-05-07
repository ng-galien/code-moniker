//! Read-only borrowed view over an encoded moniker buffer.

use super::encoding::{
	read_u16, EncodingError, HEADER_FIXED_LEN, SEG_HEADER_LEN, VERSION,
};
use crate::core::kind_registry::KindId;

/// One segment of a moniker, as observed through a [`MonikerView`].
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct Segment<'a> {
	pub kind: KindId,
	/// Arity disambiguator, meaningful for [`crate::core::kind_registry::PunctClass::Method`].
	/// `0` indicates no disambiguator.
	pub arity: u16,
	pub bytes: &'a [u8],
}

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

	/// True when `self` is `other` itself or a strict prefix of `other`.
	/// Reflexive, matches PG's `<@` / `@>` containment convention.
	///
	/// Both buffers are canonical, so once projects match the segments
	/// region is byte-prefix comparable — one `memcmp` instead of N
	/// segment-header walks.
	pub fn is_ancestor_of(&self, other: &MonikerView<'_>) -> bool {
		if self.project() != other.project() {
			return false;
		}
		if self.seg_count > other.seg_count {
			return false;
		}
		// Equal projects ⇒ equal `project_len` ⇒ equal `segs_off`.
		debug_assert_eq!(self.segs_off, other.segs_off);
		let mine = &self.bytes[self.segs_off..];
		let theirs = &other.bytes[self.segs_off..];
		theirs.starts_with(mine)
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

#[cfg(test)]
mod tests {
	use super::*;

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
		let buf: Vec<u8> = vec![1, 0, 0, 0, 0, 0xff];
		assert_eq!(
			MonikerView::from_bytes(&buf).unwrap_err(),
			EncodingError::TrailingBytes
		);
	}
}
