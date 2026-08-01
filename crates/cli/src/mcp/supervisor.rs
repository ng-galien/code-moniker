mod process;
mod protocol;

use std::ffi::OsString;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};

use self::process::{
	BinaryFingerprint, Candidate, Event, Worker, spawn_client_reader, spawn_worker_reader,
	write_frame,
};
use self::protocol::{ProtocolState, fail_pending_requests, validate_candidate_initialize};

const STDIO_WORKER_FLAG: &str = "--stdio-worker";
const BINARY_POLL_INTERVAL: Duration = Duration::from_millis(250);
const RELOAD_RETRY_DELAY: Duration = Duration::from_secs(2);

pub(crate) fn supervise_stdio() -> anyhow::Result<()> {
	let executable = std::env::current_exe().context("resolve the code-moniker executable")?;
	let worker_args = worker_args();
	let fingerprint = BinaryFingerprint::read(&executable)?;
	let (events_tx, events_rx) = mpsc::channel();
	spawn_client_reader(events_tx.clone());
	let (worker, stdout) = Worker::spawn(&executable, &worker_args)?;
	spawn_worker_reader(stdout, 1, events_tx.clone());

	let stdout = io::stdout();
	Supervisor {
		executable,
		worker_args,
		events_tx,
		events_rx,
		client_stdout: BufWriter::new(stdout.lock()),
		state: ProtocolState::default(),
		worker: Some(worker),
		candidate: None,
		active_generation: 1,
		next_generation: 2,
		active_fingerprint: fingerprint,
		requested_fingerprint: None,
		retry_at: Instant::now(),
	}
	.run()
}

fn worker_args() -> Vec<OsString> {
	let mut args = std::env::args_os().skip(1).collect::<Vec<_>>();
	args.push(OsString::from(STDIO_WORKER_FLAG));
	args
}

struct Supervisor<W> {
	executable: PathBuf,
	worker_args: Vec<OsString>,
	events_tx: Sender<Event>,
	events_rx: Receiver<Event>,
	client_stdout: W,
	state: ProtocolState,
	worker: Option<Worker>,
	candidate: Option<Candidate>,
	active_generation: u64,
	next_generation: u64,
	active_fingerprint: BinaryFingerprint,
	requested_fingerprint: Option<BinaryFingerprint>,
	retry_at: Instant,
}

impl<W: Write> Supervisor<W> {
	fn run(&mut self) -> anyhow::Result<()> {
		loop {
			match self.events_rx.recv_timeout(BINARY_POLL_INTERVAL) {
				Ok(event) => {
					if !handle_event(self, event)? {
						return Ok(());
					}
				}
				Err(RecvTimeoutError::Timeout) => {}
				Err(RecvTimeoutError::Disconnected) => {
					bail!("MCP stdio supervisor event channel disconnected");
				}
			}
			observe_executable(self);
			expire_candidate(self);
			maybe_cut_over(self)?;
			maybe_start_candidate(self)?;
		}
	}
}

fn handle_event<W: Write>(supervisor: &mut Supervisor<W>, event: Event) -> anyhow::Result<bool> {
	match event {
		Event::ClientFrame(frame) => handle_client_frame(supervisor, &frame)?,
		Event::ClientEof => return Ok(false),
		Event::ClientError(error) => bail!("read MCP client input: {error}"),
		Event::WorkerFrame { generation, frame }
			if generation == supervisor.active_generation && supervisor.worker.is_some() =>
		{
			supervisor.state.observe_worker_frame(&frame);
			write_frame(&mut supervisor.client_stdout, &frame)?;
		}
		Event::WorkerEof { generation }
			if generation == supervisor.active_generation && supervisor.worker.is_some() =>
		{
			lose_active_worker(supervisor, "MCP stdio worker exited")?;
		}
		Event::WorkerError { generation, error }
			if generation == supervisor.active_generation && supervisor.worker.is_some() =>
		{
			lose_active_worker(
				supervisor,
				&format!("MCP stdio worker output failed: {error}"),
			)?;
		}
		Event::CandidateReady {
			generation,
			reader,
			initialize_response,
		} => handle_candidate_ready(supervisor, generation, reader, initialize_response),
		Event::CandidateFailed { generation, error }
			if candidate_generation(supervisor) == Some(generation) =>
		{
			reject_candidate(supervisor, error);
		}
		_ => {}
	}
	Ok(true)
}

fn handle_client_frame<W: Write>(
	supervisor: &mut Supervisor<W>,
	frame: &[u8],
) -> anyhow::Result<()> {
	supervisor.state.observe_client_frame(frame);
	match supervisor
		.worker
		.as_mut()
		.map(|worker| worker.write_frame(frame))
	{
		Some(Ok(())) => Ok(()),
		Some(Err(error)) => {
			eprintln!("code-moniker: MCP stdio worker write failed: {error:#}");
			lose_active_worker(supervisor, "MCP stdio worker write failed")
		}
		None => fail_pending_requests(&mut supervisor.state, &mut supervisor.client_stdout),
	}
}

fn lose_active_worker<W: Write>(
	supervisor: &mut Supervisor<W>,
	reason: &str,
) -> anyhow::Result<()> {
	eprintln!("code-moniker: {reason}; recovering in place");
	supervisor.worker = None;
	fail_pending_requests(&mut supervisor.state, &mut supervisor.client_stdout)?;
	supervisor.retry_at = Instant::now();
	Ok(())
}

