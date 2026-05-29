use crate::session::SessionOptions;

#[derive(Clone)]
pub(super) struct McpContext {
	opts: SessionOptions,
	scheme: String,
}

impl McpContext {
	pub(super) fn new(opts: SessionOptions, scheme: String) -> Self {
		Self { opts, scheme }
	}

	pub(super) fn opts(&self) -> &SessionOptions {
		&self.opts
	}

	pub(super) fn scheme(&self) -> &str {
		&self.scheme
	}
}
