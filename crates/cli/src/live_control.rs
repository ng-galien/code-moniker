use std::sync::mpsc;
use std::time::Duration;

use code_moniker_workspace::live::WorkspaceLiveEvent;

pub(crate) enum LiveControlMessage {
	Event(WorkspaceLiveEvent),
	Refresh(mpsc::Sender<LiveRefreshOutcome>),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct LiveRefreshOutcome {
	pub(crate) generation: u64,
	pub(crate) files: usize,
	pub(crate) symbols: usize,
	pub(crate) references: usize,
	pub(crate) error: Option<String>,
}

#[derive(Clone)]
pub(crate) struct LiveControlHandle {
	tx: mpsc::Sender<LiveControlMessage>,
}

impl LiveControlHandle {
	pub(crate) fn new(tx: mpsc::Sender<LiveControlMessage>) -> Self {
		Self { tx }
	}

	pub(crate) fn request_refresh(&self, timeout: Duration) -> anyhow::Result<LiveRefreshOutcome> {
		let (reply_tx, reply_rx) = mpsc::channel();
		self.tx
			.send(LiveControlMessage::Refresh(reply_tx))
			.map_err(|_| anyhow::anyhow!("workspace live loop is not running"))?;
		reply_rx
			.recv_timeout(timeout)
			.map_err(|_| anyhow::anyhow!("workspace refresh timed out after {timeout:?}"))
	}
}
