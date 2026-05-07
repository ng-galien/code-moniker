//! Build canonical moniker bytes step-by-step.

use super::encoding::{write_u16, HEADER_FIXED_LEN, SEG_HEADER_LEN, VERSION};
use super::{Moniker, MonikerView};
use crate::core::kind_registry::KindId;

#[derive(Default, Debug)]
pub struct MonikerBuilder {
	project: Vec<u8>,
	segments: Vec<(KindId, u16, Vec<u8>)>,
}

impl MonikerBuilder {
	pub fn new() -> Self {
		Self::default()
	}

	/// Seed a builder with the project + segments of an existing view.
	pub fn from_view(view: MonikerView<'_>) -> Self {
		let mut b = Self::new();
		b.project(view.project());
		for seg in view.segments() {
			if seg.arity != 0 {
				b.method(seg.kind, seg.bytes, seg.arity);
			} else {
				b.segment(seg.kind, seg.bytes);
			}
		}
		b
	}

	/// Truncate the segment list to `len` (no-op if already shorter).
	pub fn truncate(&mut self, len: usize) {
		self.segments.truncate(len);
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
		Moniker::from_canonical_bytes(buf)
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

#[cfg(test)]
mod tests {
	use super::*;
	use super::super::Segment;

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
}
