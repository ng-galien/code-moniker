#[cfg(feature = "tui")]
pub mod changes;
#[cfg(feature = "tui")]
pub mod code;
#[cfg(feature = "tui")]
mod compat;
#[cfg(feature = "tui")]
pub mod facade;
mod legacy;
#[cfg(feature = "tui")]
pub mod linkage;
#[cfg(feature = "tui")]
pub mod snapshot;
#[cfg(feature = "tui")]
pub mod source;

#[cfg(feature = "tui")]
pub mod git {
	pub(crate) use super::changes::diff::*;
}

pub mod index {
	pub use super::legacy::index::*;
}

#[cfg(feature = "tui")]
pub(crate) mod model {
	pub(crate) use super::legacy::model::*;
}

#[cfg(feature = "tui")]
pub(crate) mod store {
	pub(crate) use super::legacy::store::*;
}

#[cfg(feature = "tui")]
pub use compat::SessionStoreBridge;
#[cfg(feature = "tui")]
pub use facade::{LocalWorkspaceFacade, LocalWorkspaceOptions, WorkspaceFacade, WorkspacePorts};
#[cfg(feature = "tui")]
pub(crate) use git::ChangeStatus;
pub use index::{
	CheckSummary, DefLocation, IndexedFile, IndexedRoot, RefLocation, SessionIndex, SessionOptions,
	SessionStats, ViewFilter,
};
#[cfg(feature = "tui")]
pub(crate) use model::{
	ChangeDetail, ChangeId, ReferenceGroup, ReferenceSet, UnresolvedLinkageReport, UsageFocus,
};
#[cfg(feature = "tui")]
pub(crate) use store::{
	GitOverlayRefresh, GitOverlayRefreshInput, IndexStore, StoreWatchRoot, WorkspaceHandle,
	WorkspaceStore,
};
