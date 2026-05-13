use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::core::moniker::Moniker;

pub type Position = (u32, u32);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DefRecord {
	pub moniker: Moniker,
	pub kind: Vec<u8>,
	pub parent: Option<usize>,
	pub position: Option<Position>,
	pub visibility: Vec<u8>,
	pub signature: Vec<u8>,
	pub binding: Vec<u8>,
	pub origin: Vec<u8>,
}

impl DefRecord {
	pub fn shape(&self) -> Option<crate::core::shape::Shape> {
		crate::core::shape::shape_of(&self.kind)
	}

	pub fn opens_scope(&self) -> bool {
		crate::core::shape::opens_scope(&self.kind)
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RefRecord {
	pub source: usize,
	pub target: Moniker,
	pub kind: Vec<u8>,
	pub position: Option<Position>,
	pub receiver_hint: Vec<u8>,
	pub alias: Vec<u8>,
	pub confidence: Vec<u8>,
	pub binding: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct DefAttrs<'a> {
	pub visibility: &'a [u8],
	pub signature: &'a [u8],
	pub binding: &'a [u8],
	pub origin: &'a [u8],
}

#[derive(Clone, Debug, Default)]
pub struct RefAttrs<'a> {
	pub receiver_hint: &'a [u8],
	pub alias: &'a [u8],
	pub confidence: &'a [u8],
	pub binding: &'a [u8],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphError {
	ParentNotFound,
	ParentNotAncestor,
	SourceNotFound,
	DuplicateMoniker,
}

impl std::fmt::Display for GraphError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::ParentNotFound => write!(f, "parent moniker not found in graph"),
			Self::ParentNotAncestor => {
				write!(f, "parent moniker is not an ancestor of the def")
			}
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
	#[cfg_attr(feature = "serde", serde(skip, default))]
	index: RefCell<HashMap<Moniker, usize>>,
	#[cfg_attr(feature = "serde", serde(skip, default))]
	def_monikers_cache: RefCell<Option<Arc<Vec<Moniker>>>>,
	#[cfg_attr(feature = "serde", serde(skip, default))]
	ref_targets_cache: RefCell<Option<Arc<Vec<Moniker>>>>,
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
	pub fn from_records(defs: Vec<DefRecord>, refs: Vec<RefRecord>) -> Self {
		let mut index = HashMap::with_capacity(defs.len());
		for (i, d) in defs.iter().enumerate() {
			index.insert(d.moniker.clone(), i);
		}
		Self {
			defs,
			refs,
			index: RefCell::new(index),
			def_monikers_cache: RefCell::new(None),
			ref_targets_cache: RefCell::new(None),
		}
	}