fn handle_candidate_ready<W: Write>(
	supervisor: &mut Supervisor<W>,
	generation: u64,
	reader: std::io::BufReader<std::process::ChildStdout>,
	initialize_response: serde_json::Value,
) {
	if candidate_generation(supervisor) != Some(generation) {
		return;
	}
	let compatibility = supervisor
		.state
		.initialize_response()
		.context("active MCP initialize response is unavailable")
		.and_then(|active| validate_candidate_initialize(active, &initialize_response));
	match compatibility {
		Ok(()) => {
			if let Some(candidate) = supervisor.candidate.as_mut() {
				candidate.reader = Some(reader);
			}
		}
		Err(error) => reject_candidate(supervisor, error),
	}
}

fn observe_executable<W>(supervisor: &mut Supervisor<W>) {
	let Ok(fingerprint) = BinaryFingerprint::read(&supervisor.executable) else {
		return;
	};
	if supervisor.worker.is_none() {
		supervisor.requested_fingerprint = Some(fingerprint);
		return;
	}
	if fingerprint == supervisor.active_fingerprint {
		supervisor.requested_fingerprint = None;
		return;
	}
	if supervisor.requested_fingerprint == Some(fingerprint) {
		return;
	}
	supervisor.requested_fingerprint = Some(fingerprint);
	supervisor.retry_at = Instant::now();
	eprintln!("code-moniker: installed binary changed; preparing MCP stdio worker reload");
	if supervisor
		.candidate
		.as_ref()
		.is_some_and(|candidate| candidate.fingerprint != fingerprint)
	{
		supervisor.candidate = None;
	}
}

fn expire_candidate<W>(supervisor: &mut Supervisor<W>) {
	if supervisor
		.candidate
		.as_ref()
		.is_some_and(Candidate::timed_out)
	{
		reject_candidate(
			supervisor,
			anyhow::anyhow!("new MCP stdio worker did not initialize in time"),
		);
	}
}

fn maybe_cut_over<W: Write>(supervisor: &mut Supervisor<W>) -> anyhow::Result<()> {
	if !supervisor
		.candidate
		.as_ref()
		.is_some_and(Candidate::is_ready)
		|| !supervisor.state.can_cut_over()
	{
		return Ok(());
	}
	let mut ready = supervisor
		.candidate
		.take()
		.context("ready MCP candidate is unavailable")?;
	let initialized_frame = supervisor
		.state
		.initialized_frame()
		.context("MCP initialized notification is unavailable")?;
	if let Err(error) = ready.worker.write_frame(initialized_frame) {
		drop(ready);
		defer_candidate(supervisor, error);
		return Ok(());
	}

	let was_recovery = supervisor.worker.is_none();
	let reader = ready
		.reader
		.take()
		.context("ready MCP candidate reader is unavailable")?;
	supervisor.worker = Some(ready.worker);
	supervisor.active_generation = ready.generation;
	supervisor.active_fingerprint = ready.fingerprint;
	supervisor.requested_fingerprint = None;
	spawn_worker_reader(
		reader,
		supervisor.active_generation,
		supervisor.events_tx.clone(),
	);
	write_frame(
		&mut supervisor.client_stdout,
		br#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#,
	)?;
	let action = if was_recovery {
		"recovered"
	} else {
		"reloaded"
	};
	eprintln!("code-moniker: MCP stdio worker {action}");
	Ok(())
}

fn maybe_start_candidate<W>(supervisor: &mut Supervisor<W>) -> anyhow::Result<()> {
	if supervisor.candidate.is_some()
		|| !supervisor.state.handshake_complete()
		|| Instant::now() < supervisor.retry_at
		|| (supervisor.worker.is_some() && supervisor.requested_fingerprint.is_none())
	{
		return Ok(());
	}
	let fingerprint = supervisor
		.requested_fingerprint
		.or_else(|| BinaryFingerprint::read(&supervisor.executable).ok())
		.context("replacement MCP executable is unavailable")?;
	let initialize_frame = supervisor
		.state
		.initialize_frame()
		.context("MCP initialize request is unavailable")?;
	match Candidate::start(
		&supervisor.executable,
		&supervisor.worker_args,
		supervisor.next_generation,
		fingerprint,
		initialize_frame,
		supervisor.events_tx.clone(),
	) {
		Ok(candidate) => {
			supervisor.next_generation += 1;
			supervisor.candidate = Some(candidate);
		}
		Err(error) => defer_candidate(supervisor, error),
	}
	Ok(())
}

fn candidate_generation<W>(supervisor: &Supervisor<W>) -> Option<u64> {
	supervisor
		.candidate
		.as_ref()
		.map(|candidate| candidate.generation)
}

fn reject_candidate<W>(supervisor: &mut Supervisor<W>, error: anyhow::Error) {
	supervisor.candidate = None;
	defer_candidate(supervisor, error);
}

fn defer_candidate<W>(supervisor: &mut Supervisor<W>, error: anyhow::Error) {
	eprintln!("code-moniker: MCP stdio worker reload rejected; keeping current worker: {error:#}");
	supervisor.retry_at = Instant::now() + RELOAD_RETRY_DELAY;
}
