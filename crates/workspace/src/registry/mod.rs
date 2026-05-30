//! Workspace registry vocabulary.
//!
//! This module is intentionally small for now. The local code-moniker fragment
//! records the agreed topology before runtime code is moved behind it.

mod build;
mod command;
mod event;
mod local;
mod ports;
mod runtime;
mod state;

pub use command::{
	WorkspaceCommand, WorkspaceCommandId, WorkspaceCommandKind, WorkspaceCommandSpec,
	WorkspaceScopeUri, WorkspaceSnapshotPublication,
};
pub use event::{WorkspaceEvent, WorkspaceEventCursor, WorkspaceEventKind};
pub use local::{LocalWorkspaceOptions, LocalWorkspaceRegistry};
pub use ports::{WorkspaceCommandPort, WorkspaceEventPort, WorkspacePorts, WorkspaceQueryPort};
pub use runtime::{WorkspaceCommands, WorkspaceEvents, WorkspaceQueries, WorkspaceRegistry};
