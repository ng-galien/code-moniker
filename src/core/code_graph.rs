//! `code_graph` — internal structure of a single module.
//!
//! Defs and refs are tagged by a kind name (byte string, e.g.
//! `b"class"`, `b"call"`) — not by a backend-local registry id, so
//! graphs are portable.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::core::moniker::Moniker;

pub type Position = (u32, u32);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DefRecord {
	pub moniker: Moniker,
	pub kind: Vec<u8>,
	pub parent: Option<usize>,
	pub position: Option<Position>,
	/// Language-normalised access level: `public`, `protected`,
	/// `package`, `private`, `module`. Empty when the language does not
	/// expose the concept for this def (locals, params, sections).
	pub visibility: Vec<u8>,
	/// Callable signature for languages where arity alone doesn't
	/// disambiguate overloads (Java parameter types, SQL argument
	/// types). Format is language-specific and consumer-opaque. Empty
	/// for non-callables and for languages that encode disambiguation
	/// in the moniker name (TS arity).
	pub signature: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RefRecord {
	pub source: usize,
	pub target: Moniker,
	pub kind: Vec<u8>,
	pub position: Option<Position>,
	/// Receiver shape for `method_call` refs (`this`, `super`,
	/// `identifier`, `member`, `call`, `subscript`). Empty otherwise.
	pub receiver_hint: Vec<u8>,
	/// Local binding name when the ref imports under a different name
	/// (`import { X as Y }`, `export { X as Y }`). Empty otherwise.
	pub alias: Vec<u8>,
	/// Confidence the consumer projection should attach: `external`,
	/// `imported`, `name_match`, `local`, `unresolved`. Empty when the
	/// extractor has nothing to assert.
	pub confidence: Vec<u8>,
}

/// Optional def attributes. Defaults are empty, meaning the extractor
/// has no info to record for this def.
#[derive(Clone, Debug, Default)]
pub struct DefAttrs<'a> {
	pub visibility: &'a [u8],
	pub signature: &'a [u8],
}

/// Optional ref attributes. Defaults are empty.
#[derive(Clone, Debug, Default)]
pub struct RefAttrs<'a> {
	pub receiver_hint: &'a [u8],
	pub alias: &'a [u8],
	pub confidence: &'a [u8],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphError {
	ParentNotFound,
	SourceNotFound,
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

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CodeGraph {
	defs: Vec<DefRecord>,
	refs: Vec<RefRecord>,
	/// `moniker → defs[i]` index, kept in sync with `defs`. Skipped on
	/// (de)serialization and rebuilt lazily; equality and hashing ignore
	/// it because it's a derived cache.
	#[cfg_attr(feature = "serde", serde(skip, default))]
	index: RefCell<HashMap<Moniker, usize>>,
}

impl PartialEq for CodeGraph {
	fn eq(&self, other: &Self) -> bool {
		self.defs == other.defs && self.refs == other.refs
	}
}

impl Eq for CodeGraph {}

impl Hash for CodeGraph {
	fn hash<H: Hasher>(&self, state: &mut H) {
		self.defs.hash(state);
		self.refs.hash(state);
	}
}

impl CodeGraph {
	pub fn new(root: Moniker, root_kind: &[u8]) -> Self {
		let mut index = HashMap::with_capacity(8);
		index.insert(root.clone(), 0);
		Self {
			defs: vec![DefRecord {
				moniker: root,
				kind: root_kind.to_vec(),
				parent: None,
				position: None,
				visibility: Vec::new(),
				signature: Vec::new(),
			}],
			refs: Vec::new(),
			index: RefCell::new(index),
		}
	}

	pub fn add_def(
		&mut self,
		moniker: Moniker,
		kind: &[u8],
		parent: &Moniker,
		position: Option<Position>,
	) -> Result<(), GraphError> {
		self.add_def_attrs(moniker, kind, parent, position, &DefAttrs::default())
	}

	pub fn add_def_attrs(
		&mut self,
		moniker: Moniker,
		kind: &[u8],
		parent: &Moniker,
		position: Option<Position>,
		attrs: &DefAttrs<'_>,
	) -> Result<(), GraphError> {
		if self.find_def(&moniker).is_some() {
			return Err(GraphError::DuplicateMoniker);
		}
		let parent_idx = self.find_def(parent).ok_or(GraphError::ParentNotFound)?;
		let new_idx = self.defs.len();
		self.index.borrow_mut().insert(moniker.clone(), new_idx);
		self.defs.push(DefRecord {
			moniker,
			kind: kind.to_vec(),
			parent: Some(parent_idx),
			position,
			visibility: attrs.visibility.to_vec(),
			signature: attrs.signature.to_vec(),
		});
		Ok(())
	}

