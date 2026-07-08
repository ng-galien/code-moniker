//! Parallel workspace snapshot model.
//!
//! This module is intentionally not wired directly to `WorkspaceStore`. It
//! defines the target orchestration model and tests it through semantic ports.

mod model;
mod records;
mod view;

pub use records::RecordTable;

pub use model::{
	ChangeId, ChangeOverlay, ChangeOverlayReport, ChangeRecord, ChangeRecordCoreFields,
	ChangeResource, ChangeStatus, CodeIndex, CodeIndexTimings, ExternalReference,
	ExternalReferenceOrigin, LinkageEdge, LinkageReadIndex, LinkageReadIndexHandle,
	LinkageSnapshot, ReferenceId, ReferenceRecord, ResourceGeneration, SourceCatalog,
	SourceFileRecord, SourceId, SourceUnit, SymbolId, SymbolLocation, SymbolRecord,
	UnresolvedReference, WorkspaceFailure, WorkspaceRequest, WorkspaceResource, WorkspaceResult,
	WorkspaceSnapshot, WorkspaceTimings, WorkspaceTransition,
};
pub use view::{
	ChangeDetail, ChangeSummary, ReferenceDirection, ReferenceSet, ReferenceSetSummary,
	ReferenceSummary, SearchHit, SourceSummary, SymbolDetail, SymbolReferences, SymbolSummary,
	UnresolvedLinkageReport, WorkspaceView,
};
