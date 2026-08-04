mod decision;
mod indexes;
mod metrics;
mod projection;
mod restore;
mod store;
#[cfg(test)]
mod tests;

pub(in crate::linkage) use decision::{
	BlockReason, ExternalOrigin, ReferenceLinkageDecision, ResolutionDecision, ResolutionScope,
	UnknownReason,
};
pub(in crate::linkage) use indexes::reference_indexes;
pub use metrics::LinkageMemoryMetrics;
pub(in crate::linkage) use projection::project_decisions;
pub(in crate::linkage) use store::{LinkageStore, LinkageStoreRefresh, insert_reference_ordinals};
