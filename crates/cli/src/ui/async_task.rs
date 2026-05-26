// code-moniker: ignore-file[smell-harmonious-method-size]
// TODO(smell): split TaskSpec construction from task execution and result normalization before enabling this guardrail here.
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::perf;
use crate::session::{CheckSummary, SessionOptions};
use crate::ui::workspace_read::{
	LocalWorkspaceFacade, WorkspaceCheckContext, WorkspaceRead, load_local_workspace,
};
use code_moniker_workspace::source::LocalResourceCache;

type LoadedWorkspace = (LocalWorkspaceFacade, LocalResourceCache, SessionOptions);

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(in crate::ui) enum WorkKind {
	ProjectLoad,
	FileCatalog,
	GraphIndex,
	SearchIndex,
	GitOverlay,
	ImpactIndex,
	PanelData,
	CheckPanel,
	CoverageIndex,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(in crate::ui) struct TaskId(u64);

impl TaskId {
	fn next() -> Self {
		Self(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed))
	}
}

impl fmt::Display for TaskId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "task-{}", self.0)
	}
}

pub(in crate::ui) struct TaskSpec {
	id: TaskId,
	generation: u64,
	label: String,
	kind: TaskKind,
}

impl TaskSpec {
	pub(in crate::ui) fn reload_store(opts: SessionOptions) -> Self {
		Self {
			id: TaskId::next(),
			generation: 0,
			label: "reload index".to_string(),
			kind: TaskKind::ReloadStore { opts },
		}
	}

	pub(in crate::ui) fn load_file_catalog(opts: SessionOptions) -> Self {
		Self {
			id: TaskId::next(),
			generation: 0,
			label: "load file tree".to_string(),
			kind: TaskKind::LoadFileCatalog { opts },
		}
	}

	pub(in crate::ui) fn run_check(
		context: WorkspaceCheckContext,
		rules: PathBuf,
		profile: Option<String>,
		scheme: String,
	) -> Self {
		Self {
			id: TaskId::next(),
			generation: 0,
			label: "run check".to_string(),
			kind: TaskKind::RunCheck {
				context,
				rules,
				profile,
				scheme,
			},
		}
	}

	pub(in crate::ui) fn id(&self) -> TaskId {
		self.id
	}

	pub(in crate::ui) fn label(&self) -> &str {
		&self.label
	}

	pub(in crate::ui) fn work_kind(&self) -> WorkKind {
		self.kind.work_kind()
	}

	pub(in crate::ui) fn with_generation(mut self, generation: u64) -> Self {
		self.generation = generation;
		self
	}

	fn execute(self) -> TaskResult {
		let started = Instant::now();
		let work = self.work_kind();
		let generation = self.generation;
		let label = self.label;
		let outcome = match self.kind {
			TaskKind::LoadFileCatalog { opts } => match load_local_workspace(&opts) {
				Ok((store, cache)) => {
					TaskOutcome::FileCatalogLoaded(Box::new((store, cache, opts)))
				}
				Err(error) => TaskOutcome::Failed(format!("{error:#}")),
			},
			TaskKind::ReloadStore { opts } => match load_local_workspace(&opts) {
				Ok((store, cache)) => TaskOutcome::StoreReloaded(Box::new((store, cache, opts))),
				Err(error) => TaskOutcome::Failed(format!("{error:#}")),
			},
			TaskKind::RunCheck {
				context,
				rules,
				profile,
				scheme,
			} => match context.check_summary(&rules, profile.as_deref(), &scheme) {
				Ok(summary) => TaskOutcome::CheckCompleted(Box::new(summary)),
				Err(error) => TaskOutcome::Failed(format!("{error:#}")),
			},
		};
		let outcome_label = outcome.label();
		let outcome_detail = outcome.detail();
		perf::record(
			"task.execute",
			started.elapsed(),
			format!(
				"id={} label={label} work={work:?} outcome={outcome_label} {outcome_detail}",
				self.id
			),
		);
		TaskResult {
			id: self.id,
			work,
			generation,
			label,
			outcome,
		}
	}
}

enum TaskKind {
	LoadFileCatalog {
		opts: SessionOptions,
	},
	ReloadStore {
		opts: SessionOptions,
	},
	RunCheck {
		context: WorkspaceCheckContext,
		rules: PathBuf,
		profile: Option<String>,
		scheme: String,
	},
}

impl fmt::Debug for TaskSpec {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("TaskSpec")
			.field("id", &self.id)
			.field("generation", &self.generation)
			.field("label", &self.label)
			.field("kind", &self.kind.label())
			.finish()
	}
}

impl TaskKind {
	fn label(&self) -> &'static str {
		match self {
			Self::LoadFileCatalog { .. } => "load_file_catalog",
			Self::ReloadStore { .. } => "reload_store",
			Self::RunCheck { .. } => "run_check",
		}
	}

	fn work_kind(&self) -> WorkKind {
		match self {
			Self::LoadFileCatalog { .. } => WorkKind::FileCatalog,
			Self::ReloadStore { .. } => WorkKind::GraphIndex,
			Self::RunCheck { .. } => WorkKind::CheckPanel,
		}
	}
}

#[derive(Debug)]
pub(in crate::ui) struct TaskResult {
	pub(in crate::ui) id: TaskId,
	pub(in crate::ui) work: WorkKind,
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) label: String,
	pub(in crate::ui) outcome: TaskOutcome,
}

pub(in crate::ui) enum TaskOutcome {
	FileCatalogLoaded(Box<LoadedWorkspace>),
	StoreReloaded(Box<LoadedWorkspace>),
	CheckCompleted(Box<CheckSummary>),
	Failed(String),
}

impl fmt::Debug for TaskOutcome {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::FileCatalogLoaded(_) => f.write_str("FileCatalogLoaded(..)"),
			Self::StoreReloaded(_) => f.write_str("StoreReloaded(..)"),
			Self::CheckCompleted(_) => f.write_str("CheckCompleted(..)"),
			Self::Failed(error) => f.debug_tuple("Failed").field(error).finish(),
		}
	}
}

impl TaskOutcome {
	fn label(&self) -> &'static str {
		match self {
			Self::FileCatalogLoaded(_) => "file_catalog_loaded",
			Self::StoreReloaded(_) => "store_reloaded",
			Self::CheckCompleted(_) => "check_completed",
			Self::Failed(_) => "failed",
		}
	}

	fn detail(&self) -> String {
		match self {
			Self::FileCatalogLoaded(store) | Self::StoreReloaded(store) => {
				let stats = store.0.stats();
				let linkage = store.0.linkage_stats();
				format!(
					"files={} defs={} refs={} scan_ms={} extract_ms={} index_ms={} linkage_score={} eligible_refs={} resolved_refs={} unresolved_refs={}",
					stats.files,
					stats.defs,
					stats.refs,
					stats.scan_ms,
					stats.extract_ms,
					stats.index_ms,
					linkage
						.score_percent()
						.map(|score| format!("{score}%"))
						.unwrap_or_else(|| "n/a".to_string()),
					linkage.eligible_refs(),
					linkage.resolved_refs,
					linkage.unresolved_refs
				)
			}
			Self::CheckCompleted(_) | Self::Failed(_) => String::new(),
		}
	}
}

pub(in crate::ui) struct TaskRunner;

impl TaskRunner {
	pub(in crate::ui) fn spawn(spec: TaskSpec, publish: impl FnOnce(TaskResult) + Send + 'static) {
		rayon::spawn(move || {
			publish(spec.execute());
		});
	}
}
