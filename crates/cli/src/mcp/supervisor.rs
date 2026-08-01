use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs::Metadata;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context as _, bail};
use serde_json::{Value, json};

const STDIO_WORKER_FLAG: &str = "--stdio-worker";
const BINARY_POLL_INTERVAL: Duration = Duration::from_millis(250);
const RELOAD_RETRY_DELAY: Duration = Duration::from_secs(2);
const WORKER_INITIALIZE_TIMEOUT: Duration = Duration::from_secs(45);

pub(crate) fn supervise_stdio() -> anyhow::Result<()> {
	let executable = std::env::current_exe().context("resolve the code-moniker executable")?;
	let worker_args = worker_args();
	let initial_fingerprint = BinaryFingerprint::read(&executable)?;
	let (events_tx, events_rx) = mpsc::channel();
	spawn_client_reader(events_tx.clone());

	let mut generation = 1;
	let (mut worker, stdout) = Worker::spawn(&executable, &worker_args)?;
	spawn_worker_reader(stdout, generation, events_tx.clone());

	let stdout = io::stdout();
	let mut client_stdout = BufWriter::new(stdout.lock());
	let mut state = ProtocolState::default();
	let mut active_fingerprint = initial_fingerprint;
	let mut requested_fingerprint = None;
	let mut retry_at = Instant::now();

	loop {
		match events_rx.recv_timeout(BINARY_POLL_INTERVAL) {
			Ok(Event::ClientFrame(frame)) => {
				state.observe_client_frame(&frame);
				if let Err(error) = worker.write_frame(&frame) {
					eprintln!("code-moniker: MCP stdio worker write failed: {error:#}");
					fail_pending_requests(&mut state, &mut client_stdout)?;
					worker.stop();
					return Err(error);
				}
			}
			Ok(Event::ClientEof) => {
				worker.stop();
				return Ok(());
			}
			Ok(Event::ClientError(error)) => {
				worker.stop();
				bail!("read MCP client input: {error}");
			}
			Ok(Event::WorkerFrame {
				generation: frame_generation,
				frame,
			}) if frame_generation == generation => {
				state.observe_worker_frame(&frame);
				write_frame(&mut client_stdout, &frame)?;
			}
			Ok(Event::WorkerEof {
				generation: frame_generation,
			}) if frame_generation == generation => {
				fail_pending_requests(&mut state, &mut client_stdout)?;
				worker.stop();
				bail!("MCP stdio worker exited unexpectedly");
			}
			Ok(Event::WorkerError {
				generation: frame_generation,
				error,
			}) if frame_generation == generation => {
				fail_pending_requests(&mut state, &mut client_stdout)?;
				worker.stop();
				bail!("read MCP stdio worker output: {error}");
			}
			Ok(_) | Err(RecvTimeoutError::Timeout) => {}
			Err(RecvTimeoutError::Disconnected) => {
				worker.stop();
				bail!("MCP stdio supervisor event channel disconnected");
			}
		}

		if let Ok(fingerprint) = BinaryFingerprint::read(&executable) {
			if fingerprint == active_fingerprint {
				requested_fingerprint = None;
			} else if requested_fingerprint != Some(fingerprint) {
				requested_fingerprint = Some(fingerprint);
				retry_at = Instant::now();
				eprintln!(
					"code-moniker: installed binary changed; preparing MCP stdio worker reload"
				);
			}
		}

		let Some(target_fingerprint) = requested_fingerprint else {
			continue;
		};
		if !state.can_reload() || Instant::now() < retry_at {
			continue;
		}
		let (Some(initialize_frame), Some(initialized_frame)) =
			(state.initialize_frame(), state.initialized_frame())
		else {
			continue;
		};

		let candidate_generation = generation + 1;
		match prepare_worker(
			&executable,
			&worker_args,
			initialize_frame,
			initialized_frame,
		) {
			Ok((new_worker, new_stdout)) => {
				worker.stop();
				worker = new_worker;
				generation = candidate_generation;
				spawn_worker_reader(new_stdout, generation, events_tx.clone());
				active_fingerprint = target_fingerprint;
				requested_fingerprint = None;
				write_frame(
					&mut client_stdout,
					br#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#,
				)?;
				eprintln!("code-moniker: MCP stdio worker reloaded");
			}
			Err(error) => {
				eprintln!(
					"code-moniker: MCP stdio worker reload failed; keeping current worker: {error:#}"
				);
				retry_at = Instant::now() + RELOAD_RETRY_DELAY;
			}
		}
	}
}

fn worker_args() -> Vec<OsString> {
	let mut args = std::env::args_os().skip(1).collect::<Vec<_>>();
	args.push(OsString::from(STDIO_WORKER_FLAG));
	args
}

struct Worker {
	child: Child,
	stdin: BufWriter<ChildStdin>,
}

