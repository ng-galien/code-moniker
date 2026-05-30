use std::sync::Arc;

use crate::snapshot::{
	WorkspaceFailure, WorkspaceRequest, WorkspaceSnapshot, WorkspaceTransition, WorkspaceView,
};

use super::command::{WorkspaceCommandKind, WorkspaceScopeUri, WorkspaceSnapshotPublication};
use super::event::{WorkspaceEvent, WorkspaceEventCursor};

pub struct WorkspacePorts<Sources, Index, Linkage, Changes> {
	pub(crate) source_catalog: Sources,
	pub(crate) code_index: Index,
	pub(crate) linkage: Linkage,
	pub(crate) change_overlay: Changes,
}

impl<Sources, Index, Linkage, Changes> WorkspacePorts<Sources, Index, Linkage, Changes> {
	pub fn new(
		source_catalog: Sources,
		code_index: Index,
		linkage: Linkage,
		change_overlay: Changes,
	) -> Self {
		Self {
			source_catalog,
			code_index,
			linkage,
			change_overlay,
		}
	}
}

pub trait WorkspaceCommandPort {
	fn execute_command(
		&mut self,
		kind: WorkspaceCommandKind,
		scope_uri: WorkspaceScopeUri,
		request: WorkspaceRequest,
	) -> WorkspaceTransition;

	fn publish_snapshot(
		&mut self,
		publication: WorkspaceSnapshotPublication,
	) -> WorkspaceTransition;
}

pub trait WorkspaceQueryPort {
	fn snapshot(&self) -> Option<&WorkspaceSnapshot>;
	fn snapshot_arc(&self) -> Option<Arc<WorkspaceSnapshot>>;
	fn view(&self) -> Option<WorkspaceView<'_>>;
	fn last_failure(&self) -> Option<&WorkspaceFailure>;
}

pub trait WorkspaceEventPort {
	fn event_cursor(&self) -> WorkspaceEventCursor;
	fn events_since(&self, cursor: WorkspaceEventCursor) -> &[WorkspaceEvent];
}
