pub mod changes;
pub mod code;
pub(crate) mod environment;
pub mod facade;
pub mod linkage;
pub mod snapshot;
pub mod source;

mod cache;
pub mod extract;
mod lang;
mod lines;
mod sources;
pub mod tsconfig;
mod walk;

pub const DEFAULT_IDENTITY_SCHEME: &str = "code+moniker://";

pub use facade::{LocalWorkspaceFacade, LocalWorkspaceOptions, WorkspaceFacade, WorkspacePorts};
