#[cfg(feature = "tui")]
pub mod git;
pub mod index;
#[cfg(feature = "tui")]
mod model;
#[cfg(feature = "tui")]
pub mod query;
#[cfg(feature = "tui")]
mod snapshot;
#[cfg(feature = "tui")]
mod store;
#[cfg(feature = "tui")]
mod symbols;

#[cfg(feature = "tui")]
pub(crate) use git::ChangeStatus;
pub use index::{
	CheckSummary, DefLocation, IndexedFile, IndexedRoot, RefLocation, SessionIndex, SessionOptions,
	SessionStats, ViewFilter,
};
#[cfg(feature = "tui")]
pub(crate) use model::{
	ChangeDetail, ChangeId, ReferenceGroup, ReferenceSet, SearchHit, UsageFocus,
};
#[cfg(feature = "tui")]
pub(crate) use query::{SymbolFilter, parse_filter};
#[cfg(feature = "tui")]
pub(crate) use store::{
	GitOverlayRefresh, GitOverlayRefreshInput, IndexStore, StoreWatchRoot, WorkspaceStore,
};