	pub fn add_ref(
		&mut self,
		source: &Moniker,
		target: Moniker,
		kind: &[u8],
		position: Option<Position>,
	) -> Result<(), GraphError> {
		self.add_ref_attrs(source, target, kind, position, &RefAttrs::default())
	}

	pub fn add_ref_attrs(
		&mut self,
		source: &Moniker,
		target: Moniker,
		kind: &[u8],
		position: Option<Position>,
		attrs: &RefAttrs<'_>,
	) -> Result<(), GraphError> {
		let source_idx = self.find_def(source).ok_or(GraphError::SourceNotFound)?;
		self.refs.push(RefRecord {
			source: source_idx,
			target,
			kind: kind.to_vec(),
			position,
			receiver_hint: attrs.receiver_hint.to_vec(),
			alias: attrs.alias.to_vec(),
			confidence: attrs.confidence.to_vec(),
		});
		Ok(())
	}

	pub fn root(&self) -> &Moniker {
		&self.defs[0].moniker
	}

	pub fn contains(&self, m: &Moniker) -> bool {
		self.find_def(m).is_some()
	}

	pub fn defs(&self) -> impl Iterator<Item = &DefRecord> {
		self.defs.iter()
	}

	pub fn refs(&self) -> impl Iterator<Item = &RefRecord> {
		self.refs.iter()
	}

	pub fn locate(&self, m: &Moniker) -> Option<Position> {
		self.find_def(m).and_then(|i| self.defs[i].position)
	}

	pub fn def_monikers(&self) -> Vec<Moniker> {
		let mut v: Vec<Moniker> = self.defs.iter().map(|d| d.moniker.clone()).collect();
		v.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
		v
	}

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

