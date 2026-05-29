use crate::session::SessionOptions;

use super::tools::workspace::McpWorkspace;

#[derive(Clone)]
pub(super) struct McpContext {
	opts: SessionOptions,
	scheme: String,
	workspace: McpWorkspace,
}

impl McpContext {
	pub(super) fn new(opts: SessionOptions, scheme: String) -> Self {
		let workspace = McpWorkspace::new(&opts);
		Self {
			opts,
			scheme,
			workspace,
		}
	}

	pub(super) fn opts(&self) -> &SessionOptions {
		&self.opts
	}

	pub(super) fn scheme(&self) -> &str {
		&self.scheme
	}

	pub(super) fn workspace(&self) -> &McpWorkspace {
		&self.workspace
	}
}
