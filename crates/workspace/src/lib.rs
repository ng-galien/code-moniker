pub mod cache;
pub mod changes;
pub mod code;
pub mod environment;
pub mod extract;
pub mod lang;
pub mod lines;
pub mod linkage;
pub mod registry;
pub mod snapshot;
pub mod source;
pub mod sources;
pub mod tsconfig;
pub mod walk;

pub const DEFAULT_IDENTITY_SCHEME: &str = "code+moniker://";

pub use registry::{
	LocalWorkspaceOptions, LocalWorkspaceRegistry, WorkspaceEvent, WorkspaceEventKind,
	WorkspacePorts, WorkspaceRegistry, WorkspaceScopeUri,
};
