//! Binary layout constants and shared byte helpers.
//!
//! ```text
//! [version u8 = 1]
//! [project_len u16 LE] [project bytes]
//! [seg_count u16 LE]
//!   segment[i] := [kind u16 LE] [arity u16 LE] [seg_len u16 LE] [seg bytes]
//! ```
//!
//! All multi-byte integers are little-endian. The format is canonical
//! so that monikers logically equal are byte-identical (slice compare
//! is enough for `=`, GiST, etc.).

use std::fmt;

pub(super) const VERSION: u8 = 1;
pub(super) const HEADER_FIXED_LEN: usize =
	1   /* version     */
	+ 2 /* project_len */
	+ 2 /* seg_count   */;
pub(super) const SEG_HEADER_LEN: usize = 2 /* kind */ + 2 /* arity */ + 2 /* seg_len */;

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

pub(super) fn read_u16(buf: &[u8], off: usize) -> u16 {
	u16::from_le_bytes([buf[off], buf[off + 1]])
}

pub(super) fn write_u16(buf: &mut Vec<u8>, value: u16) {
	buf.extend_from_slice(&value.to_le_bytes());
}
