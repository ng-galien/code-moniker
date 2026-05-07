//! `code_graph` — internal structure of a single module.
//!
//! A graph holds:
//!
//! - **Defs** — every node defined inside the module: the module itself
//!   (the root), then its types, members, nested functions, and so on,
//!   each addressed by its [`Moniker`] and tagged with a [`KindId`].
//! - **Refs** — outgoing references from a def to any other moniker
//!   (in this graph or another), tagged with a kind (call, import,
//!   extends, …).
//! - **Tree** — the parent/child relation between defs, encoded as a
//!   `parent: Option<usize>` index on each [`DefRecord`].
//!
//! Defs are stored in insertion order; the root is at index 0. Lookup
//! by moniker is currently linear (`O(N)`); typical fanout of a single
//! module makes this trivial. A faster lookup index is a future
//! concern, not a model concern.
//!
//! Mutation is in-place by design: a graph is built incrementally
//! during extraction, then frozen by the caller. Cloning is cheap
//! enough for the SQL surface (each `graph_add_def` call returns a new
//! cloned graph at the pgrx layer).

use crate::core::kind_registry::KindId;
use crate::core::moniker::Moniker;

/// Byte-range position in a source file (start, end), exclusive end.
pub type Position = (u32, u32);

/// One node of a code graph.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DefRecord {
	pub moniker: Moniker,
	pub kind: KindId,
	/// Index of this def's parent in the graph's def list. `None` for
	/// the root. Always points at a smaller index than this def's own.
	pub parent: Option<usize>,
	/// Position in source text. `None` for synthetic / external graphs
	/// that have no source.
	pub position: Option<Position>,
}

/// One outgoing reference of a code graph.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RefRecord {
	/// Index of the def from which this ref originates.
	pub source: usize,
	/// Target of the reference. May be any moniker — within this
	/// graph (intra-module) or outside it (cross-module).
	pub target: Moniker,
	pub kind: KindId,
	pub position: Option<Position>,
}

/// Errors raised by graph mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphError {
	/// `add_def` was called with a `parent` moniker not in the graph.
	ParentNotFound,
	/// `add_ref` was called with a `source` moniker not in the graph.
	SourceNotFound,
	/// `add_def` was called with a moniker already used by another def.
	DuplicateMoniker,
}

impl std::fmt::Display for GraphError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::ParentNotFound => write!(f, "parent moniker not found in graph"),
			Self::SourceNotFound => write!(f, "ref source moniker not found in graph"),
			Self::DuplicateMoniker => write!(f, "duplicate moniker in graph defs"),
		}
	}
}

impl std::error::Error for GraphError {}

/// Internal structure of one module.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CodeGraph {
	defs: Vec<DefRecord>,
	refs: Vec<RefRecord>,
}

impl CodeGraph {
	/// Create a graph with a single root def.
	pub fn new(root: Moniker, root_kind: KindId) -> Self {
		Self {
			defs: vec![DefRecord {
				moniker: root,
				kind: root_kind,
				parent: None,
				position: None,
			}],
			refs: Vec::new(),
		}
	}

	/// Add a def to the graph.
	///
	/// `parent` must already exist in the graph; otherwise [`GraphError::ParentNotFound`].
	/// The new def's moniker must be distinct from every existing def;
	/// otherwise [`GraphError::DuplicateMoniker`].
	pub fn add_def(
		&mut self,
		moniker: Moniker,
		kind: KindId,
		parent: &Moniker,
		position: Option<Position>,
	) -> Result<(), GraphError> {
		if self.find_def(&moniker).is_some() {
			return Err(GraphError::DuplicateMoniker);
		}
		let parent_idx = self.find_def(parent).ok_or(GraphError::ParentNotFound)?;
		self.defs.push(DefRecord {
			moniker,
			kind,
			parent: Some(parent_idx),
			position,
		});
		Ok(())
	}

	/// Add a ref to the graph.
	///
	/// `source` must be a def in the graph; otherwise
	/// [`GraphError::SourceNotFound`]. The `target` may be any moniker
	/// — inside or outside this graph.
	pub fn add_ref(
		&mut self,
		source: &Moniker,
		target: Moniker,
		kind: KindId,
		position: Option<Position>,
	) -> Result<(), GraphError> {
		let source_idx = self.find_def(source).ok_or(GraphError::SourceNotFound)?;
		self.refs.push(RefRecord {
			source: source_idx,
			target,
			kind,
			position,
		});
		Ok(())
	}

	/// The root moniker (the module's own identity).
	pub fn root(&self) -> &Moniker {
		&self.defs[0].moniker
	}

	/// Does this graph define this moniker?
	///
	/// This is the `code_graph @> moniker` operator at the type level.
	pub fn contains(&self, m: &Moniker) -> bool {
		self.find_def(m).is_some()
	}

	/// Iterate defs in insertion order. Index 0 is the root.
	pub fn defs(&self) -> impl Iterator<Item = &DefRecord> {
		self.defs.iter()
	}

