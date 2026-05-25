use crate::linkage::candidate::{CandidateCatalog, candidate_symbols};
use crate::linkage::query::LinkageQuery;
use crate::snapshot::SymbolId;

pub(super) struct LocalScopeResolver;

impl LocalScopeResolver {
	pub(super) fn resolve(
		&self,
		query: &LinkageQuery<'_>,
		candidates: &CandidateCatalog<'_>,
	) -> Vec<SymbolId> {
		candidate_symbols(candidates.local_matches(query))
	}
}

pub(super) struct GlobalScopeResolver;

impl GlobalScopeResolver {
	pub(super) fn resolve(
		&self,
		query: &LinkageQuery<'_>,
		candidates: &CandidateCatalog<'_>,
	) -> Vec<SymbolId> {
		candidate_symbols(candidates.global_matches(query))
	}
}
