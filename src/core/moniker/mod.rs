//! Moniker — byte-compact native representation of a node identity in
//! the canonical project tree.
//!
//! The format is **SCIP-inspired**: the URI representation
//! ([`crate::core::uri`]) follows SCIP descriptor conventions
//! (`Foo#`, `bar().`, `field.`, …) and the binary representation
//! mirrors that structure as a sequence of `(kind, arity, name)`
//! segments. See [`encoding`] for the byte layout.
//!
//! `arity` is meaningful only for segments whose kind has
//! [`crate::core::kind_registry::PunctClass::Method`]; it is `0` for the
//! arity-less form (`bar().`) and `N` for an arity disambiguator
//! (`bar(N).`). For other punct classes it is required to be `0`.

mod builder;
mod encoding;
mod query;
mod view;

pub use builder::MonikerBuilder;
pub use encoding::EncodingError;
pub use view::{MonikerView, Segment, SegmentIter};

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Moniker {
	bytes: Vec<u8>,
}

impl Moniker {
	pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, EncodingError> {
		MonikerView::from_bytes(&bytes)?;
		Ok(Self { bytes })
	}

	/// Internal constructor used by [`MonikerBuilder`] which produces
	/// canonical bytes by construction; skips the view re-walk.
	pub(super) fn from_canonical_bytes(bytes: Vec<u8>) -> Self {
		Self { bytes }
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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::kind_registry::KindId;

	fn kid(n: u16) -> KindId {
		KindId::from_raw(n)
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
