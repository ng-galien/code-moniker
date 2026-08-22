use std::path::{Path, PathBuf};

use code_moniker_check::IndexedCheckWorkspace;
use code_moniker_query::{NotesAction, Page, WorkspaceGeneration};
use code_moniker_workspace::glob::FilePathFilter;
use code_moniker_workspace::notes::Note;
use code_moniker_workspace::snapshot::WorkspaceSnapshot;

#[derive(Clone, Copy)]
pub(crate) struct ResponseContext<'a> {
	pub(crate) roots: &'a [PathBuf],
	pub(crate) config_root: &'a Path,
	pub(crate) generation: Option<WorkspaceGeneration>,
}

pub(crate) struct RulesListFilters {
	pub(crate) langs: Vec<String>,
	pub(crate) severities: Vec<String>,
}

pub(crate) struct RulesListEval {
	pub(crate) workspace: Option<String>,
	pub(crate) profile: Option<String>,
	pub(crate) rules: Option<String>,
	pub(crate) filters: RulesListFilters,
	pub(crate) page: Page,
}

pub(crate) struct RulesCheckEval {
	pub(crate) workspace: Option<String>,
	pub(crate) profile: Option<String>,
	pub(crate) rules: Option<String>,
	pub(crate) files: Vec<String>,
	pub(crate) report: bool,
	pub(crate) page: Page,
}

pub(crate) struct IndexedRulesCheck<'a> {
	pub(crate) root: &'a Path,
	pub(crate) config_root: &'a Path,
	pub(crate) workspace: &'a IndexedCheckWorkspace,
	pub(crate) profile: Option<String>,
	pub(crate) rules: Option<&'a str>,
	pub(crate) files: &'a [String],
	pub(crate) report: bool,
}

pub(crate) struct UsageDtoContext<'a> {
	pub(crate) snapshot: &'a WorkspaceSnapshot,
	pub(crate) roots: &'a [PathBuf],
	pub(crate) selected_roots: &'a [&'a PathBuf],
	pub(crate) path_filter: &'a FilePathFilter,
	pub(crate) langs: &'a [String],
}

pub(super) struct NotesResponseInput<'a> {
	pub(super) snapshot: &'a WorkspaceSnapshot,
	pub(super) action: NotesAction,
	pub(super) notes: Vec<Note>,
	pub(super) deleted: Option<Note>,
	pub(super) orphan: Option<bool>,
	pub(super) page: Page,
	pub(super) generation: Option<WorkspaceGeneration>,
}
