#[cfg(unix)]
use std::os::fd::FromRawFd;
#[cfg(unix)]
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock, TryLockError};
use std::time::Instant;

#[cfg(not(windows))]
use code_moniker_query::pid_is_alive;
use code_moniker_query::{
	CapabilitySet, Command, CommandRequest, CommandResponse, Consistency, DaemonRegistryEntry,
	DaemonRpcServer, DaemonWorkspaceConfig, HandshakeResponse, ProtocolRequest, ProtocolResponse,
	Query, QueryError, QueryRequest, QueryResponse, QueryResult, WorkspaceEventDto,
	WorkspaceEventKind, WorkspaceFailureDto, WorkspaceLifecycle, WorkspacePhase, WorkspaceStatus,
	bounded_debug, canonical_workspace_config, claim_registry_entry, config_from_roots,
	config_roots, current_build_identity, registry_path_for_config, remove_registry_entry_if_own,
	update_registry_entry_if_own, validate_daemon_start_config, workspace_label,
};
use code_moniker_workspace::snapshot::{WorkspaceCancellation, WorkspaceSnapshot};
use code_moniker_workspace::source::LocalResourceCache;
use jsonrpsee::core::{SubscriptionResult, async_trait};
use jsonrpsee::server::{PendingSubscriptionSink, Server};
use jsonrpsee::types::ErrorObjectOwned;

use crate::daemon::{
	WorkspaceDaemon, concurrent_snapshot_query, handle_stale_snapshot_query,
	stateless_protocol_response,
};
use crate::helpers::root_labels;
use crate::lifecycle::{
	refresh_full_cancellable, reject_conflicting_daemons, restart_live_watcher,
	workspace_failure_dto, workspace_status_result, workspace_status_without_snapshot,
};
use crate::telemetry;

const TELEMETRY_REQUEST_PAYLOAD_LIMIT: usize = 4_096;

pub fn serve_foreground<I, P>(roots: I) -> anyhow::Result<()>
where
	I: IntoIterator<Item = P>,
	P: AsRef<Path>,
{
	serve_foreground_config(config_from_roots(roots)?)
}

pub fn serve_foreground_config(config: DaemonWorkspaceConfig) -> anyhow::Result<()> {
	serve_foreground_config_supervised(config, None, None)
}

pub fn serve_foreground_config_supervised(
	config: DaemonWorkspaceConfig,
	supervisor_pid: Option<u32>,
	supervisor_fd: Option<i32>,
) -> anyhow::Result<()> {
	let runtime = tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.thread_name("code-moniker-daemon")
		.build()?;
	let result = runtime.block_on(serve_async(config, supervisor_pid, supervisor_fd));
	runtime.shutdown_timeout(std::time::Duration::from_millis(100));
	result
}

