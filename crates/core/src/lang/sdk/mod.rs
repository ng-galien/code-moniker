//! Shared extraction SDK for the target multi-pass language pipeline.
//!
//! This module defines the stable IR exchanged by language extraction phases:
//! discover definitions and imports, elaborate unresolved references, resolve
//! them locally, then emit the public `CodeGraph`.

mod emit;
mod imports;
mod model;
mod resolve;
mod scope;
mod types;

pub use emit::{EmitError, GraphEmitter};
pub use imports::{
	ImportLeaf, ImportLeafKind, ImportTree, flatten_import_tree, import_leaf_binding_name,
	importable_parent,
};
pub(crate) use model::ResolvedRefDeduper;
pub use model::{
	DefIndex, DefNameKey, DiscoveredDef, DiscoveredFile, ImportKind, ImportTable, ImportTarget,
	RefHints, ResolvedRef, TargetExpr, UnresolvedRef,
};
pub use resolve::{LangResolverStrategy, LocalResolver, Resolution};
pub use scope::{Namespace, Rib, Scope, ScopeId, ScopeTree};
pub use types::{TypeEnv, TypeExpr};

use crate::core::moniker::MonikerBuilder;

/// Starts a target owned by a language runtime or standard SDK.
/// `sdk:<lang>` is the root; callers append every lexical namespace segment,
/// including Java's repeated `path:java`.
pub(crate) fn sdk_target_builder(project: &[u8], language: &[u8]) -> MonikerBuilder {
	let mut builder = MonikerBuilder::new();
	builder.project(project);
	builder.segment(crate::lang::kinds::SDK, language);
	builder
}
