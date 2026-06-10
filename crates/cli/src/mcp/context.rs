use crate::live_control::LiveControlHandle;
use crate::session::SessionOptions;
use crate::workspace_index::SharedWorkspaceIndex;

#[derive(Clone)]
pub(crate) struct McpContext {
	opts: SessionOptions,
	scheme: String,
	index: SharedWorkspaceIndex,
	live: Option<LiveControlHandle>,
}

impl McpContext {
	pub(crate) fn new(opts: SessionOptions, scheme: String, index: SharedWorkspaceIndex) -> Self {
		Self {
			opts,
			scheme,
			index,
			live: None,
		}
	}

	pub(crate) fn with_live(mut self, live: LiveControlHandle) -> Self {
		self.live = Some(live);
		self
	}

	pub(super) fn live(&self) -> Option<&LiveControlHandle> {
		self.live.as_ref()
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