async fn serve_async(
	config: DaemonWorkspaceConfig,
	supervisor_pid: Option<u32>,
	supervisor_fd: Option<i32>,
) -> anyhow::Result<()> {
	validate_supervisor_pid(supervisor_pid)?;
	let config = canonical_workspace_config(config)?;
	validate_daemon_start_config(&config)?;
	let mut supervisor = SupervisorWatch::new(supervisor_pid, supervisor_fd)?;
	let registry_path = registry_path_for_config(&config)?;
	let registry_project = config.project.clone();
	let registry_cache_dir = config.cache_dir.clone();
	let registry_live_refresh = config.live_refresh.clone();

	let (events, _) = tokio::sync::broadcast::channel(EVENT_BUFFER);
	let daemon = WorkspaceDaemon::with_events(config.clone(), events.clone())?;
	let workspace_root = workspace_label(&daemon.roots);
	let workspace_roots = root_labels(&daemon.roots);
	let build = current_build_identity(env!("CARGO_PKG_VERSION"))?;
	let shutdown = Arc::new(tokio::sync::Notify::new());
	let daemon = Arc::new(Mutex::new(daemon));
	let published = Arc::new(RwLock::new(None));
	let lifecycle = Arc::new(RwLock::new(WorkspaceLifecycle::loading()));
	let service = DaemonRpcService {
		daemon: daemon.clone(),
		published: published.clone(),
		lifecycle: lifecycle.clone(),
		roots: Arc::from(config_roots(&config)),
		events: events.clone(),
		shutdown: shutdown.clone(),
		handshake: HandshakeResponse {
			protocol_version: code_moniker_query::PROTOCOL_VERSION,
			daemon_version: env!("CARGO_PKG_VERSION").to_string(),
			build: build.clone(),
			workspace_root: workspace_root.clone(),
			workspace_roots: workspace_roots.clone(),
			capabilities: CapabilitySet::default(),
		},
	};

	let server = Server::builder().build("127.0.0.1:0").await?;
	let addr = server.local_addr()?;
	let entry = DaemonRegistryEntry {
		workspace_root: workspace_root.clone(),
		workspace_roots,
		project: registry_project,
		cache_dir: registry_cache_dir,
		live_refresh: registry_live_refresh,
		endpoint: addr.to_string(),
		token: generate_token()?,
		pid: std::process::id(),
		build,
		heartbeat_unix_ms: code_moniker_query::registry_heartbeat_unix_ms(),
	};
	reject_conflicting_daemons(&config)?;
	if !claim_registry_entry(&config, &entry)? {
		let existing = code_moniker_query::read_registry_entry(&config)?;
		if let Some(existing) = existing {
			anyhow::bail!(
				"a daemon already claims {} (pid {}, endpoint {}); wait for it or stop it before starting another",
				existing.workspace_root,
				existing.pid,
				existing.endpoint
			);
		}
		anyhow::bail!("a daemon registry claim appeared while starting {workspace_root}");
	}
	let handle = server.start(service.into_rpc());
	eprintln!(
		"code-moniker daemon: indexing {} endpoint={} pid={} live_refresh={}",
		entry.workspace_root,
		entry.endpoint,
		entry.pid,
		entry.live_refresh.as_deref().unwrap_or("on-demand")
	);

	let (preload_cancellation, mut preload) =
		spawn_initial_preload(daemon.clone(), published, lifecycle.clone(), events.clone());
	let preload_result = tokio::select! {
		result = &mut preload => result,
		_ = shutdown.notified() => {
			preload_cancellation.cancel();
			preload.abort();
			stop_server(handle.clone(), &registry_path, &entry).await;
			return Ok(());
		}
		_ = supervisor.wait() => {
			preload_cancellation.cancel();
			preload.abort();
			stop_server(handle.clone(), &registry_path, &entry).await;
			return Ok(());
		}
		failure = maintain_registry_claim(&config, &entry) => {
			preload_cancellation.cancel();
			preload.abort();
			stop_server(handle.clone(), &registry_path, &entry).await;
			anyhow::bail!("daemon registry claim lost for {workspace_root}: {failure}");
		}
	};
	report_initial_preload(preload_result, &lifecycle, &events, &workspace_root);
	let claim_failure = tokio::select! {
		_ = shutdown.notified() => None,
		_ = handle.clone().stopped() => None,
		_ = supervisor.wait() => None,
		failure = maintain_registry_claim(&config, &entry) => Some(failure),
	};
	stop_server(handle, &registry_path, &entry).await;
	if let Some(failure) = claim_failure {
		anyhow::bail!("daemon registry claim lost for {workspace_root}: {failure}");
	}
	Ok(())
}

fn report_initial_preload(
	result: Result<anyhow::Result<WorkspaceStatus>, tokio::task::JoinError>,
	lifecycle: &RwLock<WorkspaceLifecycle>,
	events: &tokio::sync::broadcast::Sender<WorkspaceEventDto>,
	workspace_root: &str,
) {
	match result {
		Ok(Ok(status)) => eprintln!(
			"code-moniker daemon: index ready — files={} symbols={} references={}",
			status.files, status.symbols, status.references
		),
		Ok(Err(error)) => {
			eprintln!("code-moniker daemon: initial index failed for {workspace_root}: {error:#}")
		}
		Err(error) => {
			let message = format!("workspace preload worker failed: {error}");
			*lifecycle.write().unwrap_or_else(|err| err.into_inner()) =
				WorkspaceLifecycle::failed(message.clone());
			let _ = events.send(WorkspaceEventDto {
				kind: WorkspaceEventKind::Failed,
				generation: None,
				stale_summary: Some(message.clone()),
			});
			eprintln!("code-moniker daemon: initial index failed for {workspace_root}: {message}");
		}
	}
}

