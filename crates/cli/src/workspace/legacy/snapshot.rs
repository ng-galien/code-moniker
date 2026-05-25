use std::sync::Arc;

use code_moniker_core::core::moniker::Moniker;
use rustc_hash::FxHashMap;

use super::index::{DefLocation, RefLocation, SessionIndex};
use super::linkage::LinkageIndex;
use crate::workspace::git::ChangeIndex;

#[derive(Clone)]
pub(crate) struct WorkspaceSnapshot {
	pub(crate) index: Arc<SessionIndex>,
	pub(crate) linkage: Arc<LinkageIndex>,
	pub(crate) search: Arc<SearchIndex>,
	pub(crate) git: GitOverlay,
	pub(crate) coverage: CoverageOverlay,
	pub(crate) plan: PlanOverlay,
}

#[derive(Clone, Default)]
pub(crate) struct SearchIndex {
	pub(crate) docs: Vec<SearchDoc>,
}

#[derive(Clone)]
pub(crate) struct SearchDoc {
	pub(crate) loc: DefLocation,
	pub(crate) name: String,
	pub(crate) kind: String,
	pub(crate) path: String,
	pub(crate) moniker: String,
	pub(crate) signature: String,
}

#[derive(Clone, Default)]
pub(crate) struct GitOverlay {
	pub(crate) change_index: ChangeIndex,
	pub(crate) change_usage_refs: FxHashMap<Moniker, Vec<RefLocation>>,
}

#[derive(Clone, Default)]
#[allow(dead_code)]
pub(crate) struct CoverageOverlay {
	pub(crate) generation: u64,
}

#[derive(Clone, Default)]
#[allow(dead_code)]
pub(crate) struct PlanOverlay {
	pub(crate) generation: u64,
	pub(crate) planned_changes: Vec<PlannedChange>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) struct PlannedChange {
	pub(crate) label: String,
	pub(crate) target: Option<Moniker>,
}
