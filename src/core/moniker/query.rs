//! Tree-position queries on a [`Moniker`].

use super::{Moniker, MonikerBuilder};

impl Moniker {
	pub fn is_ancestor_of(&self, other: &Moniker) -> bool {
		self.as_view().is_ancestor_of(&other.as_view())
	}

	pub fn parent(&self) -> Option<Moniker> {
		let view = self.as_view();
		let n = view.segment_count() as usize;
		if n == 0 {
			return None;
		}
		let mut b = MonikerBuilder::from_view(view);
		b.truncate(n - 1);
		Some(b.build())
	}

	/// Kind of the last segment, if any. The returned slice borrows from
	/// the moniker's bytes.
	pub fn last_kind(&self) -> Option<Vec<u8>> {
		self.as_view().segments().last().map(|s| s.kind.to_vec())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn mk(project: &[u8], segs: &[(&[u8], &[u8])]) -> Moniker {
		let mut b = MonikerBuilder::new();
		b.project(project);
		for (k, name) in segs {
			b.segment(k, name);
		}
		b.build()
	}

	#[test]
	fn ancestor_is_reflexive() {
		let m = mk(b"app", &[(b"path", b"a"), (b"path", b"b")]);
		assert!(m.is_ancestor_of(&m));
	}

	#[test]
	fn ancestor_of_strict_prefix() {
		let parent = mk(b"app", &[(b"path", b"a")]);
		let child = mk(b"app", &[(b"path", b"a"), (b"path", b"b")]);
		assert!(parent.is_ancestor_of(&child));
		assert!(!child.is_ancestor_of(&parent));
	}

	#[test]
	fn ancestor_rejects_different_project() {
		let a = mk(b"app1", &[(b"path", b"x")]);
		let b = mk(b"app2", &[(b"path", b"x"), (b"path", b"y")]);
		assert!(!a.is_ancestor_of(&b));
	}

	#[test]
	fn ancestor_rejects_diverging_segment() {
		let a = mk(b"app", &[(b"path", b"a"), (b"path", b"b")]);
		let b = mk(b"app", &[(b"path", b"a"), (b"path", b"c")]);
		assert!(!a.is_ancestor_of(&b));
	}

	#[test]
	fn parent_drops_last_segment() {
		let m = mk(b"app", &[(b"path", b"a"), (b"path", b"b")]);
		let p = m.parent().unwrap();
		let expected = mk(b"app", &[(b"path", b"a")]);
		assert_eq!(p, expected);
	}

	#[test]
	fn parent_of_project_only_is_none() {
		let m = mk(b"app", &[]);
		assert!(m.parent().is_none());
	}

	#[test]
	fn parent_of_one_segment_is_project_only() {
		let m = mk(b"app", &[(b"path", b"a")]);
		let p = m.parent().unwrap();
		assert_eq!(p.as_view().segment_count(), 0);
		assert_eq!(p.as_view().project(), b"app");
	}

	#[test]
	fn last_kind_returns_kind_of_last_segment() {
		let m = mk(b"app", &[(b"path", b"a"), (b"class", b"Foo")]);
		assert_eq!(m.last_kind(), Some(b"class".to_vec()));
	}

	#[test]
	fn last_kind_is_none_for_project_only() {
		let m = mk(b"app", &[]);
		assert!(m.last_kind().is_none());
	}

	/// The crucial v2 invariant: byte-lex order coincides with tree
	/// pre-order. Parent < every descendant < every later sibling.
	#[test]
	fn byte_lex_is_tree_friendly() {
		let m1 = mk(b"app", &[(b"class", b"Foo")]);
		let descendant = mk(
			b"app",
			&[(b"class", b"Foo"), (b"method", b"bar()")],
		);
		let deeper = mk(
			b"app",
			&[
				(b"class", b"Foo"),
				(b"method", b"bar()"),
				(b"path", b"x"),
			],
		);
		// A different sibling at the same depth, byte-greater than m1.
		let sibling = mk(b"app", &[(b"class", b"Zoo")]);

		assert!(m1.as_bytes() < descendant.as_bytes());
		assert!(descendant.as_bytes() < deeper.as_bytes());
		assert!(deeper.as_bytes() < sibling.as_bytes());
	}

	/// In v1 the fixed-offset seg_count broke this when a sibling's
	/// segment was longer than the parent's. The v2 layout dropped
	/// seg_count, so longer descendants no longer leapfrog.
	#[test]
	fn descendant_with_longer_name_stays_inside_parent_range() {
		let parent = mk(b"app", &[(b"class", b"Foo")]);
		let child_long = mk(
			b"app",
			&[(b"class", b"Foo"), (b"method", b"bar_longer_than_anything()")],
		);
		let next_sibling = mk(b"app", &[(b"class", b"Zoo")]);

		assert!(parent.as_bytes() < child_long.as_bytes());
		assert!(child_long.as_bytes() < next_sibling.as_bytes());
	}
}
