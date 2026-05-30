use crate::snapshot::ResourceGeneration;

use super::command::{WorkspaceCommandId, WorkspaceScopeUri};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct WorkspaceEventCursor(usize);

impl WorkspaceEventCursor {
	pub fn start() -> Self {
		Self(0)
	}

	pub fn value(self) -> usize {
		self.0
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceEventKind {
	CommandAccepted,
	WorkStarted,
	WorkCompleted,
	WorkFailed,
	SnapshotPublished,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceEvent {
	pub scope_uri: WorkspaceScopeUri,
	pub generation: ResourceGeneration,
	pub command_id: WorkspaceCommandId,
	pub kind: WorkspaceEventKind,
}

pub(crate) struct WorkspaceEventContext {
	scope_uri: WorkspaceScopeUri,
	generation: ResourceGeneration,
	command_id: WorkspaceCommandId,
}

impl WorkspaceEventContext {
	pub(crate) fn new(
		scope_uri: WorkspaceScopeUri,
		generation: ResourceGeneration,
		command_id: WorkspaceCommandId,
	) -> Self {
		Self {
			scope_uri,
			generation,
			command_id,
		}
	}

	pub(crate) fn event(&self, kind: WorkspaceEventKind) -> WorkspaceEvent {
		WorkspaceEvent {
			scope_uri: self.scope_uri.clone(),
			generation: self.generation,
			command_id: self.command_id,
			kind,
		}
	}
}

#[derive(Default)]
pub(crate) struct WorkspaceEventLog {
	events: Vec<WorkspaceEvent>,
}

impl WorkspaceEventLog {
	pub(crate) fn cursor(&self) -> WorkspaceEventCursor {
		WorkspaceEventCursor(self.events.len())
	}

	pub(crate) fn publish(&mut self, event: WorkspaceEvent) {
		self.events.push(event);
	}

	pub(crate) fn since(&self, cursor: WorkspaceEventCursor) -> &[WorkspaceEvent] {
		let start = cursor.value().min(self.events.len());
		&self.events[start..]
	}
}