	/// Iterate outgoing refs in insertion order.
	pub fn refs(&self) -> impl Iterator<Item = &RefRecord> {
		self.refs.iter()
	}

	/// Position of a def, if it exists and carries source coordinates.
	pub fn locate(&self, m: &Moniker) -> Option<Position> {
		self.find_def(m).and_then(|i| self.defs[i].position)
	}

	/// All def monikers, sorted by their canonical byte representation.
	/// Suitable as input to a GiST array index.
	pub fn def_monikers(&self) -> Vec<Moniker> {
		let mut v: Vec<Moniker> = self.defs.iter().map(|d| d.moniker.clone()).collect();
		v.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
		v
	}

	/// All outgoing ref targets, sorted. Duplicates preserved (a single
	/// def can issue multiple refs to the same target with different
	/// kinds or positions).
	pub fn ref_targets(&self) -> Vec<Moniker> {
		let mut v: Vec<Moniker> = self.refs.iter().map(|r| r.target.clone()).collect();
		v.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
		v
	}

	pub fn def_count(&self) -> usize {
		self.defs.len()
	}

	pub fn ref_count(&self) -> usize {
		self.refs.len()
	}

	fn find_def(&self, m: &Moniker) -> Option<usize> {
		self.defs.iter().position(|d| &d.moniker == m)
	}
}