impl Worker {
	fn spawn(executable: &Path, args: &[OsString]) -> anyhow::Result<(Self, ChildStdout)> {
		let mut child = Command::new(executable)
			.args(args)
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::inherit())
			.spawn()
			.with_context(|| format!("start MCP stdio worker from {}", executable.display()))?;
		let stdin = child
			.stdin
			.take()
			.context("MCP stdio worker stdin is unavailable")?;
		let stdout = child
			.stdout
			.take()
			.context("MCP stdio worker stdout is unavailable")?;
		Ok((
			Self {
				child,
				stdin: BufWriter::new(stdin),
			},
			stdout,
		))
	}

	fn write_frame(&mut self, frame: &[u8]) -> anyhow::Result<()> {
		write_frame(&mut self.stdin, frame).context("write to MCP stdio worker")
	}

	fn stop(&mut self) {
		let _ = self.child.kill();
		let _ = self.child.wait();
	}
}

fn prepare_worker(
	executable: &Path,
	args: &[OsString],
	initialize_frame: &[u8],
	initialized_frame: &[u8],
) -> anyhow::Result<(Worker, BufReader<ChildStdout>)> {
	let (mut worker, stdout) = Worker::spawn(executable, args)?;
	if let Err(error) = worker.write_frame(initialize_frame) {
		worker.stop();
		return Err(error);
	}
	let Some(expected_id) = request_id(initialize_frame) else {
		worker.stop();
		bail!("cached initialize request has no id");
	};
	let (ready_tx, ready_rx) = mpsc::sync_channel(1);
	thread::spawn(move || {
		let mut reader = BufReader::new(stdout);
		let result = wait_for_initialize_response(&mut reader, &expected_id).map(|()| reader);
		let _ = ready_tx.send(result);
	});

	let reader = match ready_rx.recv_timeout(WORKER_INITIALIZE_TIMEOUT) {
		Ok(Ok(reader)) => reader,
		Ok(Err(error)) => {
			worker.stop();
			return Err(error);
		}
		Err(error) => {
			worker.stop();
			bail!("new MCP stdio worker did not initialize in time: {error}");
		}
	};
	if let Err(error) = worker.write_frame(initialized_frame) {
		worker.stop();
		return Err(error);
	}
	Ok((worker, reader))
}

fn wait_for_initialize_response<R: BufRead>(
	reader: &mut R,
	expected_id: &str,
) -> anyhow::Result<()> {
	loop {
		let Some(frame) = read_frame(reader)? else {
			bail!("new MCP stdio worker exited before initialize completed");
		};
		let Ok(message) = serde_json::from_slice::<Value>(&frame) else {
			continue;
		};
		if response_id(&message).as_deref() != Some(expected_id) {
			continue;
		}
		if let Some(error) = message.get("error") {
			bail!("new MCP stdio worker rejected initialize: {error}");
		}
		return Ok(());
	}
}

fn spawn_client_reader(events: Sender<Event>) {
	thread::spawn(move || {
		let stdin = io::stdin();
		let mut reader = BufReader::new(stdin);
		loop {
			match read_frame(&mut reader) {
				Ok(Some(frame)) => {
					if events.send(Event::ClientFrame(frame)).is_err() {
						return;
					}
				}
				Ok(None) => {
					let _ = events.send(Event::ClientEof);
					return;
				}
				Err(error) => {
					let _ = events.send(Event::ClientError(error.to_string()));
					return;
				}
			}
		}
	});
}

fn spawn_worker_reader<R: Read + Send + 'static>(
	reader: R,
	generation: u64,
	events: Sender<Event>,
) {
	thread::spawn(move || {
		let mut reader = BufReader::new(reader);
		loop {
			match read_frame(&mut reader) {
				Ok(Some(frame)) => {
					if events
						.send(Event::WorkerFrame { generation, frame })
						.is_err()
					{
						return;
					}
				}
				Ok(None) => {
					let _ = events.send(Event::WorkerEof { generation });
					return;
				}
				Err(error) => {
					let _ = events.send(Event::WorkerError {
						generation,
						error: error.to_string(),
					});
					return;
				}
			}
		}
	});
}

fn read_frame<R: BufRead>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
	let mut frame = Vec::new();
	let read = reader.read_until(b'\n', &mut frame)?;
	if read == 0 { Ok(None) } else { Ok(Some(frame)) }
}

fn write_frame<W: Write>(writer: &mut W, frame: &[u8]) -> anyhow::Result<()> {
	writer.write_all(frame)?;
	if !frame.ends_with(b"\n") {
		writer.write_all(b"\n")?;
	}
	writer.flush()?;
	Ok(())
}

enum Event {
	ClientFrame(Vec<u8>),
	ClientEof,
	ClientError(String),
	WorkerFrame { generation: u64, frame: Vec<u8> },
	WorkerEof { generation: u64 },
	WorkerError { generation: u64, error: String },
}

#[derive(Default)]
struct ProtocolState {
	initialize: Option<Vec<u8>>,
	initialized: Option<Vec<u8>>,
	pending_client_requests: HashMap<String, Value>,
	pending_worker_requests: HashSet<String>,
}

