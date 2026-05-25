//! Parallel workspace session model.
//!
//! This module is intentionally not wired to `WorkspaceStore` yet. It defines
//! the target orchestration model and tests it with semantic resource traits.

mod contracts;
mod engine;
mod model;
mod view;

pub use contracts::{ChangeOverlayPort, CodeIndexPort, LinkagePort, SourceCatalogPort};
pub use engine::WorkspaceSession;
pub use model::{
	ChangeId, ChangeOverlay, ChangeOverlayReport, ChangeRecord, ChangeRecordCoreFields,
	ChangeRecordFields, ChangeResource, ChangeStatus, CodeIndex, CodeIndexFields, LinkageEdge,
	LinkageGraph, ReferenceId, ReferenceRecord, ResourceGeneration, RuleDiagnostic,
	RuleDiagnosticSeverity, RuleDiagnostics, SourceCatalog, SourceFileRecord,
	SourceFileRecordFields, SourceId, SourceUnit, SymbolId, SymbolRecord, SymbolRecordFields,
	UnresolvedReference, WorkspaceFailure, WorkspaceRequest, WorkspaceResource, WorkspaceResult,
	WorkspaceSnapshot, WorkspaceTransition,
};
pub use view::{
	ChangeDetail, ChangeSummary, ReferenceDirection, ReferenceSet, ReferenceSetSummary,
	ReferenceSummary, SearchHit, SourceSummary, SymbolDetail, SymbolReferences, SymbolSummary,
	UnresolvedLinkageReport, WorkspaceView,
};
