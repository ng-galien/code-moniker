use std::sync::{Arc, Mutex};

use code_moniker_workspace::facade::{
	LocalWorkspaceFacade, LocalWorkspaceOptions, local_workspace_ports,
};
use code_moniker_workspace::snapshot::{WorkspaceRequest, WorkspaceSnapshot, WorkspaceTransition};
use code_moniker_workspace::source::LocalResourceCache;

use crate::session::SessionOptions;

#[derive(Clone)]
pub(in crate::mcp) struct McpWorkspace {
	state: Arc<Mutex<McpWorkspaceState>>,
}

impl McpWorkspace {
	pub(in crate::mcp) fn new(opts: &SessionOptions) -> Self {
		let cache = LocalResourceCache::default();
		let workspace = LocalWorkspaceFacade::new(local_workspace_ports(
			LocalWorkspaceOptions::new(opts.paths.clone(), opts.project.clone())
				.with_cache_dir(opts.cache_dir.clone()),
			cache,
		));
		Self {
			state: Arc::new(Mutex::new(McpWorkspaceState {
				workspace,
				stage: WorkspaceStage::Empty,
			})),
		}
	}

	pub(in crate::mcp) fn load_catalog_snapshot(
		&self,
		label: &str,
	) -> anyhow::Result<Arc<WorkspaceSnapshot>> {
		let mut state = self
			.state
			.lock()
			.map_err(|_| anyhow::anyhow!("mcp workspace state is poisoned"))?;
		if state.stage != WorkspaceStage::Empty {
			return state
				.workspace
				.snapshot_arc()
				.ok_or_else(|| anyhow::anyhow!("workspace catalog snapshot is unavailable"));
		}
		match state.workspace.load_catalog(WorkspaceRequest::new(label)) {
			WorkspaceTransition::Ready { .. } => {
				state.stage = WorkspaceStage::Catalog;
				state
					.workspace
					.snapshot_arc()
					.ok_or_else(|| anyhow::anyhow!("workspace catalog snapshot is unavailable"))
			}
			WorkspaceTransition::Failed { failure, .. } => anyhow::bail!(failure.message),
		}
	}

	pub(in crate::mcp) fn load_index_snapshot(
		&self,
		label: &str,
	) -> anyhow::Result<Arc<WorkspaceSnapshot>> {
		let mut state = self
			.state
			.lock()
			.map_err(|_| anyhow::anyhow!("mcp workspace state is poisoned"))?;
		if state.stage == WorkspaceStage::Index {
			return state
				.workspace
				.snapshot_arc()
				.ok_or_else(|| anyhow::anyhow!("workspace index snapshot is unavailable"));
		}
		let request = match state.stage {
			WorkspaceStage::Empty => WorkspaceRequest::new(label),
			WorkspaceStage::Catalog | WorkspaceStage::Index => {
				WorkspaceRequest::new(label).reuse_current_catalog()
			}
		};
		match state.workspace.load_index(request) {
			WorkspaceTransition::Ready { .. } => {
				state.stage = WorkspaceStage::Index;
				state
					.workspace
					.snapshot_arc()
					.ok_or_else(|| anyhow::anyhow!("workspace index snapshot is unavailable"))
			}
			WorkspaceTransition::Failed { failure, .. } => anyhow::bail!(failure.message),
		}
	}
}

struct McpWorkspaceState {
	workspace: LocalWorkspaceFacade,
	stage: WorkspaceStage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkspaceStage {
	Empty,
	Catalog,
	Index,
}
