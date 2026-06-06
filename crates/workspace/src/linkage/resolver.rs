use std::sync::Arc;
use std::time::Duration;

use crate::linkage::delta::LinkageRefreshImpact;
use crate::linkage::full::run_full_linkage_with_timings;
use crate::linkage::method_indexer::MethodIndexer;
use crate::linkage::metrics::LinkageMemoryMetrics;
use crate::linkage::refresh::run_refresh_linkage_with_timings;
use crate::linkage::store::LinkageStore;
use crate::snapshot::{
	CodeIndex, LinkageSnapshot, WorkspaceFailure, WorkspaceResource, WorkspaceResult,
};
use crate::source::{CodeIndexMaterial, LocalResourceCache};

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
	pub(super) cache: LocalResourceCache,
	pub(super) store: Option<LinkageStore>,
	pub(super) method_indexer: Option<MethodIndexer>,
	pub(super) memory: LinkageMemoryMetrics,
}

impl LocalLinkage {
	pub fn new(cache: LocalResourceCache) -> Self {
		Self {
			cache,
			store: None,
			method_indexer: None,
			memory: LinkageMemoryMetrics::default(),
		}
	}

	pub(super) fn linkage_material(
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
