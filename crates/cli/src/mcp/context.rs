use crate::session::SessionOptions;
use crate::workspace_index::SharedWorkspaceIndex;

#[derive(Clone)]
pub(super) struct McpContext {
	opts: SessionOptions,
	scheme: String,
	index: SharedWorkspaceIndex,
}

impl McpContext {
	pub(super) fn new(opts: SessionOptions, scheme: String, index: SharedWorkspaceIndex) -> Self {
		Self {
			opts,
			scheme,
			index,
		}
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
