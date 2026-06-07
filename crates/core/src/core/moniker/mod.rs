mod builder;
pub mod encoding;
pub mod query;
mod view;

pub use builder::MonikerBuilder;
pub use encoding::EncodingError;
pub use view::{MonikerView, Segment, SegmentIter};

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct Moniker {
	bytes: Box<[u8]>,
}

impl Moniker {
	pub fn from_encoded(bytes: Vec<u8>) -> Result<Self, EncodingError> {
		MonikerView::from_encoded(&bytes)?;
		Ok(Self {
			bytes: bytes.into_boxed_slice(),
		})
	}

	pub(crate) fn from_encoded_unchecked(bytes: Vec<u8>) -> Self {
		Self {
			bytes: bytes.into_boxed_slice(),
		}
	}

	pub fn as_view(&self) -> MonikerView<'_> {
		unsafe { MonikerView::from_encoded_unchecked(&self.bytes) }
	}

	pub fn as_encoded(&self) -> &[u8] {
		&self.bytes
	}

	pub fn into_encoded(self) -> Vec<u8> {
		self.bytes.into_vec()
	}
}

#[cfg(feature = "serde")]
impl serde::Serialize for Moniker {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		use serde::ser::SerializeStruct;

		let mut state = serializer.serialize_struct("Moniker", 1)?;
		state.serialize_field("bytes", &self.bytes)?;
		state.end()
	}
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Moniker {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		#[derive(serde::Deserialize)]
		struct Repr {
			bytes: Vec<u8>,
		}

		let repr = Repr::deserialize(deserializer)?;
		Self::from_encoded(repr.bytes).map_err(serde::de::Error::custom)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn builder_rebuilds_same_encoding_from_view() {
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

		assert_eq!(m1.as_encoded(), m2.as_encoded());
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
	fn from_encoded_roundtrip() {
		let m = MonikerBuilder::new()
			.project(b"pj")
			.segment(b"path", b"foo")
			.build();
		let bytes = m.clone().into_encoded();
		let m2 = Moniker::from_encoded(bytes).unwrap();
		assert_eq!(m, m2);
		assert!(Moniker::from_encoded(vec![99u8; 5]).is_err());
	}

	#[cfg(feature = "serde")]
	#[test]
	fn serde_rejects_invalid_encoded_moniker() {
		let err = serde_json::from_str::<Moniker>(r#"{"bytes":[2,0,0]}"#).unwrap_err();
		assert!(err.to_string().contains("project must not be empty"));
	}

	#[cfg(feature = "serde")]
	#[test]
	fn serde_roundtrips_valid_moniker() {
		let m = MonikerBuilder::new()
			.project(b"pj")
			.segment(b"path", b"foo")
			.build();
		let json = serde_json::to_string(&m).unwrap();
		let decoded: Moniker = serde_json::from_str(&json).unwrap();
		assert_eq!(decoded, m);
	}

	use proptest::prelude::*;

	proptest! {
		#![proptest_config(ProptestConfig {
			cases: 256,
			..ProptestConfig::default()
		})]

		#[test]
		fn moniker_from_encoded_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
			if let Ok(m) = Moniker::from_encoded(bytes.clone()) {
				prop_assert_eq!(m.as_encoded(), bytes.as_slice());
				let m2 = Moniker::from_encoded(m.as_encoded().to_vec())
					.expect("validated encoding must re-parse");
				prop_assert_eq!(m, m2);
			}
		}

		#[test]
		fn moniker_view_and_owned_agree(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
			let owned = Moniker::from_encoded(bytes.clone()).is_ok();
			let view = MonikerView::from_encoded(&bytes).is_ok();
			prop_assert_eq!(owned, view);
		}
	}
}
