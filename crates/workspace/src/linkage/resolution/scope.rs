use crate::linkage::resolution::LinkageQuery;
use crate::linkage::resolution::{CandidateCatalog, global_symbols, local_symbols};
use crate::linkage::storage::SymbolSet;

pub(in crate::linkage) struct LocalScopeResolver;

impl LocalScopeResolver {
	pub(in crate::linkage) fn resolve(
		&self,
		query: &LinkageQuery<'_>,
		candidates: &CandidateCatalog<'_>,
	) -> SymbolSet {
		local_symbols(candidates, query)
	}
}

pub(in crate::linkage) struct GlobalScopeResolver;

impl GlobalScopeResolver {
	pub(in crate::linkage) fn resolve(
		&self,
		query: &LinkageQuery<'_>,
		candidates: &CandidateCatalog<'_>,
	) -> SymbolSet {
		global_symbols(candidates, query)
	}
}
