//! Binary layout constants and shared byte helpers.
//!
//! ```text
//! [version u8 = 2]
//! [project_len u16 LE] [project bytes]
//!   segment[i] := [kind_len u16 LE] [kind bytes] [name_len u16 LE] [name bytes]
//!   (repeated until end of buffer)
//! ```
//!
//! All multi-byte integers are little-endian. The format is canonical:
//! monikers logically equal are byte-identical (slice compare is enough
//! for `=`, GiST, etc.). The segment list is delimited by EOF (no
//! seg_count field) so byte-lex order coincides with tree pre-order
//! traversal: parent < every descendant < every later sibling, no
//! length-counter at fixed offset to perturb that invariant.

use std::fmt;

pub(crate) const VERSION: u8 = 2;
pub(crate) const HEADER_FIXED_LEN: usize =
	1   /* version     */
	+ 2 /* project_len */;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum EncodingError {
	Truncated,
	UnknownVersion(u8),
	ProjectOverflow,
	SegmentOverflow,
}

impl fmt::Display for EncodingError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Truncated => write!(f, "buffer too short for header"),
			Self::UnknownVersion(v) => write!(f, "unknown encoding version: {v}"),
			Self::ProjectOverflow => write!(f, "project bytes extend past buffer"),
			Self::SegmentOverflow => write!(f, "segment extends past buffer"),
		}
	}
}

impl std::error::Error for EncodingError {}

pub(crate) fn read_u16(buf: &[u8], off: usize) -> u16 {
	u16::from_le_bytes([buf[off], buf[off + 1]])
}

pub(crate) fn write_u16(buf: &mut Vec<u8>, value: u16) {
	buf.extend_from_slice(&value.to_le_bytes());
}
