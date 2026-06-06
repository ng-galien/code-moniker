mod candidate;
mod decision;
mod full;
mod manifest;
mod method_indexer;
mod query;
mod reference_resolver;
mod scope;
mod semantic;

pub(in crate::linkage) use candidate::{
	CandidateCatalog, LinkageCandidate, global_symbols, local_symbols, matches_any_source,
	matches_any_symbol, query_keys,
};
pub(in crate::linkage) use decision::{
	ExternalOrigin, ReferenceLinkageDecision, ResolutionScope, UnknownReason, project_decisions,
};
pub(in crate::linkage) use full::run_full_linkage_with_timings;
pub(in crate::linkage) use manifest::ManifestPolicy;
pub(in crate::linkage) use method_indexer::MethodIndexer;
pub(in crate::linkage) use query::{LinkageQuery, ReferenceLocation, ReferenceLocations};
pub(in crate::linkage) use reference_resolver::ReferenceResolver;
pub(in crate::linkage) use scope::{GlobalScopeResolver, LocalScopeResolver};
pub(in crate::linkage) use semantic::{MethodTable, SemanticLinkage};
