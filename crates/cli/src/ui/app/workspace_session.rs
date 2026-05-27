use crate::session::SessionOptions;
use crate::ui::workspace_read::LocalWorkspaceFacade;
use code_moniker_workspace::source::LocalResourceCache;

pub(in crate::ui) struct WorkspaceSession {
	store: LocalWorkspaceFacade,
	cache: LocalResourceCache,
	options: SessionOptions,
}

impl WorkspaceSession {
	pub(in crate::ui) fn new(
		store: LocalWorkspaceFacade,
		cache: LocalResourceCache,
		options: SessionOptions,
	) -> Self {
		Self {
			store,
			cache,
			options,
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
}
