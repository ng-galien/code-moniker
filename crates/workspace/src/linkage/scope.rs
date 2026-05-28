use crate::linkage::candidate::CandidateCatalog;
use crate::linkage::query::LinkageQuery;
use crate::snapshot::SymbolId;

pub(super) struct LocalScopeResolver;

impl LocalScopeResolver {
	pub(super) fn resolve(
		&self,
		query: &LinkageQuery<'_>,
		candidates: &CandidateCatalog<'_>,
	) -> Vec<SymbolId> {
		candidates.local_symbols(query)
	}
}

pub(super) struct GlobalScopeResolver;

impl GlobalScopeResolver {
	pub(super) fn resolve(
		&self,
		query: &LinkageQuery<'_>,
		candidates: &CandidateCatalog<'_>,
	) -> Vec<SymbolId> {
		candidates.global_symbols(query)
	}
}