fn spawn_initial_preload(
	daemon: Arc<Mutex<WorkspaceDaemon>>,
	published: Arc<RwLock<Option<PublishedSnapshot>>>,
	lifecycle: Arc<RwLock<WorkspaceLifecycle>>,
	events: tokio::sync::broadcast::Sender<WorkspaceEventDto>,
) -> (
	WorkspaceCancellation,
	tokio::task::JoinHandle<anyhow::Result<WorkspaceStatus>>,
) {
	spawn_initial_preload_with_watcher(daemon, published, lifecycle, events, |daemon| {
		restart_live_watcher(daemon).map_err(|error| anyhow::anyhow!(error.to_string()))
	})
}

pub(super) fn spawn_initial_preload_with_watcher<F>(
	daemon: Arc<Mutex<WorkspaceDaemon>>,
	published: Arc<RwLock<Option<PublishedSnapshot>>>,
	lifecycle: Arc<RwLock<WorkspaceLifecycle>>,
	events: tokio::sync::broadcast::Sender<WorkspaceEventDto>,
	start_watcher: F,
) -> (
	WorkspaceCancellation,
	tokio::task::JoinHandle<anyhow::Result<WorkspaceStatus>>,
)
where
	F: FnOnce(&mut WorkspaceDaemon) -> anyhow::Result<()> + Send + 'static,
{
	let cancellation = WorkspaceCancellation::default();
	let worker_cancellation = cancellation.clone();
	let preload_span = telemetry::detached_operation_span("daemon.initial_preload");
	let worker = tokio::task::spawn_blocking(move || {
		preload_span.in_scope(|| {
			let mut daemon = daemon.lock().unwrap_or_else(|err| err.into_inner());
			let result = (|| {
				if daemon.registry.queries().snapshot().is_none() {
					refresh_full_cancellable(&mut daemon, worker_cancellation.clone())
						.map_err(|error| anyhow::anyhow!(error.to_string()))?;
					anyhow::ensure!(
						!worker_cancellation.is_cancelled(),
						"workspace preload cancelled"
					);
					start_watcher(&mut daemon)?;
				}
				Ok(())
			})();
			if let Err(error) = result {
				let failure = daemon
					.registry
					.queries()
					.last_failure()
					.map(workspace_failure_dto);
				let failure = failure.unwrap_or_else(|| WorkspaceFailureDto {
					resource: None,
					message: format!("{error:#}"),
				});
				*lifecycle.write().unwrap_or_else(|err| err.into_inner()) = WorkspaceLifecycle {
					phase: WorkspacePhase::Failed,
					failure: Some(failure.clone()),
				};
				let _ = events.send(WorkspaceEventDto {
					kind: WorkspaceEventKind::Failed,
					generation: None,
					stale_summary: Some(failure.message),
				});
				return Err(error);
			}
			publish_current_snapshot(&daemon, &published);
			let status = workspace_status_result(&daemon.roots, &daemon.registry);
			*lifecycle.write().unwrap_or_else(|err| err.into_inner()) = WorkspaceLifecycle {
				phase: status.phase,
				failure: status.failure.clone(),
			};
			let _ = events.send(WorkspaceEventDto {
				kind: WorkspaceEventKind::Refreshed,
				generation: status.generation,
				stale_summary: None,
			});
			Ok(status)
		})
	});
	(cancellation, worker)
}

fn validate_supervisor_pid(supervisor_pid: Option<u32>) -> anyhow::Result<()> {
	let Some(supervisor_pid) = supervisor_pid else {
		return Ok(());
	};
	anyhow::ensure!(
		supervisor_pid != 0,
		"supervisor PID must be greater than zero"
	);
	#[cfg(not(windows))]
	anyhow::ensure!(
		pid_is_alive(supervisor_pid),
		"supervisor process {supervisor_pid} is not running"
	);
	Ok(())
}

async fn stop_server(
	handle: jsonrpsee::server::ServerHandle,
	registry_path: &Path,
	entry: &DaemonRegistryEntry,
) {
	let _ = handle.stop();
	handle.stopped().await;
	remove_registry_entry_if_own(registry_path, entry);
}

struct SupervisorWatch {
	#[cfg(not(windows))]
	pid: Option<u32>,
	#[cfg(unix)]
	channel: Option<tokio::net::UnixStream>,
	#[cfg(windows)]
	process: Option<WindowsSupervisorProcess>,
}