impl ProtocolState {
	fn observe_client_frame(&mut self, frame: &[u8]) {
		let Ok(message) = serde_json::from_slice::<Value>(frame) else {
			return;
		};
		if message.get("method").and_then(Value::as_str) == Some("initialize") {
			self.initialize = Some(frame.to_vec());
		}
		if message.get("method").and_then(Value::as_str) == Some("notifications/initialized") {
			self.initialized = Some(frame.to_vec());
		}
		if let Some(id) = request_id_value(&message) {
			self.pending_client_requests.insert(id_key(id), id.clone());
		} else if let Some(id) = response_id(&message) {
			self.pending_worker_requests.remove(&id);
		}
	}

	fn observe_worker_frame(&mut self, frame: &[u8]) {
		let Ok(message) = serde_json::from_slice::<Value>(frame) else {
			return;
		};
		if let Some(id) = request_id_value(&message) {
			self.pending_worker_requests.insert(id_key(id));
		} else if let Some(id) = response_id(&message) {
			self.pending_client_requests.remove(&id);
		}
	}

	fn can_reload(&self) -> bool {
		self.initialize.is_some()
			&& self.initialized.is_some()
			&& self.pending_client_requests.is_empty()
			&& self.pending_worker_requests.is_empty()
	}

	fn initialize_frame(&self) -> Option<&[u8]> {
		self.initialize.as_deref()
	}

	fn initialized_frame(&self) -> Option<&[u8]> {
		self.initialized.as_deref()
	}
}

fn fail_pending_requests<W: Write>(
	state: &mut ProtocolState,
	client_stdout: &mut W,
) -> anyhow::Result<()> {
	for (_, id) in state.pending_client_requests.drain() {
		let response = serde_json::to_vec(&json!({
			"jsonrpc": "2.0",
			"id": id,
			"error": {
				"code": -32603,
				"message": "Code Moniker MCP worker exited during the request"
			}
		}))?;
		write_frame(client_stdout, &response)?;
	}
	state.pending_worker_requests.clear();
	Ok(())
}

fn request_id(frame: &[u8]) -> Option<String> {
	let message = serde_json::from_slice::<Value>(frame).ok()?;
	request_id_value(&message).map(id_key)
}

fn request_id_value(message: &Value) -> Option<&Value> {
	message.get("method")?;
	message.get("id").filter(|id| !id.is_null())
}

fn response_id(message: &Value) -> Option<String> {
	if message.get("method").is_some()
		|| (message.get("result").is_none() && message.get("error").is_none())
	{
		return None;
	}
	message.get("id").filter(|id| !id.is_null()).map(id_key)
}

fn id_key(id: &Value) -> String {
	id.to_string()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BinaryFingerprint {
	len: u64,
	modified: Option<SystemTime>,
	#[cfg(unix)]
	device: u64,
	#[cfg(unix)]
	inode: u64,
}

impl BinaryFingerprint {
	fn read(path: &Path) -> anyhow::Result<Self> {
		let metadata = std::fs::metadata(path)
			.with_context(|| format!("inspect executable {}", path.display()))?;
		Ok(Self::from_metadata(&metadata))
	}

	fn from_metadata(metadata: &Metadata) -> Self {
		#[cfg(unix)]
		use std::os::unix::fs::MetadataExt as _;

		Self {
			len: metadata.len(),
			modified: metadata.modified().ok(),
			#[cfg(unix)]
			device: metadata.dev(),
			#[cfg(unix)]
			inode: metadata.ino(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn protocol_state_waits_for_handshake_and_idle_transport() {
		let mut state = ProtocolState::default();
		state.observe_client_frame(br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#);
		state.observe_worker_frame(br#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
		state.observe_client_frame(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
		assert!(state.can_reload());

		state.observe_client_frame(br#"{"jsonrpc":"2.0","id":"q","method":"tools/list"}"#);
		assert!(!state.can_reload());
		state.observe_worker_frame(br#"{"jsonrpc":"2.0","id":"q","result":{"tools":[]}}"#);
		assert!(state.can_reload());
	}

	#[test]
	fn protocol_state_tracks_worker_initiated_requests() {
		let mut state = ProtocolState::default();
		state.initialize = Some(Vec::new());
		state.initialized = Some(Vec::new());
		state
			.observe_worker_frame(br#"{"jsonrpc":"2.0","id":7,"method":"sampling/createMessage"}"#);
		assert!(!state.can_reload());
		state.observe_client_frame(br#"{"jsonrpc":"2.0","id":7,"result":{}}"#);
		assert!(state.can_reload());
	}

	#[test]
	fn initialize_response_must_match_the_cached_request() {
		let input = b"{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}\n{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n";
		let mut reader = BufReader::new(input.as_slice());
		wait_for_initialize_response(&mut reader, "1").unwrap();
	}
}
