use std::collections::BTreeMap;

use crate::core::code_graph::Position;
use crate::core::moniker::Moniker;

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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
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