impl SupervisorWatch {
	fn new(pid: Option<u32>, fd: Option<i32>) -> anyhow::Result<Self> {
		#[cfg(unix)]
		let channel = fd
			.map(|fd| {
				anyhow::ensure!(fd > 2, "supervisor FD must be greater than 2");
				// SAFETY: --supervisor-fd transfers ownership of one inherited
				// descriptor to this daemon process exactly once.
				let stream = unsafe { StdUnixStream::from_raw_fd(fd) };
				stream.set_nonblocking(true)?;
				tokio::net::UnixStream::from_std(stream).map_err(anyhow::Error::from)
			})
			.transpose()?;
		#[cfg(not(unix))]
		anyhow::ensure!(fd.is_none(), "--supervisor-fd is only supported on Unix");
		#[cfg(unix)]
		{
			Ok(Self { pid, channel })
		}
		#[cfg(windows)]
		{
			let process = pid.map(WindowsSupervisorProcess::open).transpose()?;
			Ok(Self { process })
		}
		#[cfg(all(not(unix), not(windows)))]
		{
			Ok(Self { pid })
		}
	}

	async fn wait(&mut self) {
		#[cfg(unix)]
		{
			match (&self.channel, self.pid) {
				(Some(channel), Some(pid)) => tokio::select! {
					_ = wait_for_supervisor_channel(channel) => {}
					_ = wait_for_supervisor_pid(pid) => {}
				},
				(Some(channel), None) => wait_for_supervisor_channel(channel).await,
				(None, Some(pid)) => wait_for_supervisor_pid(pid).await,
				(None, None) => std::future::pending::<()>().await,
			}
		}
		#[cfg(windows)]
		{
			match &self.process {
				Some(process) => wait_for_windows_supervisor(process).await,
				None => std::future::pending::<()>().await,
			}
		}
		#[cfg(all(not(unix), not(windows)))]
		{
			match self.pid {
				Some(pid) => wait_for_supervisor_pid(pid).await,
				None => std::future::pending::<()>().await,
			}
		}
	}
}

