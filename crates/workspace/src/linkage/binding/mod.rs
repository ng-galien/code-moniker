mod decision;
mod metrics;
mod store;

pub(in crate::linkage) use decision::{
	BlockReason, ExternalOrigin, ReferenceLinkageDecision, ResolutionDecision, ResolutionScope,
	UnknownReason, project_decisions,
};
pub use metrics::LinkageMemoryMetrics;
pub(in crate::linkage) use store::{
	LinkageStore, LinkageStoreRefresh, insert_reference_ordinals, reference_indexes,
};
