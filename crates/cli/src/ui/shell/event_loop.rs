use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use crossterm::event::{self, Event};

use crate::session::StoreWatchRoot;
use crate::ui::async_task::TaskResult;
use crate::ui::clipboard::ClipboardResult;
use crate::ui::live::{LiveStoreWatcher, StoreEvent};

pub(in crate::ui) enum ShellEvent {
	Terminal(Event),
	Store(StoreEvent),
	TaskCompleted(TaskResult),
	HeaderSearchDebounced(u64),
	Clipboard(ClipboardResult),
	Error(String),
}

pub(in crate::ui) struct EventSource {
	tx: Sender<ShellEvent>,
	rx: Receiver<ShellEvent>,
	_terminal_reader: JoinHandle<()>,
	_live_watcher: Option<LiveStoreWatcher>,
	pub(in crate::ui) status: Option<String>,
}

impl EventSource {
	pub(in crate::ui) fn start(watch_roots: Vec<StoreWatchRoot>) -> Self {
		let (tx, rx) = mpsc::channel();
		let terminal_reader = spawn_terminal_reader(tx.clone());
		let (live_watcher, status) = start_live_watcher(watch_roots, &tx);
		Self {
			tx,
			rx,
			_terminal_reader: terminal_reader,
			_live_watcher: live_watcher,
			status,
		}
	}

	pub(in crate::ui) fn sender(&self) -> Sender<ShellEvent> {
		self.tx.clone()
	}

	pub(in crate::ui) fn replace_watch_roots(
		&mut self,
		watch_roots: Vec<StoreWatchRoot>,
	) -> Option<String> {
		let (live_watcher, status) = start_live_watcher(watch_roots, &self.tx);
		self._live_watcher = live_watcher;
		self.status = status.clone();
		status
	}

	pub(in crate::ui) fn recv_batch(&self) -> anyhow::Result<Vec<ShellEvent>> {
		let first = self
			.rx
			.recv()
			.map_err(|_| anyhow::anyhow!("event loop closed"))?;
		let mut batch = vec![first];
		while let Ok(event) = self.rx.try_recv() {
			batch.push(event);
		}
		Ok(batch)
	}
}

fn start_live_watcher(
	watch_roots: Vec<StoreWatchRoot>,
	tx: &Sender<ShellEvent>,
) -> (Option<LiveStoreWatcher>, Option<String>) {
	let live_tx = tx.clone();
	match LiveStoreWatcher::start(watch_roots, move |event| {
		let _ = live_tx.send(ShellEvent::Store(event));
	}) {
		Ok(watcher) => {
			let status = watcher.status();
			(Some(watcher), status)
		}
		Err(error) => (None, Some(format!("live store disabled: {error:#}"))),
	}
}

fn spawn_terminal_reader(tx: Sender<ShellEvent>) -> JoinHandle<()> {
	thread::spawn(move || {
		loop {
			match event::read() {
				Ok(event) => {
					if tx.send(ShellEvent::Terminal(event)).is_err() {
						break;
					}
				}
				Err(error) => {
					let _ = tx.send(ShellEvent::Error(error.to_string()));
					break;
				}
			}
		}
	})
}