#[cfg(windows)]
pub(crate) struct WindowsSupervisorProcess {
	handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl WindowsSupervisorProcess {
	pub(crate) fn open(pid: u32) -> anyhow::Result<Self> {
		use windows_sys::Win32::Foundation::{
			CloseHandle, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
		};
		use windows_sys::Win32::System::Threading::{
			OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
		};

		let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
		if handle.is_null() {
			return Err(anyhow::anyhow!(
				"cannot open supervisor process {pid}: {}",
				std::io::Error::last_os_error()
			));
		}
		match unsafe { WaitForSingleObject(handle, 0) } {
			WAIT_TIMEOUT => Ok(Self { handle }),
			WAIT_OBJECT_0 => {
				unsafe {
					CloseHandle(handle);
				}
				anyhow::bail!("supervisor process {pid} is not running")
			}
			WAIT_FAILED => {
				let error = std::io::Error::last_os_error();
				unsafe {
					CloseHandle(handle);
				}
				Err(anyhow::anyhow!(
					"cannot inspect supervisor process {pid}: {error}"
				))
			}
			state => {
				unsafe {
					CloseHandle(handle);
				}
				anyhow::bail!(
					"cannot inspect supervisor process {pid}: unexpected wait state {state}"
				)
			}
		}
	}

	pub(crate) fn is_running(&self) -> bool {
		use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
		use windows_sys::Win32::System::Threading::WaitForSingleObject;

		let state = unsafe { WaitForSingleObject(self.handle, 0) };
		state == WAIT_TIMEOUT
	}
}

#[cfg(windows)]
impl Drop for WindowsSupervisorProcess {
	fn drop(&mut self) {
		unsafe {
			windows_sys::Win32::Foundation::CloseHandle(self.handle);
		}
	}
}

#[cfg(unix)]
async fn wait_for_supervisor_channel(channel: &tokio::net::UnixStream) {
	loop {
		if channel.readable().await.is_err() {
			return;
		}
		let mut byte = [0_u8; 1];
		match channel.try_read(&mut byte) {
			Ok(0) => return,
			Ok(_) => {}
			Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
			Err(_) => return,
		}
	}
}

#[cfg(windows)]
async fn wait_for_windows_supervisor(process: &WindowsSupervisorProcess) {
	let mut checks = tokio::time::interval(std::time::Duration::from_secs(1));
	loop {
		checks.tick().await;
		if !process.is_running() {
			return;
		}
	}
}

#[cfg(not(windows))]
async fn wait_for_supervisor_pid(supervisor_pid: u32) {
	let mut checks = tokio::time::interval(std::time::Duration::from_secs(1));
	loop {
		checks.tick().await;
		if !pid_is_alive(supervisor_pid) {
			return;
		}
	}
}

#[derive(Debug, thiserror::Error)]
enum RegistryClaimFailure {
	#[error("cannot read the registry entry: {0}")]
	Read(String),
	#[error("the registry entry disappeared")]
	Missing,
	#[error("the registry entry was replaced by daemon pid {pid}")]
	Replaced { pid: u32 },
	#[error("cannot write the registry heartbeat: {0}")]
	HeartbeatWrite(String),
	#[error("the registry heartbeat was rejected because this daemon no longer owns the claim")]
	HeartbeatRejected,
}

async fn maintain_registry_claim(
	config: &DaemonWorkspaceConfig,
	entry: &DaemonRegistryEntry,
) -> RegistryClaimFailure {
	let mut ownership_checks = tokio::time::interval(std::time::Duration::from_millis(250));
	let mut heartbeats = tokio::time::interval(std::time::Duration::from_secs(2));
	loop {
		tokio::select! {
			_ = ownership_checks.tick() => {
				match code_moniker_query::read_registry_entry(config) {
					Err(error) => return RegistryClaimFailure::Read(format!("{error:#}")),
					Ok(None) => return RegistryClaimFailure::Missing,
					Ok(Some(current))
						if current.pid != entry.pid || current.token != entry.token =>
					{
						return RegistryClaimFailure::Replaced { pid: current.pid };
					}
					Ok(Some(_)) => {}
				}
			}
			_ = heartbeats.tick() => {
				let mut heartbeat = entry.clone();
				heartbeat.heartbeat_unix_ms = code_moniker_query::registry_heartbeat_unix_ms();
				match update_registry_entry_if_own(config, &heartbeat) {
					Err(error) => {
						return RegistryClaimFailure::HeartbeatWrite(format!("{error:#}"));
					}
					Ok(false) => return RegistryClaimFailure::HeartbeatRejected,
					Ok(true) => {}
				}
			}
		}
	}
}

const EVENT_BUFFER: usize = 256;

#[derive(Clone)]
pub(super) struct SnapshotQueryContext {
	pub(super) roots: Arc<[PathBuf]>,
	pub(super) config_root: Arc<PathBuf>,
	pub(super) cache: LocalResourceCache,
}

#[derive(Clone)]
pub(super) struct PublishedSnapshot {
	pub(super) snapshot: Arc<WorkspaceSnapshot>,
	pub(super) context: SnapshotQueryContext,
	pub(super) status: WorkspaceStatus,
}

pub(super) struct DaemonRpcService {
	pub(super) daemon: Arc<Mutex<WorkspaceDaemon>>,
	pub(super) published: Arc<RwLock<Option<PublishedSnapshot>>>,
	pub(super) lifecycle: Arc<RwLock<WorkspaceLifecycle>>,
	pub(super) roots: Arc<[PathBuf]>,
	pub(super) events: tokio::sync::broadcast::Sender<WorkspaceEventDto>,
	pub(super) shutdown: Arc<tokio::sync::Notify>,
	pub(super) handshake: HandshakeResponse,
}

pub(super) fn publish_current_snapshot(
	daemon: &WorkspaceDaemon,
	published: &RwLock<Option<PublishedSnapshot>>,
) {
	let Some(snapshot) = daemon.registry.queries().snapshot_arc() else {
		return;
	};
	let publication = PublishedSnapshot {
		snapshot,
		context: SnapshotQueryContext {
			roots: Arc::from(daemon.roots.clone()),
			config_root: Arc::new(daemon.config_root.clone()),
			cache: daemon.cache.clone(),
		},
		status: workspace_status_result(&daemon.roots, &daemon.registry),
	};
	*published.write().unwrap_or_else(|err| err.into_inner()) = Some(publication);
}

impl DaemonRpcService {
	#[tracing::instrument(
		parent = None,
		name = "daemon.request",
		skip_all,
		fields(
			request.kind = protocol_request_kind(&request),
			request.operation = protocol_request_operation(&request),
			request.consistency = protocol_request_consistency(&request),
			request.payload = %bounded_debug(&request, TELEMETRY_REQUEST_PAYLOAD_LIMIT),
			response.status = tracing::field::Empty,
			response.generation = tracing::field::Empty,
			error.message = tracing::field::Empty,
		)
	)]
	pub(super) async fn dispatch(
		&self,
		request: ProtocolRequest,
	) -> Result<ProtocolResponse, ErrorObjectOwned> {
		let request_kind = protocol_request_kind(&request);
		let request_operation = protocol_request_operation(&request);
		let started = Instant::now();
		let result = if let ProtocolRequest::Query(query) = &request
			&& let Err(error) = query.validate()
		{
			Ok(ProtocolResponse::Error(error))
		} else if let Some(response) = stateless_protocol_response(&request) {
			Ok(response)
		} else if request_needs_initial_snapshot(&request, &self.published, &self.lifecycle) {
			Ok(workspace_unavailable_response(request, &self.lifecycle))
		} else if concurrent_snapshot_request(&request) {
			dispatch_published_snapshot(self.published.clone(), request).await
		} else {
			dispatch_workspace_request(
				self.daemon.clone(),
				self.published.clone(),
				self.lifecycle.clone(),
				self.roots.clone(),
				request,
			)
			.await
		};
		let span = tracing::Span::current();
		let response_status = record_protocol_response(&span, &result);
		telemetry::record_daemon_request(
			request_kind,
			request_operation,
			response_status,
			started.elapsed(),
		);
		result
	}
}