	pub fn new(root: Moniker, root_kind: &[u8]) -> Self {
		use crate::core::kinds::{BIND_EXPORT, ORIGIN_EXTRACTED};
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
				binding: BIND_EXPORT.to_vec(),
				origin: ORIGIN_EXTRACTED.to_vec(),
			}],
			refs: Vec::new(),
			index: RefCell::new(index),
			def_monikers_cache: RefCell::new(None),
			ref_targets_cache: RefCell::new(None),
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
		use crate::core::kinds::ORIGIN_EXTRACTED;
		if self.find_def(&moniker).is_some() {
			return Err(GraphError::DuplicateMoniker);
		}
		if !parent.is_ancestor_of(&moniker) {
			return Err(GraphError::ParentNotAncestor);
		}
		let parent_idx = self.find_def(parent).ok_or(GraphError::ParentNotFound)?;
		let new_idx = self.defs.len();
		self.index.borrow_mut().insert(moniker.clone(), new_idx);
		let binding = if attrs.binding.is_empty() {
			default_def_binding(kind, attrs.visibility)
		} else {
			attrs.binding
		};
		let origin = if attrs.origin.is_empty() {
			ORIGIN_EXTRACTED
		} else {
			attrs.origin
		};
		self.defs.push(DefRecord {
			moniker,
			kind: kind.to_vec(),
			parent: Some(parent_idx),
			position,
			visibility: attrs.visibility.to_vec(),
			signature: attrs.signature.to_vec(),
			binding: binding.to_vec(),
			origin: origin.to_vec(),
		});
		self.def_monikers_cache.borrow_mut().take();
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
		let binding = if attrs.binding.is_empty() {
			default_ref_binding(kind)
		} else {
			attrs.binding
		};
		self.refs.push(RefRecord {
			source: source_idx,
			target,
			kind: kind.to_vec(),
			position,
			receiver_hint: attrs.receiver_hint.to_vec(),
			alias: attrs.alias.to_vec(),
			confidence: attrs.confidence.to_vec(),
			binding: binding.to_vec(),
		});
		self.ref_targets_cache.borrow_mut().take();
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

	pub fn def_at(&self, i: usize) -> &DefRecord {
		&self.defs[i]
	}

	pub fn refs(&self) -> impl Iterator<Item = &RefRecord> {
		self.refs.iter()
	}

	pub fn ref_at(&self, i: usize) -> &RefRecord {
		&self.refs[i]
	}

	pub fn locate(&self, m: &Moniker) -> Option<Position> {
		self.find_def(m).and_then(|i| self.defs[i].position)
	}

	pub fn def_monikers(&self) -> Arc<Vec<Moniker>> {
		if let Some(cached) = self.def_monikers_cache.borrow().as_ref() {
			return Arc::clone(cached);
		}
		let mut v: Vec<Moniker> = self.defs.iter().map(|d| d.moniker.clone()).collect();
		v.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
		let arc = Arc::new(v);
		*self.def_monikers_cache.borrow_mut() = Some(Arc::clone(&arc));
		arc
	}

	pub fn ref_targets(&self) -> Arc<Vec<Moniker>> {
		if let Some(cached) = self.ref_targets_cache.borrow().as_ref() {
			return Arc::clone(cached);
		}
		let mut v: Vec<Moniker> = self.refs.iter().map(|r| r.target.clone()).collect();
		v.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
		let arc = Arc::new(v);
		*self.ref_targets_cache.borrow_mut() = Some(Arc::clone(&arc));
		arc
	}

	pub fn def_count(&self) -> usize {
		self.defs.len()
	}

	pub fn ref_count(&self) -> usize {
		self.refs.len()
	}

	fn find_def(&self, m: &Moniker) -> Option<usize> {
		let mut idx = self.index.borrow_mut();
		if idx.len() != self.defs.len() {
			idx.clear();
			idx.reserve(self.defs.len());
			for (i, d) in self.defs.iter().enumerate() {
				idx.insert(d.moniker.clone(), i);
			}
		}
		idx.get(m).copied()
	}
}

fn default_def_binding(kind: &[u8], visibility: &[u8]) -> &'static [u8] {
	use crate::core::kinds::{
		BIND_EXPORT, BIND_LOCAL, BIND_NONE, KIND_COMMENT, KIND_LOCAL, KIND_MODULE, KIND_PARAM,
		VIS_MODULE, VIS_PACKAGE, VIS_PRIVATE, VIS_PROTECTED, VIS_PUBLIC,
	};
	if kind == KIND_COMMENT {
		return BIND_NONE;
	}
	if kind == KIND_LOCAL || kind == KIND_PARAM {
		return BIND_LOCAL;
	}
	if visibility == VIS_PRIVATE || visibility == VIS_MODULE {
		return BIND_LOCAL;
	}
	if kind == KIND_MODULE {
		return BIND_EXPORT;
	}
	if visibility == VIS_PUBLIC || visibility == VIS_PROTECTED || visibility == VIS_PACKAGE {
		return BIND_EXPORT;
	}
	BIND_LOCAL
}

