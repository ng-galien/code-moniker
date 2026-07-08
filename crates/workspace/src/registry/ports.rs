use std::sync::Arc;

use crate::changes::ChangeOverlayPort;
use crate::code::CodeIndexPort;
use crate::linkage::LinkagePort;
use crate::live::WorkspaceWatchRoot;
use crate::snapshot::{
	WorkspaceFailure, WorkspaceRequest, WorkspaceSnapshot, WorkspaceTransition, WorkspaceView,
};
use crate::source::SourceCatalogPort;

use super::command::{WorkspaceCommandKind, WorkspaceScopeUri, WorkspaceSnapshotPublication};
use super::event::{WorkspaceEvent, WorkspaceEventCursor};

pub struct WorkspacePorts {
	pub(crate) source_catalog: Box<dyn SourceCatalogPort + Send>,
	pub(crate) code_index: Box<dyn CodeIndexPort + Send>,
	pub(crate) linkage: Box<dyn LinkagePort + Send>,
	pub(crate) change_overlay: Box<dyn ChangeOverlayPort + Send>,
	live_watch_roots: Box<LiveWatchRoots>,
}

type LiveWatchRoots = dyn Fn(Option<&WorkspaceSnapshot>) -> Vec<WorkspaceWatchRoot> + Send + Sync;

impl WorkspacePorts {
	pub fn new(
		source_catalog: impl SourceCatalogPort + Send + 'static,
		code_index: impl CodeIndexPort + Send + 'static,
		linkage: impl LinkagePort + Send + 'static,
		change_overlay: impl ChangeOverlayPort + Send + 'static,
	) -> Self {
		Self {
			source_catalog: Box::new(source_catalog),
			code_index: Box::new(code_index),
			linkage: Box::new(linkage),
			change_overlay: Box::new(change_overlay),
			live_watch_roots: Box::new(|_| Vec::new()),
		}
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
