use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::session::{CheckSummary, SessionOptions};
use crate::ui::perf;
use crate::ui::workspace_read::{
	self, LocalWorkspaceRegistry, WorkspaceCheckContext, load_local_file_catalog,
	load_local_symbol_index, load_local_symbol_index_from_catalog, resolve_local_linkage,
};
use code_moniker_workspace::snapshot::WorkspaceSnapshot;
use code_moniker_workspace::source::LocalResourceCache;

type LoadedWorkspace = (LocalWorkspaceRegistry, LocalResourceCache, SessionOptions);

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(in crate::ui) enum WorkKind {
	ProjectLoad,
	FileCatalog,
	GraphIndex,
	LinkageGraph,
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
	pub(in crate::ui) fn load_file_catalog(opts: SessionOptions) -> Self {
		Self {
			id: TaskId::next(),
			generation: 0,
			label: "load file tree".to_string(),
			kind: TaskKind::LoadFileCatalog { opts },
		}
	}

	pub(in crate::ui) fn load_symbol_index(opts: SessionOptions) -> Self {
		Self {
			id: TaskId::next(),
			generation: 0,
			label: "load symbols".to_string(),
			kind: TaskKind::LoadSymbolIndex {
				opts,
				cache: None,
				snapshot: None,
			},
		}
	}

	pub(in crate::ui) fn load_symbol_index_from_catalog(
		opts: SessionOptions,
		cache: LocalResourceCache,
		snapshot: Arc<WorkspaceSnapshot>,
	) -> Self {
		Self {
			id: TaskId::next(),
			generation: 0,
			label: "load symbols".to_string(),
			kind: TaskKind::LoadSymbolIndex {
				opts,
				cache: Some(cache),
				snapshot: Some(snapshot),
			},
		}
	}

	pub(in crate::ui) fn resolve_linkage(
		opts: SessionOptions,
		cache: LocalResourceCache,
		snapshot: Arc<WorkspaceSnapshot>,
	) -> Self {
		Self {
			id: TaskId::next(),
			generation: 0,
			label: "resolve linkage".to_string(),
			kind: TaskKind::ResolveLinkage {
				opts,
				cache,
				snapshot,
			},
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
}

#[allow(clippy::large_enum_variant)]
enum TaskKind {
	LoadFileCatalog {
		opts: SessionOptions,
	},
	LoadSymbolIndex {
		opts: SessionOptions,
		cache: Option<LocalResourceCache>,
		snapshot: Option<Arc<WorkspaceSnapshot>>,
	},
	ResolveLinkage {
		opts: SessionOptions,
		cache: LocalResourceCache,
		snapshot: Arc<WorkspaceSnapshot>,
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
			Self::LoadSymbolIndex { .. } => "load_symbol_index",
			Self::ResolveLinkage { .. } => "resolve_linkage",
			Self::RunCheck { .. } => "run_check",
		}
	}

	fn work_kind(&self) -> WorkKind {
		match self {
			Self::LoadFileCatalog { .. } => WorkKind::FileCatalog,
			Self::LoadSymbolIndex { .. } => WorkKind::GraphIndex,
			Self::ResolveLinkage { .. } => WorkKind::LinkageGraph,
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
	SymbolIndexLoaded {
		workspace: Box<LoadedWorkspace>,
		linkage_seed: Arc<WorkspaceSnapshot>,
	},
	LinkageResolved(Box<LoadedWorkspace>),
	CheckCompleted(Box<CheckSummary>),
	Failed(String),
}

impl fmt::Debug for TaskOutcome {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::FileCatalogLoaded(_) => f.write_str("FileCatalogLoaded(..)"),
			Self::SymbolIndexLoaded { .. } => f.write_str("SymbolIndexLoaded(..)"),
			Self::LinkageResolved(_) => f.write_str("LinkageResolved(..)"),
			Self::CheckCompleted(_) => f.write_str("CheckCompleted(..)"),
			Self::Failed(error) => f.debug_tuple("Failed").field(error).finish(),
		}
	}
}

impl TaskOutcome {
	fn label(&self) -> &'static str {
		match self {
			Self::FileCatalogLoaded(_) => "file_catalog_loaded",
			Self::SymbolIndexLoaded { .. } => "symbol_index_loaded",
			Self::LinkageResolved(_) => "linkage_resolved",
			Self::CheckCompleted(_) => "check_completed",
			Self::Failed(_) => "failed",
		}
	}

	fn detail(&self) -> String {
		match self {
			Self::FileCatalogLoaded(store)
			| Self::LinkageResolved(store)
			| Self::SymbolIndexLoaded {
				workspace: store, ..
			} => {
				let stats = workspace_read::stats(&store.0);
				let linkage = workspace_read::linkage_stats(&store.0);
				format!(
					"files={} defs={} refs={} scan_ms={} extract_ms={} index_ms={} linkage_ms={} changes_ms={} linkage_score={} eligible_refs={} resolved_refs={} unresolved_refs={}",
					stats.files,
					stats.defs,
					stats.refs,
					stats.scan_ms,
					stats.extract_ms,
					stats.index_ms,
					stats.linkage_ms,
					stats.changes_ms,
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
			publish(execute_task(spec));
		});
	}
}

fn execute_task(spec: TaskSpec) -> TaskResult {
	let started = Instant::now();
	let work = spec.work_kind();
	let generation = spec.generation;
	let label = spec.label;
	let outcome = execute_task_kind(spec.kind);
	let outcome_label = outcome.label();
	let outcome_detail = outcome.detail();
	perf::record(
		"task.execute",
		started.elapsed(),
		format!(
			"id={} label={label} work={work:?} outcome={outcome_label} {outcome_detail}",
			spec.id
		),
	);
	TaskResult {
		id: spec.id,
		work,
		generation,
		label,
		outcome,
	}
}

fn execute_task_kind(kind: TaskKind) -> TaskOutcome {
	match kind {
		TaskKind::LoadFileCatalog { opts } => match load_local_file_catalog(&opts) {
			Ok((store, cache)) => TaskOutcome::FileCatalogLoaded(Box::new((store, cache, opts))),
			Err(error) => TaskOutcome::Failed(format!("{error:#}")),
		},
		TaskKind::LoadSymbolIndex {
			opts,
			cache,
			snapshot,
		} => {
			let loaded = match (cache, snapshot) {
				(Some(cache), Some(snapshot)) => {
					load_local_symbol_index_from_catalog(&opts, cache, snapshot)
				}
				_ => load_local_symbol_index(&opts),
			};
			match loaded {
				Ok((store, cache)) => match store.queries().snapshot_arc() {
					Some(snapshot) => TaskOutcome::SymbolIndexLoaded {
						workspace: Box::new((store, cache, opts)),
						linkage_seed: snapshot,
					},
					None => TaskOutcome::Failed("symbol index snapshot is unavailable".to_string()),
				},
				Err(error) => TaskOutcome::Failed(format!("{error:#}")),
			}
		}
		TaskKind::ResolveLinkage {
			opts,
			cache,
			snapshot,
		} => match resolve_local_linkage(&opts, cache, snapshot) {
			Ok((store, cache)) => TaskOutcome::LinkageResolved(Box::new((store, cache, opts))),
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
	}
}
