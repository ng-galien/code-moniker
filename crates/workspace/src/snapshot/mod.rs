//! Parallel workspace snapshot model.
//!
//! This module is intentionally not wired directly to `WorkspaceStore`. It
//! defines the target orchestration model and tests it through semantic ports.

mod model;
mod path;
mod records;
mod view;

pub use records::RecordTable;

pub use model::{
	CandidateReason, CandidateReference, CandidateScope, ChangeId, ChangeOverlay,
	ChangeOverlayReport, ChangeRecord, ChangeRecordCoreFields, ChangeResource, ChangeStatus,
	CodeIndex, CodeIndexTimings, DynamicReason, DynamicReference, ExternalReference,
	ExternalReferenceOrigin, LinkageEdge, LinkageReadIndex, LinkageReadIndexHandle,
	LinkageSnapshot, ReferenceId, ReferenceRecord, ResolutionEvidence, ResourceGeneration,
	SourceCatalog, SourceFileRecord, SourceId, SourceUnit, SymbolId, SymbolLocation, SymbolRecord,
	UnresolvedReason, UnresolvedReference, WorkspaceCancellation, WorkspaceFailure,
	WorkspaceRequest, WorkspaceResource, WorkspaceResult, WorkspaceSnapshot, WorkspaceTimings,
	WorkspaceTransition,
};
pub use path::{
	BoundedPathCoverage, BoundedPathEdge, BoundedPathLimits, BoundedPathScope, BoundedPathSearch,
};
pub use view::{
	ChangeDetail, ChangeSummary, ReferenceDirection, ReferenceSet, ReferenceSetSummary,
	ReferenceSummary, ReferenceView, SearchHit, SourceSummary, SourceView, SymbolDetail,
	SymbolReferences, SymbolSummary, SymbolView, UnresolvedLinkageReport, WorkspaceView,
};
