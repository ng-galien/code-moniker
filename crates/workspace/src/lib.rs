pub mod cache;
pub mod changes;
pub mod code;
pub mod environment;
pub mod extract;
pub mod facade;
pub mod lang;
pub mod lines;
pub mod linkage;
pub mod snapshot;
pub mod source;
pub mod sources;
pub mod tsconfig;
pub mod walk;

pub const DEFAULT_IDENTITY_SCHEME: &str = "code+moniker://";

pub use facade::{LocalWorkspaceFacade, LocalWorkspaceOptions, WorkspaceFacade, WorkspacePorts};