fn default_ref_binding(kind: &[u8]) -> &'static [u8] {
	use crate::core::kinds::{
		BIND_IMPORT, BIND_INJECT, BIND_LOCAL, BIND_NONE, REF_ANNOTATES, REF_CALLS, REF_DI_REGISTER,
		REF_DI_REQUIRE, REF_EXTENDS, REF_IMPLEMENTS, REF_IMPORTS_MODULE, REF_IMPORTS_SYMBOL,
		REF_INSTANTIATES, REF_METHOD_CALL, REF_READS, REF_REEXPORTS, REF_USES_TYPE,
	};
	if kind == REF_IMPORTS_SYMBOL || kind == REF_IMPORTS_MODULE || kind == REF_REEXPORTS {
		return BIND_IMPORT;
	}
	if kind == REF_DI_REGISTER || kind == REF_DI_REQUIRE {
		return BIND_INJECT;
	}
	if kind == REF_CALLS
		|| kind == REF_METHOD_CALL
		|| kind == REF_READS
		|| kind == REF_USES_TYPE
		|| kind == REF_INSTANTIATES
		|| kind == REF_EXTENDS
		|| kind == REF_IMPLEMENTS
		|| kind == REF_ANNOTATES
	{
		return BIND_LOCAL;
	}
	BIND_NONE
}

