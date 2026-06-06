use crate::linkage::candidate::{CandidateCatalog, global_symbols, local_symbols};
use crate::linkage::ordinals::SymbolSet;
use crate::linkage::query::LinkageQuery;

pub(super) struct LocalScopeResolver;

impl LocalScopeResolver {
	pub(super) fn resolve(
		&self,
		query: &LinkageQuery<'_>,
		candidates: &CandidateCatalog<'_>,
	) -> SymbolSet {
		local_symbols(candidates, query)
	}
}

pub(super) struct GlobalScopeResolver;

impl GlobalScopeResolver {
	pub(super) fn resolve(
		&self,
		query: &LinkageQuery<'_>,
		candidates: &CandidateCatalog<'_>,
	) -> SymbolSet {
		global_symbols(candidates, query)
	}
}
