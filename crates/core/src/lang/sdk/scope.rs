use std::collections::BTreeMap;

use crate::core::moniker::Moniker;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum Namespace {
	Unified,
	Value,
	Type,
	Macro,
	Lifetime,
	Module,
	Schema,
	Custom(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct ScopeId(pub usize);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Rib {
	bindings: BTreeMap<Vec<u8>, Vec<Moniker>>,
}

impl Rib {
	pub fn insert(&mut self, name: impl Into<Vec<u8>>, target: Moniker) {
		self.bindings.entry(name.into()).or_default().push(target);
	}

	pub fn get(&self, name: &[u8]) -> &[Moniker] {
		self.bindings
			.get(name)
			.map(Vec::as_slice)
			.unwrap_or_default()
	}

	pub fn is_empty(&self) -> bool {
		self.bindings.is_empty()
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Scope {
	pub id: ScopeId,
	pub parent: Option<ScopeId>,
	pub owner: Moniker,
	ribs: BTreeMap<Namespace, Rib>,
}

impl Scope {
	fn new(id: ScopeId, parent: Option<ScopeId>, owner: Moniker) -> Self {
		Self {
			id,
			parent,
			owner,
			ribs: BTreeMap::new(),
		}
	}

	pub fn insert(&mut self, namespace: Namespace, name: impl Into<Vec<u8>>, target: Moniker) {
		self.ribs.entry(namespace).or_default().insert(name, target);
	}

	pub fn rib(&self, namespace: Namespace) -> Option<&Rib> {
		self.ribs.get(&namespace)
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeTree {
	scopes: Vec<Scope>,
}

impl ScopeTree {
	pub fn new(root_owner: Moniker) -> Self {
		Self {
			scopes: vec![Scope::new(ScopeId(0), None, root_owner)],
		}
	}

	pub fn root(&self) -> ScopeId {
		ScopeId(0)
	}

	pub fn add_scope(&mut self, parent: ScopeId, owner: Moniker) -> ScopeId {
		let id = ScopeId(self.scopes.len());
		self.scopes.push(Scope::new(id, Some(parent), owner));
		id
	}

	pub fn scope(&self, id: ScopeId) -> Option<&Scope> {
		self.scopes.get(id.0)
	}

	pub fn scope_mut(&mut self, id: ScopeId) -> Option<&mut Scope> {
		self.scopes.get_mut(id.0)
	}

	pub fn resolve(&self, start: ScopeId, namespace: Namespace, name: &[u8]) -> &[Moniker] {
		let mut current = Some(start);
		while let Some(id) = current {
			let Some(scope) = self.scope(id) else {
				break;
			};
			if let Some(rib) = scope.rib(namespace) {
				let targets = rib.get(name);
				if !targets.is_empty() {
					return targets;
				}
			}
			current = scope.parent;
		}
		&[]
	}
}
