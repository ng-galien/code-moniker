//! Build canonical moniker bytes step-by-step.

use super::encoding::{write_u16, HEADER_FIXED_LEN, VERSION};
use super::{Moniker, MonikerView};

#[derive(Default, Debug)]
pub struct MonikerBuilder {
	project: Vec<u8>,
	segments: Vec<(Vec<u8>, Vec<u8>)>,
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
			b.segment(seg.kind, seg.name);
		}
		b
	}

	pub fn truncate(&mut self, len: usize) {
		self.segments.truncate(len);
	}

	pub fn project(&mut self, project: &[u8]) -> &mut Self {
		self.project.clear();
		self.project.extend_from_slice(project);
		self
	}

	/// Append a segment. `kind` is a kind name (e.g. `b"class"`,
	/// `b"method"`), `name` is the segment label. Method overload
	/// disambiguators live inside `name` (e.g. `b"findById(2)"`).
	pub fn segment(&mut self, kind: &[u8], name: &[u8]) -> &mut Self {
		self.segments.push((kind.to_vec(), name.to_vec()));
		self
	}

	pub fn build(&self) -> Moniker {
		let mut buf = Vec::with_capacity(self.estimated_size());
		buf.push(VERSION);
		write_u16(&mut buf, self.project.len() as u16);
		buf.extend_from_slice(&self.project);
		for (kind, name) in &self.segments {
			write_u16(&mut buf, kind.len() as u16);
			buf.extend_from_slice(kind);
			write_u16(&mut buf, name.len() as u16);
			buf.extend_from_slice(name);
		}
		Moniker::from_canonical_bytes(buf)
	}

	fn estimated_size(&self) -> usize {
		HEADER_FIXED_LEN
			+ self.project.len()
			+ self
				.segments
				.iter()
				.map(|(k, n)| 2 + k.len() + 2 + n.len())
				.sum::<usize>()
	}
}

#[cfg(test)]
mod tests {
	use super::super::Segment;
	use super::*;

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
			.segment(b"module", b"main")
			.segment(b"package", b"com")
			.segment(b"package", b"acme")
			.segment(b"class", b"Foo")
			.build();
		let v = m.as_view();
		assert_eq!(v.segment_count(), 4);
		let segs: Vec<_> = v.segments().collect();
		assert_eq!(segs[0], Segment { kind: b"module", name: b"main" });
		assert_eq!(segs[3], Segment { kind: b"class", name: b"Foo" });
	}

	#[test]
	fn builder_method_with_arity_in_name() {
		let m = MonikerBuilder::new()
			.project(b"app")
			.segment(b"class", b"Foo")
			.segment(b"method", b"bar()")
			.segment(b"method", b"bar(2)")
			.build();
		let segs: Vec<_> = m.as_view().segments().collect();
		assert_eq!(segs[1].name, b"bar()");
		assert_eq!(segs[2].name, b"bar(2)");
	}
}