// -----------------------------------------------------------------------------
// Tests (TDD: behaviour spec first, kept beside the implementation)
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::kind_registry::KindId;
	use crate::core::moniker::MonikerBuilder;

	fn kid(n: u16) -> KindId {
		KindId::from_raw(n)
	}

	/// Build a project-rooted moniker with one path-level segment,
	/// for tests.
	fn mk(seg: &[u8]) -> Moniker {
		MonikerBuilder::new()
			.project(b"app")
			.segment(kid(1), seg)
			.build()
	}

	// --- new ----------------------------------------------------------

	#[test]
	fn new_graph_has_only_root() {
		let root = mk(b"util");
		let g = CodeGraph::new(root.clone(), kid(1));
		assert_eq!(g.root(), &root);
		assert_eq!(g.def_count(), 1);
		assert_eq!(g.ref_count(), 0);
		assert!(g.contains(&root));
	}

	// --- add_def ------------------------------------------------------

	#[test]
	fn add_def_attaches_to_existing_parent() {
		let root = mk(b"util");
		let child = mk(b"util_child");
		let mut g = CodeGraph::new(root.clone(), kid(1));
		g.add_def(child.clone(), kid(2), &root, Some((10, 20))).unwrap();

		assert_eq!(g.def_count(), 2);
		assert!(g.contains(&child));
		assert_eq!(g.locate(&child), Some((10, 20)));
	}

	#[test]
	fn add_def_unknown_parent_fails() {
		let root = mk(b"util");
		let unknown = mk(b"nope");
		let child = mk(b"child");
		let mut g = CodeGraph::new(root, kid(1));
		assert_eq!(
			g.add_def(child, kid(2), &unknown, None).unwrap_err(),
			GraphError::ParentNotFound
		);
	}

	#[test]
	fn add_def_duplicate_moniker_fails() {
		let root = mk(b"util");
		let dup = mk(b"util"); // same bytes as root
		let mut g = CodeGraph::new(root.clone(), kid(1));
		assert_eq!(
			g.add_def(dup, kid(2), &root, None).unwrap_err(),
			GraphError::DuplicateMoniker
		);
	}

	#[test]
	fn add_def_records_parent_index() {
		let root = mk(b"util");
		let a = mk(b"a");
		let b = mk(b"b");
		let mut g = CodeGraph::new(root.clone(), kid(1));
		g.add_def(a.clone(), kid(2), &root, None).unwrap();
		g.add_def(b.clone(), kid(2), &a, None).unwrap();

		let defs: Vec<_> = g.defs().collect();
		assert_eq!(defs[0].parent, None);
		assert_eq!(defs[1].parent, Some(0));
		assert_eq!(defs[2].parent, Some(1));
	}

	// --- add_ref ------------------------------------------------------

	#[test]
	fn add_ref_records_source_index_and_target() {
		let root = mk(b"util");
		let foo = mk(b"foo");
		let target = mk(b"external_thing");
		let mut g = CodeGraph::new(root.clone(), kid(1));
		g.add_def(foo.clone(), kid(2), &root, None).unwrap();
		g.add_ref(&foo, target.clone(), kid(3), Some((5, 8))).unwrap();

		assert_eq!(g.ref_count(), 1);
		let r = g.refs().next().unwrap();
		assert_eq!(r.source, 1); // foo is at index 1
		assert_eq!(&r.target, &target);
		assert_eq!(r.kind, kid(3));
		assert_eq!(r.position, Some((5, 8)));
	}

	#[test]
	fn add_ref_unknown_source_fails() {
		let root = mk(b"util");
		let unknown = mk(b"nope");
		let target = mk(b"target");
		let mut g = CodeGraph::new(root, kid(1));
		assert_eq!(
			g.add_ref(&unknown, target, kid(3), None).unwrap_err(),
			GraphError::SourceNotFound
		);
	}

	#[test]
	fn ref_target_may_be_outside_graph() {
		// The ref's target is not required to be a def in this graph.
		// Cross-module refs are normal.
		let root = mk(b"util");
		let foo = mk(b"foo");
		let outside = mk(b"some_external_moniker");
		let mut g = CodeGraph::new(root.clone(), kid(1));
		g.add_def(foo.clone(), kid(2), &root, None).unwrap();
		g.add_ref(&foo, outside.clone(), kid(3), None).unwrap();

		assert!(!g.contains(&outside));
		assert_eq!(g.ref_count(), 1);
	}

	// --- contains / locate --------------------------------------------

	#[test]
	fn contains_distinguishes_existing_and_unknown() {
		let root = mk(b"util");
		let foo = mk(b"foo");
		let unknown = mk(b"unknown");
		let mut g = CodeGraph::new(root.clone(), kid(1));
		g.add_def(foo.clone(), kid(2), &root, None).unwrap();
		assert!(g.contains(&root));
		assert!(g.contains(&foo));
		assert!(!g.contains(&unknown));
	}

	#[test]
	fn locate_returns_none_when_no_position() {
		let root = mk(b"util");
		let foo = mk(b"foo");
		let mut g = CodeGraph::new(root.clone(), kid(1));
		g.add_def(foo.clone(), kid(2), &root, None).unwrap();
		assert_eq!(g.locate(&foo), None);
	}

	#[test]
	fn locate_returns_none_for_unknown_moniker() {
		let root = mk(b"util");
		let unknown = mk(b"unknown");
		let g = CodeGraph::new(root, kid(1));
		assert_eq!(g.locate(&unknown), None);
	}

	// --- def_monikers / ref_targets -----------------------------------

	#[test]
	fn def_monikers_returns_all_sorted() {
		let root = mk(b"a_root");
		let a = mk(b"c_zzz");
		let b = mk(b"b_aaa");
		let mut g = CodeGraph::new(root.clone(), kid(1));
		g.add_def(a.clone(), kid(2), &root, None).unwrap();
		g.add_def(b.clone(), kid(2), &root, None).unwrap();

		let monikers = g.def_monikers();
		assert_eq!(monikers.len(), 3);
		// Sorted by canonical bytes.
		for w in monikers.windows(2) {
			assert!(w[0].as_bytes() <= w[1].as_bytes());
		}
		// All monikers present.
		assert!(monikers.contains(&root));
		assert!(monikers.contains(&a));
		assert!(monikers.contains(&b));
	}

	#[test]
	fn ref_targets_collects_all_with_duplicates() {
		let root = mk(b"util");
		let foo = mk(b"foo");
		let bar = mk(b"bar");
		let target_a = mk(b"target_a");
		let target_b = mk(b"target_b");

		let mut g = CodeGraph::new(root.clone(), kid(1));
		g.add_def(foo.clone(), kid(2), &root, None).unwrap();
		g.add_def(bar.clone(), kid(2), &root, None).unwrap();
		g.add_ref(&foo, target_a.clone(), kid(3), None).unwrap();
		g.add_ref(&foo, target_b.clone(), kid(3), None).unwrap();
		// Same target referenced twice — duplicates are preserved in the
		// flattened output.
		g.add_ref(&bar, target_a.clone(), kid(3), None).unwrap();

		let targets = g.ref_targets();
		assert_eq!(targets.len(), 3);
		// Sorted.
		for w in targets.windows(2) {
			assert!(w[0].as_bytes() <= w[1].as_bytes());
		}
	}

	// --- equality / clone ---------------------------------------------

	#[test]
	fn clone_produces_equal_graph() {
		let root = mk(b"util");
		let foo = mk(b"foo");
		let mut g = CodeGraph::new(root.clone(), kid(1));
		g.add_def(foo.clone(), kid(2), &root, Some((1, 2))).unwrap();
		g.add_ref(&foo, mk(b"ext"), kid(3), Some((4, 5))).unwrap();

		let clone = g.clone();
		assert_eq!(g, clone);
	}

	#[test]
	fn equal_graphs_have_equal_hashes() {
		use std::collections::hash_map::DefaultHasher;
		use std::hash::{Hash, Hasher};

		let root = mk(b"util");
		let foo = mk(b"foo");
		let make = || {
			let mut g = CodeGraph::new(root.clone(), kid(1));
			g.add_def(foo.clone(), kid(2), &root, None).unwrap();
			g
		};
		let g1 = make();
		let g2 = make();
		assert_eq!(g1, g2);

		let mut h1 = DefaultHasher::new();
		let mut h2 = DefaultHasher::new();
		g1.hash(&mut h1);
		g2.hash(&mut h2);
		assert_eq!(h1.finish(), h2.finish());
	}
}
