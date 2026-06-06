mod decision;
mod metrics;
mod store;

pub(in crate::linkage) use decision::{
	ExternalOrigin, ReferenceLinkageDecision, ResolutionScope, UnknownReason, project_decisions,
};
pub use metrics::LinkageMemoryMetrics;
pub(in crate::linkage) use store::{LinkageStore, LinkageStoreRefresh, reference_indexes};