fn protocol_request_kind(request: &ProtocolRequest) -> &'static str {
	match request {
		ProtocolRequest::Query(_) => "query",
		ProtocolRequest::Command(_) => "command",
	}
}

fn protocol_request_operation(request: &ProtocolRequest) -> &'static str {
	match request {
		ProtocolRequest::Query(request) => request.query.capability(),
		ProtocolRequest::Command(request) => match &request.command {
			Command::WorkspaceRefresh => "workspace.refresh",
			Command::WorkspaceSourceSetReplace { .. } => "workspace.source-set.replace",
			Command::WorkspaceSourceSetRemove { .. } => "workspace.source-set.remove",
		},
	}
}

fn protocol_request_consistency(request: &ProtocolRequest) -> &'static str {
	match request {
		ProtocolRequest::Query(request) => match request.consistency {
			Consistency::Current => "current",
			Consistency::RefreshIfStale => "refresh_if_stale",
			Consistency::StaleOk => "stale_ok",
		},
		ProtocolRequest::Command(_) => "not_applicable",
	}
}

fn record_protocol_response(
	span: &tracing::Span,
	result: &Result<ProtocolResponse, ErrorObjectOwned>,
) -> &'static str {
	match result {
		Ok(ProtocolResponse::Query(response)) => {
			span.record("response.status", "ok");
			if let Some(generation) = response.generation {
				span.record("response.generation", generation.0);
			}
			"ok"
		}
		Ok(ProtocolResponse::Command(response)) => {
			span.record("response.status", "ok");
			if let Some(generation) = response.generation {
				span.record("response.generation", generation.0);
			}
			"ok"
		}
		Ok(ProtocolResponse::Error(error)) => {
			span.record("response.status", "error");
			span.record("error.message", tracing::field::debug(error));
			"error"
		}
		Err(error) => {
			span.record("response.status", "rpc_error");
			span.record("error.message", tracing::field::display(error));
			"rpc_error"
		}
	}
}

fn request_needs_initial_snapshot(
	request: &ProtocolRequest,
	published: &RwLock<Option<PublishedSnapshot>>,
	lifecycle: &RwLock<WorkspaceLifecycle>,
) -> bool {
	let needs_snapshot = match request {
		ProtocolRequest::Query(request) => request.query.requires_workspace_snapshot(),
		ProtocolRequest::Command(_) => {
			lifecycle
				.read()
				.unwrap_or_else(|err| err.into_inner())
				.phase == WorkspacePhase::Loading
		}
	};
	needs_snapshot
		&& published
			.read()
			.unwrap_or_else(|err| err.into_inner())
			.is_none()
}

fn concurrent_snapshot_request(request: &ProtocolRequest) -> bool {
	matches!(
		request,
		ProtocolRequest::Query(request)
			if request.consistency == Consistency::StaleOk
				&& concurrent_snapshot_query(&request.query)
	)
}

