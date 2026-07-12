mod full;
mod manifest;
mod method_indexer;
mod reference_resolver;
mod scope;
mod semantic;
mod workspace_packages;

pub(in crate::linkage) use full::run_full_linkage_with_timings;
pub(in crate::linkage) use manifest::ManifestPolicy;
pub(in crate::linkage) use method_indexer::MethodIndexer;
pub(in crate::linkage) use reference_resolver::{LinkagePolicies, ReferenceResolver};
pub(in crate::linkage) use scope::{
	GlobalScopeResolver, LocalScopeResolver, matches_any_source, matches_any_symbol,
};
pub(in crate::linkage) use semantic::{MethodTable, SemanticLinkage};
pub(in crate::linkage) use workspace_packages::WorkspacePackageIndex;
