//! Tree-position queries on a [`Moniker`].
//!
//! These operations treat the moniker as a node in the canonical
//! project tree (project authority + ordered segments) and answer
//! ancestor / parent / path / kind questions used by the SQL surface
//! (`<@`, `@>`, `parent_of`, `path_of`, `kind_of`).

use super::{Moniker, MonikerBuilder};
use crate::core::kind_registry::KindId;

impl Moniker {
	/// Convenience wrapper. The canonical impl lives on
	/// [`super::MonikerView::is_ancestor_of`] so callers holding views
	/// (the pgrx wrappers, the GiST opclass) skip the buffer clone.
	pub fn is_ancestor_of(&self, other: &Moniker) -> bool {
		self.as_view().is_ancestor_of(&other.as_view())
	}

	/// The parent in the canonical tree: same project, last segment
	/// dropped. `None` when the moniker is the project authority alone.
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

	/// Kind of the last segment, if any.
	pub fn last_kind(&self) -> Option<KindId> {
		self.as_view().segments().last().map(|s| s.kind)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::kind_registry::KindId;

	fn kid(n: u16) -> KindId {
		KindId::from_raw(n)
	}

	fn mk(project: &[u8], segs: &[(KindId, &[u8])]) -> Moniker {
		let mut b = MonikerBuilder::new();
		b.project(project);
		for (k, name) in segs {
			b.segment(*k, name);
		}
		b.build()
	}

	#[test]
	fn ancestor_is_reflexive() {
		let m = mk(b"app", &[(kid(1), b"a"), (kid(1), b"b")]);
		assert!(m.is_ancestor_of(&m));
	}

	#[test]
	fn ancestor_of_strict_prefix() {
		let parent = mk(b"app", &[(kid(1), b"a")]);
		let child = mk(b"app", &[(kid(1), b"a"), (kid(1), b"b")]);
		assert!(parent.is_ancestor_of(&child));
		assert!(!child.is_ancestor_of(&parent));
	}

	#[test]
	fn ancestor_rejects_different_project() {
		let a = mk(b"app1", &[(kid(1), b"x")]);
		let b = mk(b"app2", &[(kid(1), b"x"), (kid(1), b"y")]);
		assert!(!a.is_ancestor_of(&b));
	}

	#[test]
	fn ancestor_rejects_diverging_segment() {
		let a = mk(b"app", &[(kid(1), b"a"), (kid(1), b"b")]);
		let b = mk(b"app", &[(kid(1), b"a"), (kid(1), b"c")]);
		assert!(!a.is_ancestor_of(&b));
	}

	#[test]
	fn parent_drops_last_segment() {
		let m = mk(b"app", &[(kid(1), b"a"), (kid(1), b"b")]);
		let p = m.parent().unwrap();
		let expected = mk(b"app", &[(kid(1), b"a")]);
		assert_eq!(p, expected);
	}

	#[test]
	fn parent_of_project_only_is_none() {
		let m = mk(b"app", &[]);
		assert!(m.parent().is_none());
	}

	#[test]
	fn parent_of_one_segment_is_project_only() {
		let m = mk(b"app", &[(kid(1), b"a")]);
		let p = m.parent().unwrap();
		assert_eq!(p.as_view().segment_count(), 0);
		assert_eq!(p.as_view().project(), b"app");
	}

	#[test]
	fn last_kind_returns_kind_of_last_segment() {
		let m = mk(b"app", &[(kid(1), b"a"), (kid(7), b"Foo")]);
		assert_eq!(m.last_kind(), Some(kid(7)));
	}

	#[test]
	fn last_kind_is_none_for_project_only() {
		let m = mk(b"app", &[]);
		assert!(m.last_kind().is_none());
	}
}
