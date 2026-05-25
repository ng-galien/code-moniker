use crate::workspace::resources::material::LocalResourceCache;
use crate::workspace::session::{
	CodeIndex, LinkageEdge, LinkageGraph, LinkagePort, UnresolvedReference, WorkspaceFailure,
	WorkspaceResource, WorkspaceResult,
};

pub struct LocalLinkage {
	cache: LocalResourceCache,
}

impl LocalLinkage {
	pub fn new(cache: LocalResourceCache) -> Self {
		Self { cache }
	}
}

impl LinkagePort for LocalLinkage {
	fn resolve_linkage(&mut self, index: &CodeIndex) -> WorkspaceResult<LinkageGraph> {
		let material = self.cache.index_material(index.generation).ok_or_else(|| {
			WorkspaceFailure::new(
				WorkspaceResource::LinkageGraph,
				"code index material is unavailable",
			)
		})?;
		let generation = self.cache.next_generation();
		let mut resolved = Vec::new();
		let mut unresolved = Vec::new();
		for reference in &index.references {
			let Some(target) = material.reference_targets.get(&reference.id) else {
				unresolved.push(UnresolvedReference::new(
					reference.id.clone(),
					reference.target_identity.clone(),
				));
				continue;
			};
			let mut matched = false;
			for (symbol_id, moniker) in &material.symbol_monikers {
				if moniker.bind_match(target) || target.bind_match(moniker) {
					resolved.push(LinkageEdge::new(reference.id.clone(), symbol_id.clone()));
					matched = true;
				}
			}
			if !matched {
				unresolved.push(UnresolvedReference::new(
					reference.id.clone(),
					reference.target_identity.clone(),
				));
			}
		}
		Ok(LinkageGraph::with_refs(
			generation,
			index.generation,
			resolved,
			unresolved,
		))
	}
}
