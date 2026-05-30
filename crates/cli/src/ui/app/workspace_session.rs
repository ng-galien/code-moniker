use crate::session::SessionOptions;
use crate::ui::workspace_read::LocalWorkspaceFacade;
use crate::workspace_index::SharedWorkspaceIndex;
use code_moniker_workspace::source::LocalResourceCache;

pub(in crate::ui) struct WorkspaceSession {
	store: LocalWorkspaceFacade,
	cache: LocalResourceCache,
	options: SessionOptions,
	index: SharedWorkspaceIndex,
}

impl WorkspaceSession {
	pub(in crate::ui) fn new(
		store: LocalWorkspaceFacade,
		cache: LocalResourceCache,
		options: SessionOptions,
	) -> Self {
		let index = SharedWorkspaceIndex::new(store.snapshot_arc());
		Self {
			store,
			cache,
			options,
			index,
		}
	}

	pub(in crate::ui) fn store(&self) -> &LocalWorkspaceFacade {
		&self.store
	}

	pub(in crate::ui) fn store_mut(&mut self) -> &mut LocalWorkspaceFacade {
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
		self.index.publish(self.store.snapshot_arc());
	}

	pub(in crate::ui) fn replace(
		&mut self,
		store: LocalWorkspaceFacade,
		cache: LocalResourceCache,
		options: SessionOptions,
	) {
		self.store = store;
		self.cache = cache;
		self.options = options;
		self.publish_current_snapshot();
	}
}
