mod builder;
pub(crate) mod encoding;
pub(crate) mod query;
mod view;

pub use builder::MonikerBuilder;
pub use encoding::EncodingError;
pub use view::{MonikerView, Segment, SegmentIter};

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Moniker {
	bytes: Vec<u8>,
}

impl Moniker {
	pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, EncodingError> {
		MonikerView::from_bytes(&bytes)?;
		Ok(Self { bytes })
	}

	pub(crate) fn from_canonical_bytes(bytes: Vec<u8>) -> Self {
		Self { bytes }
	}

	pub fn as_view(&self) -> MonikerView<'_> {
		unsafe { MonikerView::from_canonical_bytes(&self.bytes) }
	}

	pub fn as_bytes(&self) -> &[u8] {
		&self.bytes
	}

	pub fn into_bytes(self) -> Vec<u8> {
		self.bytes
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn roundtrip_canonicality() {
		let m1 = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"module", b"main")
			.segment(b"class", b"Foo")
			.segment(b"method", b"bar(2)")
			.build();

		let v = m1.as_view();
		let mut b2 = MonikerBuilder::new();
		b2.project(v.project());
		for seg in v.segments() {
			b2.segment(seg.kind, seg.name);
		}
		let m2 = b2.build();

		assert_eq!(m1.as_bytes(), m2.as_bytes());
		assert_eq!(m1, m2);
	}

	#[test]
	fn eq_via_bytes() {
		let a = MonikerBuilder::new()
			.project(b"x")
			.segment(b"path", b"a")
			.build();
		let b = MonikerBuilder::new()
			.project(b"x")
			.segment(b"path", b"a")
			.build();
		let c = MonikerBuilder::new()
			.project(b"x")
			.segment(b"path", b"b")
			.build();
		assert_eq!(a, b);
		assert_ne!(a, c);
	}

	#[test]
	fn ord_places_parent_before_child() {
		let parent = MonikerBuilder::new()
			.project(b"app")
			.segment(b"module", b"main")
			.build();
		let child = MonikerBuilder::new()
			.project(b"app")
			.segment(b"module", b"main")
			.segment(b"class", b"Foo")
			.build();
		assert!(parent < child);
	}

	#[test]
	fn ord_separates_distinct_projects() {
		let a = MonikerBuilder::new().project(b"app1").build();
		let b = MonikerBuilder::new().project(b"app2").build();
		assert!(a < b);
	}

	#[test]
	fn from_bytes_roundtrip() {
		let m = MonikerBuilder::new()
			.project(b"pj")
			.segment(b"path", b"foo")
			.build();
		let bytes = m.clone().into_bytes();
		let m2 = Moniker::from_bytes(bytes).unwrap();
		assert_eq!(m, m2);
		assert!(Moniker::from_bytes(vec![99u8; 5]).is_err());
	}
}
