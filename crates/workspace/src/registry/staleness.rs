use std::path::PathBuf;

use crate::live::WorkspaceLiveRefreshPlan;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceStaleness {
	pub stale_paths: Vec<PathBuf>,
	pub requires_rescan: bool,
	pub git_base_stale: bool,
}

impl WorkspaceStaleness {
	pub fn from_plan(plan: &WorkspaceLiveRefreshPlan) -> Self {
		Self {
			stale_paths: plan.source_paths().to_vec(),
			requires_rescan: plan.requires_rescan(),
			git_base_stale: plan.includes_git_base(),
		}
	}

	pub fn is_stale(&self) -> bool {
		self.requires_rescan || !self.stale_paths.is_empty() || self.git_base_stale
	}

	pub fn summary(&self) -> String {
		summary_parts(
			self.stale_paths.len(),
			self.requires_rescan,
			self.git_base_stale,
		)
	}

	pub fn plan_summary(plan: &WorkspaceLiveRefreshPlan) -> String {
		summary_parts(
			plan.source_paths().len(),
			plan.requires_rescan(),
			plan.includes_git_base(),
		)
	}
}

fn summary_parts(stale_paths: usize, requires_rescan: bool, git_base_stale: bool) -> String {
	if requires_rescan {
		return "rescan required".to_string();
	}
	let mut parts = Vec::new();
	if stale_paths > 0 {
		parts.push(format!("{stale_paths} stale path(s)"));
	}
	if git_base_stale {
		parts.push("git base changed".to_string());
	}
	if parts.is_empty() {
		return "fresh".to_string();
	}
	parts.join(", ")
}
