//! Shared extraction SDK for the target multi-pass language pipeline.
//!
//! This module is intentionally separate from `canonical_walker` and
//! `LangStrategy`. It defines the stable IR exchanged by the target phases:
//! discover definitions and imports, elaborate unresolved references, resolve
//! them locally, then emit the public `CodeGraph`.

mod emit;
mod imports;
mod model;
mod resolve;
mod scope;

pub use emit::{EmitError, GraphEmitter};
pub use imports::{
	ImportLeaf, ImportLeafKind, ImportTree, flatten_import_tree, import_leaf_binding_name,
	importable_parent,
};
pub use model::{
	DefIndex, DefNameKey, DiscoveredDef, DiscoveredFile, ImportKind, ImportTable, ImportTarget,
	RefHints, ResolvedRef, TargetExpr, UnresolvedRef,
};
pub use resolve::{LangResolverStrategy, LocalResolver, Resolution};
pub use scope::{Namespace, Rib, Scope, ScopeId, ScopeTree};
