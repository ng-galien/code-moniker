use std::sync::Arc;

use crate::live::WorkspaceLiveRefreshPlan;
use crate::snapshot::{
	ResourceGeneration, WorkspaceFailure, WorkspaceResult, WorkspaceSnapshot, WorkspaceTransition,
};

pub(crate) struct WorkspaceState {
	next_generation: u64,
	snapshot: Option<Arc<WorkspaceSnapshot>>,
	last_failure: Option<WorkspaceFailure>,
	pub(crate) pending: WorkspaceLiveRefreshPlan,
}

impl WorkspaceState {
	pub(crate) fn new() -> Self {
		Self {
			next_generation: 1,
			snapshot: None,
			last_failure: None,
			pending: WorkspaceLiveRefreshPlan::default(),
		}
	}

	pub(crate) fn allocate_generation(&mut self) -> ResourceGeneration {
		let generation = ResourceGeneration::new(self.next_generation);
		self.next_generation += 1;
		generation
	}

	pub(crate) fn publish(
		&mut self,
		result: WorkspaceResult<WorkspaceSnapshot>,
	) -> WorkspaceTransition {
		match result {
			Ok(snapshot) => self.publish_snapshot(snapshot),
			Err(failure) => self.publish_failure(failure),
		}
	}

	pub(crate) fn adopt_snapshot_arc(
		&mut self,
		snapshot: Arc<WorkspaceSnapshot>,
	) -> WorkspaceTransition {
		let generation = snapshot.generation;
		self.next_generation = self.next_generation.max(snapshot.generation.value() + 1);
		self.snapshot = Some(snapshot);
		self.last_failure = None;
		WorkspaceTransition::Ready { generation }
	}

	pub(crate) fn snapshot(&self) -> Option<&WorkspaceSnapshot> {
		self.snapshot.as_deref()
	}

	pub(crate) fn snapshot_arc(&self) -> Option<Arc<WorkspaceSnapshot>> {
		self.snapshot.clone()
	}

	pub(crate) fn last_failure(&self) -> Option<&WorkspaceFailure> {
		self.last_failure.as_ref()
	}

	fn publish_snapshot(&mut self, snapshot: WorkspaceSnapshot) -> WorkspaceTransition {
		let generation = snapshot.generation;
		self.snapshot = Some(Arc::new(snapshot));
		self.last_failure = None;
		WorkspaceTransition::Ready { generation }
	}

	fn publish_failure(&mut self, failure: WorkspaceFailure) -> WorkspaceTransition {
		let preserved_generation = self.snapshot.as_ref().map(|snapshot| snapshot.generation);
		self.last_failure = Some(failure.clone());
		WorkspaceTransition::Failed {
			failure,
			preserved_generation,
		}
	}
}
