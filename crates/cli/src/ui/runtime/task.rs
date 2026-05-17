use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::inspect::{CheckSummary, SessionOptions};
use crate::ui::store::{ChangeIndexRefreshInput, IndexStore, MemoryIndexStore};

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(in crate::ui) enum WorkKind {
	ProjectLoad,
	FileCatalog,
	GraphIndex,
	SearchIndex,
	GitChangeIndex,
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
	#[allow(dead_code)]
	pub(in crate::ui) fn noop(label: impl Into<String>) -> Self {
		Self {
			id: TaskId::next(),
			generation: 0,
			label: label.into(),
			kind: TaskKind::Noop,
		}
	}

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

	pub(in crate::ui) fn refresh_change_index(input: ChangeIndexRefreshInput) -> Self {
		Self {
			id: TaskId::next(),
			generation: 0,
			label: "refresh change index".to_string(),
			kind: TaskKind::RefreshChangeIndex { input },
		}
	}

	pub(in crate::ui) fn run_check(
		store: MemoryIndexStore,
		rules: PathBuf,
		profile: Option<String>,
		scheme: String,
	) -> Self {
		Self {
			id: TaskId::next(),
			generation: 0,
			label: "run check".to_string(),
			kind: TaskKind::RunCheck {
				store,
				rules,
				profile,
				scheme,
			},
		}
	}

	pub(in crate::ui) fn id(&self) -> TaskId {
		self.id
	}

	#[cfg(test)]
	pub(in crate::ui) fn generation(&self) -> u64 {
		self.generation
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
		let work = self.work_kind();
		let generation = self.generation;
		let outcome = match self.kind {
			TaskKind::Noop => TaskOutcome::Completed("task completed".to_string()),
			TaskKind::LoadFileCatalog { opts } => match MemoryIndexStore::catalog(&opts) {
				Ok(store) => TaskOutcome::FileCatalogLoaded(Box::new(store)),
				Err(error) => TaskOutcome::Failed(format!("{error:#}")),
			},
			TaskKind::ReloadStore { opts } => match MemoryIndexStore::load(&opts) {
				Ok(store) => TaskOutcome::StoreReloaded(Box::new(store)),
				Err(error) => TaskOutcome::Failed(format!("{error:#}")),
			},
			TaskKind::RefreshChangeIndex { input } => TaskOutcome::ChangeIndexRefreshed(Box::new(
				MemoryIndexStore::refresh_change_indexed(input),
			)),
			TaskKind::RunCheck {
				store,
				rules,
				profile,
				scheme,
			} => match store.check_summary(&rules, profile.as_deref(), &scheme) {
				Ok(summary) => TaskOutcome::CheckCompleted(Box::new(summary)),
				Err(error) => TaskOutcome::Failed(format!("{error:#}")),
			},
		};
		TaskResult {
			id: self.id,
			work,
			generation,
			label: self.label,
			outcome,
		}
	}
}

enum TaskKind {
	Noop,
	LoadFileCatalog {
		opts: SessionOptions,
	},
	ReloadStore {
		opts: SessionOptions,
	},
	RefreshChangeIndex {
		input: ChangeIndexRefreshInput,
	},
	RunCheck {
		store: MemoryIndexStore,
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
			Self::Noop => "noop",
			Self::LoadFileCatalog { .. } => "load_file_catalog",
			Self::ReloadStore { .. } => "reload_store",
			Self::RefreshChangeIndex { .. } => "refresh_change_index",
			Self::RunCheck { .. } => "run_check",
		}
	}

	fn work_kind(&self) -> WorkKind {
		match self {
			Self::Noop => WorkKind::PanelData,
			Self::LoadFileCatalog { .. } => WorkKind::FileCatalog,
			Self::ReloadStore { .. } => WorkKind::GraphIndex,
			Self::RefreshChangeIndex { .. } => WorkKind::GitChangeIndex,
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
	Completed(String),
	FileCatalogLoaded(Box<MemoryIndexStore>),
	StoreReloaded(Box<MemoryIndexStore>),
	ChangeIndexRefreshed(Box<MemoryIndexStore>),
	CheckCompleted(Box<CheckSummary>),
	Failed(String),
}

impl fmt::Debug for TaskOutcome {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Completed(message) => f.debug_tuple("Completed").field(message).finish(),
			Self::FileCatalogLoaded(_) => f.write_str("FileCatalogLoaded(..)"),
			Self::StoreReloaded(_) => f.write_str("StoreReloaded(..)"),
			Self::ChangeIndexRefreshed(_) => f.write_str("ChangeIndexRefreshed(..)"),
			Self::CheckCompleted(_) => f.write_str("CheckCompleted(..)"),
			Self::Failed(error) => f.debug_tuple("Failed").field(error).finish(),
		}
	}
}

pub(in crate::ui) struct TaskRuntime;

impl TaskRuntime {
	pub(in crate::ui) fn spawn(spec: TaskSpec, publish: impl FnOnce(TaskResult) + Send + 'static) {
		rayon::spawn(move || {
			publish(spec.execute());
		});
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn noop_task_completes_with_stable_identity() {
		let spec = TaskSpec::noop("smoke");
		let id = spec.id();
		let result = spec.execute();

		assert_eq!(result.id, id);
		assert_eq!(result.label, "smoke");
		assert!(matches!(
			result.outcome,
			TaskOutcome::Completed(ref message) if message == "task completed"
		));
	}
}
