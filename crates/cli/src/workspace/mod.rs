#[cfg(feature = "tui")]
mod compat;
pub mod index;
#[cfg(feature = "tui")]
pub(crate) mod model;
#[cfg(feature = "tui")]
pub(crate) mod store;
#[cfg(all(feature = "tui", test))]
mod target_tests;

#[cfg(feature = "tui")]
pub use code_moniker_workspace::{changes, code, facade, linkage, snapshot, source};

#[cfg(feature = "tui")]
pub mod git {
	pub(crate) use super::changes::diff::*;
}

#[cfg(feature = "tui")]
pub use compat::SessionStoreBridge;
#[cfg(feature = "tui")]
pub use facade::{LocalWorkspaceFacade, LocalWorkspaceOptions, WorkspaceFacade, WorkspacePorts};
#[cfg(feature = "tui")]
pub(crate) use git::ChangeStatus;
pub use index::{CheckSummary, DefLocation, RefLocation, SessionOptions, SessionStats, ViewFilter};
#[cfg(feature = "tui")]
pub(crate) use model::{
	ChangeDetail, ChangeId, ReferenceGroup, ReferenceSet, UnresolvedLinkageReport, UsageFocus,
};
#[cfg(feature = "tui")]
pub(crate) use store::{IndexStore, StoreWatchRoot, WorkspaceHandle};
