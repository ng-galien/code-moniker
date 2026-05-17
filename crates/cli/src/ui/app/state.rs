use std::collections::{BTreeMap, BTreeSet};

use crate::inspect::CheckSummary;
use crate::ui::contracts::Route;
use crate::ui::events::UiMode;
use crate::ui::features::explorer::{ExplorerFeature, ROUTE_OVERVIEW};
use crate::ui::filter::NavFilter;
use crate::ui::live::StoreEvent;
use crate::ui::runtime::{TaskId, TaskOutcome, TaskResult, WorkKind};
use crate::ui::store::ids::{CoverageRunId, FileId, RefId, SourceRootId, SymbolId};
use crate::ui::store::navigation::NavigationState;
use crate::ui::store::{IndexStore, SearchHit, UsageFocus};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct WorkSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) pending: BTreeSet<WorkKind>,
	pub(in crate::ui) running: BTreeMap<TaskId, RunningTask>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct RunningTask {
	pub(in crate::ui) kind: WorkKind,
	pub(in crate::ui) generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(in crate::ui) enum LoadState<T> {
	Idle,
	Loading(TaskId),
	Ready(T),
	Failed(String),
}

impl<T> Default for LoadState<T> {
	fn default() -> Self {
		Self::Idle
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct ProjectSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) roots: LoadState<Vec<SourceRootId>>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct FileSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) files: LoadState<Vec<FileId>>,
	pub(in crate::ui) dirty: BTreeSet<FileId>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct GraphSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) files: BTreeMap<FileId, LoadState<GraphFileState>>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct IndexSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) status: LoadState<IndexSummary>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct SearchSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) query: Option<String>,
	pub(in crate::ui) results: LoadState<Vec<SymbolId>>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct GitSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) roots: BTreeMap<SourceRootId, LoadState<GitRootState>>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct ImpactSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) by_symbol: BTreeMap<SymbolId, LoadState<ImpactSummary>>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct PanelSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) panels: BTreeMap<PanelKey, LoadState<PanelDataState>>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct CoverageSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) runs: LoadState<Vec<CoverageRunId>>,
	pub(in crate::ui) by_symbol: BTreeMap<SymbolId, LoadState<CoverageSummary>>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum View {
	Overview,
	Tree,
	Refs,
	Check,
	Change,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum VisualizationMode {
	Explorer,
	Search,
	Usages,
	Change,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum ChangePanelMode {
	Diff,
	Usages,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum PanelPolicy {
	Contextual,
	Manual,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum ActiveFilter {
	None,
	Text { raw: String, query: NavFilter },
	Invalid { raw: String, error: String },
	Search { raw: String, hits: Vec<SearchHit> },
	Usages(UsageFocus),
	Change,
}

impl Default for ActiveFilter {
	fn default() -> Self {
		Self::None
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct ShellSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) status: String,
	pub(in crate::ui) route: Route,
	pub(in crate::ui) view: View,
	pub(in crate::ui) view_mode: VisualizationMode,
	pub(in crate::ui) panel_policy: PanelPolicy,
	pub(in crate::ui) change_panel: ChangePanelMode,
	pub(in crate::ui) mode: UiMode,
	pub(in crate::ui) active_filter: ActiveFilter,
	pub(in crate::ui) filter_draft: String,
	pub(in crate::ui) search_draft: String,
	pub(in crate::ui) last_panel_width: usize,
}

impl Default for ShellSlice {
	fn default() -> Self {
		Self {
			generation: 0,
			status: String::new(),
			route: ExplorerFeature::route(ROUTE_OVERVIEW),
			view: View::Overview,
			view_mode: VisualizationMode::Explorer,
			panel_policy: PanelPolicy::Contextual,
			change_panel: ChangePanelMode::Diff,
			mode: UiMode::Normal,
			active_filter: ActiveFilter::None,
			filter_draft: String::new(),
			search_draft: String::new(),
			last_panel_width: 100,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum CheckState {
	Pending,
	Ready(CheckSummary),
	Error(String),
}

impl Default for CheckState {
	fn default() -> Self {
		Self::Pending
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct CheckSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) state: CheckState,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct NavigationSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) state: Option<NavigationState>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct AppState {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) shell: ShellSlice,
	pub(in crate::ui) project: ProjectSlice,
	pub(in crate::ui) files: FileSlice,
	pub(in crate::ui) graph: GraphSlice,
	pub(in crate::ui) index: IndexSlice,
	pub(in crate::ui) search: SearchSlice,
	pub(in crate::ui) git: GitSlice,
	pub(in crate::ui) impact: ImpactSlice,
	pub(in crate::ui) panels: PanelSlice,
	pub(in crate::ui) coverage: CoverageSlice,
	pub(in crate::ui) check: CheckSlice,
	pub(in crate::ui) navigation: NavigationSlice,
	pub(in crate::ui) work: WorkSlice,
	pub(in crate::ui) last_task: Option<TaskSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct IndexSummary {
	pub(in crate::ui) files: usize,
	pub(in crate::ui) defs: usize,
	pub(in crate::ui) refs: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct GraphFileState {
	pub(in crate::ui) symbols: usize,
	pub(in crate::ui) refs: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct GitRootState {
	pub(in crate::ui) changed_files: usize,
	pub(in crate::ui) changed_symbols: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct ImpactSummary {
	pub(in crate::ui) refs: Vec<RefId>,
	pub(in crate::ui) consumers: Vec<FileId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(in crate::ui) struct PanelKey {
	pub(in crate::ui) component: &'static str,
	pub(in crate::ui) subject: Option<SymbolId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct PanelDataState {
	pub(in crate::ui) refs: Vec<RefId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct CoverageSummary {
	pub(in crate::ui) covered_refs: usize,
	pub(in crate::ui) total_refs: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct TaskSummary {
	pub(in crate::ui) id: TaskId,
	pub(in crate::ui) label: String,
	pub(in crate::ui) status: TaskStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum TaskStatus {
	Completed,
	Failed,
}

impl AppState {
	pub(in crate::ui) fn new() -> Self {
		Self::default()
	}

	pub(in crate::ui) fn set_index_ready(&mut self, files: usize, defs: usize, refs: usize) {
		self.index.status = LoadState::Ready(IndexSummary { files, defs, refs });
	}

	pub(in crate::ui) fn status(&self) -> &str {
		&self.shell.status
	}

	pub(in crate::ui) fn set_status(&mut self, status: impl Into<String>) {
		self.bump();
		self.shell.generation += 1;
		self.shell.status = status.into();
	}

	pub(in crate::ui) fn append_status(&mut self, suffix: impl AsRef<str>) {
		let suffix = suffix.as_ref();
		self.bump();
		self.shell.generation += 1;
		if self.shell.status.is_empty() {
			self.shell.status = suffix.to_string();
		} else {
			self.shell.status = format!("{}; {suffix}", self.shell.status);
		}
	}

	pub(in crate::ui) fn check_state(&self) -> &CheckState {
		&self.check.state
	}

	pub(in crate::ui) fn set_check_state(&mut self, state: CheckState) {
		self.bump();
		self.check.generation += 1;
		self.check.state = state;
	}

	pub(in crate::ui) fn set_navigation(&mut self, navigation: NavigationState) {
		self.bump();
		self.navigation.generation += 1;
		self.navigation.state = Some(navigation);
	}

	pub(in crate::ui) fn generation_for_work(&self, work: WorkKind) -> u64 {
		match work {
			WorkKind::ProjectLoad => self.project.generation,
			WorkKind::FileCatalog => self.files.generation,
			WorkKind::GraphIndex | WorkKind::SearchIndex => self.index.generation,
			WorkKind::GitChangeIndex => self.git.generation,
			WorkKind::ImpactIndex => self.impact.generation,
			WorkKind::PanelData => self.panels.generation,
			WorkKind::CheckPanel => self.check.generation,
			WorkKind::CoverageIndex => self.coverage.generation,
		}
	}

	pub(in crate::ui) fn start_task(&mut self, id: TaskId, kind: WorkKind, generation: u64) {
		self.bump();
		self.work.pending.remove(&kind);
		self.work
			.running
			.insert(id, RunningTask { kind, generation });
		self.mark_loading(kind, id);
	}

	pub(in crate::ui) fn invalidate_for_store_event(&mut self, event: StoreEvent) {
		self.bump();
		match event {
			StoreEvent::FullIndex => self.invalidate_full_index(),
			StoreEvent::ChangeIndex => self.invalidate_git_change_index(),
		}
	}

	pub(in crate::ui) fn accepts_task_result(&self, result: &TaskResult) -> bool {
		self.work.running.get(&result.id).is_some_and(|running| {
			running.kind == result.work
				&& running.generation == result.generation
				&& self.generation_for_work(result.work) == result.generation
		})
	}

	pub(in crate::ui) fn complete_task(&mut self, result: &TaskResult) -> bool {
		let accepted = self.accepts_task_result(result);
		self.bump();
		self.work.running.remove(&result.id);
		if !accepted {
			return false;
		}
		match &result.outcome {
			TaskOutcome::StoreReloaded(store) => {
				let stats = store.stats();
				self.set_index_ready(stats.files, stats.defs, stats.refs);
				self.files.files = LoadState::Ready(
					(0..stats.files)
						.map(|idx| FileId::new(idx.to_string()))
						.collect(),
				);
				self.work.pending.remove(&WorkKind::ProjectLoad);
				self.work.pending.remove(&WorkKind::FileCatalog);
				self.work.pending.remove(&WorkKind::GraphIndex);
				self.work.pending.remove(&WorkKind::SearchIndex);
				self.work.pending.remove(&WorkKind::GitChangeIndex);
				self.work.pending.remove(&WorkKind::ImpactIndex);
				self.work.pending.remove(&WorkKind::PanelData);
			}
			TaskOutcome::ChangeIndexRefreshed(store) => {
				let stats = store.stats();
				self.set_index_ready(stats.files, stats.defs, stats.refs);
				self.work.pending.remove(&WorkKind::GitChangeIndex);
				self.work.pending.remove(&WorkKind::ImpactIndex);
				self.work.pending.remove(&WorkKind::PanelData);
			}
			TaskOutcome::FileCatalogLoaded(store) => {
				let stats = store.stats();
				self.files.files = LoadState::Ready(
					(0..stats.files)
						.map(|idx| FileId::new(idx.to_string()))
						.collect(),
				);
				self.work.pending.remove(&WorkKind::ProjectLoad);
				self.work.pending.remove(&WorkKind::FileCatalog);
			}
			TaskOutcome::Completed(_) => {
				self.work.pending.remove(&result.work);
			}
			TaskOutcome::CheckCompleted(summary) => {
				self.check.state = CheckState::Ready((**summary).clone());
				self.work.pending.remove(&WorkKind::CheckPanel);
			}
			TaskOutcome::Failed(error) => {
				self.mark_failed(result.work, error.clone());
			}
		}
		self.last_task = Some(TaskSummary {
			id: result.id,
			label: result.label.clone(),
			status: match &result.outcome {
				TaskOutcome::Completed(_) => TaskStatus::Completed,
				TaskOutcome::FileCatalogLoaded(_) => TaskStatus::Completed,
				TaskOutcome::StoreReloaded(_) => TaskStatus::Completed,
				TaskOutcome::ChangeIndexRefreshed(_) => TaskStatus::Completed,
				TaskOutcome::CheckCompleted(_) => TaskStatus::Completed,
				TaskOutcome::Failed(_) => TaskStatus::Failed,
			},
		});
		true
	}

	fn invalidate_full_index(&mut self) {
		self.project.generation += 1;
		self.files.generation += 1;
		self.graph.generation += 1;
		self.index.generation += 1;
		self.search.generation += 1;
		self.git.generation += 1;
		self.impact.generation += 1;
		self.panels.generation += 1;
		self.coverage.generation += 1;
		self.check.generation += 1;
		self.work.generation += 1;

		self.project.roots = LoadState::Idle;
		self.files.files = LoadState::Idle;
		self.files.dirty.clear();
		self.graph.files.clear();
		self.index.status = LoadState::Idle;
		self.search.results = LoadState::Idle;
		self.git.roots.clear();
		self.impact.by_symbol.clear();
		self.panels.panels.clear();
		self.coverage.by_symbol.clear();
		self.check.state = CheckState::Pending;
		self.work.pending.extend([
			WorkKind::ProjectLoad,
			WorkKind::FileCatalog,
			WorkKind::GraphIndex,
			WorkKind::SearchIndex,
			WorkKind::GitChangeIndex,
			WorkKind::ImpactIndex,
			WorkKind::PanelData,
			WorkKind::CheckPanel,
			WorkKind::CoverageIndex,
		]);
	}

	fn invalidate_git_change_index(&mut self) {
		self.git.generation += 1;
		self.impact.generation += 1;
		self.panels.generation += 1;
		self.work.generation += 1;
		self.git.roots.clear();
		self.impact.by_symbol.clear();
		self.panels.panels.clear();
		self.work.pending.extend([
			WorkKind::GitChangeIndex,
			WorkKind::ImpactIndex,
			WorkKind::PanelData,
		]);
	}

	fn bump(&mut self) {
		self.generation += 1;
	}

	fn mark_loading(&mut self, kind: WorkKind, id: TaskId) {
		match kind {
			WorkKind::ProjectLoad => self.project.roots = LoadState::Loading(id),
			WorkKind::FileCatalog => self.files.files = LoadState::Loading(id),
			WorkKind::GraphIndex => self.index.status = LoadState::Loading(id),
			WorkKind::SearchIndex => self.search.results = LoadState::Loading(id),
			WorkKind::GitChangeIndex | WorkKind::ImpactIndex | WorkKind::PanelData => {}
			WorkKind::CheckPanel => self.check.state = CheckState::Pending,
			WorkKind::CoverageIndex => self.coverage.runs = LoadState::Loading(id),
		}
	}

	fn mark_failed(&mut self, kind: WorkKind, error: String) {
		match kind {
			WorkKind::ProjectLoad => self.project.roots = LoadState::Failed(error),
			WorkKind::FileCatalog => self.files.files = LoadState::Failed(error),
			WorkKind::GraphIndex => self.index.status = LoadState::Failed(error),
			WorkKind::SearchIndex => self.search.results = LoadState::Failed(error),
			WorkKind::GitChangeIndex | WorkKind::ImpactIndex | WorkKind::PanelData => {}
			WorkKind::CheckPanel => self.check.state = CheckState::Error(error),
			WorkKind::CoverageIndex => self.coverage.runs = LoadState::Failed(error),
		}
	}
}
