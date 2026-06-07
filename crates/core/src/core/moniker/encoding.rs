//! ```text
//! [version u8 = 2]
//! [project_len u16 LE] [project bytes]
//!   segment[i] := [kind_len u16 LE] [kind bytes] [name_len u16 LE] [name bytes]
//!   (repeated until end of buffer)
//! ```

use std::fmt;

pub const VERSION: u8 = 2;
pub const HEADER_FIXED_LEN: usize = 1 + 2;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum EncodingError {
	Truncated,
	UnknownVersion(u8),
	ProjectOverflow,
	SegmentOverflow,
	EmptyProject,
	NonUtf8Project,
	NonUtf8SegmentKind,
	NonUtf8SegmentName,
	InvalidSegmentKind,
	ProjectTooLong(usize),
	SegmentKindTooLong(usize),
	SegmentNameTooLong(usize),
}

impl fmt::Display for EncodingError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Truncated => write!(f, "buffer too short for header"),
			Self::UnknownVersion(v) => write!(f, "unknown encoding version: {v}"),
			Self::ProjectOverflow => write!(f, "project bytes extend past buffer"),
			Self::SegmentOverflow => write!(f, "segment extends past buffer"),
			Self::EmptyProject => write!(f, "project must not be empty"),
			Self::NonUtf8Project => write!(f, "project must be valid UTF-8"),
			Self::NonUtf8SegmentKind => write!(f, "segment kind must be valid UTF-8"),
			Self::NonUtf8SegmentName => write!(f, "segment name must be valid UTF-8"),
			Self::InvalidSegmentKind => write!(
				f,
				"segment kind must be a URI identifier ([A-Za-z][A-Za-z0-9_]*)"
			),
			Self::ProjectTooLong(len) => {
				write!(f, "project longer than u16::MAX bytes ({len})")
			}
			Self::SegmentKindTooLong(len) => {
				write!(f, "segment kind longer than u16::MAX bytes ({len})")
			}
			Self::SegmentNameTooLong(len) => {
				write!(f, "segment name longer than u16::MAX bytes ({len})")
			}
		}
	}
}

impl std::error::Error for EncodingError {}

pub fn read_u16(buf: &[u8], off: usize) -> u16 {
	u16::from_le_bytes([buf[off], buf[off + 1]])
}

pub fn write_u16(buf: &mut Vec<u8>, value: u16) {
	buf.extend_from_slice(&value.to_le_bytes());
}

pub fn is_uri_kind(bytes: &[u8]) -> bool {
	let Some((first, rest)) = bytes.split_first() else {
		return false;
	};
	first.is_ascii_alphabetic()
		&& rest
			.iter()
			.all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}
