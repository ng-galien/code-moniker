use super::encoding::{read_u16, EncodingError, HEADER_FIXED_LEN, VERSION};

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct Segment<'a> {
	pub kind: &'a [u8],
	pub name: &'a [u8],
}

#[derive(Copy, Clone, Debug)]
pub struct MonikerView<'a> {
	bytes: &'a [u8],
	project_off: usize,
	project_len: usize,
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
		let segs_off = project_off + project_len;
		if bytes.len() < segs_off {
			return Err(EncodingError::ProjectOverflow);
		}
		let mut cursor = segs_off;
		while cursor < bytes.len() {
			if bytes.len() < cursor + 2 {
				return Err(EncodingError::SegmentOverflow);
			}
			let kind_len = read_u16(bytes, cursor) as usize;
			cursor += 2 + kind_len;
			if bytes.len() < cursor + 2 {
				return Err(EncodingError::SegmentOverflow);
			}
			let name_len = read_u16(bytes, cursor) as usize;
			cursor += 2 + name_len;
			if bytes.len() < cursor {
				return Err(EncodingError::SegmentOverflow);
			}
		}
		Ok(Self {
			bytes,
			project_off,
			project_len,
			segs_off,
		})
	}

	pub(crate) unsafe fn from_canonical_bytes(bytes: &'a [u8]) -> Self {
		debug_assert!(bytes.len() >= HEADER_FIXED_LEN && bytes[0] == VERSION);
		let project_len = read_u16(bytes, 1) as usize;
		let project_off = 3;
		let segs_off = project_off + project_len;
		Self {
			bytes,
			project_off,
			project_len,
			segs_off,
		}
	}

	pub fn project(&self) -> &'a [u8] {
		&self.bytes[self.project_off..self.project_off + self.project_len]
	}

	pub fn segment_count(&self) -> u16 {
		self.segments().count() as u16
	}

	pub fn segments(&self) -> SegmentIter<'a> {
		SegmentIter {
			bytes: self.bytes,
			cursor: self.segs_off,
		}
	}

	pub fn as_bytes(&self) -> &'a [u8] {
		self.bytes
	}

	pub fn is_ancestor_of(&self, other: &MonikerView<'_>) -> bool {
		if self.project() != other.project() {
			return false;
		}
		other.bytes.starts_with(self.bytes)
	}
}

#[derive(Clone, Debug)]
pub struct SegmentIter<'a> {
	bytes: &'a [u8],
	cursor: usize,
}

impl<'a> Iterator for SegmentIter<'a> {
	type Item = Segment<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		if self.cursor >= self.bytes.len() {
			return None;
		}
		let kind_len = read_u16(self.bytes, self.cursor) as usize;
		let kind_start = self.cursor + 2;
		let kind = &self.bytes[kind_start..kind_start + kind_len];
		let name_len_off = kind_start + kind_len;
		let name_len = read_u16(self.bytes, name_len_off) as usize;
		let name_start = name_len_off + 2;
		let name = &self.bytes[name_start..name_start + name_len];
		self.cursor = name_start + name_len;
		Some(Segment { kind, name })
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn view_rejects_truncated_buffer() {
		assert_eq!(
			MonikerView::from_bytes(&[2, 0]).unwrap_err(),
			EncodingError::Truncated
		);
	}

	#[test]
	fn view_rejects_unknown_version() {
		let buf = [99, 0, 0];
		assert_eq!(
			MonikerView::from_bytes(&buf).unwrap_err(),
			EncodingError::UnknownVersion(99)
		);
	}

	#[test]
	fn view_rejects_project_overflow() {
		let buf: Vec<u8> = vec![2, 10, 0, 0];
		assert_eq!(
			MonikerView::from_bytes(&buf).unwrap_err(),
			EncodingError::ProjectOverflow
		);
	}

	#[test]
	fn view_rejects_segment_overflow() {
		let buf: Vec<u8> = vec![2, 0, 0, 5, 0];
		assert_eq!(
			MonikerView::from_bytes(&buf).unwrap_err(),
			EncodingError::SegmentOverflow
		);
	}

	#[test]
	fn view_accepts_project_only() {
		let buf: Vec<u8> = vec![2, 3, 0, b'a', b'p', b'p'];
		let v = MonikerView::from_bytes(&buf).unwrap();
		assert_eq!(v.project(), b"app");
		assert_eq!(v.segment_count(), 0);
	}
}
