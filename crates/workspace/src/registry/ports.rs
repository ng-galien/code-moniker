use std::sync::Arc;

use crate::live::WorkspaceWatchRoot;
use crate::snapshot::{
	WorkspaceFailure, WorkspaceRequest, WorkspaceSnapshot, WorkspaceTransition, WorkspaceView,
};
use code_moniker_core::core::logger::Logger;

use super::command::{WorkspaceCommandKind, WorkspaceScopeUri, WorkspaceSnapshotPublication};
use super::event::{WorkspaceEvent, WorkspaceEventCursor};

pub struct WorkspacePorts<Sources, Index, Linkage, Changes> {
	pub(crate) source_catalog: Sources,
	pub(crate) code_index: Index,
	pub(crate) linkage: Linkage,
	pub(crate) change_overlay: Changes,
	pub(crate) logger: Option<Arc<dyn Logger>>,
	live_watch_roots: Box<LiveWatchRoots>,
}

type LiveWatchRoots = dyn Fn(Option<&WorkspaceSnapshot>) -> Vec<WorkspaceWatchRoot> + Send + Sync;

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
			logger: None,
			live_watch_roots: Box::new(|_| Vec::new()),
		}
	}

	pub fn with_logger(mut self, logger: Arc<dyn Logger>) -> Self {
		self.logger = Some(logger);
		self
	}

	pub(crate) fn with_live_watch_roots<F>(mut self, live_watch_roots: F) -> Self
	where
		F: Fn(Option<&WorkspaceSnapshot>) -> Vec<WorkspaceWatchRoot> + Send + Sync + 'static,
	{
		self.live_watch_roots = Box::new(live_watch_roots);
		self
	}

	pub(crate) fn live_watch_roots(
		&self,
		snapshot: Option<&WorkspaceSnapshot>,
	) -> Vec<WorkspaceWatchRoot> {
		(self.live_watch_roots)(snapshot)
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