	/// O(1) average via the moniker → index cache. Cache rebuilds in O(N)
	/// the first time it's accessed after deserialization (when defs are
	/// present but the cache field defaulted to empty).
	fn find_def(&self, m: &Moniker) -> Option<usize> {
		if self.index.borrow().len() != self.defs.len() {
			let mut idx = self.index.borrow_mut();
			idx.clear();
			idx.reserve(self.defs.len());
			for (i, d) in self.defs.iter().enumerate() {
				idx.insert(d.moniker.clone(), i);
			}
		}
		self.index.borrow().get(m).copied()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::moniker::MonikerBuilder;

	fn mk(seg: &[u8]) -> Moniker {
		MonikerBuilder::new()
			.project(b"app")
			.segment(b"path", seg)
			.build()
	}

	#[test]
	fn new_graph_has_only_root() {
		let root = mk(b"util");
		let g = CodeGraph::new(root.clone(), b"module");
		assert_eq!(g.root(), &root);
		assert_eq!(g.def_count(), 1);
		assert_eq!(g.ref_count(), 0);
		assert!(g.contains(&root));
	}

	#[test]
	fn add_def_attaches_to_existing_parent() {
		let root = mk(b"util");
		let child = mk(b"util_child");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(child.clone(), b"class", &root, Some((10, 20))).unwrap();

		assert_eq!(g.def_count(), 2);
		assert!(g.contains(&child));
		assert_eq!(g.locate(&child), Some((10, 20)));
	}

	#[test]
	fn add_def_unknown_parent_fails() {
		let root = mk(b"util");
		let unknown = mk(b"nope");
		let child = mk(b"child");
		let mut g = CodeGraph::new(root, b"module");
		assert_eq!(
			g.add_def(child, b"class", &unknown, None).unwrap_err(),
			GraphError::ParentNotFound
		);
	}

	#[test]
	fn add_def_duplicate_moniker_fails() {
		let root = mk(b"util");
		let dup = mk(b"util");
		let mut g = CodeGraph::new(root.clone(), b"module");
		assert_eq!(
			g.add_def(dup, b"class", &root, None).unwrap_err(),
			GraphError::DuplicateMoniker
		);
	}

	#[test]
	fn add_def_records_parent_index() {
		let root = mk(b"util");
		let a = mk(b"a");
		let b = mk(b"b");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(a.clone(), b"class", &root, None).unwrap();
		g.add_def(b.clone(), b"class", &a, None).unwrap();

		let defs: Vec<_> = g.defs().collect();
		assert_eq!(defs[0].parent, None);
		assert_eq!(defs[1].parent, Some(0));
		assert_eq!(defs[2].parent, Some(1));
	}

	#[test]
	fn add_ref_records_source_index_and_target() {
		let root = mk(b"util");
		let foo = mk(b"foo");
		let target = mk(b"external_thing");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(foo.clone(), b"class", &root, None).unwrap();
		g.add_ref(&foo, target.clone(), b"call", Some((5, 8))).unwrap();

		assert_eq!(g.ref_count(), 1);
		let r = g.refs().next().unwrap();
		assert_eq!(r.source, 1);
		assert_eq!(&r.target, &target);
		assert_eq!(r.kind, b"call".to_vec());
		assert_eq!(r.position, Some((5, 8)));
	}

	#[test]
	fn add_ref_unknown_source_fails() {
		let root = mk(b"util");
		let unknown = mk(b"nope");
		let target = mk(b"target");
		let mut g = CodeGraph::new(root, b"module");
		assert_eq!(
			g.add_ref(&unknown, target, b"call", None).unwrap_err(),
			GraphError::SourceNotFound
		);
	}

	#[test]
	fn ref_target_may_be_outside_graph() {
		let root = mk(b"util");
		let foo = mk(b"foo");
		let outside = mk(b"some_external_moniker");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(foo.clone(), b"class", &root, None).unwrap();
		g.add_ref(&foo, outside.clone(), b"call", None).unwrap();

		assert!(!g.contains(&outside));
		assert_eq!(g.ref_count(), 1);
	}

	#[test]
	fn contains_distinguishes_existing_and_unknown() {
		let root = mk(b"util");
		let foo = mk(b"foo");
		let unknown = mk(b"unknown");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(foo.clone(), b"class", &root, None).unwrap();
		assert!(g.contains(&root));
		assert!(g.contains(&foo));
		assert!(!g.contains(&unknown));
	}

	#[test]
	fn locate_returns_none_when_no_position() {
		let root = mk(b"util");
		let foo = mk(b"foo");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(foo.clone(), b"class", &root, None).unwrap();
		assert_eq!(g.locate(&foo), None);
	}

	#[test]
	fn locate_returns_none_for_unknown_moniker() {
		let root = mk(b"util");
		let unknown = mk(b"unknown");
		let g = CodeGraph::new(root, b"module");
		assert_eq!(g.locate(&unknown), None);
	}

	#[test]
	fn def_monikers_returns_all_sorted() {
		let root = mk(b"a_root");
		let a = mk(b"c_zzz");
		let b = mk(b"b_aaa");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(a.clone(), b"class", &root, None).unwrap();
		g.add_def(b.clone(), b"class", &root, None).unwrap();

		let monikers = g.def_monikers();
		assert_eq!(monikers.len(), 3);
		for w in monikers.windows(2) {
			assert!(w[0].as_bytes() <= w[1].as_bytes());
		}
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

		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(foo.clone(), b"class", &root, None).unwrap();
		g.add_def(bar.clone(), b"class", &root, None).unwrap();
		g.add_ref(&foo, target_a.clone(), b"call", None).unwrap();
		g.add_ref(&foo, target_b.clone(), b"call", None).unwrap();
		g.add_ref(&bar, target_a.clone(), b"call", None).unwrap();

		let targets = g.ref_targets();
		assert_eq!(targets.len(), 3);
		for w in targets.windows(2) {
			assert!(w[0].as_bytes() <= w[1].as_bytes());
		}
	}

	#[test]
	fn clone_produces_equal_graph() {
		let root = mk(b"util");
		let foo = mk(b"foo");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(foo.clone(), b"class", &root, Some((1, 2))).unwrap();
		g.add_ref(&foo, mk(b"ext"), b"call", Some((4, 5))).unwrap();

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
			let mut g = CodeGraph::new(root.clone(), b"module");
			g.add_def(foo.clone(), b"class", &root, None).unwrap();
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

	#[test]
	fn find_def_self_heals_after_deserialize_like_state() {
		// Simulate the post-deserialize state where the cache field
		// defaulted empty but defs is fully populated.
		let root = mk(b"util");
		let a = mk(b"a");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(a.clone(), b"class", &root, None).unwrap();
		// Manually invalidate the cache.
		g.index.borrow_mut().clear();

		assert!(g.contains(&root));
		assert!(g.contains(&a));
		assert_eq!(g.locate(&a), None);
	}
}