async fn dispatch_published_snapshot(
	published: Arc<RwLock<Option<PublishedSnapshot>>>,
	request: ProtocolRequest,
) -> Result<ProtocolResponse, ErrorObjectOwned> {
	let published = published
		.read()
		.unwrap_or_else(|err| err.into_inner())
		.clone();
	let blocking_span = tracing::Span::current();
	tokio::task::spawn_blocking(move || {
		let _entered = blocking_span.enter();
		let ProtocolRequest::Query(request) = request else {
			unreachable!("concurrent query routing checked the request variant")
		};
		handle_stale_snapshot_query(published, *request)
	})
	.await
	.map_err(|err| internal_error(err.to_string()))
}

async fn dispatch_workspace_request(
	daemon: Arc<Mutex<WorkspaceDaemon>>,
	published: Arc<RwLock<Option<PublishedSnapshot>>>,
	lifecycle: Arc<RwLock<WorkspaceLifecycle>>,
	roots: Arc<[PathBuf]>,
	request: ProtocolRequest,
) -> Result<ProtocolResponse, ErrorObjectOwned> {
	let blocking_span = tracing::Span::current();
	tokio::task::spawn_blocking(move || {
		let _entered = blocking_span.enter();
		handle_workspace_request(&daemon, &published, &lifecycle, &roots, request)
	})
	.await
	.map_err(|err| internal_error(err.to_string()))
}

fn handle_workspace_request(
	daemon: &Mutex<WorkspaceDaemon>,
	published: &RwLock<Option<PublishedSnapshot>>,
	lifecycle: &RwLock<WorkspaceLifecycle>,
	roots: &[PathBuf],
	request: ProtocolRequest,
) -> ProtocolResponse {
	if matches!(
		&request,
		ProtocolRequest::Query(request) if matches!(&request.query, Query::WorkspaceStatus)
	) {
		return match daemon.try_lock() {
			Ok(mut guard) => handle_and_publish(&mut guard, published, lifecycle, request),
			Err(TryLockError::WouldBlock) => {
				workspace_busy_response(request, roots, published, lifecycle)
			}
			Err(TryLockError::Poisoned(err)) => {
				handle_and_publish(&mut err.into_inner(), published, lifecycle, request)
			}
		};
	}
	let mut guard = daemon.lock().unwrap_or_else(|err| err.into_inner());
	handle_and_publish(&mut guard, published, lifecycle, request)
}

fn handle_and_publish(
	daemon: &mut WorkspaceDaemon,
	published: &RwLock<Option<PublishedSnapshot>>,
	lifecycle: &RwLock<WorkspaceLifecycle>,
	request: ProtocolRequest,
) -> ProtocolResponse {
	let updates_lifecycle = matches!(&request, ProtocolRequest::Command(_));
	let response = daemon.handle_protocol(request);
	publish_current_snapshot(daemon, published);
	if updates_lifecycle {
		let status = workspace_status_result(&daemon.roots, &daemon.registry);
		*lifecycle.write().unwrap_or_else(|err| err.into_inner()) = WorkspaceLifecycle {
			phase: status.phase,
			failure: status.failure,
		};
	}
	response
}

#[async_trait]
impl DaemonRpcServer for DaemonRpcService {
	async fn handshake(&self, _client: String) -> Result<HandshakeResponse, ErrorObjectOwned> {
		Ok(self.handshake.clone())
	}

	async fn query(&self, request: QueryRequest) -> Result<QueryResponse, ErrorObjectOwned> {
		let mut response = match self
			.dispatch(ProtocolRequest::Query(Box::new(request)))
			.await?
		{
			ProtocolResponse::Query(response) => *response,
			ProtocolResponse::Error(error) => return Err(query_error(error)),
			other => {
				return Err(internal_error(format!(
					"unexpected query response: {other:?}"
				)));
			}
		};
		if let QueryResult::WorkspaceStatus(status) = &mut response.result {
			status.producer = self.handshake.build.clone();
		}
		Ok(response)
	}

	async fn command(&self, request: CommandRequest) -> Result<CommandResponse, ErrorObjectOwned> {
		match self.dispatch(ProtocolRequest::Command(request)).await? {
			ProtocolResponse::Command(response) => Ok(response),
			ProtocolResponse::Error(error) => Err(query_error(error)),
			other => Err(internal_error(format!(
				"unexpected command response: {other:?}"
			))),
		}
	}

