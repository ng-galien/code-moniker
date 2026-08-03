mod c_includes;
mod crate_forwards;
mod full;
mod manifest;
mod method_indexer;
mod python_bindings;
mod reference_resolver;
mod scope;
mod semantic;
mod workspace_packages;

pub(in crate::linkage) use c_includes::CIncludeVisibility;
pub(in crate::linkage) use crate_forwards::CrateForwards;
pub(in crate::linkage) use full::run_full_linkage_with_timings;
pub(in crate::linkage) use manifest::ManifestPolicy;
pub(in crate::linkage) use method_indexer::MethodIndexer;
pub(in crate::linkage) use reference_resolver::{LinkagePolicies, ReferenceResolver};
pub(in crate::linkage) use scope::{
	matches_any_source, matches_any_symbol, resolve_global_scope, resolve_local_scope,
};
pub(in crate::linkage) use semantic::{MethodTable, SemanticLinkage, SemanticPolicies};
pub(in crate::linkage) use workspace_packages::WorkspacePackageIndex;
