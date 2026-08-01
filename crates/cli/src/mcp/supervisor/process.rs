use std::ffi::OsString;
use std::fs::Metadata;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context as _, bail};
use serde_json::Value;

const WORKER_INITIALIZE_TIMEOUT: Duration = Duration::from_secs(45);

pub(super) struct Worker {
	child: Child,
	stdin: BufWriter<ChildStdin>,
}

impl Worker {
	pub(super) fn spawn(
		executable: &Path,
		args: &[OsString],
	) -> anyhow::Result<(Self, ChildStdout)> {
		let mut child = Command::new(executable)
			.args(args)
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::inherit())
			.spawn()
			.with_context(|| format!("start MCP stdio worker from {}", executable.display()))?;
		let Some(stdin) = child.stdin.take() else {
			stop_child(&mut child);
			bail!("MCP stdio worker stdin is unavailable");
		};
		let Some(stdout) = child.stdout.take() else {
			stop_child(&mut child);
			bail!("MCP stdio worker stdout is unavailable");
		};
		Ok((
			Self {
				child,
				stdin: BufWriter::new(stdin),
			},
			stdout,
		))
	}

	pub(super) fn write_frame(&mut self, frame: &[u8]) -> anyhow::Result<()> {
		write_frame(&mut self.stdin, frame).context("write to MCP stdio worker")
	}
}

impl Drop for Worker {
	fn drop(&mut self) {
		stop_child(&mut self.child);
	}
}

fn stop_child(child: &mut Child) {
	let _ = child.kill();
	let _ = child.wait();
}

pub(super) struct Candidate {
	pub(super) worker: Worker,
	pub(super) generation: u64,
	pub(super) fingerprint: BinaryFingerprint,
	deadline: Instant,
	pub(super) reader: Option<BufReader<ChildStdout>>,
}

impl Candidate {
	pub(super) fn start(
		executable: &Path,
		args: &[OsString],
		generation: u64,
		fingerprint: BinaryFingerprint,
		initialize_frame: &[u8],
		events: Sender<Event>,
	) -> anyhow::Result<Self> {
		let (mut worker, stdout) = Worker::spawn(executable, args)?;
		worker.write_frame(initialize_frame)?;
		let expected_id =
			request_id(initialize_frame).context("cached initialize request has no id")?;
		spawn_candidate_reader(stdout, generation, expected_id, events);
		Ok(Self {
			worker,
			generation,
			fingerprint,
			deadline: Instant::now() + WORKER_INITIALIZE_TIMEOUT,
			reader: None,
		})
	}

	pub(super) fn timed_out(&self) -> bool {
		!self.is_ready() && Instant::now() >= self.deadline
	}

	pub(super) fn is_ready(&self) -> bool {
		self.reader.is_some()
	}
}

pub(super) fn spawn_client_reader(events: Sender<Event>) {
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

pub(super) fn spawn_worker_reader<R: Read + Send + 'static>(
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

fn spawn_candidate_reader(
	stdout: ChildStdout,
	generation: u64,
	expected_id: String,
	events: Sender<Event>,
) {
	thread::spawn(move || {
		let mut reader = BufReader::new(stdout);
		let event = match wait_for_initialize_response(&mut reader, &expected_id) {
			Ok(initialize_response) => Event::CandidateReady {
				generation,
				reader,
				initialize_response,
			},
			Err(error) => Event::CandidateFailed { generation, error },
		};
		let _ = events.send(event);
	});
}

fn wait_for_initialize_response<R: BufRead>(
	reader: &mut R,
	expected_id: &str,
) -> anyhow::Result<Value> {
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
		return Ok(message);
	}
}

fn read_frame<R: BufRead>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
	let mut frame = Vec::new();
	let read = reader.read_until(b'\n', &mut frame)?;
	if read == 0 { Ok(None) } else { Ok(Some(frame)) }
}

pub(super) fn write_frame<W: Write>(writer: &mut W, frame: &[u8]) -> anyhow::Result<()> {
	writer.write_all(frame)?;
	if !frame.ends_with(b"\n") {
		writer.write_all(b"\n")?;
	}
	writer.flush()?;
	Ok(())
}

pub(super) enum Event {
	ClientFrame(Vec<u8>),
	ClientEof,
	ClientError(String),
	WorkerFrame {
		generation: u64,
		frame: Vec<u8>,
	},
	WorkerEof {
		generation: u64,
	},
	WorkerError {
		generation: u64,
		error: String,
	},
	CandidateReady {
		generation: u64,
		reader: BufReader<ChildStdout>,
		initialize_response: Value,
	},
	CandidateFailed {
		generation: u64,
		error: anyhow::Error,
	},
}

fn request_id(frame: &[u8]) -> Option<String> {
	let message = serde_json::from_slice::<Value>(frame).ok()?;
	request_id_value(&message).map(Value::to_string)
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
	message
		.get("id")
		.filter(|id| !id.is_null())
		.map(Value::to_string)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BinaryFingerprint {
	len: u64,
	modified: Option<SystemTime>,
	#[cfg(unix)]
	device: u64,
	#[cfg(unix)]
	inode: u64,
}

impl BinaryFingerprint {
	pub(super) fn read(path: &Path) -> anyhow::Result<Self> {
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
	fn initialize_response_must_match_the_cached_request() {
		let input = b"{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}\n{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n";
		let mut reader = BufReader::new(input.as_slice());
		let response = wait_for_initialize_response(&mut reader, "1").unwrap();
		assert_eq!(response["id"], 1);
	}
}
