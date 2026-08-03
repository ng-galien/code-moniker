mod full;
mod java_imports;
mod manifest;
mod method_indexer;
mod reference_resolver;
mod refinement;
mod scope;
mod workspace_packages;

mod binding_forwards;
pub(in crate::linkage) use binding_forwards::BindingForwards;
pub(in crate::linkage) use full::run_full_linkage_with_timings;
pub(in crate::linkage) use java_imports::JavaOnDemandImports;
pub(in crate::linkage) use manifest::ManifestPolicy;
pub(in crate::linkage) use method_indexer::MethodIndexer;
pub(in crate::linkage) use reference_resolver::{LinkagePolicies, ReferenceResolver};
pub(in crate::linkage) use refinement::{
	DecisionSelection, LinkageRefiner, MethodTable, RefinementPolicies,
};
pub(in crate::linkage) use refinement::{
	MethodCallReference, ReceiverFieldTables, resolve_method_through_supers,
};
pub(in crate::linkage) use scope::{
	matches_any_source, matches_any_symbol, resolve_global_scope, resolve_local_scope,
};
pub(in crate::linkage) use workspace_packages::WorkspacePackageIndex;