#[cfg(test)]
pub(crate) fn assert_local_refs_closed(g: &CodeGraph) {
	use crate::core::uri::{UriConfig, to_uri};
	let cfg = UriConfig::default();
	let render = |m: &Moniker| to_uri(m, &cfg).unwrap_or_else(|_| format!("{:?}", m.as_bytes()));
	let defs: Vec<&Moniker> = g.defs().map(|d| &d.moniker).collect();
	for r in g.refs() {
		if r.confidence != b"local" {
			continue;
		}
		let resolved = defs.iter().any(|d| d.bind_match(&r.target));
		assert!(
			resolved,
			"DANGLING local ref: target={} kind={}, no def bind_matches.\n  Defs:\n{}",
			render(&r.target),
			std::str::from_utf8(&r.kind).unwrap_or("<non-utf8>"),
			defs.iter()
				.map(|d| format!("    {}", render(d)))
				.collect::<Vec<_>>()
				.join("\n"),
		);
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

	fn mk_under(parent: &Moniker, kind: &[u8], name: &[u8]) -> Moniker {
		let mut b = MonikerBuilder::from_view(parent.as_view());
		b.segment(kind, name);
		b.build()
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
	fn def_shape_is_derived_from_kind() {
		use crate::core::shape::Shape;
		let root = mk(b"util");
		let mut g = CodeGraph::new(root.clone(), b"module");
		let class = mk_under(&root, b"class", b"Foo");
		g.add_def(class.clone(), b"class", &root, None).unwrap();
		let comment = mk_under(&root, b"comment", b"42");
		g.add_def(comment.clone(), b"comment", &root, None).unwrap();

		let class_def = g.defs().find(|d| d.moniker == class).unwrap();
		assert_eq!(class_def.shape(), Some(Shape::Type));
		assert!(class_def.opens_scope());

		let comment_def = g.defs().find(|d| d.moniker == comment).unwrap();
		assert_eq!(comment_def.shape(), Some(Shape::Annotation));
		assert!(!comment_def.opens_scope());

		let root_def = g.defs().next().unwrap();
		assert_eq!(root_def.shape(), Some(Shape::Namespace));
		assert!(root_def.opens_scope());
	}

	#[test]
	fn add_def_attaches_to_existing_parent() {
		let root = mk(b"util");
		let child = mk_under(&root, b"path", b"util_child");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(child.clone(), b"class", &root, Some((10, 20)))
			.unwrap();

		assert_eq!(g.def_count(), 2);
		assert!(g.contains(&child));
		assert_eq!(g.locate(&child), Some((10, 20)));
	}

	#[test]
	fn add_def_unknown_parent_fails() {
		let root = mk(b"util");
		let unknown = mk(b"nope");
		let child = mk_under(&unknown, b"path", b"child");
		let mut g = CodeGraph::new(root, b"module");
		assert_eq!(
			g.add_def(child, b"class", &unknown, None).unwrap_err(),
			GraphError::ParentNotFound
		);
	}

	#[test]
	fn add_def_parent_not_ancestor_fails() {
		let root = mk(b"util");
		let unrelated = mk_under(&root, b"path", b"sibling");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(unrelated.clone(), b"class", &root, None).unwrap();
		let child = mk_under(&root, b"path", b"orphan");
		assert_eq!(
			g.add_def(child, b"class", &unrelated, None).unwrap_err(),
			GraphError::ParentNotAncestor
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
		let a = mk_under(&root, b"path", b"a");
		let b = mk_under(&a, b"path", b"b");
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
		let foo = mk_under(&root, b"path", b"foo");
		let target = mk(b"external_thing");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(foo.clone(), b"class", &root, None).unwrap();
		g.add_ref(&foo, target.clone(), b"call", Some((5, 8)))
			.unwrap();

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
		let foo = mk_under(&root, b"path", b"foo");
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
		let foo = mk_under(&root, b"path", b"foo");
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
		let foo = mk_under(&root, b"path", b"foo");
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
		let a = mk_under(&root, b"path", b"c_zzz");
		let b = mk_under(&root, b"path", b"b_aaa");
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
	fn def_monikers_cache_invalidates_on_add_def() {
		let root = mk(b"util");
		let foo = mk_under(&root, b"path", b"foo");
		let bar = mk_under(&root, b"path", b"bar");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(foo.clone(), b"class", &root, None).unwrap();
		let first = g.def_monikers();
		assert_eq!(first.len(), 2);
		g.add_def(bar.clone(), b"class", &root, None).unwrap();
		let second = g.def_monikers();
		assert_eq!(second.len(), 3, "cache must reflect post-add state");
	}

	#[test]
	fn ref_targets_cache_invalidates_on_add_ref() {
		let root = mk(b"util");
		let foo = mk_under(&root, b"path", b"foo");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(foo.clone(), b"class", &root, None).unwrap();
		let first = g.ref_targets();
		assert!(first.is_empty());
		g.add_ref(&foo, mk(b"ext"), b"call", None).unwrap();
		let second = g.ref_targets();
		assert_eq!(second.len(), 1, "cache must reflect post-add state");
	}

	#[test]
	fn ref_targets_collects_all_with_duplicates() {
		let root = mk(b"util");
		let foo = mk_under(&root, b"path", b"foo");
		let bar = mk_under(&root, b"path", b"bar");
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
		let foo = mk_under(&root, b"path", b"foo");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(foo.clone(), b"class", &root, Some((1, 2)))
			.unwrap();
		g.add_ref(&foo, mk(b"ext"), b"call", Some((4, 5))).unwrap();

		let clone = g.clone();
		assert_eq!(g, clone);
	}

	#[test]
	fn equal_graphs_have_equal_hashes() {
		use std::collections::hash_map::DefaultHasher;
		use std::hash::{Hash, Hasher};

		let root = mk(b"util");
		let foo = mk_under(&root, b"path", b"foo");
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
		let root = mk(b"util");
		let a = mk_under(&root, b"path", b"a");
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(a.clone(), b"class", &root, None).unwrap();
		g.index.borrow_mut().clear();

		assert!(g.contains(&root));
		assert!(g.contains(&a));
		assert_eq!(g.locate(&a), None);
	}

	#[test]
	fn def_binding_comment_is_none() {
		assert_eq!(default_def_binding(b"comment", b""), b"none");
	}

	#[test]
	fn def_binding_local_kind_is_local_regardless_of_visibility() {
		assert_eq!(default_def_binding(b"local", b""), b"local");
		assert_eq!(default_def_binding(b"param", b"public"), b"local");
	}

	#[test]
	fn def_binding_private_or_module_visibility_is_local() {
		assert_eq!(default_def_binding(b"function", b"private"), b"local");
		assert_eq!(default_def_binding(b"function", b"module"), b"local");
	}

	#[test]
	fn def_binding_module_kind_is_export_even_without_visibility() {
		assert_eq!(default_def_binding(b"module", b""), b"export");
	}

	#[test]
	fn def_binding_public_protected_package_is_export() {
		assert_eq!(default_def_binding(b"class", b"public"), b"export");
		assert_eq!(default_def_binding(b"method", b"protected"), b"export");
		assert_eq!(default_def_binding(b"class", b"package"), b"export");
	}

	#[test]
	fn def_binding_field_without_visibility_is_local() {
		assert_eq!(default_def_binding(b"field", b""), b"local");
	}

	#[test]
	fn ref_binding_imports_are_import() {
		assert_eq!(default_ref_binding(b"imports_symbol"), b"import");
		assert_eq!(default_ref_binding(b"imports_module"), b"import");
		assert_eq!(default_ref_binding(b"reexports"), b"import");
	}

	#[test]
	fn ref_binding_di_register_is_inject() {
		assert_eq!(default_ref_binding(b"di_register"), b"inject");
	}

	#[test]
	fn ref_binding_di_require_is_inject() {
		assert_eq!(default_ref_binding(b"di_require"), b"inject");
	}

	#[test]
	fn ref_binding_intra_module_kinds_are_local() {
		for k in &[
			b"calls".as_slice(),
			b"method_call",
			b"reads",
			b"uses_type",
			b"instantiates",
			b"extends",
			b"implements",
			b"annotates",
		] {
			assert_eq!(default_ref_binding(k), b"local");
		}
	}

	#[test]
	fn ref_binding_unknown_kind_is_none() {
		assert_eq!(default_ref_binding(b"weird_kind"), b"none");
	}

	#[test]
	fn add_def_attrs_auto_computes_binding_when_attrs_empty() {
		let root = mk(b"util");
		let mut g = CodeGraph::new(root.clone(), b"module");
		let class_m = MonikerBuilder::from_view(root.as_view())
			.segment(b"class", b"Foo")
			.build();
		let attrs = DefAttrs {
			visibility: b"public",
			..DefAttrs::default()
		};
		g.add_def_attrs(class_m.clone(), b"class", &root, None, &attrs)
			.unwrap();
		let def = g.defs().find(|d| d.moniker == class_m).unwrap();
		assert_eq!(def.binding, b"export".to_vec());
	}

	#[test]
	fn add_def_attrs_respects_explicit_inject_override() {
		let root = mk(b"util");
		let mut g = CodeGraph::new(root.clone(), b"module");
		let class_m = MonikerBuilder::from_view(root.as_view())
			.segment(b"class", b"FooService")
			.build();
		let attrs = DefAttrs {
			visibility: b"public",
			binding: b"inject",
			..DefAttrs::default()
		};
		g.add_def_attrs(class_m.clone(), b"class", &root, None, &attrs)
			.unwrap();
		let def = g.defs().find(|d| d.moniker == class_m).unwrap();
		assert_eq!(def.binding, b"inject".to_vec());
	}

	#[test]
	fn root_def_origin_defaults_to_extracted() {
		use crate::core::kinds::ORIGIN_EXTRACTED;
		let root = mk(b"util");
		let g = CodeGraph::new(root.clone(), b"module");
		let root_def = g.defs().next().unwrap();
		assert_eq!(root_def.origin, ORIGIN_EXTRACTED.to_vec());
	}

	#[test]
	fn add_def_attrs_defaults_origin_to_extracted_when_unset() {
		use crate::core::kinds::ORIGIN_EXTRACTED;
		let root = mk(b"util");
		let mut g = CodeGraph::new(root.clone(), b"module");
		let class_m = MonikerBuilder::from_view(root.as_view())
			.segment(b"class", b"Foo")
			.build();
		g.add_def_attrs(class_m.clone(), b"class", &root, None, &DefAttrs::default())
			.unwrap();
		let def = g.defs().find(|d| d.moniker == class_m).unwrap();
		assert_eq!(def.origin, ORIGIN_EXTRACTED.to_vec());
	}

	#[test]
	fn add_def_attrs_respects_explicit_origin_declared() {
		use crate::core::kinds::ORIGIN_DECLARED;
		let root = mk(b"util");
		let mut g = CodeGraph::new(root.clone(), b"module");
		let class_m = MonikerBuilder::from_view(root.as_view())
			.segment(b"class", b"Foo")
			.build();
		let attrs = DefAttrs {
			visibility: b"public",
			origin: ORIGIN_DECLARED,
			..DefAttrs::default()
		};
		g.add_def_attrs(class_m.clone(), b"class", &root, None, &attrs)
			.unwrap();
		let def = g.defs().find(|d| d.moniker == class_m).unwrap();
		assert_eq!(def.origin, ORIGIN_DECLARED.to_vec());
	}
}
