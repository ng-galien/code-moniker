// code-moniker: ignore-file[smell-god-type-local-metrics, smell-harmonious-method-size, smell-large-type]
// Transitional CLI/UI boundary. `IndexStore` remains broad until the UI consumes
// workspace crate read models directly; the bridge is the only implementation.
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::core::shape::Shape;
use code_moniker_core::lang::Lang;

use crate::workspace::SessionStoreBridge;
use crate::workspace::index::{CheckSummary, DefLocation, SessionOptions, SessionStats};
use crate::workspace::model::{
	ChangeDetail, ChangeId, ChangeOverview, ChangeSummary, FileSummary, SearchHit, SourceLine,
	SymbolDetail, SymbolReferences, SymbolSummary, UnresolvedLinkageReport, UsageFocus,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct LinkageStats {
	pub(crate) resolved_refs: usize,
	pub(crate) external_refs: usize,
	pub(crate) manifest_blocked_refs: usize,
	pub(crate) unresolved_refs: usize,
	pub(crate) ambiguous_refs: usize,
}

impl LinkageStats {
	pub(crate) fn eligible_refs(&self) -> usize {
		self.resolved_refs + self.manifest_blocked_refs + self.unresolved_refs
	}

	pub(crate) fn score_percent(&self) -> Option<u32> {
		let eligible = self.eligible_refs();
		(eligible > 0).then(|| ((self.resolved_refs * 100) / eligible) as u32)
	}
}

pub(crate) trait IndexStore {
	fn root(&self) -> &str;
	fn stats(&self) -> &SessionStats;
	fn linkage_stats(&self) -> &LinkageStats;
	fn file_count(&self) -> usize;
	fn file_summary(&self, file_idx: usize) -> FileSummary;
	fn all_navigable_defs(&self) -> Vec<DefLocation>;
	fn root_defs(&self, file_idx: usize) -> Vec<DefLocation>;
	fn child_defs(&self, parent: &DefLocation) -> Vec<DefLocation>;
	fn compare_defs_for_navigation(&self, left: &DefLocation, right: &DefLocation) -> Ordering;
	fn is_navigable_symbol(&self, loc: &DefLocation) -> bool;
	fn symbol_summary(&self, loc: &DefLocation) -> SymbolSummary;
	fn symbol_detail(&self, loc: &DefLocation) -> SymbolDetail;
	fn symbol_references(&self, loc: &DefLocation) -> SymbolReferences;
	fn source_snippet(&self, loc: &DefLocation, context: u32) -> Vec<SourceLine>;
	fn search_symbols_filtered(
		&self,
		query: &str,
		limit: usize,
		langs: &[Lang],
		kinds: &[String],
		shapes: &[Shape],
	) -> Vec<SearchHit>;
	fn change_overview(&self) -> ChangeOverview;
	fn change_rows(&self) -> Vec<ChangeSummary>;
	fn change_summary(&self, change: ChangeId) -> Option<ChangeSummary>;
	fn change_detail(&self, change: ChangeId) -> Option<ChangeDetail>;
	fn changed_defs(&self) -> Vec<DefLocation>;
	fn change_detail_for_symbol(&self, loc: &DefLocation) -> Option<ChangeDetail>;
	fn change_count_for_file(&self, file_idx: usize) -> usize;
	fn usage_focus(&self, loc: DefLocation) -> UsageFocus;
	fn unresolved_linkage_report(
		&self,
		file_limit: usize,
		samples_per_file: usize,
	) -> UnresolvedLinkageReport;
	fn check_summary(
		&self,
		rules: &Path,
		profile: Option<&str>,
		scheme: &str,
	) -> anyhow::Result<CheckSummary>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoreWatchRoot {
	pub(crate) path: PathBuf,
	pub(crate) git_root: Option<PathBuf>,
	pub(crate) ignored_paths: Vec<PathBuf>,
}

#[derive(Clone)]
pub(crate) struct WorkspaceHandle {
	bridge: SessionStoreBridge,
}

impl WorkspaceHandle {
	pub(crate) fn load(opts: &SessionOptions) -> anyhow::Result<Self> {
		SessionStoreBridge::load(opts.clone()).map(Self::new)
	}

	pub(crate) fn catalog(opts: &SessionOptions) -> anyhow::Result<Self> {
		SessionStoreBridge::load(opts.clone()).map(Self::new)
	}

	pub(crate) fn empty(opts: SessionOptions) -> Self {
		Self::new(SessionStoreBridge::empty(opts))
	}

	fn new(bridge: SessionStoreBridge) -> Self {
		Self { bridge }
	}

	pub(crate) fn options(&self) -> SessionOptions {
		self.bridge.options()
	}

	pub(crate) fn watch_roots(&self) -> Vec<StoreWatchRoot> {
		watch_roots_for_options(&self.bridge.options())
	}

	pub(crate) fn refresh_git_overlay(&mut self) {
		let _ = self.reload();
	}

	pub(crate) fn reload(&mut self) -> anyhow::Result<()> {
		let opts = self.options();
		*self = Self::load(&opts)?;
		Ok(())
	}

	pub(crate) fn usage_focus_for_target(&self, target: Moniker, label: String) -> UsageFocus {
		self.bridge.usage_focus_for_target(target, label)
	}
}

impl IndexStore for WorkspaceHandle {
	fn root(&self) -> &str {
		self.bridge.root()
	}

	fn stats(&self) -> &SessionStats {
		self.bridge.stats()
	}

	fn linkage_stats(&self) -> &LinkageStats {
		self.bridge.linkage_stats()
	}

	fn file_count(&self) -> usize {
		self.bridge.file_count()
	}

	fn file_summary(&self, file_idx: usize) -> FileSummary {
		self.bridge.file_summary(file_idx)
	}

	fn all_navigable_defs(&self) -> Vec<DefLocation> {
		self.bridge.all_navigable_defs()
	}

	fn root_defs(&self, file_idx: usize) -> Vec<DefLocation> {
		self.bridge.root_defs(file_idx)
	}

	fn child_defs(&self, parent: &DefLocation) -> Vec<DefLocation> {
		self.bridge.child_defs(parent)
	}

	fn compare_defs_for_navigation(&self, left: &DefLocation, right: &DefLocation) -> Ordering {
		self.bridge.compare_defs_for_navigation(left, right)
	}

	fn is_navigable_symbol(&self, loc: &DefLocation) -> bool {
		self.bridge.is_navigable_symbol(loc)
	}

	fn symbol_summary(&self, loc: &DefLocation) -> SymbolSummary {
		self.bridge.symbol_summary(loc)
	}

	fn symbol_detail(&self, loc: &DefLocation) -> SymbolDetail {
		self.bridge.symbol_detail(loc)
	}

	fn symbol_references(&self, loc: &DefLocation) -> SymbolReferences {
		self.bridge.symbol_references(loc)
	}

	fn source_snippet(&self, loc: &DefLocation, context: u32) -> Vec<SourceLine> {
		self.bridge.source_snippet(loc, context)
	}

	fn search_symbols_filtered(
		&self,
		query: &str,
		limit: usize,
		langs: &[Lang],
		kinds: &[String],
		shapes: &[Shape],
	) -> Vec<SearchHit> {
		self.bridge
			.search_symbols_filtered(query, limit, langs, kinds, shapes)
	}

	fn change_overview(&self) -> ChangeOverview {
		self.bridge.change_overview()
	}

	fn change_rows(&self) -> Vec<ChangeSummary> {
		self.bridge.change_rows()
	}

	fn change_summary(&self, change: ChangeId) -> Option<ChangeSummary> {
		self.bridge.change_summary(change)
	}

	fn change_detail(&self, change: ChangeId) -> Option<ChangeDetail> {
		self.bridge.change_detail(change)
	}

	fn changed_defs(&self) -> Vec<DefLocation> {
		self.bridge.changed_defs()
	}

	fn change_detail_for_symbol(&self, loc: &DefLocation) -> Option<ChangeDetail> {
		self.bridge.change_detail_for_symbol(loc)
	}

	fn change_count_for_file(&self, file_idx: usize) -> usize {
		self.bridge.change_count_for_file(file_idx)
	}

	fn usage_focus(&self, loc: DefLocation) -> UsageFocus {
		self.bridge.usage_focus(loc)
	}

	fn unresolved_linkage_report(
		&self,
		file_limit: usize,
		samples_per_file: usize,
	) -> UnresolvedLinkageReport {
		self.bridge
			.unresolved_linkage_report(file_limit, samples_per_file)
	}

	fn check_summary(
		&self,
		rules: &Path,
		profile: Option<&str>,
		scheme: &str,
	) -> anyhow::Result<CheckSummary> {
		self.bridge.check_summary(rules, profile, scheme)
	}
}

fn watch_roots_for_options(opts: &SessionOptions) -> Vec<StoreWatchRoot> {
	let ignored_paths = opts
		.cache_dir
		.as_ref()
		.map(|path| vec![absolute_path(path)])
		.unwrap_or_default();
	opts.paths
		.iter()
		.map(|path| StoreWatchRoot {
			path: watch_path(path),
			git_root: None,
			ignored_paths: ignored_paths.clone(),
		})
		.collect()
}

fn watch_path(path: &Path) -> PathBuf {
	let path = absolute_path(path);
	if path.is_file() {
		path.parent().map(Path::to_path_buf).unwrap_or(path)
	} else {
		path
	}
}

fn absolute_path(path: &Path) -> PathBuf {
	if path.is_absolute() {
		path.to_path_buf()
	} else {
		std::env::current_dir()
			.map(|cwd| cwd.join(path))
			.unwrap_or_else(|_| path.to_path_buf())
	}
}
