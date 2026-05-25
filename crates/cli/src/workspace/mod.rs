#[cfg(feature = "tui")]
mod bridge;
#[cfg(feature = "tui")]
pub mod git;
pub mod index;
#[cfg(feature = "tui")]
mod linkage;
#[cfg(feature = "tui")]
mod model;
#[cfg(feature = "tui")]
pub mod resources;
#[cfg(feature = "tui")]
pub mod session;
#[cfg(feature = "tui")]
mod snapshot;
#[cfg(feature = "tui")]
mod store;
#[cfg(feature = "tui")]
mod symbols;

#[cfg(feature = "tui")]
pub use bridge::SessionStoreBridge;
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
	GitOverlayRefresh, GitOverlayRefreshInput, IndexStore, StoreWatchRoot, WorkspaceStore,
};
