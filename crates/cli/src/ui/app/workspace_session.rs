use crate::session::SessionOptions;
use crate::ui::workspace_read::LocalWorkspaceRegistry;
use crate::workspace_index::SharedWorkspaceIndex;
use code_moniker_workspace::registry::WorkspaceSnapshotPublication;
use code_moniker_workspace::snapshot::{WorkspaceRequest, WorkspaceTransition};
use code_moniker_workspace::source::LocalResourceCache;

pub(in crate::ui) struct WorkspaceSession {
	store: LocalWorkspaceRegistry,
	cache: LocalResourceCache,
	options: SessionOptions,
	index: SharedWorkspaceIndex,
}

impl WorkspaceSession {
	pub(in crate::ui) fn new(
		store: LocalWorkspaceRegistry,
		cache: LocalResourceCache,
		options: SessionOptions,
	) -> Self {
		let index = SharedWorkspaceIndex::new(store.queries().snapshot_arc());
		Self {
			store,
			cache,
			options,
			index,
		}
	}

	pub(in crate::ui) fn store(&self) -> &LocalWorkspaceRegistry {
		&self.store
	}

	pub(in crate::ui) fn store_mut(&mut self) -> &mut LocalWorkspaceRegistry {
		&mut self.store
	}

	pub(in crate::ui) fn cache(&self) -> &LocalResourceCache {
		&self.cache
	}

	pub(in crate::ui) fn options(&self) -> &SessionOptions {
		&self.options
	}

	pub(in crate::ui) fn shared_index(&self) -> SharedWorkspaceIndex {
		self.index.clone()
	}

	pub(in crate::ui) fn publish_current_snapshot(&self) {
		self.index.publish(self.store.queries().snapshot_arc());
	}

	pub(in crate::ui) fn replace(
		&mut self,
		store: LocalWorkspaceRegistry,
		cache: LocalResourceCache,
		options: SessionOptions,
	) {
		self.apply_task_store(store, &options);
		self.cache = cache;
		self.options = options;
		self.publish_current_snapshot();
	}

	fn apply_task_store(&mut self, store: LocalWorkspaceRegistry, options: &SessionOptions) {
		if self.options != *options || !self.publish_task_snapshot(&store) {
			self.store = store;
		}
	}

	fn publish_task_snapshot(&mut self, store: &LocalWorkspaceRegistry) -> bool {
		let Some(snapshot) = store.queries().snapshot_arc() else {
			return false;
		};
		let transition =
			self.store
				.commands()
				.publish_snapshot(WorkspaceSnapshotPublication::workspace(
					WorkspaceRequest::new("ui-task-snapshot"),
					snapshot,
				));
		matches!(transition, WorkspaceTransition::Ready { .. })
	}
}
