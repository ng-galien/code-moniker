use std::sync::Arc;

use crate::session::SessionOptions;
use crate::workspace_index::SharedWorkspaceIndex;
use code_moniker_core::core::logger::Logger;

#[derive(Clone)]
pub(crate) struct McpContext {
	opts: SessionOptions,
	scheme: String,
	index: SharedWorkspaceIndex,
	logger: Arc<dyn Logger>,
}

impl McpContext {
	pub(crate) fn new(
		opts: SessionOptions,
		scheme: String,
		index: SharedWorkspaceIndex,
		logger: Arc<dyn Logger>,
	) -> Self {
		Self {
			opts,
			scheme,
			index,
			logger,
		}
	}

	pub(super) fn logger(&self) -> &Arc<dyn Logger> {
		&self.logger
	}

	pub(super) fn opts(&self) -> &SessionOptions {
		&self.opts
	}

	pub(super) fn scheme(&self) -> &str {
		&self.scheme
	}

	pub(super) fn index(&self) -> &SharedWorkspaceIndex {
		&self.index
	}
}
