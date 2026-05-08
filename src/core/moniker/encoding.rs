//! ```text
//! [version u8 = 2]
//! [project_len u16 LE] [project bytes]
//!   segment[i] := [kind_len u16 LE] [kind bytes] [name_len u16 LE] [name bytes]
//!   (repeated until end of buffer)
//! ```

use std::fmt;

pub(crate) const VERSION: u8 = 2;
pub(crate) const HEADER_FIXED_LEN: usize = 1 + 2;

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
