use std::sync::{Arc, RwLock};

use code_moniker_workspace::notes::{NotesDocument, WorkspaceNotes};
use code_moniker_workspace::registry::WorkspaceStaleness;
use code_moniker_workspace::snapshot::WorkspaceSnapshot;

#[derive(Clone)]
pub(crate) struct SharedWorkspaceIndex {
	state: Arc<RwLock<Option<Arc<WorkspaceSnapshot>>>>,
	staleness: Arc<RwLock<WorkspaceStaleness>>,
	notes: WorkspaceNotes,
}

impl SharedWorkspaceIndex {
	pub(crate) fn new(snapshot: Option<Arc<WorkspaceSnapshot>>) -> Self {
		Self {
			state: Arc::new(RwLock::new(snapshot)),
			staleness: Arc::new(RwLock::new(WorkspaceStaleness::default())),
			notes: WorkspaceNotes::default(),
		}
	}

	pub(crate) fn publish(&self, snapshot: Option<Arc<WorkspaceSnapshot>>) {
		if let Ok(mut state) = self.state.write() {
			*state = snapshot;
		}
	}

	pub(crate) fn publish_staleness(&self, staleness: WorkspaceStaleness) {
		if let Ok(mut state) = self.staleness.write() {
			*state = staleness;
		}
	}

	pub(crate) fn staleness(&self) -> WorkspaceStaleness {
		self.staleness
			.read()
			.map(|staleness| staleness.clone())
			.unwrap_or_default()
	}

	pub(crate) fn reload_notes(&self, paths: &[std::path::PathBuf]) -> anyhow::Result<()> {
		self.notes.reload(paths)
	}

	pub(crate) fn notes_snapshot(&self) -> anyhow::Result<NotesDocument> {
		self.notes.snapshot()
	}

	pub(crate) fn mutate_notes<F, T>(
		&self,
		paths: &[std::path::PathBuf],
		mutate: F,
	) -> anyhow::Result<T>
	where
		F: FnOnce(&mut NotesDocument) -> anyhow::Result<T>,
	{
		self.notes.mutate(paths, mutate)
	}

	pub(crate) fn catalog_snapshot(&self) -> anyhow::Result<Arc<WorkspaceSnapshot>> {
		self.ensure_fresh()?;
		self.snapshot()
			.ok_or_else(|| anyhow::anyhow!("workspace catalog snapshot is not ready"))
	}

	pub(crate) fn index_snapshot(&self) -> anyhow::Result<Arc<WorkspaceSnapshot>> {
		self.ensure_fresh()?;
		let snapshot = self
			.snapshot()
			.ok_or_else(|| anyhow::anyhow!("workspace index snapshot is not ready"))?;
		if snapshot.index.sources.is_empty() && snapshot.index.symbols.is_empty() {
			anyhow::bail!("workspace index snapshot is not ready");
		}
		Ok(snapshot)
	}

	fn ensure_fresh(&self) -> anyhow::Result<()> {
		let staleness = self.staleness();
		if staleness.is_stale() {
			anyhow::bail!(
				"workspace index is stale ({}); call the code_moniker_refresh tool to reindex, then retry",
				staleness.summary()
			);
		}
		Ok(())
	}

	fn snapshot(&self) -> Option<Arc<WorkspaceSnapshot>> {
		self.state
			.read()
			.ok()
			.and_then(|snapshot| snapshot.as_ref().map(Arc::clone))
	}
}
