use std::path::PathBuf;

use crate::live::WorkspaceLiveRefreshPlan;
use crate::snapshot::ResourceGeneration;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceStaleness {
	pub stale_paths: Vec<PathBuf>,
	pub requires_rescan: bool,
	pub git_base_stale: bool,
	pub since_generation: Option<ResourceGeneration>,
}

impl WorkspaceStaleness {
	pub(crate) fn from_pending(
		pending: &WorkspaceLiveRefreshPlan,
		since_generation: Option<ResourceGeneration>,
	) -> Self {
		Self {
			stale_paths: pending.source_paths().to_vec(),
			requires_rescan: pending.requires_rescan(),
			git_base_stale: pending.includes_git_base(),
			since_generation,
		}
	}

	pub fn is_stale(&self) -> bool {
		self.requires_rescan || !self.stale_paths.is_empty() || self.git_base_stale
	}
}

#[derive(Default)]
pub(crate) struct PendingStaleness {
	plan: WorkspaceLiveRefreshPlan,
	since: Option<ResourceGeneration>,
}

impl PendingStaleness {
	pub(crate) fn coalesce(
		&mut self,
		current: Option<ResourceGeneration>,
		plan: WorkspaceLiveRefreshPlan,
	) -> WorkspaceStaleness {
		if self.plan.is_empty() {
			self.since = current;
		}
		let pending = std::mem::take(&mut self.plan);
		self.plan = pending.coalesce(plan);
		self.staleness()
	}

	pub(crate) fn take(&mut self) -> WorkspaceLiveRefreshPlan {
		self.since = None;
		std::mem::take(&mut self.plan)
	}

	pub(crate) fn staleness(&self) -> WorkspaceStaleness {
		WorkspaceStaleness::from_pending(&self.plan, self.since)
	}
}
