mod binding;
mod catalog;
mod change;
mod language;
mod resolve;
mod source_groups;

use std::sync::Arc;
use std::time::Duration;

use crate::linkage::binding::LinkageStore;
use crate::linkage::catalog::CandidateCatalog;
use crate::linkage::change::run_refresh_linkage_with_timings;
use crate::linkage::resolve::{MethodIndexer, run_full_linkage_with_timings};
use crate::snapshot::{
	CodeIndex, LinkageSnapshot, WorkspaceFailure, WorkspaceResource, WorkspaceResult,
};
use crate::source::{CodeIndexMaterial, LocalResourceCache};

pub use binding::LinkageMemoryMetrics;
pub use change::{LinkageGraphDelta, LinkageRefreshImpact};

pub trait LinkagePort {
	fn resolve_linkage(&mut self, index: &CodeIndex) -> WorkspaceResult<LinkageSnapshot>;
	fn refresh_linkage(
		&mut self,
		current: &LinkageSnapshot,
		index: &CodeIndex,
		impact: LinkageRefreshImpact,
	) -> WorkspaceResult<LinkageSnapshot>;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LinkageTimings {
	pub candidate_index: Duration,
	pub manifest_policy: Duration,
	pub resolve_references: Duration,
	pub semantic_enhance: Duration,
	pub store_index: Duration,
	pub project_snapshot: Duration,
	pub total: Duration,
}

pub struct TimedLinkageSnapshot {
	pub snapshot: LinkageSnapshot,
	pub timings: LinkageTimings,
	pub memory: LinkageMemoryMetrics,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LinkageRefreshTimings {
	pub candidate_index: Duration,
	pub plan_invalidation: Duration,
	pub resolve_references: Duration,
	pub apply_store: Duration,
	pub semantic_enhance: Duration,
	pub rebuild_indexes: Duration,
	pub project_snapshot: Duration,
	pub total: Duration,
	pub stale_refs: usize,
	pub changed_refs: usize,
}

pub struct TimedLinkageRefresh {
	pub snapshot: LinkageSnapshot,
	pub timings: LinkageRefreshTimings,
	pub memory: LinkageMemoryMetrics,
}

pub struct LocalLinkage {
	pub(in crate::linkage) cache: LocalResourceCache,
	pub(in crate::linkage) store: Option<LinkageStore>,
	pub(in crate::linkage) candidates: Option<CandidateCatalog>,
	pub(in crate::linkage) method_indexer: Option<MethodIndexer>,
	pub(in crate::linkage) memory: LinkageMemoryMetrics,
}

impl LocalLinkage {
	pub fn new(cache: LocalResourceCache) -> Self {
		Self {
			cache,
			store: None,
			candidates: None,
			method_indexer: None,
			memory: LinkageMemoryMetrics::default(),
		}
	}

	pub(in crate::linkage) fn linkage_material(
		&self,
		index: &CodeIndex,
	) -> WorkspaceResult<Arc<CodeIndexMaterial>> {
		self.cache.index_material(index.generation).ok_or_else(|| {
			WorkspaceFailure::new(
				WorkspaceResource::LinkageSnapshot,
				"code index material is unavailable",
			)
		})
	}

	pub fn refresh_linkage_with_timings(
		&mut self,
		previous: &LinkageSnapshot,
		code_index: &CodeIndex,
		refresh_impact: LinkageRefreshImpact,
	) -> WorkspaceResult<TimedLinkageRefresh> {
		run_refresh_linkage_with_timings(self, previous, code_index, refresh_impact)
	}

	pub fn resolve_linkage_with_timings(
		&mut self,
		index: &CodeIndex,
	) -> WorkspaceResult<TimedLinkageSnapshot> {
		run_full_linkage_with_timings(self, index)
	}
}

impl LinkagePort for LocalLinkage {
	fn resolve_linkage(&mut self, index: &CodeIndex) -> WorkspaceResult<LinkageSnapshot> {
		Ok(self.resolve_linkage_with_timings(index)?.snapshot)
	}

	fn refresh_linkage(
		&mut self,
		snapshot: &LinkageSnapshot,
		indexed: &CodeIndex,
		change: LinkageRefreshImpact,
	) -> WorkspaceResult<LinkageSnapshot> {
		Ok(self
			.refresh_linkage_with_timings(snapshot, indexed, change)?
			.snapshot)
	}
}