	async fn shutdown(&self) -> Result<(), ErrorObjectOwned> {
		self.shutdown.notify_one();
		Ok(())
	}

	async fn subscribe_events(&self, pending: PendingSubscriptionSink) -> SubscriptionResult {
		let mut rx = self.events.subscribe();
		let sink = pending.accept().await?;
		loop {
			tokio::select! {
				_ = sink.closed() => break,
				received = rx.recv() => match received {
					Ok(event) => {
						let message = serde_json::value::to_raw_value(&event)?;
						if sink.send(message).await.is_err() {
							break;
						}
					}
					Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
					Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
				},
			}
		}
		Ok(())
	}
}

fn internal_error(message: String) -> ErrorObjectOwned {
	ErrorObjectOwned::owned(
		jsonrpsee::types::error::INTERNAL_ERROR_CODE,
		message,
		None::<()>,
	)
}

/// Maps a structured `QueryError` to a JSON-RPC error, preserving the stable
/// `code` in `data` so clients can branch on it instead of parsing the message.
pub(super) fn query_error(error: QueryError) -> ErrorObjectOwned {
	let message = error.message.clone();
	ErrorObjectOwned::owned(
		jsonrpsee::types::error::INTERNAL_ERROR_CODE,
		message,
		Some(error),
	)
}

pub(super) fn generate_token() -> anyhow::Result<String> {
	let mut bytes = [0_u8; 16];
	getrandom::fill(&mut bytes)
		.map_err(|error| anyhow::anyhow!("cannot generate token: {error}"))?;
	Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn workspace_busy_response(
	request: ProtocolRequest,
	roots: &[PathBuf],
	published: &RwLock<Option<PublishedSnapshot>>,
	lifecycle: &RwLock<WorkspaceLifecycle>,
) -> ProtocolResponse {
	match request {
		ProtocolRequest::Query(request) if matches!(&request.query, Query::WorkspaceStatus) => {
			let published = published
				.read()
				.unwrap_or_else(|err| err.into_inner())
				.clone();
			let Some(mut published) = published else {
				let lifecycle = lifecycle
					.read()
					.unwrap_or_else(|err| err.into_inner())
					.clone();
				return ProtocolResponse::Query(Box::new(workspace_status_without_snapshot(
					roots, lifecycle,
				)));
			};
			let summary = format!(
				"workspace refresh in progress; stale-ok reads continue on generation {}",
				published.snapshot.generation.value()
			);
			published.status.stale = true;
			published.status.stale_summary = summary.clone();
			published.status.phase = WorkspacePhase::Refreshing;
			for root in &mut published.status.roots {
				root.stale = true;
				root.stale_summary = summary.clone();
			}
			ProtocolResponse::Query(Box::new(QueryResponse {
				generation: published.status.generation,
				result: QueryResult::WorkspaceStatus(published.status),
				next_cursor: None,
			}))
		}
		_ => ProtocolResponse::Error(QueryError::new(
			"workspace_busy",
			"workspace daemon is applying an exclusive mutation; retry the request",
		)),
	}
}

pub(super) fn workspace_unavailable_response(
	request: ProtocolRequest,
	lifecycle: &RwLock<WorkspaceLifecycle>,
) -> ProtocolResponse {
	let lifecycle = lifecycle
		.read()
		.unwrap_or_else(|err| err.into_inner())
		.clone();
	let error = match lifecycle.phase {
		WorkspacePhase::Failed => QueryError::new(
			"workspace_load_failed",
			lifecycle
				.failure
				.map(|failure| failure.message)
				.unwrap_or_else(|| "workspace initial index failed".to_string()),
		),
		WorkspacePhase::Loading | WorkspacePhase::Refreshing | WorkspacePhase::Ready => {
			let subject = match request {
				ProtocolRequest::Query(_) => "workspace snapshot is still indexing",
				ProtocolRequest::Command(_) => {
					"workspace snapshot is still indexing; commands are not available yet"
				}
			};
			QueryError::new("workspace_loading", subject)
		}
	};
	ProtocolResponse::Error(error)
}

// A second start for the same workspace must be refused while the first
// lives, own registry slot included — overwriting it silently orphans the
// running daemon.
