//! Parallel workspace snapshot model.
//!
//! This module is intentionally not wired directly to `WorkspaceStore`. It
//! defines the target orchestration model and tests it through semantic ports.

mod model;
mod refresh;
mod view;

pub use model::{
	ChangeId, ChangeOverlay, ChangeOverlayReport, ChangeRecord, ChangeRecordCoreFields,
	ChangeRecordFields, ChangeResource, ChangeStatus, CodeIndex, CodeIndexFields, LinkageEdge,
	LinkageGraph, LinkageGraphReport, ReferenceId, ReferenceRecord, ResourceGeneration,
	SourceCatalog, SourceFileRecord, SourceFileRecordFields, SourceId, SourceUnit, SymbolId,
	SymbolLocation, SymbolRecord, SymbolRecordFields, UnresolvedReference, WorkspaceFailure,
	WorkspaceRequest, WorkspaceResource, WorkspaceResult, WorkspaceSnapshot, WorkspaceTransition,
};
pub use refresh::WorkspaceSnapshotRefresh;
pub use view::{
	ChangeDetail, ChangeSummary, ReferenceDirection, ReferenceSet, ReferenceSetSummary,
	ReferenceSummary, SearchHit, SourceSummary, SymbolDetail, SymbolReferences, SymbolSummary,
	UnresolvedLinkageReport, WorkspaceView,
};

#[cfg(test)]
mod tests;
