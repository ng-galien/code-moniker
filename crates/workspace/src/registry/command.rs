use std::sync::Arc;

use crate::snapshot::{WorkspaceRequest, WorkspaceSnapshot};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct WorkspaceCommandId(u64);

impl WorkspaceCommandId {
	pub fn new(value: u64) -> Self {
		Self(value)
	}

	pub fn value(self) -> u64 {
		self.0
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct WorkspaceScopeUri(String);

impl WorkspaceScopeUri {
	pub fn new(value: impl Into<String>) -> Self {
		Self(value.into())
	}

	pub fn workspace() -> Self {
		Self(format!("{}./", crate::DEFAULT_IDENTITY_SCHEME))
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceCommandKind {
	LoadSources,
	BuildIndex,
	ResolveLinkage,
	PublishSnapshot,
	RefreshPaths,
	RefreshChanges,
	Refresh,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceCommand {
	pub id: WorkspaceCommandId,
	pub scope_uri: WorkspaceScopeUri,
	pub kind: WorkspaceCommandKind,
	pub request: WorkspaceRequest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceCommandSpec {
	pub scope_uri: WorkspaceScopeUri,
	pub kind: WorkspaceCommandKind,
	pub request: WorkspaceRequest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSnapshotPublication {
	pub scope_uri: WorkspaceScopeUri,
	pub request: WorkspaceRequest,
	pub snapshot: Arc<WorkspaceSnapshot>,
}

impl WorkspaceCommandSpec {
	pub fn new(
		kind: WorkspaceCommandKind,
		scope_uri: WorkspaceScopeUri,
		request: WorkspaceRequest,
	) -> Self {
		Self {
			scope_uri,
			kind,
			request,
		}
	}
}

impl WorkspaceSnapshotPublication {
	pub fn new(
		scope_uri: WorkspaceScopeUri,
		request: WorkspaceRequest,
		snapshot: Arc<WorkspaceSnapshot>,
	) -> Self {
		Self {
			scope_uri,
			request,
			snapshot,
		}
	}

	pub fn workspace(request: WorkspaceRequest, snapshot: Arc<WorkspaceSnapshot>) -> Self {
		Self::new(WorkspaceScopeUri::workspace(), request, snapshot)
	}
}

impl WorkspaceCommand {
	pub fn new(
		id: WorkspaceCommandId,
		scope_uri: WorkspaceScopeUri,
		kind: WorkspaceCommandKind,
		request: WorkspaceRequest,
	) -> Self {
		Self {
			id,
			scope_uri,
			kind,
			request,
		}
	}
}
