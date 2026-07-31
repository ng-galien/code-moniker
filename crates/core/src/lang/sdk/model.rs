use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use crate::core::code_graph::Position;
use crate::core::moniker::Moniker;
use rustc_hash::FxHashMap;

use super::scope::{Namespace, ScopeId, ScopeTree};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct DefNameKey {
	pub namespace: Namespace,
	pub name: Vec<u8>,
}

impl DefNameKey {
	pub fn new(namespace: Namespace, name: impl Into<Vec<u8>>) -> Self {
		Self {
			namespace,
			name: name.into(),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredDef {
	pub moniker: Moniker,
	pub parent: Moniker,
	pub namespace: Namespace,
	pub name: Vec<u8>,
	pub kind: &'static [u8],
	pub visibility: &'static [u8],
	pub signature: Vec<u8>,
	pub position: Option<Position>,
	pub call_name: Vec<u8>,
	pub call_arity: Option<usize>,
}

impl DiscoveredDef {
	pub fn key(&self) -> DefNameKey {
		DefNameKey::new(self.namespace, self.name.clone())
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DefIndex {
	by_moniker: BTreeMap<Moniker, DiscoveredDef>,
	by_name: BTreeMap<DefNameKey, Vec<Moniker>>,
}

impl DefIndex {
	pub fn from_defs(defs: &[DiscoveredDef]) -> Self {
		let mut index = Self::default();
		for def in defs {
			index.insert(def.clone());
		}
		index
	}

	pub fn insert(&mut self, def: DiscoveredDef) {
		self.by_name
			.entry(def.key())
			.or_default()
			.push(def.moniker.clone());
		self.by_moniker.insert(def.moniker.clone(), def);
	}

	pub fn contains(&self, moniker: &Moniker) -> bool {
		self.by_moniker.contains_key(moniker)
	}

	pub fn get(&self, moniker: &Moniker) -> Option<&DiscoveredDef> {
		self.by_moniker.get(moniker)
	}

	pub fn by_name(&self, namespace: Namespace, name: &[u8]) -> &[Moniker] {
		self.by_name
			.get(&DefNameKey::new(namespace, name.to_vec()))
			.map(Vec::as_slice)
			.unwrap_or_default()
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportKind {
	Symbol,
	Module,
	Wildcard,
	Alias,
	Reexport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportTarget {
	pub kind: ImportKind,
	pub namespace: Namespace,
	pub alias: Vec<u8>,
	pub target: Moniker,
	pub confidence: &'static [u8],
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImportTable {
	by_scope: BTreeMap<ScopeId, Vec<ImportTarget>>,
}

impl ImportTable {
	pub fn insert(&mut self, scope: ScopeId, target: ImportTarget) {
		self.by_scope.entry(scope).or_default().push(target);
	}

	pub fn scoped(&self, scope: ScopeId) -> &[ImportTarget] {
		self.by_scope
			.get(&scope)
			.map(Vec::as_slice)
			.unwrap_or_default()
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredFile {
	pub root: Moniker,
	pub root_kind: &'static [u8],
	pub defs: Vec<DiscoveredDef>,
	pub def_index: DefIndex,
	pub scopes: ScopeTree,
	pub imports: ImportTable,
}

impl DiscoveredFile {
	pub fn new(
		root: Moniker,
		root_kind: &'static [u8],
		defs: Vec<DiscoveredDef>,
		scopes: ScopeTree,
		imports: ImportTable,
	) -> Self {
		let def_index = DefIndex::from_defs(&defs);
		Self {
			root,
			root_kind,
			defs,
			def_index,
			scopes,
			imports,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetExpr {
	Bare(Vec<u8>),
	Path(Vec<Vec<u8>>),
	Receiver {
		receiver: Box<TargetExpr>,
		name: Vec<u8>,
	},
	SelfType(Vec<u8>),
	External {
		package: Vec<u8>,
		path: Vec<Vec<u8>>,
	},
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct RefHints {
	pub receiver_hint: Vec<u8>,
	pub alias: Vec<u8>,
	pub namespace: Option<Namespace>,
	pub call_name: Vec<u8>,
	pub call_arity: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnresolvedRef {
	pub source: Moniker,
	pub kind: &'static [u8],
	pub source_scope: ScopeId,
	pub position: Option<Position>,
	pub target: TargetExpr,
	pub hints: RefHints,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedRef {
	pub source: Moniker,
	pub target: Moniker,
	pub kind: &'static [u8],
	pub position: Option<Position>,
	pub confidence: &'static [u8],
	pub hints: RefHints,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ResolvedRefDeduper {
	unique: FxHashMap<u64, usize>,
	collisions: FxHashMap<u64, Vec<usize>>,
	include_hints: bool,
}

impl ResolvedRefDeduper {
	pub(crate) fn with_hints() -> Self {
		Self {
			include_hints: true,
			..Self::default()
		}
	}

	pub(crate) fn push(&mut self, refs: &mut Vec<ResolvedRef>, reference: ResolvedRef) {
		let hash = resolved_ref_hash(&reference, self.include_hints);
		self.push_hashed(refs, reference, hash);
	}

	fn push_hashed(&mut self, refs: &mut Vec<ResolvedRef>, reference: ResolvedRef, hash: u64) {
		let candidates = self
			.collisions
			.get(&hash)
			.map(Vec::as_slice)
			.or_else(|| self.unique.get(&hash).map(std::slice::from_ref))
			.unwrap_or_default();
		if candidates
			.iter()
			.any(|index| same_ref(&refs[*index], &reference, self.include_hints))
		{
			return;
		}

		let index = refs.len();
		refs.push(reference);
		let Some(existing) = self.unique.get(&hash).copied() else {
			self.unique.insert(hash, index);
			return;
		};
		self.collisions
			.entry(hash)
			.or_insert_with(|| vec![existing])
			.push(index);
	}
}

fn resolved_ref_hash(reference: &ResolvedRef, include_hints: bool) -> u64 {
	let mut hasher = rustc_hash::FxHasher::default();
	reference.source.hash(&mut hasher);
	reference.target.hash(&mut hasher);
	reference.kind.hash(&mut hasher);
	reference.position.hash(&mut hasher);
	reference.confidence.hash(&mut hasher);
	if include_hints {
		reference.hints.hash(&mut hasher);
	}
	hasher.finish()
}

fn same_ref(left: &ResolvedRef, right: &ResolvedRef, include_hints: bool) -> bool {
	left.source == right.source
		&& left.target == right.target
		&& left.kind == right.kind
		&& left.position == right.position
		&& left.confidence == right.confidence
		&& (!include_hints || left.hints == right.hints)
}

#[cfg(test)]
mod resolved_ref_deduper_tests {
	use super::*;
	use crate::core::moniker::MonikerBuilder;

	fn reference(target: &[u8], receiver_hint: &[u8]) -> ResolvedRef {
		let mut source = MonikerBuilder::new();
		source.project(b"app").segment(b"fn", b"source");
		let mut destination = MonikerBuilder::new();
		destination.project(b"app").segment(b"fn", target);
		ResolvedRef {
			source: source.build(),
			target: destination.build(),
			kind: b"calls",
			position: Some((10, 20)),
			confidence: b"resolved",
			hints: RefHints {
				receiver_hint: receiver_hint.to_vec(),
				..RefHints::default()
			},
		}
	}

	#[test]
	fn exact_duplicates_and_hint_policy_match_existing_language_contracts() {
		let mut refs = Vec::new();
		let mut linkage = ResolvedRefDeduper::default();
		linkage.push(&mut refs, reference(b"target", b"left"));
		linkage.push(&mut refs, reference(b"target", b"right"));
		assert_eq!(refs.len(), 1, "C/Go/Java ignore hints while deduplicating");

		let mut refs = Vec::new();
		let mut full = ResolvedRefDeduper::with_hints();
		full.push(&mut refs, reference(b"target", b"left"));
		full.push(&mut refs, reference(b"target", b"right"));
		full.push(&mut refs, reference(b"target", b"right"));
		assert_eq!(refs.len(), 2, "Rust keeps distinct hints");
	}

	#[test]
	fn hash_collisions_are_resolved_by_exact_comparison() {
		let mut refs = Vec::new();
		let mut deduper = ResolvedRefDeduper::default();
		deduper.push_hashed(&mut refs, reference(b"first", b""), 7);
		deduper.push_hashed(&mut refs, reference(b"second", b""), 7);
		deduper.push_hashed(&mut refs, reference(b"first", b""), 7);
		assert_eq!(refs.len(), 2);
	}
}
