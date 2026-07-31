// code-moniker: ignore-file[smell-low-cohesion-module, smell-clone-reflex]
// Daemon bootstrap clones config and handles into independently owned runtime services.
#![cfg(unix)]

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock, TryLockError, mpsc};
use std::time::{SystemTime, UNIX_EPOCH};

use code_moniker_check::{
	CheckRequest, CheckSkipReason, CheckSummary, CompiledRuleSpec, DefaultRulesSelection,
	IndexedCheckWorkspace, RuleCoverage, RulePathReport, RulePathStep, RuleReport, RuleSetRequest,
	RuleSeverity, RuleVerdict, Violation,
};
use code_moniker_core::core::shape::{Shape, shape_of};
use code_moniker_core::lang::Lang;
use code_moniker_query::{
	AuditClusterDto, AuditSampleDto, AuditTotalsDto, AuditZoneDto, BuildIdentity, CapabilitySet,
	CheckSummaryDto, Command, CommandRequest, CommandResponse, Consistency, CountDto,
	DaemonRpcServer, DaemonWorkspaceConfig, FailedRuleDto, FileErrorDto, HandshakeResponse,
	NoteDto, NoteResolutionDto, NotesAction, NotesQuery, NotesResult, Page, ProtocolRequest,
	ProtocolResponse, Query, QueryCursor, QueryError, QueryRequest, QueryResponse, QueryResult,
	ResolutionAuditResult, RuleCoverageDto, RuleDto, RulePathReportDto, RulePathStepDto,
	RuleReportDto, RulesCheckResult, RulesCheckRootResult, RulesCheckVerdict, RulesListResult,
	SourceLine, SourceSnippet, SymbolDetailResult, SymbolDto, SymbolInsightsResult,
	SymbolListResult, SymbolSearchQuery, SymbolUsagesQuery, SymbolUsagesResult, TreeChildrenQuery,
	TreeChildrenResult, TreeNode, TreeNodeKind, UsageDirection, UsageDto, UsageSummaryDto,
	ViewReadQuery, ViolationDto, WorkspaceEventDto, WorkspaceEventKind, WorkspaceFailureDto,
	WorkspaceGeneration, WorkspaceLifecycle, WorkspacePhase, WorkspaceRootStatus,
	WorkspaceSourceSetDto, WorkspaceStatus, WorkspaceTimingsDto, current_build_identity,
	describe_query_capabilities, symbol_is_test_artifact,
};
use code_moniker_workspace::code::compact_identity;
use code_moniker_workspace::glob::FilePathFilter;
use code_moniker_workspace::live::{
	LiveWorkspaceWatcher, WorkspaceLiveEvent, WorkspaceLiveRefreshPlan,
};
use code_moniker_workspace::notes::{
	Note, NoteAuthor, NoteChanges, NoteId, NoteKind, NoteResolution, NoteStatus, NotesDocument,
	ResolvedNote, WorkspaceNotes, resolve_notes,
};
use code_moniker_workspace::registry::{LocalWorkspaceOptions, LocalWorkspaceRegistry};
use code_moniker_workspace::snapshot::{
	BoundedPathLimits, BoundedPathScope, ExternalReferenceOrigin, ReferenceId, ReferenceRecord,
	SourceFileRecord, SourceId, SymbolId, SymbolRecord, WorkspaceRequest, WorkspaceResource,
	WorkspaceSnapshot, WorkspaceTransition, WorkspaceView,
};
use code_moniker_workspace::source::{
	LocalResourceCache, MEMORY_SOURCE_ROOT, MEMORY_SOURCE_ROOT_LABEL, MemorySourceDocument,
	MemorySourceSet, MemorySourceSetUpdate, is_memory_source_path,
};
use jsonrpsee::core::{SubscriptionResult, async_trait};
use jsonrpsee::server::{PendingSubscriptionSink, Server};
use jsonrpsee::types::ErrorObjectOwned;

const DEFAULT_SCHEME: &str = "code+moniker://";
const MEMORY_SOURCE_LIMITS: MemorySourceLimits = MemorySourceLimits {
	max_source_sets: 128,
	max_documents_per_set: 10_000,
	max_uri_bytes: 4 * 1024,
	max_document_bytes: 16 * 1024 * 1024,
	max_source_set_bytes: 64 * 1024 * 1024,
	max_total_bytes: 256 * 1024 * 1024,
};

#[derive(Clone, Copy)]
struct MemorySourceLimits {
	max_source_sets: usize,
	max_documents_per_set: usize,
	max_uri_bytes: usize,
	max_document_bytes: usize,
	max_source_set_bytes: usize,
	max_total_bytes: usize,
}

use code_moniker_query::{
	ChangeContextCoverageDto, ChangeContextQuery, ChangeContextResult, ChangeReviewFile,
	ChangeReviewQuery, ChangeReviewRef, ChangeReviewResult, ChangeReviewSide, ChangeReviewSummary,
	ChangeReviewSymbol, GraphPathCoverage, GraphPathExpectation, GraphPathQuery, GraphPathResult,
	GraphPathSearchStats, GraphPathStep, GraphPathVerdict, GraphSectionCoverage,
	IdentityChildrenQuery, IdentityChildrenResult, IdentityGraphCoverage, IdentityGraphEdge,
	IdentityGraphPort, IdentityGraphQuery, IdentityGraphResult, IdentitySegmentDto,
	RuleApplicabilityDto, RulesApplicableQuery, RulesApplicableResult, SymbolGraphCoverage,
	SymbolGraphEdge, SymbolGraphFocus, SymbolGraphNeighbor, SymbolGraphQuery, SymbolGraphResult,
	UnlinkedRefsDto,
};

use helpers::*;

mod syntax;
pub mod views;

pub use code_moniker_workspace::snapshot::WorkspaceCancellation;

pub use code_moniker_query::{
	DaemonRegistryEntry, canonical_workspace_config, canonical_workspace_root,
	canonical_workspace_roots, claim_registry_entry, config_from_roots, config_roots,
	daemon_workspace_config, list_registry_entries, pid_is_alive, registry_dir,
	registry_path_for_config, registry_path_for_root, registry_path_for_roots,
	remove_registry_entry_if_own, update_registry_entry_if_own, validate_daemon_start_config,
	workspace_label, write_registry_entry,
};

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
	supervisor_fd: Option<RawFd>,
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
	supervisor_fd: Option<RawFd>,
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
	let cancellation = WorkspaceCancellation::default();
	let worker_cancellation = cancellation.clone();
	let worker = tokio::task::spawn_blocking(move || {
		let mut daemon = daemon.lock().unwrap_or_else(|err| err.into_inner());
		let result = (|| {
			if daemon.registry.queries().snapshot().is_none() {
				refresh_full_cancellable(&mut daemon, worker_cancellation.clone())
					.map_err(|error| anyhow::anyhow!(error.to_string()))?;
				anyhow::ensure!(
					!worker_cancellation.is_cancelled(),
					"workspace preload cancelled"
				);
				restart_live_watcher(&mut daemon)
					.map_err(|error| anyhow::anyhow!(error.to_string()))?;
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
	pid: Option<u32>,
	channel: Option<tokio::net::UnixStream>,
}

impl SupervisorWatch {
	fn new(pid: Option<u32>, fd: Option<RawFd>) -> anyhow::Result<Self> {
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
		Ok(Self { pid, channel })
	}

	async fn wait(&mut self) {
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
}

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
struct SnapshotQueryContext {
	roots: Arc<[PathBuf]>,
	config_root: Arc<PathBuf>,
	cache: LocalResourceCache,
}

#[derive(Clone)]
struct PublishedSnapshot {
	snapshot: Arc<WorkspaceSnapshot>,
	context: SnapshotQueryContext,
	status: WorkspaceStatus,
}

struct DaemonRpcService {
	daemon: Arc<Mutex<WorkspaceDaemon>>,
	published: Arc<RwLock<Option<PublishedSnapshot>>>,
	lifecycle: Arc<RwLock<WorkspaceLifecycle>>,
	roots: Arc<[PathBuf]>,
	events: tokio::sync::broadcast::Sender<WorkspaceEventDto>,
	shutdown: Arc<tokio::sync::Notify>,
	handshake: HandshakeResponse,
}

fn publish_current_snapshot(
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
	async fn dispatch(
		&self,
		request: ProtocolRequest,
	) -> Result<ProtocolResponse, ErrorObjectOwned> {
		if let Some(response) = stateless_protocol_response(&request) {
			return Ok(response);
		}
		if request_needs_initial_snapshot(&request, &self.published, &self.lifecycle) {
			return Ok(workspace_unavailable_response(request, &self.lifecycle));
		}
		if concurrent_snapshot_request(&request) {
			return dispatch_published_snapshot(self.published.clone(), request).await;
		}
		dispatch_workspace_request(
			self.daemon.clone(),
			self.published.clone(),
			self.lifecycle.clone(),
			self.roots.clone(),
			request,
		)
		.await
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
	tokio::task::spawn_blocking(move || {
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
	tokio::task::spawn_blocking(move || {
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
fn query_error(error: QueryError) -> ErrorObjectOwned {
	let message = error.message.clone();
	ErrorObjectOwned::owned(
		jsonrpsee::types::error::INTERNAL_ERROR_CODE,
		message,
		Some(error),
	)
}

fn generate_token() -> anyhow::Result<String> {
	use std::io::Read;
	let mut file = std::fs::File::open("/dev/urandom")?;
	let mut bytes = [0u8; 16];
	file.read_exact(&mut bytes)?;
	Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub struct WorkspaceDaemon {
	roots: Vec<PathBuf>,
	config_root: PathBuf,
	registry: LocalWorkspaceRegistry,
	cache: LocalResourceCache,
	notes: WorkspaceNotes,
	live: DaemonLiveState,
}

#[derive(Clone, Copy)]
struct ResponseContext<'a> {
	roots: &'a [PathBuf],
	config_root: &'a Path,
	generation: Option<WorkspaceGeneration>,
}

struct RulesListFilters {
	langs: Vec<String>,
	severities: Vec<String>,
}

struct RulesListEval {
	workspace: Option<String>,
	profile: Option<String>,
	rules: Option<String>,
	filters: RulesListFilters,
	page: Page,
}

struct DaemonLiveState {
	policy: DaemonLiveRefreshPolicy,
	tx: mpsc::Sender<WorkspaceLiveEvent>,
	rx: mpsc::Receiver<WorkspaceLiveEvent>,
	watcher: Option<LiveWorkspaceWatcher>,
	events: Option<tokio::sync::broadcast::Sender<WorkspaceEventDto>>,
}

struct WorkspaceDaemonInit {
	roots: Vec<PathBuf>,
	config_root: PathBuf,
	registry: LocalWorkspaceRegistry,
	cache: LocalResourceCache,
	live: DaemonLiveState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DaemonLiveRefreshPolicy {
	OnDemand,
	Auto,
}

struct RulesCheckEval {
	workspace: Option<String>,
	profile: Option<String>,
	rules: Option<String>,
	files: Vec<String>,
	report: bool,
	page: Page,
}

struct IndexedRulesCheck<'a> {
	root: &'a Path,
	config_root: &'a Path,
	workspace: &'a IndexedCheckWorkspace,
	profile: Option<String>,
	rules: Option<&'a str>,
	files: &'a [String],
	report: bool,
}

struct UsageDtoContext<'a> {
	snapshot: &'a WorkspaceSnapshot,
	roots: &'a [PathBuf],
	selected_roots: &'a [&'a PathBuf],
	path_filter: &'a FilePathFilter,
	langs: &'a [String],
}

struct NotesResponseInput<'a> {
	snapshot: &'a WorkspaceSnapshot,
	action: NotesAction,
	notes: Vec<Note>,
	deleted: Option<Note>,
	orphan: Option<bool>,
	page: Page,
	generation: Option<WorkspaceGeneration>,
}

impl WorkspaceDaemon {
	pub fn new(roots: Vec<PathBuf>) -> anyhow::Result<Self> {
		Self::new_with_config(config_from_roots(roots)?)
	}

	pub fn new_with_config(config: DaemonWorkspaceConfig) -> anyhow::Result<Self> {
		Self::build(config, None)
	}

	fn with_events(
		config: DaemonWorkspaceConfig,
		events: tokio::sync::broadcast::Sender<WorkspaceEventDto>,
	) -> anyhow::Result<Self> {
		Self::build(config, Some(events))
	}

	fn build(
		config: DaemonWorkspaceConfig,
		events: Option<tokio::sync::broadcast::Sender<WorkspaceEventDto>>,
	) -> anyhow::Result<Self> {
		validate_daemon_start_config(&config)?;
		let init = WorkspaceDaemonInit::new(config)?;
		let mut daemon = Self {
			roots: init.roots,
			config_root: init.config_root,
			registry: init.registry,
			cache: init.cache,
			notes: WorkspaceNotes::default(),
			live: init.live,
		};
		daemon.live.events = events;
		Ok(daemon)
	}

	pub fn handle_protocol(&mut self, request: ProtocolRequest) -> ProtocolResponse {
		handle_protocol(self, request)
	}

	pub fn refresh_cancellable(
		&mut self,
		cancellation: WorkspaceCancellation,
	) -> Result<CommandResponse, QueryError> {
		refresh_full_cancellable(self, cancellation.clone())?;
		if cancellation.is_cancelled() {
			return Err(QueryError::new(
				"workspace_cancelled",
				"workspace refresh was cancelled",
			));
		}
		restart_live_watcher(self)?;
		let status = workspace_status_result(&self.roots, &self.registry);
		Ok(CommandResponse {
			generation: generation(&self.registry),
			message: "workspace refreshed".to_string(),
			status: Some(Box::new(status)),
		})
	}

	fn restart_live_watcher(&mut self) -> anyhow::Result<()> {
		let tx = self.live.tx.clone();
		let events = self.live.events.clone();
		let watcher = LiveWorkspaceWatcher::start(self.registry.watch_roots(), move |event| {
			if let Some(events) = &events {
				let _ = events.send(event_dto(&event));
			}
			let _ = tx.send(event);
		})?;
		self.live.watcher = Some(watcher);
		Ok(())
	}
}

fn event_dto(event: &WorkspaceLiveEvent) -> WorkspaceEventDto {
	let kind = match event {
		WorkspaceLiveEvent::Notes => WorkspaceEventKind::Notes,
		WorkspaceLiveEvent::GitBaseChanged => WorkspaceEventKind::GitBase,
		_ => WorkspaceEventKind::Stale,
	};
	WorkspaceEventDto {
		kind,
		generation: None,
		stale_summary: None,
	}
}

impl WorkspaceDaemonInit {
	fn new(config: DaemonWorkspaceConfig) -> anyhow::Result<Self> {
		let config = canonical_workspace_config(config)?;
		let roots = config_roots(&config);
		let (registry, cache) = daemon_registry(&config, &roots);
		Ok(Self {
			config_root: rules_config_root(&roots)?,
			registry,
			cache,
			live: DaemonLiveState::new(DaemonLiveRefreshPolicy::parse(
				config.live_refresh.as_deref(),
			)?),
			roots,
		})
	}
}

impl DaemonLiveState {
	fn new(policy: DaemonLiveRefreshPolicy) -> Self {
		let (tx, rx) = mpsc::channel();
		Self {
			policy,
			tx,
			rx,
			watcher: None,
			events: None,
		}
	}
}

impl DaemonLiveRefreshPolicy {
	fn parse(value: Option<&str>) -> anyhow::Result<Self> {
		match value.unwrap_or("on-demand") {
			"on-demand" => Ok(Self::OnDemand),
			"auto" => Ok(Self::Auto),
			other => anyhow::bail!("unknown daemon live refresh policy `{other}`"),
		}
	}
}

fn daemon_registry(
	config: &DaemonWorkspaceConfig,
	roots: &[PathBuf],
) -> (LocalWorkspaceRegistry, LocalResourceCache) {
	let cache = LocalResourceCache::default();
	let registry = LocalWorkspaceRegistry::local_with_cache(
		LocalWorkspaceOptions::new(roots.to_vec(), config.project.clone())
			.with_cache_dir(config.cache_dir.as_ref().map(PathBuf::from)),
		cache.clone(),
	);
	(registry, cache)
}

fn handle_protocol(daemon: &mut WorkspaceDaemon, request: ProtocolRequest) -> ProtocolResponse {
	match request {
		ProtocolRequest::Query(request) => match handle_query(daemon, *request) {
			Ok(response) => ProtocolResponse::Query(Box::new(response)),
			Err(error) => ProtocolResponse::Error(error),
		},
		ProtocolRequest::Command(request) => match handle_command(daemon, request) {
			Ok(response) => ProtocolResponse::Command(response),
			Err(error) => ProtocolResponse::Error(error),
		},
	}
}

fn handle_command(
	daemon: &mut WorkspaceDaemon,
	request: CommandRequest,
) -> Result<CommandResponse, QueryError> {
	drain_live_events(daemon)?;
	match request.command {
		Command::WorkspaceRefresh => daemon.refresh_cancellable(WorkspaceCancellation::default()),
		Command::WorkspaceSourceSetReplace { source_set } => {
			let source_set = parse_memory_source_set(source_set)?;
			validate_memory_source_set_limits(&daemon.cache, &source_set, MEMORY_SOURCE_LIMITS)?;
			let srcset = source_set.srcset.clone();
			let update = daemon.cache.replace_memory_source_set(source_set);
			refresh_memory_source_set(daemon, update, format!("source set `{srcset}` replaced"))
		}
		Command::WorkspaceSourceSetRemove { srcset } => {
			validate_srcset(&srcset)?;
			let update = daemon.cache.remove_memory_source_set(&srcset);
			refresh_memory_source_set(daemon, update, format!("source set `{srcset}` removed"))
		}
	}
}

fn parse_memory_source_set(dto: WorkspaceSourceSetDto) -> Result<MemorySourceSet, QueryError> {
	validate_srcset(&dto.srcset)?;
	let mut seen = HashSet::new();
	let mut documents = Vec::with_capacity(dto.documents.len());
	for document in dto.documents {
		validate_memory_source_uri(&document.uri)?;
		if !seen.insert(document.uri.clone()) {
			return Err(QueryError::new(
				"duplicate_workspace_source_uri",
				format!(
					"source set `{}` contains duplicate URI `{}`",
					dto.srcset, document.uri
				),
			));
		}
		let lang = Lang::from_tag(&document.language).ok_or_else(|| {
			QueryError::new(
				"unsupported_workspace_source_language",
				format!(
					"unsupported language `{}` for `{}`; expected one of: {}",
					document.language,
					document.uri,
					Lang::ALL
						.iter()
						.map(|lang| lang.tag())
						.collect::<Vec<_>>()
						.join(", ")
				),
			)
		})?;
		documents.push(MemorySourceDocument {
			uri: document.uri,
			lang,
			content: document.content,
		});
	}
	documents.sort_by(|left, right| left.uri.cmp(&right.uri));
	Ok(MemorySourceSet {
		srcset: dto.srcset,
		revision: dto.revision,
		documents,
	})
}

fn validate_memory_source_set_limits(
	cache: &LocalResourceCache,
	source_set: &MemorySourceSet,
	limits: MemorySourceLimits,
) -> Result<(), QueryError> {
	if source_set.documents.len() > limits.max_documents_per_set {
		return Err(memory_source_limit_error(format!(
			"source set `{}` contains {} documents; the limit is {}",
			source_set.srcset,
			source_set.documents.len(),
			limits.max_documents_per_set
		)));
	}
	for document in &source_set.documents {
		if document.uri.len() > limits.max_uri_bytes {
			return Err(memory_source_limit_error(format!(
				"document URI in source set `{}` uses {} bytes; the limit is {}",
				source_set.srcset,
				document.uri.len(),
				limits.max_uri_bytes
			)));
		}
		if document.content.len() > limits.max_document_bytes {
			return Err(memory_source_limit_error(format!(
				"document `{}` uses {} content bytes; the limit is {}",
				document.uri,
				document.content.len(),
				limits.max_document_bytes
			)));
		}
	}
	let source_set_bytes = source_set.size_bytes();
	if source_set_bytes > limits.max_source_set_bytes {
		return Err(memory_source_limit_error(format!(
			"source set `{}` uses {source_set_bytes} bytes; the limit is {}",
			source_set.srcset, limits.max_source_set_bytes
		)));
	}
	let (active_sets, _active_documents, active_bytes) =
		cache.memory_source_usage_after_replacing(source_set);
	if active_sets > limits.max_source_sets {
		return Err(memory_source_limit_error(format!(
			"the replacement would keep {active_sets} active source sets; the limit is {}",
			limits.max_source_sets
		)));
	}
	if active_bytes > limits.max_total_bytes {
		return Err(memory_source_limit_error(format!(
			"the replacement would keep {active_bytes} bytes of active source text; the limit is {}",
			limits.max_total_bytes
		)));
	}
	Ok(())
}

fn memory_source_limit_error(message: String) -> QueryError {
	QueryError::new("workspace_source_set_limit_exceeded", message)
}

fn validate_srcset(srcset: &str) -> Result<(), QueryError> {
	let valid = !srcset.is_empty()
		&& srcset.len() <= 128
		&& !matches!(srcset, "." | "..")
		&& srcset
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
	if valid {
		return Ok(());
	}
	Err(QueryError::new(
		"invalid_workspace_srcset",
		"srcset must contain 1 to 128 ASCII letters, digits, dots, dashes, or underscores, and cannot be `.` or `..`",
	))
}

fn validate_memory_source_uri(uri: &str) -> Result<(), QueryError> {
	if !uri.is_empty() && !uri.contains('\0') {
		return Ok(());
	}
	Err(QueryError::new(
		"invalid_workspace_source_uri",
		"workspace source URI must be non-empty and contain no NUL byte",
	))
}

fn refresh_memory_source_set(
	daemon: &mut WorkspaceDaemon,
	update: MemorySourceSetUpdate,
	message: String,
) -> Result<CommandResponse, QueryError> {
	if !update.changed {
		return Ok(CommandResponse {
			generation: generation(&daemon.registry),
			message: format!("{message}: unchanged"),
			status: Some(Box::new(workspace_status_result(
				&daemon.roots,
				&daemon.registry,
			))),
		});
	}
	let MemorySourceSetUpdate {
		paths,
		srcset,
		previous,
		..
	} = update;
	let transition = if daemon.registry.queries().snapshot().is_some() {
		daemon
			.registry
			.commands()
			.refresh_paths(WorkspaceRequest::new("daemon-memory-source-set"), paths)
	} else {
		daemon
			.registry
			.commands()
			.refresh(WorkspaceRequest::new("daemon-memory-source-set"))
	};
	if let Err(error) = workspace_transition_result(transition) {
		daemon.cache.restore_memory_source_set(srcset, previous);
		return Err(error);
	}
	let generation = generation(&daemon.registry);
	if let Some(events) = &daemon.live.events {
		let _ = events.send(WorkspaceEventDto {
			kind: WorkspaceEventKind::Refreshed,
			generation,
			stale_summary: None,
		});
	}
	Ok(CommandResponse {
		generation,
		message,
		status: Some(Box::new(workspace_status_result(
			&daemon.roots,
			&daemon.registry,
		))),
	})
}

fn handle_query(
	daemon: &mut WorkspaceDaemon,
	request: QueryRequest,
) -> Result<QueryResponse, QueryError> {
	if let Query::SyntaxParse(query) = &request.query {
		return syntax::syntax_parse_response(query.clone());
	}
	drain_live_events(daemon)?;
	if let Query::QueryDescribe(query) = &request.query {
		return query_describe_response(query.verb.as_deref());
	}
	if matches!(&request.query, Query::WorkspaceStatus) {
		return workspace_status(&daemon.roots, &daemon.registry);
	}
	if request.consistency == Consistency::RefreshIfStale
		&& daemon.registry.queries().staleness().is_stale()
	{
		refresh_stale(daemon)?;
	}
	if daemon.registry.queries().snapshot().is_none() {
		return Err(QueryError::new(
			"workspace_loading",
			"workspace snapshot is still loading; retry after workspace.status reports phase ready",
		));
	}
	if request.consistency == Consistency::Current
		&& daemon.registry.queries().staleness().is_stale()
	{
		return Err(QueryError::new(
			"workspace_stale",
			"workspace is stale; request consistency refresh-if-stale or stale-ok",
		));
	}
	let snapshot =
		daemon.registry.queries().snapshot_arc().ok_or_else(|| {
			QueryError::new("workspace_loading", "workspace snapshot is not ready")
		})?;
	let current_generation = Some(WorkspaceGeneration(snapshot.generation.value()));
	let response_roots = daemon.roots.clone();
	let response_config_root = daemon.config_root.clone();
	let response = ResponseContext {
		roots: &response_roots,
		config_root: &response_config_root,
		generation: current_generation,
	};
	dispatch_loaded_query(daemon, snapshot, response, request)
}

fn concurrent_snapshot_query(query: &Query) -> bool {
	query.requires_workspace_snapshot()
		&& !matches!(query, Query::ChangeContext(_) | Query::Notes(_))
}

fn handle_stale_snapshot_query(
	published: Option<PublishedSnapshot>,
	request: QueryRequest,
) -> ProtocolResponse {
	let published = match published {
		Some(published) => published,
		None => {
			return ProtocolResponse::Error(QueryError::new(
				"workspace_loading",
				"workspace snapshot is still loading; retry after workspace.status reports phase ready",
			));
		}
	};
	let generation = Some(WorkspaceGeneration(published.snapshot.generation.value()));
	let response = ResponseContext {
		roots: &published.context.roots,
		config_root: &published.context.config_root,
		generation,
	};
	let QueryRequest { query, page, .. } = request;
	dispatch_snapshot_query(
		&published.context,
		published.snapshot,
		response,
		query,
		page,
	)
	.map_or_else(ProtocolResponse::Error, |response| {
		ProtocolResponse::Query(Box::new(response))
	})
}

fn dispatch_loaded_query(
	daemon: &mut WorkspaceDaemon,
	snapshot: Arc<WorkspaceSnapshot>,
	response: ResponseContext<'_>,
	request: QueryRequest,
) -> Result<QueryResponse, QueryError> {
	let QueryRequest { query, page, .. } = request;
	match query {
		Query::ChangeContext(query) => change_context_response(daemon, &snapshot, response, query),
		Query::Notes(query) => notes_response(daemon, &snapshot, query, page, response.generation),
		query => {
			let context = SnapshotQueryContext {
				roots: Arc::from(daemon.roots.clone()),
				config_root: Arc::new(daemon.config_root.clone()),
				cache: daemon.cache.clone(),
			};
			dispatch_snapshot_query(&context, snapshot, response, query, page)
		}
	}
}

fn dispatch_snapshot_query(
	context: &SnapshotQueryContext,
	snapshot: Arc<WorkspaceSnapshot>,
	response: ResponseContext<'_>,
	query: Query,
	page: Page,
) -> Result<QueryResponse, QueryError> {
	let current_generation = response.generation;
	match query {
		Query::QueryDescribe(_) => unreachable!("query describe handled before snapshot load"),
		Query::WorkspaceStatus => unreachable!("workspace status handled before snapshot load"),
		Query::SyntaxParse(_) => {
			unreachable!("stateless syntax parse handled before snapshot load")
		}
		Query::TreeChildren(query) => {
			tree_children_response(&snapshot, &context.roots, query, page, current_generation)
		}
		Query::SymbolSearch(query) => {
			symbol_search_response(&snapshot, &context.roots, query, page, current_generation)
		}
		Query::SymbolInsights(query) => {
			symbol_insights_response(&snapshot, &context.roots, query, current_generation)
		}
		Query::SymbolDetail(query) => symbol_detail_response(
			&snapshot,
			&context.roots,
			query.workspace.as_deref(),
			&query.uri,
			query.context_lines,
			current_generation,
		),
		Query::SyntaxTree(query) => {
			syntax::syntax_tree_response(&snapshot, &context.roots, query, current_generation)
		}
		Query::SymbolUsages(query) => {
			symbol_usages_response(&snapshot, &context.roots, query, page, current_generation)
		}
		Query::ViewRead(query) => {
			view_read_response(&snapshot, &context.roots, query, current_generation)
		}
		Query::RulesList(query) => rules_list_response(
			&snapshot,
			response,
			RulesListEval {
				workspace: query.workspace,
				profile: query.profile,
				rules: query.rules,
				filters: RulesListFilters {
					langs: query.lang,
					severities: query.severity,
				},
				page,
			},
		),
		Query::RulesCheck(query) => rules_check_response(
			&context.cache,
			Arc::clone(&snapshot),
			response,
			RulesCheckEval {
				workspace: query.workspace,
				profile: query.profile,
				rules: query.rules,
				files: query.file,
				report: query.report,
				page,
			},
		),
		Query::RulesApplicable(query) => {
			rules_applicable_response(&snapshot, response, query, page)
		}
		Query::ChangeReview(query) => {
			change_review_response(&snapshot, &context.roots, query, current_generation)
		}
		Query::ChangeContext(_) => {
			unreachable!("change context is dispatched with exclusive workspace access")
		}
		Query::SymbolGraph(query) => {
			symbol_graph_response(&snapshot, &context.roots, query, current_generation)
		}
		Query::GraphPath(query) => {
			graph_path_response(&snapshot, &context.roots, query, current_generation)
		}
		Query::IdentityChildren(query) => {
			identity_children_response(&snapshot, &context.roots, query, current_generation)
		}
		Query::IdentityGraph(query) => {
			identity_graph_response(&snapshot, &context.roots, query, page, current_generation)
		}
		Query::ResolutionAudit(query) => {
			resolution_audit_response(&snapshot, &context.roots, query, page, current_generation)
		}
		Query::Notes(_) => {
			unreachable!("notes are dispatched with exclusive workspace access")
		}
	}
}

fn query_describe_response(verb: Option<&str>) -> Result<QueryResponse, QueryError> {
	let result = describe_query_capabilities(verb).ok_or_else(|| {
		QueryError::new(
			"unknown_query",
			format!("unknown query operation `{}`", verb.unwrap_or_default()),
		)
	})?;
	Ok(QueryResponse {
		generation: None,
		result: QueryResult::QueryDescribe(result),
		next_cursor: None,
	})
}

enum UnitBoundary {
	IdentityPrefix { prefix: String, slot: usize },
	File { source: SourceId, slot: usize },
}

impl UnitBoundary {
	fn slot(&self) -> usize {
		match self {
			Self::IdentityPrefix { slot, .. } | Self::File { slot, .. } => *slot,
		}
	}

	fn contains(&self, symbol: &SymbolRecord) -> bool {
		match self {
			Self::IdentityPrefix { prefix, .. } => {
				let identity = symbol.identity.as_ref();
				identity == prefix
					|| (identity.len() > prefix.len()
						&& identity.starts_with(prefix.as_str())
						&& identity.as_bytes()[prefix.len()] == b'/')
			}
			Self::File { source, .. } => &symbol.source == source,
		}
	}
}

struct NeighborBag {
	entries: BTreeMap<SymbolId, (BTreeSet<String>, usize)>,
}

impl NeighborBag {
	fn new() -> Self {
		Self {
			entries: BTreeMap::new(),
		}
	}

	fn add(&mut self, symbol: SymbolId, kind: &str) {
		let entry = self.entries.entry(symbol).or_default();
		entry.0.insert(kind.to_string());
		entry.1 += 1;
	}

	fn into_neighbors(
		self,
		snapshot: &WorkspaceSnapshot,
		roots: &[PathBuf],
	) -> Vec<SymbolGraphNeighbor> {
		let symbols = WorkspaceView::new(snapshot).symbols();
		let sources = WorkspaceView::new(snapshot).sources();
		let mut neighbors: Vec<SymbolGraphNeighbor> = self
			.entries
			.into_iter()
			.filter_map(|(id, (kinds, count))| {
				let symbol = symbols.find(&id)?;
				let source = sources.record(&symbol.source)?;
				Some(SymbolGraphNeighbor {
					symbol: symbol_dto(symbol, source, roots),
					kinds: kinds.into_iter().collect(),
					count,
				})
			})
			.collect();
		neighbors.sort_by(|a, b| {
			(&a.symbol.file, a.symbol.line_range).cmp(&(&b.symbol.file, b.symbol.line_range))
		});
		neighbors
	}
}

fn symbol_graph_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: SymbolGraphQuery,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let _ = selected_roots(roots, query.workspace.as_deref())?;
	let (boundary, focus) = resolve_unit_boundary(snapshot, roots, &query.focus)?;
	let symbols_view = WorkspaceView::new(snapshot).symbols();
	let sources_view = WorkspaceView::new(snapshot).sources();
	let slot = boundary.slot();
	let members: Vec<&SymbolRecord> = snapshot
		.index
		.symbols
		.file_records(slot)
		.iter()
		.filter(|symbol| symbol.navigable && boundary.contains(symbol))
		.collect();
	let mut internal: BTreeMap<(SymbolId, SymbolId), (BTreeSet<String>, usize)> = BTreeMap::new();
	let mut callees = NeighborBag::new();
	let mut callers = NeighborBag::new();
	let classifier = UnlinkedClassifier::new(snapshot);
	let mut unlinked = UnlinkedRefsDto::default();
	for reference in snapshot.index.references.file_records(slot).iter() {
		let Some(source) = navigable_anchor(&symbols_view, reference.source_symbol) else {
			continue;
		};
		if !boundary.contains(source) {
			continue;
		}
		let Some(target_id) = resolved_reference_target(snapshot, &reference.id) else {
			classifier.tally(&reference.id, &mut unlinked);
			continue;
		};
		let Some(target) = navigable_anchor(&symbols_view, target_id) else {
			continue;
		};
		let kind = reference.kind.as_str();
		if boundary.contains(target) {
			let entry = internal.entry((source.id, target.id)).or_default();
			entry.0.insert(kind.to_string());
			entry.1 += 1;
		} else {
			callees.add(target.id, kind);
		}
	}
	for member in &members {
		for reference_id in incoming_reference_ids(snapshot, &member.id) {
			let Some(reference) = WorkspaceView::new(snapshot)
				.references()
				.reference(&reference_id)
			else {
				continue;
			};
			let Some(source) = navigable_anchor(&symbols_view, reference.source_symbol) else {
				continue;
			};
			if boundary.contains(source) {
				continue;
			}
			callers.add(source.id, reference.kind.as_str());
		}
	}
	let mut member_dtos: Vec<SymbolDto> = members
		.iter()
		.filter_map(|symbol| {
			let source = sources_view.record(&symbol.source)?;
			Some(symbol_dto(symbol, source, roots))
		})
		.collect();
	member_dtos.sort_by_key(|dto| dto.line_range);
	let graph = filter_symbol_graph_sections(
		internal,
		callers.into_neighbors(snapshot, roots),
		callees.into_neighbors(snapshot, roots),
		&query,
		member_dtos.len(),
	);
	let result = SymbolGraphResult {
		focus,
		coverage: graph.coverage,
		members: member_dtos,
		internal_edges: graph.internal_edges,
		callers: graph.callers,
		callees: graph.callees,
		unlinked,
	};
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::SymbolGraph(Box::new(result)),
		next_cursor: None,
	})
}

struct FilteredSymbolGraph {
	internal_edges: Vec<SymbolGraphEdge>,
	callers: Vec<SymbolGraphNeighbor>,
	callees: Vec<SymbolGraphNeighbor>,
	coverage: SymbolGraphCoverage,
}

fn filter_symbol_graph_sections(
	internal: BTreeMap<(SymbolId, SymbolId), (BTreeSet<String>, usize)>,
	callers: Vec<SymbolGraphNeighbor>,
	callees: Vec<SymbolGraphNeighbor>,
	query: &SymbolGraphQuery,
	member_count: usize,
) -> FilteredSymbolGraph {
	let relation_matches = |kinds: &[String]| {
		query.relation.is_empty()
			|| kinds
				.iter()
				.any(|kind| query.relation.iter().any(|expected| expected == kind))
	};
	let internal_edges = internal
		.into_iter()
		.map(|((source, target), (kinds, count))| SymbolGraphEdge {
			source: source.to_string(),
			target: target.to_string(),
			kinds: kinds.into_iter().collect(),
			count,
		})
		.collect::<Vec<_>>();
	let internal_edges_total = internal_edges.len();
	let mut internal_edges = internal_edges
		.into_iter()
		.filter(|edge| edge.count >= query.min_count && relation_matches(&edge.kinds))
		.collect::<Vec<_>>();
	let internal_edges_matching = internal_edges.len();
	if !query.include_internal {
		internal_edges.clear();
	}
	let filter_neighbors = |neighbors: Vec<SymbolGraphNeighbor>| {
		neighbors
			.into_iter()
			.filter(|neighbor| {
				neighbor.count >= query.min_count && relation_matches(&neighbor.kinds)
			})
			.collect::<Vec<_>>()
	};
	let callers_total = callers.len();
	let callees_total = callees.len();
	let mut callers = filter_neighbors(callers);
	let mut callees = filter_neighbors(callees);
	let callers_matching = callers.len();
	let callees_matching = callees.len();
	match query.direction {
		UsageDirection::Incoming => callees.clear(),
		UsageDirection::Outgoing => callers.clear(),
		UsageDirection::Both => {}
	}
	FilteredSymbolGraph {
		coverage: SymbolGraphCoverage {
			members: GraphSectionCoverage {
				total: member_count,
				matching: member_count,
				returned: member_count,
			},
			internal_edges: GraphSectionCoverage {
				total: internal_edges_total,
				matching: internal_edges_matching,
				returned: internal_edges.len(),
			},
			callers: GraphSectionCoverage {
				total: callers_total,
				matching: callers_matching,
				returned: callers.len(),
			},
			callees: GraphSectionCoverage {
				total: callees_total,
				matching: callees_matching,
				returned: callees.len(),
			},
		},
		internal_edges,
		callers,
		callees,
	}
}

fn graph_path_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: GraphPathQuery,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, query.workspace.as_deref())?;
	let from = find_symbol(snapshot, &query.from)?;
	let to = find_symbol(snapshot, &query.to)?;
	let view = WorkspaceView::new(snapshot);
	let sources = view.sources();
	let from_source = sources
		.record(&from.source)
		.ok_or_else(|| QueryError::new("source_not_found", "source symbol source not found"))?;
	let to_source = sources
		.record(&to.source)
		.ok_or_else(|| QueryError::new("source_not_found", "target symbol source not found"))?;
	for (uri, source) in [(&query.from, from_source), (&query.to, to_source)] {
		if source_root(roots, &selected_roots, source).is_none() {
			return Err(QueryError::new(
				"symbol_not_in_workspace",
				format!("symbol {uri} is not in the selected workspace"),
			));
		}
	}
	let path_scope = BoundedPathScope::from_sources(
		snapshot
			.index
			.sources
			.iter()
			.filter(|source| source_root(roots, &selected_roots, source).is_some())
			.map(|source| source.id),
	);
	let search = snapshot
		.bounded_path(
			from.id,
			to.id,
			&query.relation,
			BoundedPathLimits {
				max_depth: query.max_depth,
				max_symbols: query.max_symbols,
				max_edges: query.max_edges,
			},
			&path_scope,
		)
		.ok_or_else(|| {
			QueryError::new(
				"path_index_unavailable",
				"the linkage snapshot has no symbol ordinal index; refresh the workspace",
			)
		})?;
	let found = from.id == to.id || !search.path.is_empty();
	let coverage_percent = search.coverage.percent();
	let complete = !search.depth_limit_reached
		&& !search.symbol_limit_reached
		&& !search.edge_limit_reached
		&& coverage_percent >= query.min_coverage;
	let (reachable, no_path, verdict) = graph_path_truth(found, complete, query.expect);
	let mut reasons = Vec::new();
	if search.depth_limit_reached {
		reasons.push("depth_limit".to_string());
	}
	if search.symbol_limit_reached {
		reasons.push("symbol_limit".to_string());
	}
	if search.edge_limit_reached {
		reasons.push("edge_limit".to_string());
	}
	if coverage_percent < query.min_coverage {
		reasons.push("coverage_below_threshold".to_string());
	}
	for (reason, count) in &search.coverage.gap_reasons {
		push_path_gap_reason(&mut reasons, reason, *count);
	}
	let path = search
		.path
		.iter()
		.map(|edge| graph_path_step(snapshot, roots, edge))
		.collect::<Result<Vec<_>, _>>()?;
	let result = GraphPathResult {
		from: symbol_dto(from, from_source, roots),
		to: symbol_dto(to, to_source, roots),
		expectation: query.expect,
		verdict,
		reachable,
		no_path,
		path,
		coverage: GraphPathCoverage {
			total: search.coverage.total,
			decided: search.coverage.decided,
			resolved: search.coverage.resolved,
			external: search.coverage.external,
			candidate: search.coverage.candidate,
			dynamic: search.coverage.dynamic,
			manifest_blocked: search.coverage.manifest_blocked,
			unresolved: search.coverage.unresolved,
			percent: coverage_percent,
			gap_reasons: search.coverage.gap_reasons,
		},
		search: GraphPathSearchStats {
			max_depth: query.max_depth,
			depth_reached: search.depth_reached,
			explored_symbols: search.explored_symbols,
			explored_edges: search.explored_edges,
			depth_limit_reached: search.depth_limit_reached,
			symbol_limit_reached: search.symbol_limit_reached,
			edge_limit_reached: search.edge_limit_reached,
		},
		reasons,
	};
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::GraphPath(Box::new(result)),
		next_cursor: None,
	})
}

fn graph_path_truth(
	found: bool,
	complete: bool,
	expectation: GraphPathExpectation,
) -> (Option<bool>, Option<bool>, GraphPathVerdict) {
	if found {
		return (
			Some(true),
			Some(false),
			if expectation == GraphPathExpectation::Reachable {
				GraphPathVerdict::Pass
			} else {
				GraphPathVerdict::Fail
			},
		);
	}
	if !complete {
		return (None, None, GraphPathVerdict::Inconclusive);
	}
	(
		Some(false),
		Some(true),
		if expectation == GraphPathExpectation::NoPath {
			GraphPathVerdict::Pass
		} else {
			GraphPathVerdict::Fail
		},
	)
}

fn push_path_gap_reason(reasons: &mut Vec<String>, reason: &str, count: usize) {
	if count > 0 {
		reasons.push(format!("{reason}:{count}"));
	}
}

fn graph_path_step(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	edge: &code_moniker_workspace::snapshot::BoundedPathEdge,
) -> Result<GraphPathStep, QueryError> {
	let view = WorkspaceView::new(snapshot);
	let symbols = view.symbols();
	let sources = view.sources();
	let references = view.references();
	let source_symbol = symbols
		.find(&edge.source)
		.ok_or_else(|| QueryError::new("symbol_not_found", "path source symbol not found"))?;
	let target_symbol = symbols
		.find(&edge.target)
		.ok_or_else(|| QueryError::new("symbol_not_found", "path target symbol not found"))?;
	let reference = references
		.reference(&edge.reference)
		.ok_or_else(|| QueryError::new("reference_not_found", "path reference not found"))?;
	let source = sources
		.record(&reference.source)
		.ok_or_else(|| QueryError::new("source_not_found", "path reference source not found"))?;
	let source_symbol_source = sources
		.record(&source_symbol.source)
		.ok_or_else(|| QueryError::new("source_not_found", "path source source not found"))?;
	let target_symbol_source = sources
		.record(&target_symbol.source)
		.ok_or_else(|| QueryError::new("source_not_found", "path target source not found"))?;
	Ok(GraphPathStep {
		source: symbol_dto(source_symbol, source_symbol_source, roots),
		target: symbol_dto(target_symbol, target_symbol_source, roots),
		relation: reference.kind.clone(),
		reference: reference.id.to_string(),
		file: source.rel_path.clone(),
		line_range: reference.line_range,
	})
}

struct SegmentAgg<'a> {
	defs: usize,
	grandchildren: bool,
	direct: Option<&'a SymbolRecord>,
}

// One level of the identity tree: group every navigable definition under the
// prefix by its next identity segment. Segments that are definitions attach
// their SymbolDto; organizational segments only aggregate.
fn identity_children_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: IdentityChildrenQuery,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let _ = selected_roots(roots, query.workspace.as_deref())?;
	let prefix = identity_path(query.prefix.trim_matches('/')).trim_matches('/');
	let children = identity_segments(snapshot, roots, prefix);
	if children.is_empty() {
		require_known_identity_prefix(snapshot, roots, prefix)?;
	}
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::IdentityChildren(IdentityChildrenResult {
			prefix: prefix.to_string(),
			children,
		}),
		next_cursor: None,
	})
}

fn require_known_identity_prefix(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	prefix: &str,
) -> Result<(), QueryError> {
	if prefix.is_empty() || identity_prefix_exists(snapshot, prefix) {
		return Ok(());
	}
	let heads = identity_segments(snapshot, roots, "")
		.into_iter()
		.map(|segment| segment.segment)
		.take(8)
		.collect::<Vec<_>>();
	let guidance = if heads.is_empty() {
		"the workspace has no indexed symbols".to_string()
	} else {
		format!(
			"a prefix is a head sequence of canonical identity segments; valid heads: {}",
			heads.join(", ")
		)
	};
	Err(QueryError::new(
		"prefix_not_found",
		format!("no symbol identity starts with `{prefix}`; {guidance}"),
	))
}

fn directory_or_unknown_focus_error(snapshot: &WorkspaceSnapshot, focus: &str) -> QueryError {
	let dir_prefix = format!("{}/", focus.trim_end_matches('/'));
	let is_directory = snapshot
		.index
		.sources
		.iter()
		.any(|source| source.rel_path.starts_with(&dir_prefix));
	if is_directory {
		return QueryError::new(
			"focus_is_directory",
			format!(
				"focus `{focus}` is a directory; the unit graph takes a symbol URI or a \
				 file path - for scope-level coupling use identity.graph (list valid \
				 heads with identity.children prefix:\"\"), or pick a file via \
				 symbol.search path:\"{focus}/**\""
			),
		);
	}
	QueryError::new(
		"focus_not_found",
		format!("no symbol or file matches focus `{focus}`"),
	)
}

fn identity_prefix_exists(snapshot: &WorkspaceSnapshot, prefix: &str) -> bool {
	snapshot
		.index
		.symbols
		.iter()
		.filter(|symbol| symbol.navigable)
		.any(|symbol| {
			let identity = identity_path(symbol.identity.as_ref());
			identity == prefix || identity_rest(identity, prefix).is_some()
		})
}

fn identity_segments(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	prefix: &str,
) -> Vec<IdentitySegmentDto> {
	identity_segments_scoped(snapshot, roots, prefix, &FilePathFilter::default())
}

fn identity_segments_scoped(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	prefix: &str,
	path_filter: &FilePathFilter,
) -> Vec<IdentitySegmentDto> {
	let sources_view = WorkspaceView::new(snapshot).sources();
	let mut groups: BTreeMap<&str, SegmentAgg> = BTreeMap::new();
	for symbol in snapshot.index.symbols.iter() {
		if !symbol.navigable {
			continue;
		}
		let Some(source) = sources_view.record(&symbol.source) else {
			continue;
		};
		if !path_filter.matches(&source.rel_path) {
			continue;
		}
		let Some(rest) = identity_rest(identity_path(symbol.identity.as_ref()), prefix) else {
			continue;
		};
		let (segment, tail) = match rest.split_once('/') {
			Some((segment, tail)) => (segment, Some(tail)),
			None => (rest, None),
		};
		if segment.is_empty() {
			continue;
		}
		let entry = groups.entry(segment).or_insert(SegmentAgg {
			defs: 0,
			grandchildren: false,
			direct: None,
		});
		match tail {
			None => entry.direct = Some(symbol),
			Some(_) => {
				entry.defs += 1;
				entry.grandchildren = true;
			}
		}
	}
	groups
		.into_iter()
		.map(|(segment, agg)| identity_segment_dto(segment, agg, prefix, &sources_view, roots))
		.collect()
}

// The scoped exploration graph: the prefix's children as nodes, every
// resolved reference rolled up to the pair of child segments it connects,
// and boundary crossings aggregated into ports at the scope's own depth.
fn identity_graph_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: IdentityGraphQuery,
	page: Page,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let _ = selected_roots(roots, query.workspace.as_deref())?;
	let path_filter = FilePathFilter::compile(&query.path)
		.map_err(|err| QueryError::new("invalid_path_filter", err.to_string()))?;
	let prefix = identity_path(query.prefix.trim_matches('/'))
		.trim_matches('/')
		.to_string();
	let nodes = identity_segments_scoped(snapshot, roots, &prefix, &path_filter);
	if nodes.is_empty() {
		require_known_identity_prefix(snapshot, roots, &prefix)?;
	}
	let symbols_view = WorkspaceView::new(snapshot).symbols();
	let sources_view = WorkspaceView::new(snapshot).sources();
	let mut edges: BTreeMap<(String, String), (BTreeSet<String>, usize)> = BTreeMap::new();
	let mut ports_in: BTreeMap<String, (BTreeSet<String>, usize)> = BTreeMap::new();
	let mut ports_out: BTreeMap<String, (BTreeSet<String>, usize)> = BTreeMap::new();
	let classifier = UnlinkedClassifier::new(snapshot);
	let mut unlinked = UnlinkedRefsDto::default();
	let port_depth = if prefix.is_empty() {
		1
	} else {
		prefix.split('/').count() + 1
	};
	for reference in snapshot.index.references.iter() {
		let Some(source) = navigable_anchor(&symbols_view, reference.source_symbol) else {
			continue;
		};
		let source_selected = sources_view
			.record(&source.source)
			.is_some_and(|record| path_filter.matches(&record.rel_path));
		let source_segment = source_selected
			.then(|| scope_segment(source, &prefix))
			.flatten();
		let Some(target_id) = resolved_reference_target(snapshot, &reference.id) else {
			if source_segment.is_some() {
				classifier.tally(&reference.id, &mut unlinked);
			}
			continue;
		};
		let Some(target) = navigable_anchor(&symbols_view, target_id) else {
			continue;
		};
		let target_selected = sources_view
			.record(&target.source)
			.is_some_and(|record| path_filter.matches(&record.rel_path));
		let kind = reference.kind.as_str();
		let target_segment = target_selected
			.then(|| scope_segment(target, &prefix))
			.flatten();
		match (source_segment, target_segment) {
			(Some(from), Some(to)) => {
				if from != to {
					let entry = edges.entry((from, to)).or_default();
					entry.0.insert(kind.to_string());
					entry.1 += 1;
				}
			}
			(Some(_), None) => bump_port(
				&mut ports_out,
				truncate_identity(identity_path(target.identity.as_ref()), port_depth),
				kind,
			),
			(None, Some(_)) => bump_port(
				&mut ports_in,
				truncate_identity(identity_path(source.identity.as_ref()), port_depth),
				kind,
			),
			(None, None) => {}
		}
	}
	let edges = edges
		.into_iter()
		.map(|((source, target), (kinds, count))| IdentityGraphEdge {
			source,
			target,
			kinds: kinds.into_iter().collect(),
			count,
		})
		.collect::<Vec<_>>();
	let ports_in = into_ports(ports_in);
	let ports_out = into_ports(ports_out);
	let graph_page = page_identity_graph(
		IdentityGraphSections {
			nodes,
			edges,
			ports_in,
			ports_out,
		},
		query.min_count,
		page,
		current_generation,
	)?;
	let result = IdentityGraphResult {
		prefix,
		path: query.path,
		min_count: query.min_count,
		coverage: graph_page.coverage,
		nodes: graph_page.nodes,
		edges: graph_page.edges,
		ports_in: graph_page.ports_in,
		ports_out: graph_page.ports_out,
		unlinked,
	};
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::IdentityGraph(Box::new(result)),
		next_cursor: graph_page.next_cursor,
	})
}

struct IdentityGraphPage {
	nodes: Vec<IdentitySegmentDto>,
	edges: Vec<IdentityGraphEdge>,
	ports_in: Vec<IdentityGraphPort>,
	ports_out: Vec<IdentityGraphPort>,
	coverage: IdentityGraphCoverage,
	next_cursor: Option<QueryCursor>,
}

struct IdentityGraphSections {
	nodes: Vec<IdentitySegmentDto>,
	edges: Vec<IdentityGraphEdge>,
	ports_in: Vec<IdentityGraphPort>,
	ports_out: Vec<IdentityGraphPort>,
}

fn page_identity_graph(
	sections: IdentityGraphSections,
	min_count: usize,
	page: Page,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<IdentityGraphPage, QueryError> {
	let IdentityGraphSections {
		nodes,
		edges,
		ports_in,
		ports_out,
	} = sections;
	let nodes_total = nodes.len();
	let edges_total = edges.len();
	let ports_in_total = ports_in.len();
	let ports_out_total = ports_out.len();
	let edges = edges
		.into_iter()
		.filter(|edge| edge.count >= min_count)
		.collect::<Vec<_>>();
	let ports_in = ports_in
		.into_iter()
		.filter(|port| port.count >= min_count)
		.collect::<Vec<_>>();
	let ports_out = ports_out
		.into_iter()
		.filter(|port| port.count >= min_count)
		.collect::<Vec<_>>();
	let edges_matching = edges.len();
	let ports_in_matching = ports_in.len();
	let ports_out_matching = ports_out.len();
	let rows_total = nodes_total + edges_total + ports_in_total + ports_out_total;
	let rows_matching = nodes_total + edges_matching + ports_in_matching + ports_out_matching;
	let rows = nodes
		.into_iter()
		.map(IdentityGraphRow::Node)
		.chain(edges.into_iter().map(IdentityGraphRow::Edge))
		.chain(ports_in.into_iter().map(IdentityGraphRow::PortIn))
		.chain(ports_out.into_iter().map(IdentityGraphRow::PortOut))
		.collect();
	let paged = page_rows(rows, page, current_generation)?;
	let mut nodes = Vec::new();
	let mut edges = Vec::new();
	let mut ports_in = Vec::new();
	let mut ports_out = Vec::new();
	for row in paged.items {
		match row {
			IdentityGraphRow::Node(row) => nodes.push(row),
			IdentityGraphRow::Edge(row) => edges.push(row),
			IdentityGraphRow::PortIn(row) => ports_in.push(row),
			IdentityGraphRow::PortOut(row) => ports_out.push(row),
		}
	}
	Ok(IdentityGraphPage {
		coverage: IdentityGraphCoverage {
			rows_total,
			rows_matching,
			rows_emitted: nodes.len() + edges.len() + ports_in.len() + ports_out.len(),
			nodes_total,
			nodes_emitted: nodes.len(),
			edges_total,
			edges_matching,
			edges_emitted: edges.len(),
			ports_in_total,
			ports_in_matching,
			ports_in_emitted: ports_in.len(),
			ports_out_total,
			ports_out_matching,
			ports_out_emitted: ports_out.len(),
		},
		nodes,
		edges,
		ports_in,
		ports_out,
		next_cursor: paged.next_cursor,
	})
}

enum IdentityGraphRow {
	Node(IdentitySegmentDto),
	Edge(IdentityGraphEdge),
	PortIn(IdentityGraphPort),
	PortOut(IdentityGraphPort),
}

fn resolution_audit_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: code_moniker_query::ResolutionAuditQuery,
	page: Page,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let _ = selected_roots(roots, query.workspace.as_deref())?;
	validate_page_cursor(&page, current_generation)?;
	let prefix = identity_path(query.prefix.trim_matches('/'))
		.trim_matches('/')
		.to_string();
	let sample_offset = page
		.cursor
		.as_ref()
		.map(|cursor| cursor.offset)
		.unwrap_or(0);
	let drill_down = query.cluster.is_some();
	let options = code_moniker_workspace::audit::AuditOptions {
		cluster_limit: query.limit.clamp(1, 200),
		sample_limit: if drill_down {
			query.limit.clamp(1, 200)
		} else {
			code_moniker_workspace::audit::AuditOptions::default().sample_limit
		},
		sample_offset,
		cluster: query.cluster,
		..code_moniker_workspace::audit::AuditOptions::default()
	};
	let audit = code_moniker_workspace::audit::resolution_audit(snapshot, &prefix, options);
	let next_cursor = drill_down
		.then(|| audit.clusters.first())
		.flatten()
		.filter(|cluster| sample_offset + cluster.samples.len() < cluster.count)
		.map(|cluster| QueryCursor {
			offset: sample_offset + cluster.samples.len(),
			generation: current_generation,
		});
	let result = ResolutionAuditResult {
		prefix,
		totals: AuditTotalsDto {
			references: audit.totals.references,
			resolved: audit.totals.resolved,
			unique: audit.totals.unique,
			candidate: audit.totals.candidate,
			external: audit.totals.external,
			sdk: audit.totals.sdk,
			dependency: audit.totals.dependency,
			injected_external: audit.totals.injected_external,
			unknown_external: audit.totals.unknown_external,
			dynamic: audit.totals.dynamic,
			blocked: audit.totals.blocked,
			unresolved: audit.totals.unresolved,
			explained: audit.totals.explained,
			weak_or_unexplained: audit.totals.weak_or_unexplained,
			name_match_resolved: audit.totals.name_match_resolved,
			name_match_candidate: audit.totals.name_match_candidate,
		},
		clusters: audit
			.clusters
			.iter()
			.map(|cluster| AuditClusterDto {
				id: cluster.id.clone(),
				pattern: code_moniker_workspace::audit::pattern_label(&cluster.pattern),
				count: cluster.count,
				samples: cluster
					.samples
					.iter()
					.map(|sample| AuditSampleDto {
						file: sample.file.clone(),
						line_range: sample.line_range,
						snippet: audit_sample_snippet(snapshot, sample),
						source: sample.source.clone(),
						call_name: sample.call_name.clone(),
						receiver: sample.receiver.clone(),
						target: sample.target.clone(),
						evidence: sample.evidence.clone(),
						constraints: sample.constraints.clone(),
						candidates: sample.candidates.clone(),
					})
					.collect(),
			})
			.collect(),
		zones: audit
			.zones
			.iter()
			.map(|zone| AuditZoneDto {
				zone: zone.zone.clone(),
				unresolved: zone.unresolved,
				dominant_pattern: zone.dominant_pattern.clone(),
			})
			.collect(),
	};
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::ResolutionAudit(Box::new(result)),
		next_cursor,
	})
}

fn audit_sample_snippet(
	snapshot: &WorkspaceSnapshot,
	sample: &code_moniker_workspace::audit::AuditSample,
) -> String {
	if !sample.snippet.is_empty() {
		return sample.snippet.clone();
	}
	let Some(line_range) = sample.line_range else {
		return String::new();
	};
	let Some(source) = snapshot
		.index
		.sources
		.iter()
		.find(|source| source.rel_path == sample.file)
	else {
		return String::new();
	};
	let Ok(text) = std::fs::read_to_string(&source.path) else {
		return String::new();
	};
	bounded_source_excerpt(&text, line_range)
}

fn bounded_source_excerpt(source: &str, (start, end): (u32, u32)) -> String {
	let line_count = end.saturating_sub(start).saturating_add(1).min(3) as usize;
	let excerpt = source
		.lines()
		.skip(start.saturating_sub(1) as usize)
		.take(line_count)
		.map(str::trim)
		.filter(|line| !line.is_empty())
		.collect::<Vec<_>>()
		.join(" ");
	excerpt.chars().take(240).collect()
}

// Classifies references without an in-workspace target so external-by-design
// links never masquerade as resolution gaps in graph outputs.
struct UnlinkedClassifier {
	external: HashMap<ReferenceId, ExternalReferenceOrigin>,
	candidate: HashSet<ReferenceId>,
	dynamic: HashSet<ReferenceId>,
	manifest_blocked: HashSet<ReferenceId>,
	unresolved: HashMap<ReferenceId, code_moniker_workspace::snapshot::UnresolvedReason>,
}

impl UnlinkedClassifier {
	fn new(snapshot: &WorkspaceSnapshot) -> Self {
		Self {
			external: snapshot
				.linkage
				.external
				.iter()
				.map(|reference| (reference.reference, reference.origin))
				.collect(),
			candidate: snapshot
				.linkage
				.candidates
				.iter()
				.map(|reference| reference.reference)
				.collect(),
			dynamic: snapshot
				.linkage
				.dynamic
				.iter()
				.map(|reference| reference.reference)
				.collect(),
			manifest_blocked: snapshot
				.linkage
				.blocked
				.iter()
				.chain(snapshot.linkage.manifest_blocked.iter())
				.map(|reference| reference.reference)
				.collect(),
			unresolved: snapshot
				.linkage
				.unresolved
				.iter()
				.map(|reference| (reference.reference, reference.reason))
				.collect(),
		}
	}

	fn tally(&self, reference: &ReferenceId, unlinked: &mut UnlinkedRefsDto) {
		if let Some(origin) = self.external.get(reference) {
			unlinked.external += 1;
			match origin {
				ExternalReferenceOrigin::Sdk => unlinked.sdk += 1,
				ExternalReferenceOrigin::Dependency => unlinked.dependency += 1,
				ExternalReferenceOrigin::Injected => unlinked.injected_external += 1,
				ExternalReferenceOrigin::UnknownExternal => unlinked.unknown_external += 1,
			}
		} else if self.candidate.contains(reference) {
			unlinked.candidate += 1;
		} else if self.dynamic.contains(reference) {
			unlinked.dynamic += 1;
		} else if self.manifest_blocked.contains(reference) {
			unlinked.manifest_blocked += 1;
		} else {
			unlinked.unresolved += 1;
			let reason = self
				.unresolved
				.get(reference)
				.map_or("unclassified", |reason| reason.as_str());
			*unlinked
				.unresolved_reasons
				.entry(reason.to_string())
				.or_default() += 1;
		}
	}
}

// The child identity of the scope that contains this symbol, if any.
fn scope_segment(symbol: &SymbolRecord, prefix: &str) -> Option<String> {
	let rest = identity_rest(identity_path(symbol.identity.as_ref()), prefix)?;
	let segment = rest
		.split('/')
		.next()
		.filter(|segment| !segment.is_empty())?;
	Some(if prefix.is_empty() {
		segment.to_string()
	} else {
		format!("{prefix}/{segment}")
	})
}

fn truncate_identity(path: &str, segments: usize) -> String {
	path.split('/').take(segments).collect::<Vec<_>>().join("/")
}

fn bump_port(map: &mut BTreeMap<String, (BTreeSet<String>, usize)>, key: String, kind: &str) {
	let entry = map.entry(key).or_default();
	entry.0.insert(kind.to_string());
	entry.1 += 1;
}

fn into_ports(map: BTreeMap<String, (BTreeSet<String>, usize)>) -> Vec<IdentityGraphPort> {
	map.into_iter()
		.map(|(identity, (kinds, count))| IdentityGraphPort {
			identity,
			kinds: kinds.into_iter().collect(),
			count,
		})
		.collect()
}

fn identity_segment_dto(
	segment: &str,
	agg: SegmentAgg<'_>,
	prefix: &str,
	sources_view: &code_moniker_workspace::snapshot::SourceView<'_>,
	roots: &[PathBuf],
) -> IdentitySegmentDto {
	let (kind, name) = segment.split_once(':').unwrap_or(("", segment));
	let identity = if prefix.is_empty() {
		segment.to_string()
	} else {
		format!("{prefix}/{segment}")
	};
	let symbol = agg.direct.and_then(|record| {
		let source = sources_view.record(&record.source)?;
		Some(Box::new(symbol_dto(record, source, roots)))
	});
	IdentitySegmentDto {
		segment: segment.to_string(),
		kind: kind.to_string(),
		name: name.to_string(),
		identity,
		defs: agg.defs,
		has_children: agg.grandchildren,
		symbol,
	}
}

// Record identities are full moniker URIs; the identity tree navigates the
// path AFTER the scheme and root anchor (`code+moniker://./`). Full URIs are
// accepted as prefixes and normalized to the same space.
fn identity_path(identity: &str) -> &str {
	let Some(rest) = identity.strip_prefix(DEFAULT_SCHEME) else {
		return identity;
	};
	match rest.split_once('/') {
		Some((_, path)) => path,
		None => "",
	}
}

fn identity_rest<'a>(identity: &'a str, prefix: &str) -> Option<&'a str> {
	if prefix.is_empty() {
		return Some(identity);
	}
	if identity.len() > prefix.len()
		&& identity.starts_with(prefix)
		&& identity.as_bytes()[prefix.len()] == b'/'
	{
		Some(&identity[prefix.len() + 1..])
	} else {
		None
	}
}

fn resolve_unit_boundary(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	focus: &str,
) -> Result<(UnitBoundary, SymbolGraphFocus), QueryError> {
	if let Ok(symbol) = find_symbol(snapshot, focus) {
		let source = WorkspaceView::new(snapshot)
			.sources()
			.record(&symbol.source)
			.ok_or_else(|| QueryError::new("source_not_found", "focus source not found"))?;
		return Ok((
			UnitBoundary::IdentityPrefix {
				prefix: symbol.identity.to_string(),
				slot: symbol.id.file(),
			},
			SymbolGraphFocus::Symbol {
				symbol: Box::new(symbol_dto(symbol, source, roots)),
			},
		));
	}
	let source = snapshot
		.index
		.sources
		.iter()
		.find(|source| source.rel_path == focus)
		.ok_or_else(|| directory_or_unknown_focus_error(snapshot, focus))?;
	Ok((
		UnitBoundary::File {
			source: source.id,
			slot: source.id.file(),
		},
		SymbolGraphFocus::File {
			path: source.rel_path.clone(),
		},
	))
}

fn navigable_anchor<'a>(
	symbols: &code_moniker_workspace::snapshot::SymbolView<'a>,
	id: SymbolId,
) -> Option<&'a SymbolRecord> {
	let mut current = symbols.find(&id)?;
	loop {
		if current.navigable {
			return Some(current);
		}
		let parent = current.parent?;
		current = symbols.find(&parent)?;
	}
}

fn resolved_reference_target(
	snapshot: &WorkspaceSnapshot,
	reference: &ReferenceId,
) -> Option<SymbolId> {
	if let Some(index) = snapshot.linkage.read_index.get() {
		return index.resolved_target(reference).copied();
	}
	snapshot
		.linkage
		.resolved
		.iter()
		.find(|edge| &edge.reference == reference)
		.map(|edge| edge.target)
}

fn incoming_reference_ids(snapshot: &WorkspaceSnapshot, symbol: &SymbolId) -> Vec<ReferenceId> {
	WorkspaceView::new(snapshot)
		.references()
		.incoming_ids(symbol)
}

fn change_review_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: ChangeReviewQuery,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let _ = selected_roots(roots, query.workspace.as_deref())?;
	let result = match snapshot.changes.semantic.as_deref() {
		Some(review) => change_review_dto(review),
		None => ChangeReviewResult {
			scope: snapshot.changes.scope.clone(),
			summary: ChangeReviewSummary::default(),
			files: Vec::new(),
			symbol_changes: Vec::new(),
			ref_changes: Vec::new(),
			diagnostics: vec!["semantic change review is unavailable in this snapshot".to_string()],
		},
	};
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::ChangeReview(Box::new(result)),
		next_cursor: None,
	})
}

fn change_review_dto(
	review: &code_moniker_workspace::changes::semantic::review::SemanticReview,
) -> ChangeReviewResult {
	ChangeReviewResult {
		scope: review.scope.clone(),
		summary: ChangeReviewSummary {
			files: review.files.len(),
			analyzable_files: review.files.iter().filter(|facts| facts.analyzable).count(),
			symbol_changes: review.symbol_changes.len(),
			ref_changes: review.ref_changes.len(),
			retargeted_refs: review
				.ref_changes
				.iter()
				.filter(|change| change.kind.is_retarget())
				.count(),
			residual_files: review
				.files
				.iter()
				.filter(|facts| !facts.coverage.explained())
				.count(),
		},
		files: review.files.iter().map(change_review_file).collect(),
		symbol_changes: review
			.symbol_changes
			.iter()
			.map(change_review_symbol)
			.collect(),
		ref_changes: review.ref_changes.iter().map(change_review_ref).collect(),
		diagnostics: review.diagnostics.clone(),
	}
}

fn change_review_file(
	facts: &code_moniker_workspace::changes::semantic::review::FileFacts,
) -> ChangeReviewFile {
	ChangeReviewFile {
		old_path: facts
			.rollup
			.old_path
			.as_ref()
			.map(|path| path.display().to_string()),
		new_path: facts
			.rollup
			.new_path
			.as_ref()
			.map(|path| path.display().to_string()),
		disposition: facts.rollup.disposition.label().to_string(),
		analyzable: facts.analyzable,
		symbol_changes: facts.rollup.symbol_changes,
		moved_symbols: facts.rollup.moved_symbols,
		coverage_explained: facts.coverage.explained(),
		old_residual: facts.coverage.old_residual.clone(),
		new_residual: facts.coverage.new_residual.clone(),
	}
}

fn change_review_symbol(
	change: &code_moniker_workspace::changes::semantic::model::SymbolChange,
) -> ChangeReviewSymbol {
	ChangeReviewSymbol {
		kind: change.kind.label().to_string(),
		confidence: change.confidence.label().to_string(),
		body_changed: change.facets.body_changed,
		signature_changed: change.facets.signature_changed,
		visibility_changed: change.facets.visibility_changed,
		header_changed: change.facets.header_changed,
		file_moved: change.facets.file_moved,
		old: change.old.as_ref().map(change_review_side),
		new: change.new.as_ref().map(change_review_side),
	}
}

fn change_review_side(
	side: &code_moniker_workspace::changes::semantic::model::SymbolSide,
) -> ChangeReviewSide {
	ChangeReviewSide {
		identity: code_moniker_core::core::uri::to_uri(
			&side.moniker,
			&code_moniker_core::core::uri::UriConfig {
				scheme: DEFAULT_SCHEME,
			},
		),
		file: side.file_path.display().to_string(),
		kind: side.kind.clone(),
		name: side.name.clone(),
		visibility: side.visibility.clone(),
		lines: side.line_range,
	}
}

fn change_review_ref(
	change: &code_moniker_workspace::changes::semantic::model::RefChange,
) -> ChangeReviewRef {
	let config = code_moniker_core::core::uri::UriConfig {
		scheme: DEFAULT_SCHEME,
	};
	ChangeReviewRef {
		kind: change.kind.label().to_string(),
		file: change.file_path.display().to_string(),
		ref_kind: change.ref_kind.clone(),
		old_target: change
			.old_target
			.as_ref()
			.map(|target| code_moniker_core::core::uri::to_uri(target, &config)),
		new_target: change
			.new_target
			.as_ref()
			.map(|target| code_moniker_core::core::uri::to_uri(target, &config)),
		old_lines: change.old_line_range,
		new_lines: change.new_line_range,
	}
}

fn stateless_protocol_response(request: &ProtocolRequest) -> Option<ProtocolResponse> {
	let ProtocolRequest::Query(request) = request else {
		return None;
	};
	let response = match &request.query {
		Query::QueryDescribe(query) => query_describe_response(query.verb.as_deref()),
		Query::SyntaxParse(query) => syntax::syntax_parse_response(query.clone()),
		_ => return None,
	};
	Some(response.map_or_else(ProtocolResponse::Error, |response| {
		ProtocolResponse::Query(Box::new(response))
	}))
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

fn workspace_unavailable_response(
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
fn reject_conflicting_daemons(config: &DaemonWorkspaceConfig) -> anyhow::Result<()> {
	for (path, entry) in code_moniker_query::list_registry_files()? {
		let shares_root = entry
			.workspace_roots
			.iter()
			.any(|root| config.roots.contains(root));
		if !shares_root {
			continue;
		}
		if code_moniker_query::pid_is_alive(entry.pid)
			&& !code_moniker_query::daemon_registry_heartbeat_expired(&entry)
		{
			anyhow::bail!(
				"a daemon already serves {} (pid {}, endpoint {}); stop it before starting another",
				entry.workspace_root,
				entry.pid,
				entry.endpoint
			);
		}
		remove_registry_entry_if_own(&path, &entry);
	}
	Ok(())
}

fn drain_live_events(daemon: &mut WorkspaceDaemon) -> Result<(), QueryError> {
	while let Ok(event) = daemon.live.rx.try_recv() {
		apply_live_event(daemon, event)?;
	}
	if daemon.live.policy == DaemonLiveRefreshPolicy::Auto
		&& daemon.registry.queries().staleness().is_stale()
	{
		refresh_stale(daemon)?;
	}
	Ok(())
}

fn apply_live_event(
	daemon: &mut WorkspaceDaemon,
	event: WorkspaceLiveEvent,
) -> Result<(), QueryError> {
	let plan = WorkspaceLiveRefreshPlan::from_event(event);
	if plan.is_empty() {
		return Ok(());
	}
	match daemon.live.policy {
		DaemonLiveRefreshPolicy::OnDemand => {
			daemon.registry.live_commands().mark_stale(plan);
			Ok(())
		}
		DaemonLiveRefreshPolicy::Auto => apply_live_plan(daemon, plan),
	}
}

fn apply_live_plan(
	daemon: &mut WorkspaceDaemon,
	plan: WorkspaceLiveRefreshPlan,
) -> Result<(), QueryError> {
	let live = daemon
		.registry
		.live_commands()
		.apply_plan(WorkspaceRequest::new("daemon-live-refresh"), plan);
	let replace_watcher = live.replace_watcher();
	workspace_transition_result(live.transition())?;
	if replace_watcher {
		restart_live_watcher(daemon)?;
	}
	Ok(())
}

fn refresh_full_cancellable(
	daemon: &mut WorkspaceDaemon,
	cancellation: WorkspaceCancellation,
) -> Result<(), QueryError> {
	workspace_transition_result(
		daemon
			.registry
			.commands()
			.refresh(WorkspaceRequest::new("daemon-refresh").with_cancellation(cancellation)),
	)
}

fn refresh_stale(daemon: &mut WorkspaceDaemon) -> Result<(), QueryError> {
	let live = daemon
		.registry
		.live_commands()
		.refresh_stale(WorkspaceRequest::new("daemon-refresh-stale"));
	let replace_watcher = live.replace_watcher();
	workspace_transition_result(live.transition())?;
	if replace_watcher {
		restart_live_watcher(daemon)?;
	}
	Ok(())
}

fn workspace_transition_result(transition: WorkspaceTransition) -> Result<(), QueryError> {
	match transition {
		WorkspaceTransition::Ready { .. } => Ok(()),
		WorkspaceTransition::Failed { failure, .. } => {
			Err(QueryError::new("workspace_refresh_failed", failure.message))
		}
	}
}

fn restart_live_watcher(daemon: &mut WorkspaceDaemon) -> Result<(), QueryError> {
	daemon
		.restart_live_watcher()
		.map_err(|err| QueryError::new("live_watcher_failed", err.to_string()))
}

fn generation(registry: &LocalWorkspaceRegistry) -> Option<WorkspaceGeneration> {
	registry
		.queries()
		.snapshot()
		.map(|snapshot| WorkspaceGeneration(snapshot.generation.value()))
}

fn workspace_status(
	roots: &[PathBuf],
	registry: &LocalWorkspaceRegistry,
) -> Result<QueryResponse, QueryError> {
	let status = workspace_status_result(roots, registry);
	Ok(QueryResponse {
		generation: status.generation,
		result: QueryResult::WorkspaceStatus(status),
		next_cursor: None,
	})
}

fn workspace_status_without_snapshot(
	roots: &[PathBuf],
	lifecycle: WorkspaceLifecycle,
) -> QueryResponse {
	let summary = lifecycle
		.failure
		.as_ref()
		.map(|failure| failure.message.clone())
		.unwrap_or_else(|| lifecycle.phase.to_string());
	let status = WorkspaceStatus {
		producer: producer_identity(),
		root: workspace_label(roots),
		phase: lifecycle.phase,
		failure: lifecycle.failure,
		roots: roots
			.iter()
			.map(|root| WorkspaceRootStatus {
				root: root.display().to_string(),
				generation: None,
				files: 0,
				symbols: 0,
				references: 0,
				stale: false,
				stale_summary: summary.clone(),
			})
			.collect(),
		generation: None,
		files: 0,
		symbols: 0,
		references: 0,
		stale: false,
		stale_summary: summary,
		timings: WorkspaceTimingsDto::default(),
	};
	QueryResponse {
		generation: None,
		result: QueryResult::WorkspaceStatus(status),
		next_cursor: None,
	}
}

fn workspace_status_result(
	roots: &[PathBuf],
	registry: &LocalWorkspaceRegistry,
) -> WorkspaceStatus {
	let staleness = registry.queries().staleness();
	let generation = registry
		.queries()
		.snapshot()
		.map(|snapshot| WorkspaceGeneration(snapshot.generation.value()));
	let root_statuses = registry
		.queries()
		.snapshot()
		.map(|snapshot| {
			roots
				.iter()
				.map(|root| {
					root_status(
						snapshot,
						roots,
						root,
						staleness.is_stale(),
						&staleness.summary(),
					)
				})
				.collect::<Vec<_>>()
		})
		.unwrap_or_else(|| {
			roots
				.iter()
				.map(|root| WorkspaceRootStatus {
					root: root.display().to_string(),
					generation,
					files: 0,
					symbols: 0,
					references: 0,
					stale: staleness.is_stale(),
					stale_summary: staleness.summary(),
				})
				.collect()
		});
	let files = root_statuses.iter().map(|root| root.files).sum();
	let symbols = root_statuses.iter().map(|root| root.symbols).sum();
	let references = root_statuses.iter().map(|root| root.references).sum();
	let failure = registry.queries().last_failure().map(workspace_failure_dto);
	WorkspaceStatus {
		producer: producer_identity(),
		root: workspace_label(roots),
		phase: if generation.is_some() {
			WorkspacePhase::Ready
		} else if failure.is_some() {
			WorkspacePhase::Failed
		} else {
			WorkspacePhase::Loading
		},
		failure,
		roots: root_statuses,
		generation,
		files,
		symbols,
		references,
		stale: staleness.is_stale(),
		stale_summary: staleness.summary(),
		timings: registry
			.queries()
			.snapshot()
			.map(workspace_timings_dto)
			.unwrap_or_default(),
	}
}

fn workspace_failure_dto(
	failure: &code_moniker_workspace::snapshot::WorkspaceFailure,
) -> WorkspaceFailureDto {
	WorkspaceFailureDto {
		resource: Some(
			match failure.resource {
				WorkspaceResource::SourceCatalog => "source_catalog",
				WorkspaceResource::CodeIndex => "code_index",
				WorkspaceResource::LinkageSnapshot => "linkage_snapshot",
				WorkspaceResource::ChangeOverlay => "change_overlay",
			}
			.to_string(),
		),
		message: failure.message.clone(),
	}
}

fn workspace_timings_dto(snapshot: &WorkspaceSnapshot) -> WorkspaceTimingsDto {
	let milliseconds =
		|duration: std::time::Duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
	WorkspaceTimingsDto {
		source_catalog_ms: milliseconds(snapshot.timings.source_catalog),
		extract_sources_ms: milliseconds(snapshot.timings.extract_sources),
		semantic_index_ms: milliseconds(snapshot.timings.semantic_index),
		code_index_ms: milliseconds(snapshot.timings.code_index),
		linkage_ms: milliseconds(snapshot.timings.linkage),
		change_overlay_ms: milliseconds(snapshot.timings.change_overlay),
		total_ms: milliseconds(snapshot.timings.total),
	}
}

fn producer_identity() -> BuildIdentity {
	current_build_identity(env!("CARGO_PKG_VERSION")).unwrap_or_else(|error| BuildIdentity {
		version: env!("CARGO_PKG_VERSION").to_string(),
		fingerprint: format!("unavailable:{error}"),
	})
}

fn change_counts_by_source(snapshot: &WorkspaceSnapshot) -> BTreeMap<SourceId, usize> {
	let mut counts = BTreeMap::new();
	for change in &snapshot.changes.changes {
		let Some(source) = change.source else {
			continue;
		};
		*counts.entry(source).or_insert(0) += 1;
	}
	counts
}

fn tree_children_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: TreeChildrenQuery,
	page: Page,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, query.workspace.as_deref())?;
	let path_filter = FilePathFilter::compile(&query.path)
		.map_err(|err| QueryError::new("invalid_path_filter", err.to_string()))?;
	let plain_scope = tree_plain_scope(&query.path);
	let prefix = plain_scope.as_deref().unwrap_or_default();
	let mut map = BTreeMap::<String, TreeNode>::new();
	let mut scoped_sources = Vec::new();
	let change_counts = change_counts_by_source(snapshot);
	for source in &snapshot.index.sources {
		let Some(root) = source_root(roots, &selected_roots, source) else {
			continue;
		};
		if !query.lang.is_empty() && !query.lang.iter().any(|lang| lang == &source.language) {
			continue;
		}
		if !path_filter.matches(&source.rel_path) {
			continue;
		}
		scoped_sources.push((root, source));
		let exact_file_scope = plain_scope
			.as_deref()
			.is_some_and(|scope| source.rel_path == scope);
		let remainder = if exact_file_scope {
			source.rel_path.as_str()
		} else {
			source.rel_path[prefix.len()..].trim_start_matches('/')
		};
		if remainder.is_empty() {
			continue;
		}
		let parts = remainder.split('/').collect::<Vec<_>>();
		let depth = query.depth.max(1);
		let take = parts.len().min(depth);
		let row_path = if exact_file_scope || prefix.is_empty() {
			parts[..take].join("/")
		} else {
			format!(
				"{}/{}",
				prefix.trim_end_matches('/'),
				parts[..take].join("/")
			)
		};
		let kind = if take < parts.len() {
			TreeNodeKind::Directory
		} else {
			TreeNodeKind::File
		};
		let root_label = root.display().to_string();
		let entry_key = format!("{root_label}\0{row_path}");
		let entry = map.entry(entry_key).or_insert_with(|| TreeNode {
			root: root_label,
			path: row_path,
			kind,
			language: (kind == TreeNodeKind::File).then(|| source.language.clone()),
			defs: 0,
			refs: 0,
			change_count: 0,
		});
		entry.defs += snapshot
			.index
			.symbols
			.file_records(source.id.file())
			.iter()
			.filter(|symbol| symbol.navigable)
			.count();
		entry.refs += snapshot
			.index
			.references
			.file_records(source.id.file())
			.len();
		entry.change_count += change_counts.get(&source.id).copied().unwrap_or(0);
	}
	let total_files = snapshot
		.index
		.sources
		.iter()
		.filter(|source| source_root(roots, &selected_roots, source).is_some())
		.count();
	let languages = sorted_counts(
		scoped_sources
			.iter()
			.map(|(_, source)| source.language.clone()),
	);
	let prefixes = sorted_counts(
		scoped_sources
			.iter()
			.map(|(_, source)| path_prefix(&source.rel_path)),
	);
	let paged = page_rows(map.into_values().collect(), page, current_generation)?;
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::TreeChildren(TreeChildrenResult {
			root: workspace_label_from_paths(&selected_roots),
			roots: selected_roots
				.iter()
				.map(|root| root.display().to_string())
				.collect(),
			total: paged.total,
			rows: paged.items,
			total_files,
			scoped_files: scoped_sources.len(),
			languages,
			prefixes,
		}),
		next_cursor: paged.next_cursor,
	})
}

fn tree_plain_scope(paths: &[String]) -> Option<String> {
	let [path] = paths else {
		return None;
	};
	if path.contains(['*', '?']) {
		None
	} else {
		Some(normalize_tree_path(path))
	}
}

fn normalize_tree_path(path: &str) -> String {
	path.trim()
		.replace('\\', "/")
		.trim_start_matches("./")
		.trim_start_matches('/')
		.trim_end_matches('/')
		.to_string()
}

fn symbol_search_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: SymbolSearchQuery,
	page: Page,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, query.workspace.as_deref())?;
	let path_filter = FilePathFilter::compile(&query.path)
		.map_err(|err| QueryError::new("invalid_path_filter", err.to_string()))?;
	let name_filter = query
		.name
		.as_ref()
		.map(|pattern| regex::Regex::new(pattern))
		.transpose()
		.map_err(|err| QueryError::new("invalid_name_filter", err.to_string()))?;
	let sources = WorkspaceView::new(snapshot).sources();
	let matches_query = |symbol: &SymbolRecord| {
		let Some(source) = sources.record(&symbol.source) else {
			return false;
		};
		source_root(roots, &selected_roots, source).is_some()
			&& path_filter.matches(&source.rel_path)
			&& (query.lang.is_empty() || query.lang.iter().any(|lang| lang == &source.language))
			&& matches_kind_shape(symbol, &query)
			&& name_filter
				.as_ref()
				.is_none_or(|regex| regex.is_match(&symbol.name))
	};
	let mut rows = if let Some(text) = query.text.as_deref().filter(|text| !text.trim().is_empty())
		&& !query.include_non_navigable
	{
		let symbols = WorkspaceView::new(snapshot).symbols();
		WorkspaceView::new(snapshot)
			.search()
			.search_symbols_matching(text, usize::MAX, matches_query)
			.into_iter()
			.map(|hit| {
				let Some(symbol) = symbols.find(&hit.symbol) else {
					return Ok(None);
				};
				let Some(source) = sources.record(&symbol.source) else {
					return Ok(None);
				};
				let mut row = symbol_search_dto(symbol, source, roots, hit.score, hit.reason);
				if query.include_code {
					row.source = source_snippet(source, symbol, query.context_lines)?;
				}
				Ok(Some(row))
			})
			.collect::<Result<Vec<_>, QueryError>>()?
			.into_iter()
			.flatten()
			.collect::<Vec<_>>()
	} else {
		snapshot
			.index
			.symbols
			.iter()
			.filter(|symbol| query.include_non_navigable || symbol.navigable)
			.filter(|symbol| matches_query(symbol))
			.filter_map(|symbol| {
				let source = sources.record(&symbol.source)?;
				Some((symbol, source))
			})
			.map(|(symbol, source)| {
				let mut row = symbol_dto(symbol, source, roots);
				if query.include_code {
					row.source = source_snippet(source, symbol, query.context_lines)?;
				}
				Ok(row)
			})
			.collect::<Result<Vec<_>, QueryError>>()?
	};
	if query
		.text
		.as_deref()
		.is_none_or(|text| text.trim().is_empty())
		|| query.include_non_navigable
	{
		rows.sort_by(symbol_dto_navigation_cmp);
	}
	let paged = page_rows(rows, page, current_generation)?;
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::SymbolList(SymbolListResult {
			total: paged.total,
			rows: paged.items,
		}),
		next_cursor: paged.next_cursor,
	})
}

fn symbol_dto_navigation_cmp(left: &SymbolDto, right: &SymbolDto) -> std::cmp::Ordering {
	symbol_is_test_artifact(&left.kind, &left.file, &left.uri)
		.cmp(&symbol_is_test_artifact(
			&right.kind,
			&right.file,
			&right.uri,
		))
		.then_with(|| left.file.cmp(&right.file))
		.then_with(|| left.line_range.cmp(&right.line_range))
		.then_with(|| left.uri.cmp(&right.uri))
}

fn matches_kind_shape(symbol: &SymbolRecord, query: &SymbolSearchQuery) -> bool {
	let kind_matches = query.kind.iter().any(|kind| kind == &symbol.kind);
	let shape_matches = query
		.shape
		.iter()
		.any(|shape| Shape::for_kind(symbol.kind.as_bytes()).as_str() == shape);
	if query
		.text
		.as_deref()
		.is_some_and(|text| !text.trim().is_empty())
		&& !query.kind.is_empty()
		&& !query.shape.is_empty()
	{
		return kind_matches || shape_matches;
	}
	(query.kind.is_empty() || kind_matches) && (query.shape.is_empty() || shape_matches)
}

fn symbol_insights_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: SymbolSearchQuery,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, query.workspace.as_deref())?;
	let path_filter = FilePathFilter::compile(&query.path)
		.map_err(|err| QueryError::new("invalid_path_filter", err.to_string()))?;
	let name_filter = query
		.name
		.as_ref()
		.map(|pattern| regex::Regex::new(pattern))
		.transpose()
		.map_err(|err| QueryError::new("invalid_name_filter", err.to_string()))?;
	let sources = WorkspaceView::new(snapshot).sources();
	let scoped_sources = snapshot
		.index
		.sources
		.iter()
		.filter(|source| source_root(roots, &selected_roots, source).is_some())
		.filter(|source| path_filter.matches(&source.rel_path))
		.filter(|source| {
			query.lang.is_empty() || query.lang.iter().any(|lang| lang == &source.language)
		})
		.collect::<Vec<_>>();
	let scoped_source_ids = scoped_sources
		.iter()
		.map(|source| source.id)
		.collect::<BTreeSet<_>>();
	let scoped_symbols = snapshot
		.index
		.symbols
		.iter()
		.filter(|symbol| scoped_source_ids.contains(&symbol.source))
		.filter(|symbol| query.include_non_navigable || symbol.navigable)
		.filter(|symbol| {
			query.kind.is_empty() || query.kind.iter().any(|kind| kind == &symbol.kind)
		})
		.filter(|symbol| {
			query.shape.is_empty()
				|| query
					.shape
					.iter()
					.any(|shape| Shape::for_kind(symbol.kind.as_bytes()).as_str() == shape)
		})
		.filter(|symbol| {
			name_filter
				.as_ref()
				.is_none_or(|regex| regex.is_match(&symbol.name))
		})
		.collect::<Vec<_>>();
	let scoped_refs = snapshot
		.index
		.references
		.iter()
		.filter(|reference| scoped_source_ids.contains(&reference.source))
		.collect::<Vec<_>>();
	let mut symbol_counts = BTreeMap::<String, usize>::new();
	let mut ref_counts = BTreeMap::<String, usize>::new();
	for symbol in &scoped_symbols {
		if let Some(source) = sources.record(&symbol.source) {
			*symbol_counts.entry(source.rel_path.to_owned()).or_default() += 1;
		}
	}
	for reference in &scoped_refs {
		if let Some(source) = sources.record(&reference.source) {
			*ref_counts.entry(source.rel_path.to_owned()).or_default() += 1;
		}
	}
	let result = SymbolInsightsResult {
		files: scoped_sources.len(),
		symbols: scoped_symbols.len(),
		references: scoped_refs.len(),
		navigable_symbols: scoped_symbols
			.iter()
			.filter(|symbol| symbol.navigable)
			.count(),
		non_navigable_symbols: scoped_symbols
			.iter()
			.filter(|symbol| !symbol.navigable)
			.count(),
		languages: sorted_counts(
			scoped_sources
				.iter()
				.map(|source| source.language.to_owned()),
		),
		kinds: sorted_counts(scoped_symbols.iter().map(|symbol| symbol.kind.to_owned())),
		shapes: sorted_counts(
			scoped_symbols
				.iter()
				.map(|symbol| Shape::for_kind(symbol.kind.as_bytes()).as_str().to_string()),
		),
		top_files_by_symbols: count_rows(symbol_counts),
		top_files_by_refs: count_rows(ref_counts),
	};
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::SymbolInsights(result),
		next_cursor: None,
	})
}

fn symbol_detail_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	workspace: Option<&str>,
	uri: &str,
	context_lines: usize,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, workspace)?;
	let symbol = find_symbol(snapshot, uri)?;
	let source = WorkspaceView::new(snapshot)
		.sources()
		.record(&symbol.source)
		.ok_or_else(|| QueryError::new("source_not_found", "symbol source not found"))?;
	if source_root(roots, &selected_roots, source).is_none() {
		return Err(QueryError::new(
			"symbol_not_in_workspace",
			format!("symbol {uri} is not in the selected workspace"),
		));
	}
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::SymbolDetail(SymbolDetailResult {
			symbol: symbol_dto(symbol, source, roots),
			source: source_snippet(source, symbol, context_lines)?,
		}),
		next_cursor: None,
	})
}

fn symbol_usages_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: SymbolUsagesQuery,
	page: Page,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, query.workspace.as_deref())?;
	let path_filter = FilePathFilter::compile(&query.path)
		.map_err(|err| QueryError::new("invalid_path_filter", err.to_string()))?;
	let target = find_symbol(snapshot, &query.uri)?;
	let target_source = WorkspaceView::new(snapshot)
		.sources()
		.record(&target.source)
		.ok_or_else(|| QueryError::new("source_not_found", "target source not found"))?;
	if source_root(roots, &selected_roots, target_source).is_none() {
		return Err(QueryError::new(
			"symbol_not_in_workspace",
			format!("symbol {} is not in the selected workspace", query.uri),
		));
	}
	let mut incoming_rows = Vec::new();
	let mut outgoing_rows = Vec::new();
	let targets = if query.include_descendants {
		snapshot
			.index
			.symbols
			.iter()
			.filter(|symbol| symbol.navigable && symbol_is_owned_by(snapshot, symbol.id, target.id))
			.collect::<Vec<_>>()
	} else {
		vec![target]
	};
	let usage_context = UsageDtoContext {
		snapshot,
		roots,
		selected_roots: &selected_roots,
		path_filter: &path_filter,
		langs: &query.lang,
	};
	if matches!(
		query.direction,
		UsageDirection::Incoming | UsageDirection::Both
	) {
		for selected in &targets {
			incoming_rows.extend(collect_incoming_usages(snapshot, selected, &usage_context));
		}
		if query.include_descendants {
			incoming_rows.retain(|row| !usage_source_is_owned_by(snapshot, row, target.id));
		}
		deduplicate_usage_rows(&mut incoming_rows);
	}
	if matches!(
		query.direction,
		UsageDirection::Outgoing | UsageDirection::Both
	) {
		let references = WorkspaceView::new(snapshot).references();
		for selected in &targets {
			for id in references.outgoing_ids(&selected.id) {
				let Some(reference) = references.reference(&id) else {
					continue;
				};
				let internal = query.include_descendants
					&& resolved_reference_target(snapshot, &reference.id)
						.is_some_and(|id| symbol_is_owned_by(snapshot, id, target.id));
				if !internal
					&& let Some(row) =
						usage_dto(reference, UsageDirection::Outgoing, &usage_context)
				{
					outgoing_rows.push(row);
				}
			}
		}
		deduplicate_usage_rows(&mut outgoing_rows);
	}
	let incoming_summary = matches!(
		query.direction,
		UsageDirection::Incoming | UsageDirection::Both
	)
	.then(|| usage_summary(&incoming_rows, true));
	let outgoing_summary = matches!(
		query.direction,
		UsageDirection::Outgoing | UsageDirection::Both
	)
	.then(|| usage_summary(&outgoing_rows, false));
	let mut rows = Vec::new();
	rows.extend(incoming_rows);
	rows.extend(outgoing_rows);
	rows.sort_by(usage_cmp_for_navigation);
	let page = expand_usage_page_to_group(&rows, page);
	let paged = page_rows(rows, page, current_generation)?;
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::SymbolUsages(Box::new(SymbolUsagesResult {
			target: symbol_dto(target, target_source, roots),
			direction: query.direction,
			include_descendants: query.include_descendants,
			targets: targets.len(),
			total: paged.total,
			rows: paged.items,
			incoming_summary,
			outgoing_summary,
		})),
		next_cursor: paged.next_cursor,
	})
}

fn deduplicate_usage_rows(rows: &mut Vec<UsageDto>) {
	let mut seen = BTreeSet::new();
	rows.retain(|row| seen.insert(row.reference.clone()));
}

fn usage_source_is_owned_by(
	snapshot: &WorkspaceSnapshot,
	usage: &UsageDto,
	owner: SymbolId,
) -> bool {
	ReferenceId::parse(&usage.reference)
		.and_then(|id| WorkspaceView::new(snapshot).references().reference(&id))
		.is_some_and(|reference| symbol_is_owned_by(snapshot, reference.source_symbol, owner))
}

fn symbol_is_owned_by(snapshot: &WorkspaceSnapshot, symbol: SymbolId, owner: SymbolId) -> bool {
	let symbols = WorkspaceView::new(snapshot).symbols();
	let mut current = symbols.find(&symbol);
	while let Some(candidate) = current {
		if candidate.id == owner {
			return true;
		}
		current = candidate.parent.and_then(|parent| symbols.find(&parent));
	}
	false
}

fn view_read_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: ViewReadQuery,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let result = views::read(
		&query.uri,
		roots,
		query.scheme.as_deref().unwrap_or(DEFAULT_SCHEME),
		snapshot,
		query.context_lines,
		query.include_code,
	)
	.map_err(|err| QueryError::new("view_read_failed", err.to_string()))?;
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::ViewRead(result),
		next_cursor: None,
	})
}

fn collect_incoming_usages(
	snapshot: &WorkspaceSnapshot,
	target: &SymbolRecord,
	context: &UsageDtoContext<'_>,
) -> Vec<UsageDto> {
	let references = WorkspaceView::new(snapshot).references();
	let mut rows = references
		.incoming_ids(&target.id)
		.into_iter()
		.filter_map(|id| references.reference(&id))
		.filter_map(|reference| usage_dto(reference, UsageDirection::Incoming, context))
		.collect::<Vec<_>>();
	let mut seen = rows
		.iter()
		.filter_map(|row| ReferenceId::parse(&row.reference))
		.collect::<BTreeSet<_>>();
	let mut visited = BTreeSet::from([target.id]);
	collect_indirect_incoming_usages(
		snapshot,
		&target.id,
		context,
		IndirectUsageState {
			depth: 0,
			visited: &mut visited,
			seen: &mut seen,
			rows: &mut rows,
		},
	);
	rows
}

struct IndirectUsageState<'a> {
	depth: usize,
	visited: &'a mut BTreeSet<SymbolId>,
	seen: &'a mut BTreeSet<ReferenceId>,
	rows: &'a mut Vec<UsageDto>,
}

fn collect_indirect_incoming_usages(
	snapshot: &WorkspaceSnapshot,
	target: &SymbolId,
	context: &UsageDtoContext<'_>,
	state: IndirectUsageState<'_>,
) {
	const MAX_INDIRECT_USAGE_DEPTH: usize = 4;
	if state.depth >= MAX_INDIRECT_USAGE_DEPTH {
		return;
	}
	let references = WorkspaceView::new(snapshot).references();
	let symbols = WorkspaceView::new(snapshot).symbols();
	let aliases = references
		.incoming_ids(target)
		.into_iter()
		.filter_map(|id| references.reference(&id))
		.filter(|reference| reference.kind == "uses_type")
		.filter_map(|reference| symbols.find(&reference.source_symbol))
		.filter(|symbol| symbol.kind == "type")
		.filter(|symbol| state.visited.insert(symbol.id))
		.collect::<Vec<_>>();
	for alias in aliases {
		collect_direct_usages_via(snapshot, alias, context, state.seen, state.rows);
		collect_indirect_incoming_usages(
			snapshot,
			&alias.id,
			context,
			IndirectUsageState {
				depth: state.depth + 1,
				visited: state.visited,
				seen: state.seen,
				rows: state.rows,
			},
		);
	}
}

fn collect_direct_usages_via(
	snapshot: &WorkspaceSnapshot,
	alias: &SymbolRecord,
	context: &UsageDtoContext<'_>,
	seen: &mut BTreeSet<ReferenceId>,
	rows: &mut Vec<UsageDto>,
) {
	let references = WorkspaceView::new(snapshot).references();
	for id in references.incoming_ids(&alias.id) {
		let Some(reference) = references.reference(&id) else {
			continue;
		};
		if reference.source_symbol == alias.id || !seen.insert(reference.id) {
			continue;
		}
		let Some(mut row) = usage_dto(reference, UsageDirection::Incoming, context) else {
			continue;
		};
		row.via = Some(format!("{} ({})", alias.name, alias.identity));
		rows.push(row);
	}
}

fn usage_cmp_for_navigation(left: &UsageDto, right: &UsageDto) -> std::cmp::Ordering {
	usage_direction_priority(left.direction)
		.cmp(&usage_direction_priority(right.direction))
		.then_with(|| usage_kind_priority(&left.kind).cmp(&usage_kind_priority(&right.kind)))
		.then_with(|| left.root.cmp(&right.root))
		.then_with(|| left.file.cmp(&right.file))
		.then_with(|| left.actor.cmp(&right.actor))
		.then_with(|| left.context.cmp(&right.context))
		.then_with(|| left.endpoint.cmp(&right.endpoint))
		.then_with(|| left.via.cmp(&right.via))
		.then_with(|| left.line_range.cmp(&right.line_range))
		.then_with(|| left.reference.cmp(&right.reference))
}

fn expand_usage_page_to_group(rows: &[UsageDto], mut page: Page) -> Page {
	let start = page
		.cursor
		.as_ref()
		.map(|cursor| cursor.offset)
		.unwrap_or(0);
	if page.limit == 0 || start >= rows.len() {
		return page;
	}
	let mut end = start.saturating_add(page.limit).min(rows.len());
	while end < rows.len() && same_usage_group(&rows[end - 1], &rows[end]) {
		end += 1;
	}
	page.limit = end - start;
	page
}

fn same_usage_group(left: &UsageDto, right: &UsageDto) -> bool {
	left.direction == right.direction
		&& left.kind == right.kind
		&& left.root == right.root
		&& left.file == right.file
		&& left.via == right.via
		&& match left.direction {
			UsageDirection::Incoming => left.actor == right.actor && left.context == right.context,
			UsageDirection::Outgoing => left.endpoint == right.endpoint,
			UsageDirection::Both => {
				left.actor == right.actor
					&& left.context == right.context
					&& left.endpoint == right.endpoint
			}
		}
}

fn usage_direction_priority(direction: UsageDirection) -> u8 {
	match direction {
		UsageDirection::Incoming => 0,
		UsageDirection::Outgoing => 1,
		UsageDirection::Both => 2,
	}
}

fn usage_kind_priority(kind: &str) -> u8 {
	match kind {
		"calls" | "constructs" => 10,
		"extends" | "implements" | "inherits" => 20,
		"reads" | "uses_type" | "returns_type" | "annotates" => 30,
		"imports" => 40,
		_ => 50,
	}
}

fn usage_summary(rows: &[UsageDto], shared_signal: bool) -> UsageSummaryDto {
	let mut files = BTreeSet::new();
	let mut contexts = BTreeSet::new();
	let mut prefixes = BTreeMap::<&str, usize>::new();
	let mut kinds = BTreeMap::<&str, usize>::new();
	let mut actors = BTreeMap::<&str, usize>::new();
	for row in rows {
		files.insert(row.file.as_str());
		contexts.insert(row.context.as_str());
		*prefixes.entry(row.prefix.as_str()).or_default() += 1;
		*kinds.entry(row.kind.as_str()).or_default() += 1;
		*actors.entry(row.actor.as_str()).or_default() += 1;
	}
	let top_prefixes = count_rows_borrowed(&prefixes);
	let dominant_prefix = top_prefixes
		.first()
		.map(|row| {
			format!(
				"{} ({} refs, {}%)",
				row.name,
				row.count,
				percent(row.count, rows.len())
			)
		})
		.unwrap_or_default();
	UsageSummaryDto {
		refs: rows.len(),
		files: files.len(),
		contexts: contexts.len(),
		prefixes: prefixes.len(),
		dominant_prefix,
		kinds: count_rows_borrowed(&kinds),
		top_actors: count_rows_borrowed(&actors),
		top_prefixes,
		shared_helper_signal: if shared_signal {
			shared_helper_signal(rows.len(), files.len(), contexts.len(), prefixes)
		} else {
			String::new()
		},
	}
}

fn shared_helper_signal(
	refs: usize,
	files: usize,
	contexts: usize,
	prefixes: BTreeMap<&str, usize>,
) -> String {
	if refs == 0 {
		return "unused_or_unresolved".to_string();
	}
	let prefix_count = prefixes.len();
	let dominant = count_rows_borrowed(&prefixes)
		.first()
		.map(|row| percent(row.count, refs))
		.unwrap_or(0);
	if files >= 3 && contexts >= 3 && prefix_count >= 2 {
		"shared_helper_candidate".to_string()
	} else if files <= 1 || dominant >= 80 {
		"localized_not_shared".to_string()
	} else {
		"mixed_review_needed".to_string()
	}
}

fn count_rows_borrowed(counts: &BTreeMap<&str, usize>) -> Vec<CountDto> {
	let mut rows = counts
		.iter()
		.map(|(name, count)| CountDto {
			name: (*name).to_string(),
			count: *count,
		})
		.collect::<Vec<_>>();
	rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
	rows
}

fn percent(count: usize, total: usize) -> usize {
	count
		.checked_mul(100)
		.and_then(|value| value.checked_div(total))
		.unwrap_or(0)
}

fn rules_list_response(
	snapshot: &WorkspaceSnapshot,
	response: ResponseContext<'_>,
	request: RulesListEval,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(response.roots, request.workspace.as_deref())?;
	let mut rule_roots = selected_roots
		.iter()
		.map(|root| (*root).to_path_buf())
		.collect::<Vec<_>>();
	if workspace_selector_is_all(request.workspace.as_deref())
		&& has_memory_sources(snapshot, response.roots)
	{
		rule_roots.push(PathBuf::from(MEMORY_SOURCE_ROOT));
	}
	let mut rows = Vec::new();
	for root in &rule_roots {
		let requested_langs =
			workspace_langs(snapshot, response.roots, root, &request.filters.langs);
		let rules_path = resolve_rules_path(response.config_root, request.rules.as_deref());
		let specs = RuleSetRequest::with_rules(rules_path, DEFAULT_SCHEME)
			.with_profile(request.profile.clone())
			.compiled_specs_for_langs(requested_langs)
			.map_err(|err| QueryError::new("rules_compile_failed", err.to_string()))?;
		for spec in specs {
			if !request.filters.severities.is_empty()
				&& !request
					.filters
					.severities
					.iter()
					.any(|severity| severity == spec.severity.as_str())
			{
				continue;
			}
			rows.push(rule_dto(root, spec));
		}
	}
	rows.sort_by(|a, b| {
		a.root
			.cmp(&b.root)
			.then_with(|| a.id.cmp(&b.id))
			.then_with(|| a.lang.cmp(&b.lang))
			.then_with(|| a.domain.cmp(&b.domain))
	});
	let paged = page_rows(rows, request.page, response.generation)?;
	Ok(QueryResponse {
		generation: response.generation,
		result: QueryResult::RulesList(RulesListResult {
			roots: rule_roots
				.iter()
				.map(|root| root.display().to_string())
				.collect(),
			total: paged.total,
			rows: paged.items,
		}),
		next_cursor: paged.next_cursor,
	})
}

fn rules_applicable_response(
	snapshot: &WorkspaceSnapshot,
	response: ResponseContext<'_>,
	query: RulesApplicableQuery,
	page: Page,
) -> Result<QueryResponse, QueryError> {
	let (_, focus) = resolve_unit_boundary(snapshot, response.roots, &query.focus)?;
	let (file, language, symbol_kind) = focus_rule_coordinates(snapshot, &focus)?;
	let listed = rules_list_response(
		snapshot,
		response,
		RulesListEval {
			workspace: query.workspace,
			profile: query.profile,
			rules: query.rules,
			filters: RulesListFilters {
				langs: vec![language.clone()],
				severities: Vec::new(),
			},
			page: Page {
				cursor: None,
				limit: usize::MAX,
			},
		},
	)?;
	let QueryResult::RulesList(listed) = listed.result else {
		return Err(QueryError::new(
			"rules_contract",
			"unexpected rules list response",
		));
	};
	let rows = listed
		.rows
		.into_iter()
		.map(|rule| rule_applicability(rule, &language, symbol_kind.as_deref()))
		.collect::<Vec<_>>();
	let paged = page_rows(rows, page, response.generation)?;
	Ok(QueryResponse {
		generation: response.generation,
		result: QueryResult::RulesApplicable(Box::new(RulesApplicableResult {
			focus,
			file,
			language,
			symbol_kind,
			total: paged.total,
			rows: paged.items,
		})),
		next_cursor: paged.next_cursor,
	})
}

fn focus_rule_coordinates(
	snapshot: &WorkspaceSnapshot,
	focus: &SymbolGraphFocus,
) -> Result<(String, String, Option<String>), QueryError> {
	match focus {
		SymbolGraphFocus::Symbol { symbol } => Ok((
			symbol.file.clone(),
			symbol.language.clone(),
			Some(symbol.kind.clone()),
		)),
		SymbolGraphFocus::File { path } => {
			let source = snapshot
				.index
				.sources
				.iter()
				.find(|source| &source.rel_path == path)
				.ok_or_else(|| {
					QueryError::new("source_not_found", format!("source not found: {path}"))
				})?;
			Ok((path.clone(), source.language.clone(), None))
		}
	}
}

fn rule_applicability(
	rule: RuleDto,
	language: &str,
	symbol_kind: Option<&str>,
) -> RuleApplicabilityDto {
	let (status, reason) = if rule.lang != language {
		(
			"ignored",
			format!("rule language {} does not match {language}", rule.lang),
		)
	} else if rule.domain == "refs" {
		(
			"potential",
			"reference rule may evaluate references anchored in this scope".to_string(),
		)
	} else if let (Some(expected), Some(actual)) = (rule.kind.as_deref(), symbol_kind) {
		if expected == actual {
			(
				"applicable",
				format!("language and symbol kind `{actual}` match"),
			)
		} else {
			(
				"ignored",
				format!("rule kind `{expected}` does not match symbol kind `{actual}`"),
			)
		}
	} else if let Some(expected) = rule
		.domain
		.strip_prefix("shape:")
		.and_then(|domain| domain.split_whitespace().next())
	{
		match symbol_kind
			.and_then(|kind| shape_of(kind.as_bytes()))
			.map(Shape::as_str)
		{
			Some(actual) if actual == expected => (
				"applicable",
				format!("language and symbol shape `{actual}` match"),
			),
			Some(actual) => (
				"ignored",
				format!("rule shape `{expected}` does not match symbol shape `{actual}`"),
			),
			None => (
				"potential",
				format!("file scope matches; select a `{expected}` symbol to prove applicability"),
			),
		}
	} else if rule.kind.is_some() && symbol_kind.is_none() {
		(
			"potential",
			"file scope matches the language; select a symbol to prove kind applicability"
				.to_string(),
		)
	} else {
		("applicable", "language and scope match".to_string())
	};
	RuleApplicabilityDto {
		rule,
		status: status.to_string(),
		reason,
	}
}

fn change_context_response(
	daemon: &mut WorkspaceDaemon,
	snapshot: &WorkspaceSnapshot,
	response: ResponseContext<'_>,
	query: ChangeContextQuery,
) -> Result<QueryResponse, QueryError> {
	let max_items = query.max_items.clamp(1, 100);
	let context_graph = bounded_context_graph(snapshot, response, &query, max_items)?;
	let (notes_total, notes) = context_notes(
		daemon,
		snapshot,
		&context_graph.focus,
		max_items,
		response.generation,
	)?;
	let (rules_total, rules) = context_rules(snapshot, response, &query, max_items)?;
	let changes = context_changes(
		snapshot,
		response,
		query.workspace.clone(),
		&context_graph.file,
		max_items,
	)?;
	let profile_arg = query
		.profile
		.as_deref()
		.map_or_else(String::new, |profile| {
			format!(" profile=\"{}\"", profile.replace('"', "\\\""))
		});
	let escaped_file = context_graph.file.replace('"', "\\\"");
	let suggested_checks = vec![format!(
		"code_moniker_rules uri=\"workspace\" action=\"run\"{profile_arg} file=\"{escaped_file}\" limit=20"
	)];
	let coverage = ChangeContextCoverageDto {
		members_total: context_graph.members_total,
		members_emitted: context_graph.graph.members.len(),
		internal_edges_total: context_graph.internal_edges_total,
		internal_edges_emitted: context_graph.graph.internal_edges.len(),
		callers_total: context_graph.callers_total,
		callers_emitted: context_graph.graph.callers.len(),
		callees_total: context_graph.callees_total,
		callees_emitted: context_graph.graph.callees.len(),
		notes_total,
		notes_emitted: notes.len(),
		rules_total,
		rules_emitted: rules.len(),
		changes_total: changes.total,
		changes_emitted: changes.files.len() + changes.symbols.len(),
	};
	Ok(QueryResponse {
		generation: response.generation,
		result: QueryResult::ChangeContext(Box::new(ChangeContextResult {
			focus: context_graph.focus,
			source: context_graph.source,
			graph: Box::new(context_graph.graph),
			notes,
			rules,
			changed_files: changes.files,
			changed_symbols: changes.symbols,
			suggested_checks,
			coverage,
		})),
		next_cursor: None,
	})
}

struct BoundedContextGraph {
	graph: SymbolGraphResult,
	focus: SymbolGraphFocus,
	file: String,
	source: Option<SourceSnippet>,
	members_total: usize,
	internal_edges_total: usize,
	callers_total: usize,
	callees_total: usize,
}

fn bounded_context_graph(
	snapshot: &WorkspaceSnapshot,
	response: ResponseContext<'_>,
	query: &ChangeContextQuery,
	max_items: usize,
) -> Result<BoundedContextGraph, QueryError> {
	let graph_response = symbol_graph_response(
		snapshot,
		response.roots,
		SymbolGraphQuery {
			workspace: query.workspace.clone(),
			focus: query.focus.clone(),
			..Default::default()
		},
		response.generation,
	)?;
	let QueryResult::SymbolGraph(graph) = graph_response.result else {
		return Err(QueryError::new(
			"graph_contract",
			"unexpected symbol graph response",
		));
	};
	let mut graph = *graph;
	let members_total = graph.coverage.members.total;
	let internal_edges_total = graph.coverage.internal_edges.total;
	let callers_total = graph.coverage.callers.total;
	let callees_total = graph.coverage.callees.total;
	graph.callers.truncate(max_items);
	graph.callees.truncate(max_items);
	graph.members.truncate(max_items);
	graph.internal_edges.truncate(max_items);
	graph.coverage.members.returned = graph.members.len();
	graph.coverage.internal_edges.returned = graph.internal_edges.len();
	graph.coverage.callers.returned = graph.callers.len();
	graph.coverage.callees.returned = graph.callees.len();
	let focus = graph.focus.clone();
	let (file, _, _) = focus_rule_coordinates(snapshot, &focus)?;
	let source = match &focus {
		SymbolGraphFocus::Symbol { symbol } => {
			let detail = symbol_detail_response(
				snapshot,
				response.roots,
				query.workspace.as_deref(),
				&symbol.uri,
				2,
				response.generation,
			)?;
			match detail.result {
				QueryResult::SymbolDetail(detail) => detail.source,
				_ => None,
			}
		}
		SymbolGraphFocus::File { .. } => None,
	};
	Ok(BoundedContextGraph {
		graph,
		focus,
		file,
		source,
		members_total,
		internal_edges_total,
		callers_total,
		callees_total,
	})
}

fn context_notes(
	daemon: &mut WorkspaceDaemon,
	snapshot: &WorkspaceSnapshot,
	focus: &SymbolGraphFocus,
	max_items: usize,
	generation: Option<WorkspaceGeneration>,
) -> Result<(usize, Vec<NoteDto>), QueryError> {
	Ok(match focus {
		SymbolGraphFocus::Symbol { symbol } => {
			let notes = notes_response(
				daemon,
				snapshot,
				NotesQuery {
					action: NotesAction::List,
					id: None,
					moniker: Some(symbol.uri.clone()),
					kind: None,
					status: None,
					title: None,
					body: None,
					created_by: None,
					orphan: None,
					include_done: false,
				},
				Page {
					cursor: None,
					limit: max_items,
				},
				generation,
			)?;
			match notes.result {
				QueryResult::Notes(notes) => (notes.total, notes.rows),
				_ => (0, Vec::new()),
			}
		}
		SymbolGraphFocus::File { .. } => (0, Vec::new()),
	})
}

fn context_rules(
	snapshot: &WorkspaceSnapshot,
	response: ResponseContext<'_>,
	query: &ChangeContextQuery,
	max_items: usize,
) -> Result<(usize, Vec<RuleApplicabilityDto>), QueryError> {
	let applicable = rules_applicable_response(
		snapshot,
		response,
		RulesApplicableQuery {
			workspace: query.workspace.clone(),
			focus: query.focus.clone(),
			profile: query.profile.clone(),
			rules: None,
		},
		Page {
			cursor: None,
			limit: usize::MAX,
		},
	)?;
	let QueryResult::RulesApplicable(applicable) = applicable.result else {
		return Err(QueryError::new(
			"rules_contract",
			"unexpected applicable rules response",
		));
	};
	let total = applicable
		.rows
		.iter()
		.filter(|row| row.status == "applicable")
		.count();
	let rows = applicable
		.rows
		.into_iter()
		.filter(|row| row.status == "applicable")
		.take(max_items)
		.collect::<Vec<_>>();
	Ok((total, rows))
}

struct ContextChanges {
	total: usize,
	files: Vec<ChangeReviewFile>,
	symbols: Vec<ChangeReviewSymbol>,
}

fn context_changes(
	snapshot: &WorkspaceSnapshot,
	response: ResponseContext<'_>,
	workspace: Option<String>,
	file: &str,
	max_items: usize,
) -> Result<ContextChanges, QueryError> {
	let review = change_review_response(
		snapshot,
		response.roots,
		ChangeReviewQuery { workspace },
		response.generation,
	)?;
	let QueryResult::ChangeReview(review) = review.result else {
		return Err(QueryError::new(
			"change_contract",
			"unexpected change review response",
		));
	};
	let all_changed_files = review
		.files
		.iter()
		.filter(|changed| {
			changed.old_path.as_deref() == Some(file) || changed.new_path.as_deref() == Some(file)
		})
		.cloned()
		.collect::<Vec<_>>();
	let all_changed_symbols = review
		.symbol_changes
		.iter()
		.filter(|changed| {
			changed.old.as_ref().is_some_and(|side| side.file == file)
				|| changed.new.as_ref().is_some_and(|side| side.file == file)
		})
		.cloned()
		.collect::<Vec<_>>();
	let changes_total = all_changed_files.len() + all_changed_symbols.len();
	let changed_files = all_changed_files
		.into_iter()
		.take(max_items)
		.collect::<Vec<_>>();
	let changed_symbols = all_changed_symbols
		.into_iter()
		.take(max_items.saturating_sub(changed_files.len()))
		.collect::<Vec<_>>();
	Ok(ContextChanges {
		total: changes_total,
		files: changed_files,
		symbols: changed_symbols,
	})
}

fn rules_check_response(
	cache: &LocalResourceCache,
	snapshot: Arc<WorkspaceSnapshot>,
	response: ResponseContext<'_>,
	request: RulesCheckEval,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(response.roots, request.workspace.as_deref())?;
	let mut check_roots = selected_roots
		.iter()
		.map(|root| (*root).to_path_buf())
		.collect::<Vec<_>>();
	if workspace_selector_is_all(request.workspace.as_deref())
		&& has_memory_sources(&snapshot, response.roots)
	{
		check_roots.push(PathBuf::from(MEMORY_SOURCE_ROOT));
	}
	let mut roots = Vec::new();
	for root in &check_roots {
		let workspace =
			IndexedCheckWorkspace::from_snapshot(root.clone(), cache, Arc::clone(&snapshot))
				.map_err(|error| {
					QueryError::new("indexed_corpus_unavailable", error.to_string())
				})?;
		roots.push(run_rules_for_root(IndexedRulesCheck {
			root,
			config_root: response.config_root,
			workspace: &workspace,
			profile: request.profile.clone(),
			rules: request.rules.as_deref(),
			files: &request.files,
			report: request.report,
		})?);
	}
	let exit = aggregate_check_exit(&roots);
	let verdict = RulesCheckVerdict::from_exit(&exit);
	let summary = aggregate_check_summary(&roots);
	let rows = rules_check_rows(&roots);
	let paged = page_rows(rows, request.page, response.generation)?;
	let mut violations = Vec::new();
	let mut errors = Vec::new();
	let mut rule_reports = Vec::new();
	let mut skip_reasons = Vec::new();
	for row in paged.items {
		match row {
			RulesCheckRow::Violation(violation) => violations.push(violation),
			RulesCheckRow::Error(error) => errors.push(error),
			RulesCheckRow::RuleReport(report) => rule_reports.push(*report),
			RulesCheckRow::SkipReason(reason) => skip_reasons.push(reason),
		}
	}
	let root_summaries = roots
		.into_iter()
		.map(clear_root_payloads)
		.collect::<Vec<_>>();
	Ok(QueryResponse {
		generation: response.generation,
		result: QueryResult::RulesCheck(RulesCheckResult {
			verdict,
			exit,
			summary,
			roots: root_summaries,
			violations,
			errors,
			rule_reports,
			skip_reasons,
		}),
		next_cursor: paged.next_cursor,
	})
}

fn clear_root_payloads(mut root: RulesCheckRootResult) -> RulesCheckRootResult {
	root.violations.clear();
	root.errors.clear();
	root.rule_reports.clear();
	root.skip_reason = None;
	root
}

fn notes_response(
	daemon: &mut WorkspaceDaemon,
	snapshot: &WorkspaceSnapshot,
	mut request: NotesQuery,
	page: Page,
	generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	if let Some(moniker) = request.moniker.as_deref() {
		match find_symbol(snapshot, moniker) {
			Ok(symbol) => request.moniker = Some(symbol.identity.to_string()),
			Err(error) if error.code == "symbol_not_found" => {}
			Err(error) => return Err(error),
		}
	}
	daemon
		.notes
		.reload(&daemon.roots)
		.map_err(|err| QueryError::new("notes_load_failed", err.to_string()))?;
	let action = request.action;
	let deleted = match action {
		NotesAction::Create => {
			let note = note_from_create(&daemon.notes.snapshot().map_err(note_error)?, &request)?;
			let id = note.id.clone();
			daemon
				.notes
				.mutate(&daemon.roots, |document| {
					document.insert(note)?;
					Ok(())
				})
				.map_err(note_error)?;
			Some(id)
		}
		NotesAction::Update => {
			if request.status.is_some() {
				return Err(QueryError::new(
					"invalid_note_update",
					"status changes require action=transition",
				));
			}
			let id = required_note_id(&request)?;
			let changes = note_changes(&request)?;
			daemon
				.notes
				.mutate(&daemon.roots, |document| {
					document.update(id, changes, current_timestamp())?;
					Ok(())
				})
				.map_err(note_error)?;
			Some(NoteId::new(id))
		}
		NotesAction::Transition => {
			let id = required_note_id(&request)?;
			let status = request
				.status
				.as_deref()
				.ok_or_else(|| QueryError::new("missing_status", "status is required"))?;
			let status = parse_note_status(status)?;
			daemon
				.notes
				.mutate(&daemon.roots, |document| {
					document.transition(id, status, current_timestamp())?;
					Ok(())
				})
				.map_err(note_error)?;
			Some(NoteId::new(id))
		}
		NotesAction::Delete => {
			let id = required_note_id(&request)?;
			let deleted = daemon
				.notes
				.mutate(&daemon.roots, |document| document.delete(id))
				.map_err(note_error)?;
			return notes_query_response(NotesResponseInput {
				snapshot,
				action,
				notes: Vec::new(),
				deleted: Some(deleted),
				orphan: None,
				page,
				generation,
			});
		}
		NotesAction::List | NotesAction::Get => None,
	};
	daemon.notes.reload(&daemon.roots).map_err(note_error)?;
	let document = daemon.notes.snapshot().map_err(note_error)?;
	let mut notes = document.notes;
	if let Some(id) = deleted {
		notes.retain(|note| note.id == id);
	}
	if action == NotesAction::Get {
		let id = required_note_id(&request)?;
		notes.retain(|note| note.id.as_str() == id);
		if notes.is_empty() {
			return Err(QueryError::new(
				"note_not_found",
				format!("note id `{id}` does not exist"),
			));
		}
	}
	if action == NotesAction::List {
		notes = filter_notes(notes, &request);
	}
	notes_query_response(NotesResponseInput {
		snapshot,
		action,
		notes,
		deleted: None,
		orphan: request.orphan,
		page,
		generation,
	})
}

fn note_changes(request: &NotesQuery) -> Result<NoteChanges, QueryError> {
	Ok(NoteChanges {
		moniker: request.moniker.clone(),
		kind: request.kind.as_deref().map(parse_note_kind).transpose()?,
		title: request.title.clone(),
		body: request.body.clone(),
	})
}

fn notes_query_response(input: NotesResponseInput<'_>) -> Result<QueryResponse, QueryError> {
	let mut resolved = resolve_notes(&input.notes, input.snapshot);
	if let Some(orphan) = input.orphan {
		resolved.retain(|note| note.resolution.is_orphan() == orphan);
	}
	let rows = resolved
		.iter()
		.map(note_dto)
		.collect::<Result<Vec<_>, _>>()?;
	let paged = page_rows(rows, input.page, input.generation)?;
	let deleted = input
		.deleted
		.as_ref()
		.map(|note| note_dto_from_note(note, input.snapshot))
		.transpose()?;
	Ok(QueryResponse {
		generation: input.generation,
		result: QueryResult::Notes(NotesResult {
			action: notes_action_label(input.action).to_string(),
			total: paged.total,
			rows: paged.items,
			deleted,
		}),
		next_cursor: paged.next_cursor,
	})
}

fn note_error(error: anyhow::Error) -> QueryError {
	QueryError::new("notes_failed", error.to_string())
}

fn note_from_create(document: &NotesDocument, request: &NotesQuery) -> Result<Note, QueryError> {
	let moniker = required_note_string(request.moniker.as_deref(), "moniker")?.to_string();
	let title = required_note_string(request.title.as_deref(), "title")?.to_string();
	let now = current_timestamp();
	let id = request
		.id
		.as_deref()
		.map(NoteId::new)
		.unwrap_or_else(|| generated_note_id(document));
	Ok(Note {
		id,
		moniker,
		kind: request
			.kind
			.as_deref()
			.map(parse_note_kind)
			.transpose()?
			.unwrap_or(NoteKind::Note),
		status: request
			.status
			.as_deref()
			.map(parse_note_status)
			.transpose()?
			.unwrap_or(NoteStatus::Pending),
		title,
		body: request.body.clone().unwrap_or_default(),
		created_by: request
			.created_by
			.as_deref()
			.map(parse_note_author)
			.transpose()?
			.unwrap_or(NoteAuthor::Agent),
		created_at: now.clone(),
		updated_at: now,
	})
}

fn generated_note_id(document: &NotesDocument) -> NoteId {
	for attempt in 0..1000_u32 {
		let nanos = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map(|duration| duration.as_nanos())
			.unwrap_or_default();
		let id = NoteId::new(format!("note_{nanos:x}_{attempt:x}"));
		if document.get(id.as_str()).is_none() {
			return id;
		}
	}
	NoteId::new("note_exhausted")
}

fn required_note_id(request: &NotesQuery) -> Result<&str, QueryError> {
	required_note_string(request.id.as_deref(), "id")
}

fn required_note_string<'a>(value: Option<&'a str>, key: &str) -> Result<&'a str, QueryError> {
	value.ok_or_else(|| QueryError::new(format!("missing_{key}"), format!("{key} is required")))
}

fn parse_note_kind(value: &str) -> Result<NoteKind, QueryError> {
	NoteKind::parse(value).map_err(|err| QueryError::new("invalid_note_kind", err.to_string()))
}

fn parse_note_status(value: &str) -> Result<NoteStatus, QueryError> {
	NoteStatus::parse(value).map_err(|err| QueryError::new("invalid_note_status", err.to_string()))
}

fn parse_note_author(value: &str) -> Result<NoteAuthor, QueryError> {
	NoteAuthor::parse(value).map_err(|err| QueryError::new("invalid_note_author", err.to_string()))
}

fn current_timestamp() -> String {
	let seconds = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|duration| duration.as_secs())
		.unwrap_or_default();
	format!("unix:{seconds}")
}

fn filter_notes(notes: Vec<Note>, request: &NotesQuery) -> Vec<Note> {
	notes
		.into_iter()
		.filter(|note| {
			request
				.moniker
				.as_ref()
				.is_none_or(|moniker| note.moniker == *moniker)
		})
		.filter(|note| request.include_done || note.status != NoteStatus::Done)
		.collect()
}

fn note_dto(note: &ResolvedNote) -> Result<NoteDto, QueryError> {
	Ok(NoteDto {
		id: note.note.id.as_str().to_string(),
		moniker: note.note.moniker.to_owned(),
		kind: note.note.kind.as_str().to_string(),
		status: note.note.status.as_str().to_string(),
		title: note.note.title.to_owned(),
		body: note.note.body.to_owned(),
		created_by: note.note.created_by.as_str().to_string(),
		updated_at: note.note.updated_at.to_owned(),
		resolution: note_resolution_dto(&note.resolution),
	})
}

fn note_dto_from_note(note: &Note, snapshot: &WorkspaceSnapshot) -> Result<NoteDto, QueryError> {
	let mut resolved = resolve_notes(std::slice::from_ref(note), snapshot);
	let resolved = resolved
		.pop()
		.ok_or_else(|| QueryError::new("note_resolution_failed", "note did not resolve"))?;
	note_dto(&resolved)
}

fn note_resolution_dto(resolution: &NoteResolution) -> NoteResolutionDto {
	match resolution {
		NoteResolution::Resolved {
			target_label,
			target_file,
			target_slice,
		} => NoteResolutionDto::Resolved {
			target: target_label.clone(),
			file: target_file.clone(),
			slice: *target_slice,
		},
		NoteResolution::Orphan => NoteResolutionDto::Orphan,
	}
}

fn notes_action_label(action: NotesAction) -> &'static str {
	match action {
		NotesAction::List => "list",
		NotesAction::Get => "get",
		NotesAction::Create => "create",
		NotesAction::Update => "update",
		NotesAction::Transition => "transition",
		NotesAction::Delete => "delete",
	}
}

#[derive(Debug)]
struct Paged<T> {
	items: Vec<T>,
	total: usize,
	next_cursor: Option<QueryCursor>,
}

enum RulesCheckRow {
	Violation(ViolationDto),
	Error(FileErrorDto),
	RuleReport(Box<RuleReportDto>),
	SkipReason(code_moniker_query::CheckSkipReasonDto),
}

fn rules_check_rows(roots: &[RulesCheckRootResult]) -> Vec<RulesCheckRow> {
	let mut rows = Vec::new();
	for root in roots {
		rows.extend(
			root.violations
				.iter()
				.cloned()
				.map(RulesCheckRow::Violation),
		);
		rows.extend(root.errors.iter().cloned().map(RulesCheckRow::Error));
		rows.extend(
			root.rule_reports
				.iter()
				.cloned()
				.map(Box::new)
				.map(RulesCheckRow::RuleReport),
		);
		rows.extend(
			root.skip_reason
				.iter()
				.cloned()
				.map(RulesCheckRow::SkipReason),
		);
	}
	rows
}

fn page_rows<T>(
	rows: Vec<T>,
	page: Page,
	generation: Option<WorkspaceGeneration>,
) -> Result<Paged<T>, QueryError> {
	validate_page_cursor(&page, generation)?;
	let total = rows.len();
	let start = page
		.cursor
		.as_ref()
		.map(|cursor| cursor.offset)
		.unwrap_or(0)
		.min(total);
	let end = start.saturating_add(page.limit).min(total);
	let next_cursor = (end < total).then(|| QueryCursor::new(end, generation));
	Ok(Paged {
		items: rows.into_iter().skip(start).take(end - start).collect(),
		total,
		next_cursor,
	})
}

fn validate_page_cursor(
	page: &Page,
	generation: Option<WorkspaceGeneration>,
) -> Result<(), QueryError> {
	if let Some(cursor) = page.cursor.as_ref() {
		if cursor.generation != generation {
			return Err(QueryError::new(
				"cursor_generation_mismatch",
				"query cursor belongs to a different workspace generation",
			));
		}
	}
	Ok(())
}

mod helpers {
	use super::*;

	pub(super) fn find_symbol<'a>(
		snapshot: &'a WorkspaceSnapshot,
		uri: &str,
	) -> Result<&'a SymbolRecord, QueryError> {
		if let Some(symbol) = snapshot
			.index
			.symbols
			.iter()
			.find(|symbol| symbol.identity.as_ref() == uri || symbol.id.to_string() == uri)
		{
			return Ok(symbol);
		}
		let mut matches = snapshot.index.symbols.iter().filter(|symbol| {
			compact_identity(symbol.identity.as_ref(), &snapshot.index.identity_scheme).as_deref()
				== Some(uri)
		});
		let Some(symbol) = matches.next() else {
			return Err(QueryError::new(
				"symbol_not_found",
				format!("symbol not found: {uri}"),
			));
		};
		if matches.next().is_some() {
			return Err(QueryError::new(
				"symbol_ambiguous",
				format!("compact moniker matches multiple symbols: {uri}"),
			));
		}
		Ok(symbol)
	}

	pub(super) fn symbol_dto(
		symbol: &SymbolRecord,
		source: &SourceFileRecord,
		roots: &[PathBuf],
	) -> SymbolDto {
		SymbolDto {
			root: source_root_label(roots, source),
			uri: symbol.identity.to_string(),
			id: symbol.id.to_string(),
			name: symbol.name.to_string(),
			kind: symbol.kind.to_string(),
			visibility: symbol.visibility.to_string(),
			signature: symbol.signature.to_string(),
			file: source.rel_path.to_string(),
			language: source.language.to_string(),
			line_range: symbol.line_range,
			navigable: symbol.navigable,
			score: None,
			match_reason: None,
			source: None,
		}
	}

	pub(super) fn symbol_search_dto(
		symbol: &SymbolRecord,
		source: &SourceFileRecord,
		roots: &[PathBuf],
		score: u32,
		reason: String,
	) -> SymbolDto {
		let mut dto = symbol_dto(symbol, source, roots);
		dto.score = Some(score);
		dto.match_reason = Some(reason);
		dto
	}

	pub(super) fn usage_dto(
		reference: &ReferenceRecord,
		direction: UsageDirection,
		context: &UsageDtoContext<'_>,
	) -> Option<UsageDto> {
		let source = WorkspaceView::new(context.snapshot)
			.sources()
			.record(&reference.source)?;
		source_root(context.roots, context.selected_roots, source)?;
		if !context.path_filter.matches(&source.rel_path)
			|| (!context.langs.is_empty()
				&& !context.langs.iter().any(|lang| lang == &source.language))
		{
			return None;
		}
		let source_symbol = WorkspaceView::new(context.snapshot)
			.symbols()
			.find(&reference.source_symbol);
		let actor = source_symbol
			.map(|symbol| symbol.name.to_string())
			.unwrap_or_else(|| reference.source_symbol.to_string());
		let source_context = source_symbol
			.map(|symbol| symbol.identity.to_string())
			.unwrap_or_else(|| reference.source_symbol.to_string());
		Some(UsageDto {
			root: source_root_label(context.roots, source),
			direction,
			reference: reference.id.to_string(),
			kind: reference.kind.to_string(),
			actor,
			context: source_context,
			endpoint: reference.target_identity.to_string(),
			file: source.rel_path.to_string(),
			prefix: path_prefix(&source.rel_path),
			location: reference_location(source, reference),
			line_range: reference.line_range,
			via: None,
		})
	}

	pub(super) fn source_snippet(
		source: &SourceFileRecord,
		symbol: &SymbolRecord,
		context_lines: usize,
	) -> Result<Option<SourceSnippet>, QueryError> {
		let Some((start, end)) = symbol.line_range else {
			return Ok(None);
		};
		let first = start.saturating_sub(context_lines as u32).max(1);
		let last = end.saturating_add(context_lines as u32);
		let source_text = load_source_text(source)?;
		let lines = source_text
			.lines()
			.enumerate()
			.filter_map(|(idx, text)| {
				let number = idx as u32 + 1;
				(number >= first && number <= last).then(|| SourceLine {
					number,
					text: text.to_string(),
				})
			})
			.collect();
		Ok(Some(SourceSnippet {
			file: source.rel_path.to_owned(),
			first_line: first,
			last_line: last,
			lines,
		}))
	}

	pub(super) fn load_source_text(source: &SourceFileRecord) -> Result<String, QueryError> {
		if source.text.is_empty() && !is_memory_source_path(Path::new(&source.path)) {
			std::fs::read_to_string(&source.path).map_err(|err| {
				QueryError::new(
					"source_read_failed",
					format!("cannot read source {}: {err}", source.path),
				)
			})
		} else {
			Ok(source.text.to_string())
		}
	}

	pub(super) fn workspace_langs(
		snapshot: &WorkspaceSnapshot,
		roots: &[PathBuf],
		root: &Path,
		filter: &[String],
	) -> Vec<Lang> {
		let mut langs = snapshot
			.index
			.sources
			.iter()
			.filter(|source| source_in_root(roots, source, root))
			.filter(|source| {
				filter.is_empty() || filter.iter().any(|lang| lang == &source.language)
			})
			.filter_map(|source| Lang::from_tag(&source.language))
			.collect::<Vec<_>>();
		langs.sort_by_key(|lang| lang.tag());
		langs.dedup();
		langs
	}

	pub(super) fn resolve_rules_path(root: &Path, rules: Option<&str>) -> PathBuf {
		let path = rules
			.map(PathBuf::from)
			.unwrap_or_else(|| PathBuf::from(".code-moniker.toml"));
		if path.is_absolute() {
			path
		} else {
			root.join(path)
		}
	}

	pub(super) fn violation_dto(root: &Path, path: &Path, violation: &Violation) -> ViolationDto {
		ViolationDto {
			root: root.display().to_string(),
			path: path.display().to_string(),
			rule_id: violation.rule_id.to_string(),
			severity: violation.severity.as_str().to_string(),
			moniker: violation.moniker.to_string(),
			srcset: violation.srcset.clone(),
			kind: violation.kind.to_string(),
			lines: violation.lines,
			message: violation.message.to_string(),
		}
	}

	pub(super) fn file_error_dto(root: &Path, path: &Path, error: &str) -> FileErrorDto {
		FileErrorDto {
			root: root.display().to_string(),
			path: path.display().to_string(),
			error: error.to_string(),
		}
	}

	pub(super) fn rule_report_dto(
		root: &Path,
		path: Option<&Path>,
		report: &RuleReport,
	) -> RuleReportDto {
		RuleReportDto {
			root: root.display().to_string(),
			path: path.map(|path| path.display().to_string()),
			rule_id: report.rule_id.to_string(),
			severity: report.severity.as_str().to_string(),
			domain: report.domain.to_string(),
			evaluated: report.evaluated,
			matches: report.matches,
			violations: report.violations,
			antecedent_matches: report.antecedent_matches,
			warning: report.warning.clone(),
			inconclusive: report.inconclusive,
			verdict: report.verdict.map(rule_verdict_label),
			coverage: report.coverage.as_ref().map(rule_coverage_dto),
			path_analysis: report.path.as_ref().map(rule_path_report_dto),
		}
	}

	fn rule_verdict_label(verdict: RuleVerdict) -> String {
		match verdict {
			RuleVerdict::Pass => "pass",
			RuleVerdict::Fail => "fail",
			RuleVerdict::Inconclusive => "inconclusive",
		}
		.to_string()
	}

	fn rule_coverage_dto(coverage: &RuleCoverage) -> RuleCoverageDto {
		RuleCoverageDto {
			total: coverage.total,
			decided: coverage.decided,
			resolved: coverage.resolved,
			external: coverage.external,
			candidate: coverage.candidate,
			dynamic: coverage.dynamic,
			blocked: coverage.blocked,
			unresolved: coverage.unresolved,
			percent: coverage.percent,
			min_percent: coverage.min_percent,
		}
	}

	fn rule_path_report_dto(path: &RulePathReport) -> RulePathReportDto {
		RulePathReportDto {
			expectation: path.expectation.clone(),
			relation: path.relation.clone(),
			max_depth: path.max_depth,
			max_symbols: path.max_symbols,
			max_edges: path.max_edges,
			max_pairs: path.max_pairs,
			min_coverage: path.min_coverage,
			source_symbols: path.source_symbols,
			target_symbols: path.target_symbols,
			via_symbols: path.via_symbols,
			evaluated_pairs: path.evaluated_pairs,
			explored_symbols: path.explored_symbols,
			explored_edges: path.explored_edges,
			depth_limit_reached: path.depth_limit_reached,
			symbol_limit_reached: path.symbol_limit_reached,
			edge_limit_reached: path.edge_limit_reached,
			pair_limit_reached: path.pair_limit_reached,
			reasons: path.reasons.clone(),
			witness: path.witness.iter().map(rule_path_step_dto).collect(),
		}
	}

	fn rule_path_step_dto(step: &RulePathStep) -> RulePathStepDto {
		RulePathStepDto {
			source: step.source.clone(),
			target: step.target.clone(),
			relation: step.relation.clone(),
			reference: step.reference.clone(),
			file: step.file.clone(),
			line_range: step.line_range,
		}
	}

	pub(super) fn rule_dto(root: &Path, spec: CompiledRuleSpec) -> RuleDto {
		RuleDto {
			root: root.display().to_string(),
			id: spec.rule_id,
			severity: spec.severity.as_str().to_string(),
			lang: spec.lang,
			rule_root: spec.root,
			subject: spec.subject,
			plan: spec.plan,
			capabilities: spec.capabilities,
			group_by: spec.group_by,
			domain: spec.domain,
			kind: spec.kind,
			expr: spec.expr,
			expanded_expr: spec.expanded_expr,
			message: spec.message,
			rationale: spec.rationale,
			require_doc_comment: spec.require_doc_comment,
		}
	}

	pub(super) fn run_rules_for_root(
		check: IndexedRulesCheck<'_>,
	) -> Result<RulesCheckRootResult, QueryError> {
		let rules_path = resolve_rules_path(check.config_root, check.rules);
		let rules = RuleSetRequest::with_rules(rules_path, DEFAULT_SCHEME)
			.with_default_rules(DefaultRulesSelection::Config)
			.with_profile(check.profile);
		let request = CheckRequest::new(check.root.to_path_buf(), rules)
			.with_report(check.report)
			.with_files(check.files.iter().map(PathBuf::from).collect());
		let run = request
			.run_with_workspace(check.workspace)
			.map_err(|err| QueryError::new("rules_check_failed", err.to_string()))?;
		let exit = check_exit(&run);
		let summary = check_summary_dto(&run.summary());
		let violations = run
			.file_violations()
			.map(|(path, violation)| violation_dto(check.root, path, violation))
			.collect();
		let errors = run
			.error_summaries()
			.map(|(path, error)| file_error_dto(check.root, path, error))
			.collect();
		let rule_reports = run
			.reports
			.iter()
			.flat_map(|report| {
				report
					.rule_reports
					.iter()
					.map(move |rule| rule_report_dto(check.root, Some(&report.path), rule))
			})
			.collect();
		let skip_reason = run
			.skip_reason
			.map(|reason| check_skip_reason_dto(check.root, reason));
		Ok(RulesCheckRootResult {
			root: check.root.display().to_string(),
			verdict: RulesCheckVerdict::from_exit(&exit),
			exit,
			summary,
			violations,
			errors,
			rule_reports,
			skip_reason,
		})
	}

	pub(super) fn check_exit(run: &code_moniker_check::CheckRun) -> String {
		if run.any_error() {
			"error"
		} else if run.any_error_violation() {
			"no_match"
		} else {
			"match"
		}
		.to_string()
	}

	pub(super) fn aggregate_check_exit(roots: &[RulesCheckRootResult]) -> String {
		if roots.iter().any(|root| root.exit == "error") {
			"error"
		} else if roots.iter().any(|root| root.exit == "no_match") {
			"no_match"
		} else {
			"match"
		}
		.to_string()
	}

	pub(super) fn aggregate_check_summary(roots: &[RulesCheckRootResult]) -> CheckSummaryDto {
		let mut summary = CheckSummaryDto::default();
		let mut unspecified_srcset = 0usize;
		for root in roots {
			summary.files_scanned += root.summary.files_scanned;
			summary.files_with_violations += root.summary.files_with_violations;
			summary.total_violations += root.summary.total_violations;
			summary.total_rule_errors += root.summary.total_rule_errors;
			summary.total_warnings += root.summary.total_warnings;
			summary.files_with_errors += root.summary.files_with_errors;
			summary.total_errors += root.summary.total_errors;
			summary.elapsed_ms += root.summary.elapsed_ms;
			summary
				.failed_rules
				.extend(root.summary.failed_rules.iter().cloned());
			unspecified_srcset += root
				.summary
				.total_violations
				.saturating_sub(root.summary.violations_by_srcset.values().sum::<usize>());
			for (srcset, violations) in &root.summary.violations_by_srcset {
				*summary
					.violations_by_srcset
					.entry(srcset.clone())
					.or_default() += violations;
			}
		}
		if !summary.violations_by_srcset.is_empty() && unspecified_srcset > 0 {
			*summary
				.violations_by_srcset
				.entry("unspecified".to_string())
				.or_default() += unspecified_srcset;
		}
		summary.failed_rules.sort_by(|a, b| {
			a.rule_id
				.cmp(&b.rule_id)
				.then_with(|| a.severity.cmp(&b.severity))
		});
		summary
	}

	pub(super) fn check_summary_dto(summary: &CheckSummary) -> CheckSummaryDto {
		CheckSummaryDto {
			files_scanned: summary.files_scanned,
			files_with_violations: summary.files_with_violations,
			total_violations: summary.total_violations,
			total_rule_errors: summary.total_rule_errors,
			total_warnings: summary.total_warnings,
			files_with_errors: summary.files_with_errors,
			total_errors: summary.total_errors,
			elapsed_ms: summary.elapsed_ms,
			failed_rules: summary
				.failed_rules
				.iter()
				.map(|rule| FailedRuleDto {
					rule_id: rule.rule_id.to_string(),
					severity: rule.severity.as_str().to_string(),
					violations: rule.violations,
				})
				.collect(),
			violations_by_srcset: summary.violations_by_srcset.clone(),
		}
	}

	pub(super) fn check_skip_reason_dto(
		root: &Path,
		reason: CheckSkipReason,
	) -> code_moniker_query::CheckSkipReasonDto {
		let reason = match reason {
			CheckSkipReason::ExcludedSingleFile => "excluded_single_file",
			CheckSkipReason::UnsupportedSingleFile => "unsupported_single_file",
			CheckSkipReason::NoMatchingFiles => "no_matching_files",
		};
		code_moniker_query::CheckSkipReasonDto {
			root: root.display().to_string(),
			reason: reason.to_string(),
		}
	}

	pub(super) fn root_status(
		snapshot: &WorkspaceSnapshot,
		roots: &[PathBuf],
		root: &Path,
		stale: bool,
		stale_summary: &str,
	) -> WorkspaceRootStatus {
		let sources = snapshot
			.index
			.sources
			.iter()
			.filter(|source| source_in_root(roots, source, root))
			.collect::<Vec<_>>();
		let source_ids = sources
			.iter()
			.map(|source| source.id)
			.collect::<std::collections::BTreeSet<_>>();
		WorkspaceRootStatus {
			root: root.display().to_string(),
			generation: Some(WorkspaceGeneration(snapshot.generation.value())),
			files: sources.len(),
			symbols: snapshot
				.index
				.symbols
				.iter()
				.filter(|symbol| source_ids.contains(&symbol.source))
				.count(),
			references: snapshot
				.index
				.references
				.iter()
				.filter(|reference| source_ids.contains(&reference.source))
				.count(),
			stale,
			stale_summary: stale_summary.to_string(),
		}
	}

	pub(super) fn selected_roots<'a>(
		roots: &'a [PathBuf],
		selector: Option<&str>,
	) -> Result<Vec<&'a PathBuf>, QueryError> {
		if selector.is_none_or(|selector| selector.trim().is_empty()) {
			return Ok(roots.iter().collect());
		}
		let selected = roots
			.iter()
			.filter(|root| root_matches_selector(root, selector))
			.collect::<Vec<_>>();
		if selected.is_empty() {
			let value = selector.unwrap_or("<all>");
			return Err(QueryError::new(
				"workspace_not_found",
				format!("workspace selector matched no root: {value}"),
			));
		}
		if selected.len() > 1 {
			let value = selector.unwrap_or("<all>");
			return Err(QueryError::new(
				"workspace_selector_ambiguous",
				format!("workspace selector matched multiple roots: {value}"),
			));
		}
		Ok(selected)
	}

	pub(super) fn root_matches_selector(root: &Path, selector: Option<&str>) -> bool {
		let Some(selector) = selector.map(str::trim).filter(|value| !value.is_empty()) else {
			return true;
		};
		root.display().to_string() == selector
			|| root
				.file_name()
				.and_then(|name| name.to_str())
				.is_some_and(|name| name == selector)
	}

	pub(super) fn source_root<'a>(
		roots: &'a [PathBuf],
		selected_roots: &[&PathBuf],
		source: &SourceFileRecord,
	) -> Option<&'a Path> {
		let Some(root) = roots.get(source.source_root) else {
			return (source.source_root == roots.len() && selected_roots.len() == roots.len())
				.then(|| Path::new(MEMORY_SOURCE_ROOT));
		};
		selected_roots
			.iter()
			.any(|selected| selected.as_path() == root.as_path())
			.then_some(root.as_path())
	}

	pub(super) fn source_in_root(
		roots: &[PathBuf],
		source: &SourceFileRecord,
		root: &Path,
	) -> bool {
		if source.source_root == roots.len() {
			return root == Path::new(MEMORY_SOURCE_ROOT);
		}
		roots
			.get(source.source_root)
			.is_some_and(|declared_root| declared_root == root)
	}

	pub(super) fn source_root_label(roots: &[PathBuf], source: &SourceFileRecord) -> String {
		if source.source_root == roots.len() {
			return MEMORY_SOURCE_ROOT_LABEL.to_string();
		}
		roots
			.get(source.source_root)
			.map(|root| root.display().to_string())
			.unwrap_or_default()
	}

	pub(super) fn has_memory_sources(snapshot: &WorkspaceSnapshot, roots: &[PathBuf]) -> bool {
		let active_sources = snapshot
			.catalog
			.sources
			.iter()
			.map(|source| source.id)
			.collect::<HashSet<_>>();
		snapshot
			.index
			.sources
			.iter()
			.any(|source| source.source_root == roots.len() && active_sources.contains(&source.id))
	}

	pub(super) fn workspace_selector_is_all(selector: Option<&str>) -> bool {
		selector.is_none_or(|selector| selector.trim().is_empty())
	}

	pub(super) fn sorted_counts<I>(values: I) -> Vec<CountDto>
	where
		I: IntoIterator<Item = String>,
	{
		let mut counts = BTreeMap::<String, usize>::new();
		for value in values {
			*counts.entry(value).or_default() += 1;
		}
		count_rows(counts)
	}

	pub(super) fn count_rows(counts: BTreeMap<String, usize>) -> Vec<CountDto> {
		let mut rows = counts
			.into_iter()
			.map(|(name, count)| CountDto { name, count })
			.collect::<Vec<_>>();
		rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
		rows
	}

	pub(super) fn path_prefix(path: &str) -> String {
		let parts = Path::new(path)
			.parent()
			.unwrap_or_else(|| Path::new(""))
			.components()
			.filter_map(|component| component.as_os_str().to_str())
			.take(2)
			.collect::<Vec<_>>();
		if parts.is_empty() {
			"<root>".to_string()
		} else {
			parts.join("/")
		}
	}

	pub(super) fn reference_location(
		source: &SourceFileRecord,
		reference: &ReferenceRecord,
	) -> String {
		let suffix = reference
			.line_range
			.map(|(start, end)| {
				if start == end {
					format!(":L{start}")
				} else {
					format!(":L{start}-L{end}")
				}
			})
			.unwrap_or_else(|| ":L?".to_string());
		format!("{}{}", source.rel_path, suffix)
	}

	pub(super) fn root_labels(roots: &[PathBuf]) -> Vec<String> {
		roots
			.iter()
			.map(|root| root.display().to_string())
			.collect()
	}

	pub(super) fn common_workspace_root(roots: &[PathBuf]) -> anyhow::Result<PathBuf> {
		let Some(first) = roots.first() else {
			anyhow::bail!("workspace daemon requires at least one root");
		};
		let mut common = first.clone();
		for root in roots.iter().skip(1) {
			while !root.starts_with(&common) {
				if !common.pop() {
					anyhow::bail!("cannot find common root for workspace daemon roots");
				}
			}
		}
		Ok(common)
	}

	pub(super) fn rules_config_root(roots: &[PathBuf]) -> anyhow::Result<PathBuf> {
		let common = common_workspace_root(roots)?;
		let mut cursor = if common.is_file() {
			common
				.parent()
				.map(Path::to_path_buf)
				.unwrap_or_else(|| common.clone())
		} else {
			common.clone()
		};
		loop {
			if cursor.join(".code-moniker.toml").is_file() {
				return Ok(cursor);
			}
			if !cursor.pop() {
				return Ok(common);
			}
		}
	}

	pub(super) fn workspace_label_from_paths(roots: &[&PathBuf]) -> String {
		if roots.len() == 1 {
			roots[0].display().to_string()
		} else {
			roots
				.iter()
				.map(|root| root.display().to_string())
				.collect::<Vec<_>>()
				.join(";")
		}
	}
}

#[allow(dead_code)]
fn _assert_public_boundary_types(_: ReferenceId, _: SourceId, _: SymbolId, _: RuleSeverity) {}

#[cfg(test)]
mod tests {
	use std::fs;

	use code_moniker_query::{
		Page, ProtocolRequest, ProtocolResponse, Query, QueryCursor, QueryRequest, QueryResult,
		RulesCheckQuery, SyntaxNodeDto, SyntaxTreeQuery, WorkspaceGeneration,
		WorkspaceSourceDocumentDto,
	};

	use super::*;

	fn test_rpc_service(
		daemon: WorkspaceDaemon,
		roots: Vec<PathBuf>,
		events: tokio::sync::broadcast::Sender<WorkspaceEventDto>,
	) -> DaemonRpcService {
		DaemonRpcService {
			daemon: Arc::new(Mutex::new(daemon)),
			published: Arc::new(RwLock::new(None)),
			lifecycle: Arc::new(RwLock::new(WorkspaceLifecycle::ready())),
			roots: Arc::from(roots),
			events,
			shutdown: Arc::new(tokio::sync::Notify::new()),
			handshake: HandshakeResponse {
				protocol_version: code_moniker_query::PROTOCOL_VERSION,
				daemon_version: "test".to_string(),
				build: producer_identity(),
				workspace_root: "test".to_string(),
				workspace_roots: Vec::new(),
				capabilities: CapabilitySet::default(),
			},
		}
	}

	fn hold_workspace_lock(
		daemon: Arc<Mutex<WorkspaceDaemon>>,
	) -> (std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>) {
		let (locked_tx, locked_rx) = std::sync::mpsc::channel();
		let (release_tx, release_rx) = std::sync::mpsc::channel();
		let holder = std::thread::spawn(move || {
			let _workspace_lock = daemon.lock().expect("workspace lock");
			locked_tx.send(()).expect("announce workspace lock");
			let _ = release_rx.recv();
		});
		locked_rx.recv().expect("wait for workspace lock");
		(release_tx, holder)
	}

	fn search_symbols(daemon: &mut WorkspaceDaemon, text: &str) -> QueryResult {
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::SymbolSearch(code_moniker_query::SymbolSearchQuery {
				workspace: None,
				text: Some(text.to_string()),
				path: Vec::new(),
				lang: Vec::new(),
				kind: Vec::new(),
				shape: Vec::new(),
				name: None,
				include_non_navigable: false,
				include_code: false,
				context_lines: 0,
				projection: Vec::new(),
			}),
			consistency: code_moniker_query::Consistency::Current,
			page: Page::default(),
		})));
		match response {
			ProtocolResponse::Query(query) => query.result,
			other => panic!("expected query response, got {other:?}"),
		}
	}

	fn search_symbols_named(daemon: &mut WorkspaceDaemon, name: &str) -> QueryResult {
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::SymbolSearch(code_moniker_query::SymbolSearchQuery {
				name: Some(format!("^{}$", regex::escape(name))),
				include_code: true,
				context_lines: 0,
				..Default::default()
			}),
			consistency: code_moniker_query::Consistency::Current,
			page: Page::default(),
		})));
		match response {
			ProtocolResponse::Query(query) => query.result,
			other => panic!("expected query response, got {other:?}"),
		}
	}

	fn replace_source_set(
		daemon: &mut WorkspaceDaemon,
		source_set: WorkspaceSourceSetDto,
	) -> CommandResponse {
		let response = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceSourceSetReplace { source_set },
		}));
		let ProtocolResponse::Command(response) = response else {
			panic!("expected source-set replacement, got {response:?}");
		};
		response
	}

	fn remove_source_set(daemon: &mut WorkspaceDaemon, srcset: &str) -> CommandResponse {
		let response = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceSourceSetRemove {
				srcset: srcset.to_string(),
			},
		}));
		let ProtocolResponse::Command(response) = response else {
			panic!("expected source-set removal, got {response:?}");
		};
		response
	}

	fn assert_symbol_total(daemon: &mut WorkspaceDaemon, text: &str, expected: usize) {
		let QueryResult::SymbolList(symbols) = search_symbols_named(daemon, text) else {
			panic!("expected symbol list");
		};
		assert_eq!(symbols.total, expected, "{symbols:?}");
	}

	fn assert_memory_root_absent_from_rules(daemon: &mut WorkspaceDaemon, rules: &Path) {
		let listed = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::RulesList(code_moniker_query::RulesListQuery {
				rules: Some(rules.display().to_string()),
				..Default::default()
			}),
		))));
		let ProtocolResponse::Query(listed) = listed else {
			panic!("expected rules list, got {listed:?}");
		};
		let QueryResult::RulesList(listed) = listed.result else {
			panic!("expected rules list result, got {:?}", listed.result);
		};
		assert!(
			listed.roots.iter().all(|root| root != MEMORY_SOURCE_ROOT),
			"removed memory roots must disappear from rules.list: {listed:?}"
		);

		let checked = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::RulesCheck(RulesCheckQuery {
				workspace: None,
				profile: None,
				rules: Some(rules.display().to_string()),
				file: Vec::new(),
				report: true,
			}),
		))));
		let ProtocolResponse::Query(checked) = checked else {
			panic!("expected rules check, got {checked:?}");
		};
		let QueryResult::RulesCheck(checked) = checked.result else {
			panic!("expected rules check result, got {:?}", checked.result);
		};
		assert!(
			checked
				.roots
				.iter()
				.all(|root| root.root != MEMORY_SOURCE_ROOT),
			"removed memory roots must disappear from rules.check: {checked:?}"
		);
	}

	#[test]
	fn rules_check_evaluates_the_selected_daemon_generation() {
		let temp = tempfile::tempdir().expect("tempdir");
		let src = temp.path().join("src");
		fs::create_dir_all(&src).expect("src dir");
		let lib = src.join("lib.rs");
		fs::write(&lib, "pub fn indexed_name() {}\n").expect("write indexed source");
		let rules = temp.path().join("scratch-rules.toml");
		fs::write(
			&rules,
			r#"
default_rules = false

[[rust.fn.where]]
id = "indexed-name-is-visible"
expr = "name != 'indexed_name'"
message = "the rule must observe the indexed generation"
"#,
		)
		.expect("write rules");
		let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
			roots: vec![temp.path().display().to_string()],
			project: None,
			cache_dir: None,
			live_refresh: Some("on-demand".to_string()),
		})
		.expect("daemon");
		let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refreshed, ProtocolResponse::Command(_)));

		fs::write(&lib, "pub fn filesystem_name() {}\n").expect("change filesystem source");
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::RulesCheck(RulesCheckQuery {
				workspace: None,
				profile: None,
				rules: Some(rules.display().to_string()),
				file: Vec::new(),
				report: true,
			}),
			consistency: code_moniker_query::Consistency::StaleOk,
			page: Page::default(),
		})));
		let ProtocolResponse::Query(response) = response else {
			panic!("expected rules check response, got {response:?}");
		};
		assert_eq!(response.generation, Some(WorkspaceGeneration(1)));
		let QueryResult::RulesCheck(result) = response.result else {
			panic!("expected rules check result, got {:?}", response.result);
		};
		assert_eq!(result.summary.files_scanned, 1, "{result:?}");
		assert_eq!(result.summary.total_violations, 1, "{result:?}");
		assert_eq!(
			result.violations[0].rule_id,
			"rust.fn.indexed-name-is-visible"
		);

		fs::write(
			&rules,
			r#"
default_rules = false

[[rust.fn.where]]
id = "changed-rules-are-reloaded"
expr = "name == 'filesystem_name'"
message = "the current rules file must run against the pinned index"
"#,
		)
		.expect("change rules");
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::RulesCheck(RulesCheckQuery {
				workspace: None,
				profile: None,
				rules: Some(rules.display().to_string()),
				file: Vec::new(),
				report: true,
			}),
			consistency: code_moniker_query::Consistency::StaleOk,
			page: Page::default(),
		})));
		let ProtocolResponse::Query(response) = response else {
			panic!("expected rules check response, got {response:?}");
		};
		assert_eq!(response.generation, Some(WorkspaceGeneration(1)));
		let QueryResult::RulesCheck(result) = response.result else {
			panic!("expected rules check result, got {:?}", response.result);
		};
		assert_eq!(result.summary.total_violations, 1, "{result:?}");
		assert_eq!(
			result.violations[0].rule_id,
			"rust.fn.changed-rules-are-reloaded"
		);
	}

	#[test]
	fn rules_check_scopes_nested_roots_by_source_identity() {
		let temp = tempfile::tempdir().expect("tempdir");
		let child = temp.path().join("apps/child");
		fs::create_dir_all(&child).expect("child root");
		let parent = temp.path().canonicalize().expect("canonical parent");
		let child = child.canonicalize().expect("canonical child");
		fs::write(parent.join("parent.rs"), "pub fn parent_fn() {}\n").expect("parent source");
		let child_file = child.join("child.rs");
		fs::write(&child_file, "pub fn child_fn() {}\n").expect("child source");
		let rules = parent.join("scratch-rules.toml");
		fs::write(
			&rules,
			r#"
default_rules = false

[[rust.fn.where]]
id = "count-selected-source-root"
expr = "name == 'never'"
message = "every selected function is observable"
"#,
		)
		.expect("rules");
		let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
			roots: vec![parent.display().to_string(), child.display().to_string()],
			project: None,
			cache_dir: None,
			live_refresh: Some("on-demand".to_string()),
		})
		.expect("daemon");
		let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refreshed, ProtocolResponse::Command(_)));

		let parent_result = rules_check_result(&mut daemon, &rules, &parent, Vec::new());
		assert_eq!(parent_result.summary.files_scanned, 2, "{parent_result:?}");
		assert_eq!(
			parent_result.summary.total_violations, 2,
			"{parent_result:?}"
		);
		let parent_child_moniker = parent_result
			.violations
			.iter()
			.find(|violation| violation.path == child_file.display().to_string())
			.expect("child file through parent source root")
			.moniker
			.clone();

		let nested = rules_check_result(&mut daemon, &rules, &child, Vec::new());
		assert_eq!(nested.summary.files_scanned, 1, "{nested:?}");
		assert_eq!(nested.summary.total_violations, 1, "{nested:?}");
		let nested_moniker = nested.violations[0].moniker.clone();
		assert_ne!(
			parent_child_moniker, nested_moniker,
			"the same physical file must keep the selected source root anchor"
		);

		for file in [
			vec!["apps/child/child.rs".to_string()],
			vec![child_file.display().to_string()],
		] {
			let filtered = rules_check_result(&mut daemon, &rules, &parent, file);
			assert_eq!(filtered.summary.files_scanned, 1, "{filtered:?}");
			assert_eq!(filtered.violations[0].moniker, parent_child_moniker);
		}

		for file in [
			vec!["child.rs".to_string()],
			vec![child_file.display().to_string()],
		] {
			let filtered = rules_check_result(&mut daemon, &rules, &child, file);
			assert_eq!(filtered.summary.files_scanned, 1, "{filtered:?}");
			assert_eq!(filtered.summary.total_violations, 1, "{filtered:?}");
			assert_eq!(filtered.violations[0].moniker, nested_moniker);
		}
	}

	fn rules_check_result(
		daemon: &mut WorkspaceDaemon,
		rules: &Path,
		workspace: &Path,
		file: Vec<String>,
	) -> code_moniker_query::RulesCheckResult {
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::RulesCheck(RulesCheckQuery {
				workspace: Some(workspace.display().to_string()),
				profile: None,
				rules: Some(rules.display().to_string()),
				file,
				report: true,
			}),
			consistency: code_moniker_query::Consistency::StaleOk,
			page: Page::default(),
		})));
		let ProtocolResponse::Query(response) = response else {
			panic!("expected rules check response, got {response:?}");
		};
		let QueryResult::RulesCheck(result) = response.result else {
			panic!("expected rules check result, got {:?}", response.result);
		};
		result
	}

	fn assert_filtered_outgoing_graph(daemon: &mut WorkspaceDaemon, entry_uri: String) {
		let filtered = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::SymbolGraph(code_moniker_query::SymbolGraphQuery {
				workspace: None,
				focus: entry_uri,
				direction: code_moniker_query::UsageDirection::Outgoing,
				relation: vec!["calls".to_string()],
				min_count: 2,
				include_internal: false,
			}),
			consistency: code_moniker_query::Consistency::Current,
			page: Page::default(),
		})));
		let ProtocolResponse::Query(filtered) = filtered else {
			panic!("expected filtered graph response");
		};
		let QueryResult::SymbolGraph(filtered) = filtered.result else {
			panic!("expected filtered graph, got {:?}", filtered.result);
		};
		assert!(filtered.callers.is_empty(), "{filtered:?}");
		assert_eq!(filtered.coverage.callers.total, 1, "{filtered:?}");
		assert_eq!(filtered.coverage.callers.matching, 0, "{filtered:?}");
		assert_eq!(filtered.coverage.callers.returned, 0, "{filtered:?}");
		assert!(filtered.internal_edges.is_empty(), "{filtered:?}");
		assert_eq!(filtered.callees.len(), 1, "{filtered:?}");
		assert_eq!(filtered.coverage.callees.total, 2, "{filtered:?}");
		assert_eq!(filtered.coverage.callees.matching, 1, "{filtered:?}");
		assert_eq!(filtered.coverage.callees.returned, 1, "{filtered:?}");
		assert!(filtered.callees[0].symbol.name.starts_with("helper"));
	}

	fn graph_path(
		daemon: &mut WorkspaceDaemon,
		from: &str,
		to: &str,
		expect: GraphPathExpectation,
		max_depth: usize,
	) -> GraphPathResult {
		graph_path_with_limits(
			daemon,
			from,
			to,
			expect,
			BoundedPathLimits {
				max_depth,
				max_symbols: 10_000,
				max_edges: 50_000,
			},
		)
	}

	fn graph_path_with_limits(
		daemon: &mut WorkspaceDaemon,
		from: &str,
		to: &str,
		expect: GraphPathExpectation,
		limits: BoundedPathLimits,
	) -> GraphPathResult {
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::GraphPath(GraphPathQuery {
				workspace: None,
				from: from.to_string(),
				to: to.to_string(),
				expect,
				relation: vec!["calls".to_string(), "method_call".to_string()],
				max_depth: limits.max_depth,
				max_symbols: limits.max_symbols,
				max_edges: limits.max_edges,
				min_coverage: 100,
			}),
		))));
		let ProtocolResponse::Query(response) = response else {
			panic!("expected graph path response, got {response:?}");
		};
		let rendered = code_moniker_query::format_query_response(&response);
		assert!(rendered.contains("reachable:"), "{rendered}");
		assert!(rendered.contains("coverage:"), "{rendered}");
		let QueryResult::GraphPath(result) = response.result else {
			panic!("expected graph path result, got {:?}", response.result);
		};
		*result
	}

	struct GraphPathFixture {
		_temp: tempfile::TempDir,
		daemon: WorkspaceDaemon,
		uris: BTreeMap<&'static str, String>,
	}

	impl GraphPathFixture {
		fn uri(&self, name: &'static str) -> String {
			self.uris
				.get(name)
				.unwrap_or_else(|| panic!("missing fixture symbol {name}"))
				.clone()
		}
	}

	fn graph_path_fixture() -> GraphPathFixture {
		let temp = tempfile::tempdir().expect("tempdir");
		let src_dir = temp.path().join("src");
		fs::create_dir_all(&src_dir).expect("src dir");
		fs::write(
			src_dir.join("lib.rs"),
			concat!(
				"pub fn callback() { service(); alternative(); }\n",
				"fn service() { repository(); }\n",
				"fn alternative() { repository(); }\n",
				"fn repository() {}\n",
				"pub fn safe() { audit(); }\n",
				"fn audit() {}\n",
				"pub fn uncertain() { missing(); }\n",
				"pub fn cyclic() { cycle_a(); }\n",
				"fn cycle_a() { cycle_b(); }\n",
				"fn cycle_b() { cycle_a(); }\n",
			),
		)
		.expect("write lib");
		let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
			roots: vec![temp.path().display().to_string()],
			project: None,
			cache_dir: None,
			live_refresh: None,
		})
		.expect("daemon");
		let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refreshed, ProtocolResponse::Command(_)));
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::SymbolGraph(SymbolGraphQuery {
				workspace: None,
				focus: "src/lib.rs".to_string(),
				..Default::default()
			}),
		))));
		let ProtocolResponse::Query(response) = response else {
			panic!("expected symbol graph response");
		};
		let QueryResult::SymbolGraph(graph) = response.result else {
			panic!("expected symbol graph, got {:?}", response.result);
		};
		let uris = ["callback", "repository", "safe", "uncertain", "cyclic"]
			.into_iter()
			.map(|name| {
				let uri = graph
					.members
					.iter()
					.find(|member| member.name.starts_with(name))
					.unwrap_or_else(|| panic!("missing {name}: {graph:?}"))
					.uri
					.clone();
				(name, uri)
			})
			.collect();
		GraphPathFixture {
			_temp: temp,
			daemon,
			uris,
		}
	}

	#[test]
	fn query_describe_does_not_require_a_loaded_snapshot() {
		let temp = tempfile::tempdir().expect("tempdir");
		let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::QueryDescribe(code_moniker_query::QueryDescribeQuery {
				verb: Some("change.context".to_string()),
			}),
		))));
		let ProtocolResponse::Query(response) = response else {
			panic!("expected query response, got {response:?}");
		};
		let QueryResult::QueryDescribe(result) = response.result else {
			panic!("expected query describe, got {:?}", response.result);
		};
		assert_eq!(result.capabilities.len(), 1);
		assert_eq!(result.capabilities[0].name, "change.context");
	}

	#[test]
	fn stateless_syntax_parse_does_not_require_a_loaded_snapshot() {
		let temp = tempfile::tempdir().expect("tempdir");
		let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::SyntaxParse(code_moniker_query::SyntaxParseQuery {
				language: "plpgsql".to_string(),
				source: "DECLARE total numeric; BEGIN total := 1; RETURN total; END;".to_string(),
				uri: None,
				max_depth: 12,
				max_nodes: 200,
				named_only: true,
				include_text: true,
				max_text_chars: 40,
			}),
		))));
		let ProtocolResponse::Query(response) = response else {
			panic!("expected stateless syntax response, got {response:?}");
		};
		assert_eq!(response.generation, None);
		let QueryResult::SyntaxTree(tree) = response.result else {
			panic!("expected syntax tree, got {:?}", response.result);
		};
		assert_eq!(tree.file, "snippet.plpgsql");
		assert_eq!(tree.language, "plpgsql");
		assert_eq!(tree.root.kind, "source_file");
		assert!(syntax_node_contains(&tree.root, "decl_statement", None));
		assert!(syntax_node_contains(&tree.root, "stmt_assign", None));
		assert!(syntax_node_contains(&tree.root, "stmt_return", None));
	}

	#[tokio::test]
	async fn rpc_syntax_parse_does_not_wait_for_the_workspace_lock() {
		let temp = tempfile::tempdir().expect("tempdir");
		let (events, _) = tokio::sync::broadcast::channel(16);
		let service = test_rpc_service(
			WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon"),
			vec![temp.path().to_path_buf()],
			events,
		);
		let (release_lock, lock_holder) = hold_workspace_lock(Arc::clone(&service.daemon));

		let response = service
			.dispatch(ProtocolRequest::Query(Box::new(QueryRequest::new(
				Query::SyntaxParse(code_moniker_query::SyntaxParseQuery {
					language: "rs".to_string(),
					source: "fn answer() -> u32 { 42 }".to_string(),
					uri: None,
					max_depth: 6,
					max_nodes: 100,
					named_only: true,
					include_text: false,
					max_text_chars: 80,
				}),
			))))
			.await
			.expect("RPC dispatch");
		release_lock.send(()).expect("release workspace lock");
		lock_holder.join().expect("workspace lock holder");

		let ProtocolResponse::Query(response) = response else {
			panic!("expected stateless syntax response, got {response:?}");
		};
		assert!(matches!(response.result, QueryResult::SyntaxTree(_)));
	}

	#[tokio::test]
	async fn rpc_exclusive_requests_queue_instead_of_reporting_workspace_loading() {
		let temp = tempfile::tempdir().expect("tempdir");
		let (events, _) = tokio::sync::broadcast::channel(16);
		let service = Arc::new(test_rpc_service(
			WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon"),
			vec![temp.path().to_path_buf()],
			events,
		));
		let (release_lock, lock_holder) = hold_workspace_lock(Arc::clone(&service.daemon));
		let mut pending = tokio::spawn({
			let service = Arc::clone(&service);
			async move {
				service
					.dispatch(ProtocolRequest::Command(CommandRequest {
						command: Command::WorkspaceRefresh,
					}))
					.await
			}
		});

		assert!(
			tokio::time::timeout(std::time::Duration::from_millis(50), &mut pending)
				.await
				.is_err(),
			"the request should wait for the active workspace mutation"
		);
		release_lock.send(()).expect("release workspace lock");
		lock_holder.join().expect("workspace lock holder");

		let response = pending.await.expect("dispatch task").expect("RPC dispatch");
		assert!(matches!(response, ProtocolResponse::Command(_)));
	}

	#[tokio::test]
	async fn rpc_stale_reads_use_the_published_snapshot_during_workspace_mutation() {
		let temp = tempfile::tempdir().expect("tempdir");
		fs::write(temp.path().join("lib.rs"), "pub struct Customer;\n").expect("seed fixture");
		let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
		let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refreshed, ProtocolResponse::Command(_)));
		let (events, _) = tokio::sync::broadcast::channel(16);
		let service = test_rpc_service(daemon, vec![temp.path().to_path_buf()], events);
		{
			let daemon = service.daemon.lock().expect("workspace lock");
			publish_current_snapshot(&daemon, &service.published);
		}
		let (release_lock, lock_holder) = hold_workspace_lock(Arc::clone(&service.daemon));

		let response = tokio::time::timeout(
			std::time::Duration::from_millis(100),
			service.dispatch(ProtocolRequest::Query(Box::new(QueryRequest {
				query: Query::SymbolSearch(code_moniker_query::SymbolSearchQuery {
					name: Some("Customer".to_string()),
					..Default::default()
				}),
				consistency: Consistency::StaleOk,
				page: Page::default(),
			}))),
		)
		.await
		.expect("stale read must not wait for the workspace mutation")
		.expect("RPC dispatch");
		let ProtocolResponse::Query(response) = response else {
			panic!("expected symbol response, got {response:?}");
		};
		let QueryResult::SymbolList(symbols) = response.result else {
			panic!("expected symbol list, got {:?}", response.result);
		};
		assert_eq!(symbols.total, 1);

		let response = service
			.dispatch(ProtocolRequest::Query(Box::new(QueryRequest::new(
				Query::WorkspaceStatus,
			))))
			.await
			.expect("workspace status");
		let ProtocolResponse::Query(response) = response else {
			panic!("expected workspace status, got {response:?}");
		};
		let QueryResult::WorkspaceStatus(status) = response.result else {
			panic!("expected workspace status, got {:?}", response.result);
		};
		assert_eq!(status.phase, WorkspacePhase::Refreshing);
		assert!(status.stale);
		assert!(status.stale_summary.contains("refresh in progress"));
		release_lock.send(()).expect("release workspace lock");
		lock_holder.join().expect("workspace lock holder");
	}

	#[test]
	fn stateless_syntax_parse_accepts_quoted_plpgsql_labels() {
		let temp = tempfile::tempdir().expect("tempdir");
		let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
		let source = r#"BEGIN
  <<"outer ""loop">>
  FOR i IN 1..10 LOOP
    EXIT "outer ""loop";
  END LOOP "outer ""loop";
END;"#;
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::SyntaxParse(code_moniker_query::SyntaxParseQuery {
				language: "plpgsql".to_string(),
				source: source.to_string(),
				uri: Some("quoted-label.plpgsql".to_string()),
				max_depth: 16,
				max_nodes: 300,
				named_only: false,
				include_text: true,
				max_text_chars: 80,
			}),
		))));
		let ProtocolResponse::Query(response) = response else {
			panic!("expected stateless syntax response, got {response:?}");
		};
		let QueryResult::SyntaxTree(tree) = response.result else {
			panic!("expected syntax tree, got {:?}", response.result);
		};
		assert!(
			!tree.has_error,
			"quoted PL/pgSQL label must parse: {tree:#?}"
		);
		assert!(syntax_node_contains(&tree.root, "loop_label", None));
		assert!(syntax_node_contains(
			&tree.root,
			"quoted_identifier",
			Some("\"outer \"\"loop\""),
		));
		let quoted_label =
			syntax_node_find(&tree.root, "quoted_identifier", Some("\"outer \"\"loop\""))
				.expect("quoted loop label");
		let label_text = "\"outer \"\"loop\"";
		let label_start = source.find(label_text).expect("label text in source");
		assert_eq!(
			quoted_label.byte_range,
			(label_start, label_start + label_text.len())
		);
		assert_eq!((quoted_label.start.line, quoted_label.start.column), (2, 4));
		assert_eq!((quoted_label.end.line, quoted_label.end.column), (2, 18));
	}

	#[test]
	fn stateless_syntax_parse_rejects_unsupported_languages_and_large_sources() {
		let temp = tempfile::tempdir().expect("tempdir");
		let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
		for (query, expected_code) in [
			(
				code_moniker_query::SyntaxParseQuery {
					language: "brainfuck".to_string(),
					source: "+++".to_string(),
					uri: None,
					max_depth: 6,
					max_nodes: 100,
					named_only: true,
					include_text: false,
					max_text_chars: 80,
				},
				"syntax_language_unsupported",
			),
			(
				code_moniker_query::SyntaxParseQuery {
					language: "rs".to_string(),
					source: "x".repeat(code_moniker_query::SYNTAX_PARSE_MAX_SOURCE_BYTES + 1),
					uri: None,
					max_depth: 6,
					max_nodes: 100,
					named_only: true,
					include_text: false,
					max_text_chars: 80,
				},
				"syntax_source_too_large",
			),
		] {
			let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(
				QueryRequest::new(Query::SyntaxParse(query)),
			)));
			let ProtocolResponse::Error(error) = response else {
				panic!("expected {expected_code}, got {response:?}");
			};
			assert_eq!(error.code, expected_code);
		}
	}

	#[test]
	fn stateless_syntax_parse_does_not_drain_or_refresh_live_workspace_events() {
		let temp = tempfile::tempdir().expect("tempdir");
		let changed = temp.path().join("later.rs");
		let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
			roots: vec![temp.path().display().to_string()],
			project: None,
			cache_dir: None,
			live_refresh: Some("auto".to_string()),
		})
		.expect("auto daemon");
		daemon
			.live
			.tx
			.send(WorkspaceLiveEvent::SourcesChanged(vec![changed.clone()]))
			.expect("queue live event");

		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::SyntaxParse(code_moniker_query::SyntaxParseQuery {
				language: "rs".to_string(),
				source: "fn answer() -> u32 { 42 }".to_string(),
				uri: None,
				max_depth: 6,
				max_nodes: 100,
				named_only: true,
				include_text: false,
				max_text_chars: 80,
			}),
		))));
		assert!(
			matches!(response, ProtocolResponse::Query(_)),
			"{response:?}"
		);
		let queued = daemon
			.live
			.rx
			.try_recv()
			.expect("live event remains queued");
		assert!(
			matches!(queued, WorkspaceLiveEvent::SourcesChanged(paths) if paths == vec![changed])
		);
		assert!(
			daemon.registry.queries().snapshot().is_none(),
			"stateless parse must not create a workspace snapshot"
		);
	}

	#[test]
	fn applicable_rules_and_change_context_are_symbol_scoped() {
		let temp = tempfile::tempdir().expect("tempdir");
		let src = temp.path().join("src");
		fs::create_dir_all(&src).expect("src dir");
		fs::write(
			temp.path().join(".code-moniker.toml"),
			concat!(
				"default_rules = false\n\n",
				"[[rust.fn.where]]\n",
				"id = \"function-snake-case\"\n",
				"expr = \"name =~ ^[a-z][a-z0-9_]*$\"\n",
				"severity = \"warn\"\n",
				"message = \"Function `{name}` should be snake_case.\"\n",
				"\n[[rust.shape.type.where]]\n",
				"id = \"type-rule\"\n",
				"expr = \"name =~ .\"\n",
				"message = \"Type rule.\"\n",
				"\n[[refs.where]]\n",
				"id = \"reference-rule\"\n",
				"expr = \"source ~ '**'\"\n",
				"message = \"Reference rule.\"\n",
			),
		)
		.expect("rules config");
		fs::write(
			src.join("lib.rs"),
			"pub fn entry() { helper(); }\nfn helper() {}\n",
		)
		.expect("source");
		let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
		let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refreshed, ProtocolResponse::Command(_)));
		let QueryResult::SymbolList(symbols) = search_symbols(&mut daemon, "entry") else {
			panic!("expected symbol search result");
		};
		let entry = symbols
			.rows
			.iter()
			.find(|symbol| symbol.name.starts_with("entry"))
			.expect("entry symbol");
		let entry_uri = entry.uri.clone();

		let applicable =
			daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
				Query::RulesApplicable(code_moniker_query::RulesApplicableQuery {
					workspace: None,
					focus: entry_uri.clone(),
					profile: None,
					rules: None,
				}),
			))));
		let ProtocolResponse::Query(applicable) = applicable else {
			panic!("expected applicable rules response, got {applicable:?}");
		};
		let QueryResult::RulesApplicable(applicable) = applicable.result else {
			panic!("expected applicable rules, got {:?}", applicable.result);
		};
		assert_eq!(applicable.file, "src/lib.rs");
		assert_eq!(applicable.symbol_kind.as_deref(), Some("fn"));
		assert!(
			applicable.rows.iter().any(
				|row| row.status == "applicable" && row.rule.id.contains("function-snake-case")
			),
			"{applicable:?}"
		);
		assert!(
			applicable
				.rows
				.iter()
				.any(|row| row.status == "ignored" && row.rule.id.contains("type-rule")),
			"{applicable:?}"
		);
		assert!(
			applicable
				.rows
				.iter()
				.any(|row| row.status == "potential" && row.rule.id.contains("reference-rule")),
			"{applicable:?}"
		);

		let context = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::ChangeContext(code_moniker_query::ChangeContextQuery {
				workspace: None,
				focus: entry_uri.clone(),
				profile: None,
				max_items: 1,
			}),
		))));
		let ProtocolResponse::Query(context) = context else {
			panic!("expected change context response");
		};
		let QueryResult::ChangeContext(context) = context.result else {
			panic!("expected change context, got {:?}", context.result);
		};
		assert!(
			matches!(&context.focus, code_moniker_query::SymbolGraphFocus::Symbol { symbol } if symbol.uri == entry_uri),
			"{context:?}"
		);
		assert_eq!(context.coverage.callees_emitted, 1);
		assert!(context.coverage.callees_total >= context.coverage.callees_emitted);
		assert_eq!(context.coverage.rules_emitted, 1);
		assert_eq!(context.suggested_checks.len(), 1);
		assert!(
			context.suggested_checks[0].starts_with("code_moniker_rules "),
			"{:?}",
			context.suggested_checks
		);
		assert!(!context.suggested_checks[0].contains("@m"));
	}

	#[test]
	fn change_review_query_serves_semantic_facts_from_the_snapshot() {
		let temp = tempfile::tempdir().expect("tempdir");
		let git = |args: &[&str]| {
			let output = std::process::Command::new("git")
				.arg("-C")
				.arg(temp.path())
				.args(args)
				.output()
				.expect("run git");
			assert!(
				output.status.success(),
				"git {args:?}: {}",
				String::from_utf8_lossy(&output.stderr)
			);
		};
		git(&["init"]);
		git(&["config", "user.email", "cm@example.test"]);
		git(&["config", "user.name", "Code Moniker"]);
		let src = temp.path().join("src");
		fs::create_dir_all(&src).expect("src dir");
		fs::write(
			src.join("util.rs"),
			"pub fn assist() { work(); }\npub fn sidekick() { rest(); }\n",
		)
		.expect("write util");
		git(&["add", "."]);
		git(&["commit", "-m", "initial"]);
		git(&["mv", "src/util.rs", "src/support.rs"]);
		let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
			roots: vec![temp.path().display().to_string()],
			project: None,
			cache_dir: None,
			live_refresh: None,
		})
		.expect("daemon");
		let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refreshed, ProtocolResponse::Command(_)));

		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::ChangeReview(code_moniker_query::ChangeReviewQuery { workspace: None }),
			consistency: code_moniker_query::Consistency::Current,
			page: Page::default(),
		})));

		let ProtocolResponse::Query(query) = response else {
			panic!("expected query response");
		};
		let QueryResult::ChangeReview(result) = query.result else {
			panic!("expected change review result, got {:?}", query.result);
		};
		assert_eq!(result.scope, "HEAD..worktree");
		assert!(
			result
				.files
				.iter()
				.any(|file| file.disposition == "moved" && file.coverage_explained),
			"{result:?}"
		);
		assert!(
			result
				.symbol_changes
				.iter()
				.all(|change| change.kind == "moved" && change.file_moved),
			"{result:?}"
		);

		let tree = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::TreeChildren(code_moniker_query::TreeChildrenQuery {
				workspace: None,
				path: Vec::new(),
				depth: 1,
				lang: Vec::new(),
				projection: Vec::new(),
			}),
			consistency: code_moniker_query::Consistency::Current,
			page: Page::default(),
		})));
		let ProtocolResponse::Query(tree) = tree else {
			panic!("expected tree response");
		};
		let QueryResult::TreeChildren(tree) = tree.result else {
			panic!("expected tree children, got {:?}", tree.result);
		};
		assert!(
			tree.rows.iter().any(|row| row.change_count > 0),
			"tree rows must carry the change count: {:?}",
			tree.rows
		);
	}

	#[test]
	fn symbol_graph_partitions_unit_boundary_edges() {
		let temp = tempfile::tempdir().expect("tempdir");
		let src_dir = temp.path().join("src");
		fs::create_dir_all(&src_dir).expect("src dir");
		fs::write(src_dir.join("lib.rs"), "pub mod engine;\npub mod driver;\n").expect("write lib");
		fs::write(
			src_dir.join("engine.rs"),
			"pub fn entry() { helper(); helper(); crate::driver::remote(); }\nfn helper() { helper(); }\n",
		)
		.expect("write engine");
		fs::write(
			src_dir.join("driver.rs"),
			"pub fn remote() {}\npub fn boss() { crate::engine::entry(); }\n",
		)
		.expect("write driver");
		let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
			roots: vec![temp.path().display().to_string()],
			project: None,
			cache_dir: None,
			live_refresh: None,
		})
		.expect("daemon");
		let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refreshed, ProtocolResponse::Command(_)));
		let mut graph = |focus: &str| {
			let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
				query: Query::SymbolGraph(code_moniker_query::SymbolGraphQuery {
					workspace: None,
					focus: focus.to_string(),
					..Default::default()
				}),
				consistency: code_moniker_query::Consistency::Current,
				page: Page::default(),
			})));
			let ProtocolResponse::Query(query) = response else {
				panic!("expected query response");
			};
			let QueryResult::SymbolGraph(result) = query.result else {
				panic!("expected symbol graph, got {:?}", query.result);
			};
			result
		};

		let file = graph("src/engine.rs");
		assert_eq!(file.members.len(), 2, "{file:?}");
		assert!(
			file.internal_edges.iter().any(|edge| edge.count == 2),
			"entry -> helper twice: {file:?}"
		);
		assert!(
			file.internal_edges
				.iter()
				.any(|edge| edge.source == edge.target),
			"helper recursion stays internal: {file:?}"
		);
		assert!(
			file.callers
				.iter()
				.any(|caller| caller.symbol.name.starts_with("boss")),
			"{file:?}"
		);
		assert!(
			file.callees
				.iter()
				.any(|callee| callee.symbol.name.starts_with("remote")),
			"{file:?}"
		);

		let entry_uri = file
			.members
			.iter()
			.find(|member| member.name.starts_with("entry"))
			.expect("entry member")
			.uri
			.clone();
		let unit = graph(&entry_uri);
		assert!(
			matches!(&unit.focus, code_moniker_query::SymbolGraphFocus::Symbol { symbol } if symbol.name.starts_with("entry")),
			"{unit:?}"
		);
		assert!(
			unit.callees
				.iter()
				.any(|callee| callee.symbol.name.starts_with("helper") && callee.count == 2),
			"same-file helper is OUTSIDE the fn unit: {unit:?}"
		);
		assert!(
			unit.callees
				.iter()
				.any(|callee| callee.symbol.name.starts_with("remote")),
			"{unit:?}"
		);
		assert!(
			unit.callers
				.iter()
				.any(|caller| caller.symbol.name.starts_with("boss")),
			"{unit:?}"
		);
		assert!(unit.internal_edges.is_empty(), "{unit:?}");
		assert_filtered_outgoing_graph(&mut daemon, entry_uri);
	}

	#[test]
	fn graph_path_returns_minimal_witness_and_tri_state_confidence() {
		let mut fixture = graph_path_fixture();
		let callback = fixture.uri("callback");
		let repository = fixture.uri("repository");
		let safe = fixture.uri("safe");
		let uncertain = fixture.uri("uncertain");

		let reachable = graph_path(
			&mut fixture.daemon,
			&callback,
			&repository,
			GraphPathExpectation::Reachable,
			6,
		);
		assert_eq!(reachable.verdict, GraphPathVerdict::Pass, "{reachable:?}");
		assert_eq!(reachable.reachable, Some(true), "{reachable:?}");
		assert_eq!(reachable.no_path, Some(false), "{reachable:?}");
		assert_eq!(reachable.path.len(), 2, "{reachable:?}");
		assert!(
			reachable.path[0].target.name.starts_with("service"),
			"deterministic shortest witness: {reachable:?}"
		);
		assert!(
			reachable.path[1].target.name.starts_with("repository"),
			"{reachable:?}"
		);

		let safe = graph_path(
			&mut fixture.daemon,
			&safe,
			&repository,
			GraphPathExpectation::NoPath,
			6,
		);
		assert_eq!(safe.verdict, GraphPathVerdict::Pass, "{safe:?}");
		assert_eq!(safe.reachable, Some(false), "{safe:?}");
		assert_eq!(safe.no_path, Some(true), "{safe:?}");
		assert_eq!(safe.coverage.percent, 100, "{safe:?}");

		let uncertain = graph_path(
			&mut fixture.daemon,
			&uncertain,
			&repository,
			GraphPathExpectation::NoPath,
			6,
		);
		assert_eq!(
			uncertain.verdict,
			GraphPathVerdict::Inconclusive,
			"{uncertain:?}"
		);
		assert_eq!(uncertain.reachable, None, "{uncertain:?}");
		assert!(uncertain.coverage.unresolved > 0, "{uncertain:?}");
		assert!(!uncertain.coverage.gap_reasons.is_empty(), "{uncertain:?}");

		let bounded = graph_path(
			&mut fixture.daemon,
			&callback,
			&repository,
			GraphPathExpectation::Reachable,
			1,
		);
		assert_eq!(
			bounded.verdict,
			GraphPathVerdict::Inconclusive,
			"{bounded:?}"
		);
		assert!(bounded.search.depth_limit_reached, "{bounded:?}");
		assert!(bounded.path.is_empty(), "{bounded:?}");
	}

	#[test]
	fn graph_path_bounds_cycles_and_exploration_budgets() {
		let mut fixture = graph_path_fixture();
		let callback = fixture.uri("callback");
		let repository = fixture.uri("repository");
		let cyclic = fixture.uri("cyclic");
		let cycle = graph_path(
			&mut fixture.daemon,
			&cyclic,
			&repository,
			GraphPathExpectation::NoPath,
			6,
		);
		assert_eq!(cycle.verdict, GraphPathVerdict::Pass, "{cycle:?}");
		assert_eq!(cycle.no_path, Some(true), "{cycle:?}");
		assert!(!cycle.search.depth_limit_reached, "{cycle:?}");
		assert!(cycle.search.explored_symbols <= 3, "{cycle:?}");

		let budgeted = graph_path_with_limits(
			&mut fixture.daemon,
			&callback,
			&repository,
			GraphPathExpectation::Reachable,
			BoundedPathLimits {
				max_depth: 6,
				max_symbols: 1,
				max_edges: 50_000,
			},
		);
		assert_eq!(
			budgeted.verdict,
			GraphPathVerdict::Inconclusive,
			"{budgeted:?}"
		);
		assert!(budgeted.search.symbol_limit_reached, "{budgeted:?}");
		assert!(
			budgeted
				.reasons
				.iter()
				.any(|reason| reason == "symbol_limit"),
			"{budgeted:?}"
		);
		let edge_budgeted = graph_path_with_limits(
			&mut fixture.daemon,
			&callback,
			&repository,
			GraphPathExpectation::Reachable,
			BoundedPathLimits {
				max_depth: 6,
				max_symbols: 10_000,
				max_edges: 1,
			},
		);
		assert_eq!(
			edge_budgeted.verdict,
			GraphPathVerdict::Inconclusive,
			"{edge_budgeted:?}"
		);
		assert!(edge_budgeted.search.edge_limit_reached, "{edge_budgeted:?}");
	}

	#[test]
	fn identity_children_walks_the_symbolic_tree() {
		let temp = tempfile::tempdir().expect("tempdir");
		let src_dir = temp.path().join("src");
		fs::create_dir_all(&src_dir).expect("src dir");
		fs::write(src_dir.join("lib.rs"), "pub mod engine;\n").expect("write lib");
		fs::write(
			src_dir.join("engine.rs"),
			"pub fn entry() { helper(); }\nfn helper() {}\n",
		)
		.expect("write engine");
		let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
			roots: vec![temp.path().display().to_string()],
			project: None,
			cache_dir: None,
			live_refresh: None,
		})
		.expect("daemon");
		let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refreshed, ProtocolResponse::Command(_)));
		let mut children = |prefix: &str| {
			let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
				query: Query::IdentityChildren(code_moniker_query::IdentityChildrenQuery {
					workspace: None,
					prefix: prefix.to_string(),
				}),
				consistency: code_moniker_query::Consistency::Current,
				page: Page::default(),
			})));
			let ProtocolResponse::Query(query) = response else {
				panic!("expected query response");
			};
			let QueryResult::IdentityChildren(result) = query.result else {
				panic!("expected identity children, got {:?}", query.result);
			};
			result.children
		};

		// Walk organizational segments (lang, dir, module wrappers) down to
		// the level that holds the engine module.
		let mut prefix = String::new();
		let engine = loop {
			let rows = children(&prefix);
			assert!(!rows.is_empty(), "no children under `{prefix}`");
			if let Some(engine) = rows.iter().find(|row| row.name == "engine") {
				break engine.clone();
			}
			let next = rows
				.iter()
				.find(|row| row.has_children)
				.unwrap_or_else(|| panic!("no descent from `{prefix}`: {rows:?}"));
			assert!(next.symbol.is_none(), "organizational segment: {next:?}");
			assert!(next.defs > 0, "{next:?}");
			prefix = next.identity.clone();
		};
		assert!(engine.defs >= 2, "entry + helper below engine: {engine:?}");

		let functions = children(&engine.identity);
		let entry = functions
			.iter()
			.find(|row| row.name.starts_with("entry"))
			.unwrap_or_else(|| panic!("entry under engine: {functions:?}"));
		assert_eq!(entry.kind, "fn");
		let symbol = entry.symbol.as_ref().expect("entry is a definition");
		assert_eq!(symbol.kind, "fn");
		assert!(symbol.file.ends_with("engine.rs"), "{symbol:?}");
		assert!(!entry.has_children, "{entry:?}");
		assert!(
			functions.iter().any(|row| row.name.starts_with("helper")),
			"{functions:?}"
		);
	}

	#[test]
	fn identity_graph_rolls_up_cross_module_calls() {
		let temp = tempfile::tempdir().expect("tempdir");
		let src_dir = temp.path().join("src");
		fs::create_dir_all(&src_dir).expect("src dir");
		fs::write(src_dir.join("lib.rs"), "pub mod engine;\npub mod driver;\n").expect("write lib");
		fs::write(
			src_dir.join("engine.rs"),
			"pub fn entry() { crate::driver::remote(); crate::driver::remote(); helper(); }\nfn helper() {}\n",
		)
		.expect("write engine");
		fs::write(src_dir.join("driver.rs"), "pub fn remote() {}\n").expect("write driver");
		let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
			roots: vec![temp.path().display().to_string()],
			project: None,
			cache_dir: None,
			live_refresh: None,
		})
		.expect("daemon");
		let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refreshed, ProtocolResponse::Command(_)));
		let mut graph = |prefix: &str| {
			let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
				query: Query::IdentityGraph(code_moniker_query::IdentityGraphQuery {
					workspace: None,
					prefix: prefix.to_string(),
					path: Vec::new(),
					min_count: 1,
				}),
				consistency: code_moniker_query::Consistency::Current,
				page: Page::default(),
			})));
			let ProtocolResponse::Query(query) = response else {
				panic!("expected query response");
			};
			let QueryResult::IdentityGraph(result) = query.result else {
				panic!("expected identity graph, got {:?}", query.result);
			};
			result
		};

		// At the level that holds both modules, the two calls roll up into one
		// aggregated engine -> driver edge.
		let modules = graph("lang:rs/dir:src");
		assert!(
			modules.nodes.iter().any(|node| node.name == "engine"),
			"{modules:?}"
		);
		let rollup = modules
			.edges
			.iter()
			.find(|edge| {
				edge.source.ends_with("module:engine") && edge.target.ends_with("module:driver")
			})
			.unwrap_or_else(|| panic!("engine -> driver rollup: {modules:?}"));
		assert_eq!(rollup.count, 2, "{rollup:?}");
		assert!(
			rollup.kinds.iter().any(|kind| kind == "calls"),
			"{rollup:?}"
		);
		// entry -> helper stays inside module:engine: not an edge at this level.
		assert!(
			!modules.edges.iter().any(|edge| edge.source == edge.target),
			"{modules:?}"
		);

		// One level deeper the boundary crossing becomes an outgoing port.
		let engine = graph("lang:rs/dir:src/module:engine");
		assert!(
			engine
				.edges
				.iter()
				.any(|edge| edge.source.ends_with("fn:entry()")
					&& edge.target.ends_with("fn:helper()")),
			"{engine:?}"
		);
		let port = engine
			.ports_out
			.iter()
			.find(|port| {
				port.identity.ends_with("module:driver") || port.identity.contains("driver")
			})
			.unwrap_or_else(|| panic!("outgoing port toward driver: {engine:?}"));
		assert_eq!(port.count, 2, "{port:?}");
		assert_identity_graph_filtering_and_pagination(&mut daemon);
	}

	fn assert_identity_graph_filtering_and_pagination(daemon: &mut WorkspaceDaemon) {
		let filtered = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::IdentityGraph(code_moniker_query::IdentityGraphQuery {
				workspace: None,
				prefix: "lang:rs/dir:src".to_string(),
				path: Vec::new(),
				min_count: 3,
			}),
			consistency: code_moniker_query::Consistency::Current,
			page: Page::default(),
		})));
		let ProtocolResponse::Query(filtered) = filtered else {
			panic!("expected filtered identity graph response");
		};
		let QueryResult::IdentityGraph(filtered) = filtered.result else {
			panic!("expected filtered identity graph result");
		};
		assert_eq!(filtered.coverage.edges_total, 1, "{filtered:?}");
		assert_eq!(filtered.coverage.edges_matching, 0, "{filtered:?}");
		assert!(filtered.edges.is_empty(), "{filtered:?}");

		let mut cursor = None;
		let mut emitted = 0usize;
		let mut pages = 0usize;
		let mut expected_matching = None;
		loop {
			let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
				query: Query::IdentityGraph(code_moniker_query::IdentityGraphQuery {
					workspace: None,
					prefix: "lang:rs/dir:src".to_string(),
					path: Vec::new(),
					min_count: 1,
				}),
				consistency: code_moniker_query::Consistency::Current,
				page: Page { cursor, limit: 1 },
			})));
			let ProtocolResponse::Query(response) = response else {
				panic!("expected paged identity graph response");
			};
			cursor = response.next_cursor.clone();
			let QueryResult::IdentityGraph(page) = response.result else {
				panic!("expected paged identity graph result");
			};
			assert!(page.coverage.rows_emitted <= 1, "{page:?}");
			emitted += page.coverage.rows_emitted;
			pages += 1;
			match expected_matching {
				Some(expected) => assert_eq!(page.coverage.rows_matching, expected),
				None => expected_matching = Some(page.coverage.rows_matching),
			}
			if cursor.is_none() {
				break;
			}
		}
		assert!(pages > 1, "pagination must expose more than one page");
		assert_eq!(emitted, expected_matching.expect("matching row count"));
	}

	#[test]
	fn identity_graph_applies_path_scope_before_java_package_rollup() {
		let temp = tempfile::tempdir().expect("tempdir");
		let main = temp.path().join("src/com/acme");
		let tests = temp.path().join("tests/com/acme");
		fs::create_dir_all(&main).expect("main sources");
		fs::create_dir_all(&tests).expect("test sources");
		fs::write(
			main.join("StorageService.java"),
			"package com.acme; public class StorageService { public static void save() {} }\n",
		)
		.expect("write main source");
		fs::write(
			tests.join("StorageServiceTest.java"),
			"package com.acme; public class StorageServiceTest { void testSave() { StorageService.save(); } }\n",
		)
		.expect("write test source");
		let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
			roots: vec![temp.path().display().to_string()],
			project: None,
			cache_dir: None,
			live_refresh: None,
		})
		.expect("daemon");
		let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refreshed, ProtocolResponse::Command(_)));

		let mut graph = |expression: &str| {
			let request =
				code_moniker_query::parse_query(expression).expect("identity graph query");
			let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(request)));
			let ProtocolResponse::Query(response) = response else {
				panic!("expected identity graph response, got {response:?}");
			};
			let QueryResult::IdentityGraph(result) = response.result else {
				panic!("expected identity graph result, got {:?}", response.result);
			};
			result
		};

		let complete = graph("identity.graph prefix:\"lang:java\"");
		let main_only = graph("identity.graph prefix:\"lang:java\" path:\"src/**\"");
		assert_eq!(main_only.path, vec!["src/**"]);
		let complete_defs: usize = complete.nodes.iter().map(|node| node.defs).sum();
		let main_defs: usize = main_only.nodes.iter().map(|node| node.defs).sum();
		assert!(main_defs > 0, "{main_only:?}");
		assert!(
			main_defs < complete_defs,
			"the test package must not be merged into the selected main package: complete={complete:?} main={main_only:?}"
		);
		assert!(
			main_only.ports_in.iter().any(|port| port.count > 0),
			"references from excluded test sources must remain visible as incoming boundary crossings: {main_only:?}"
		);
	}

	#[test]
	fn symbol_usages_rolls_up_singleton_member_activity_without_internal_refs() {
		let temp = tempfile::tempdir().expect("tempdir");
		let src = temp.path().join("src/com/acme");
		fs::create_dir_all(&src).expect("sources");
		fs::write(
			src.join("StorageService.java"),
			concat!(
				"package com.acme; public class StorageService { ",
				"public static final StorageService instance = new StorageService(); ",
				"public void save() {} }\n"
			),
		)
		.expect("write singleton");
		fs::write(
			src.join("ClientA.java"),
			"package com.acme; public class ClientA { void run() { StorageService.instance.save(); } }\n",
		)
		.expect("write client A");
		fs::write(
			src.join("ClientB.java"),
			"package com.acme; public class ClientB { void run() { StorageService.instance.save(); } }\n",
		)
		.expect("write client B");
		let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
			roots: vec![temp.path().display().to_string()],
			project: None,
			cache_dir: None,
			live_refresh: None,
		})
		.expect("daemon");
		let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refreshed, ProtocolResponse::Command(_)));
		let QueryResult::SymbolList(symbols) = search_symbols_named(&mut daemon, "StorageService")
		else {
			panic!("expected storage service symbol");
		};
		let service = symbols
			.rows
			.iter()
			.find(|symbol| symbol.kind == "class")
			.expect("storage service class")
			.uri
			.clone();

		let mut usages = |include_descendants| {
			let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
				query: Query::SymbolUsages(code_moniker_query::SymbolUsagesQuery {
					workspace: None,
					uri: service.clone(),
					direction: code_moniker_query::UsageDirection::Incoming,
					path: Vec::new(),
					lang: Vec::new(),
					include_descendants,
					projection: Vec::new(),
				}),
				consistency: code_moniker_query::Consistency::Current,
				page: Page {
					cursor: None,
					limit: 1_000,
				},
			})));
			let ProtocolResponse::Query(response) = response else {
				panic!("expected usage response");
			};
			let QueryResult::SymbolUsages(result) = response.result else {
				panic!("expected usage result");
			};
			result
		};

		let exact = usages(false);
		let rolled = usages(true);
		assert_eq!(exact.targets, 1, "{exact:?}");
		assert!(rolled.targets > 1, "{rolled:?}");
		assert!(
			exact
				.rows
				.iter()
				.all(|row| !row.context.contains("module:Client")),
			"exact type usages must keep their existing meaning: {exact:?}"
		);
		assert!(
			rolled
				.rows
				.iter()
				.any(|row| row.context.contains("module:ClientA"))
				&& rolled
					.rows
					.iter()
					.any(|row| row.context.contains("module:ClientB")),
			"member-mediated singleton consumers must become visible: {rolled:?}"
		);
		assert!(
			rolled
				.incoming_summary
				.as_ref()
				.is_some_and(|summary| summary.contexts >= 2),
			"{rolled:?}"
		);
		let unique_refs = rolled
			.rows
			.iter()
			.map(|row| row.reference.as_str())
			.collect::<BTreeSet<_>>();
		assert_eq!(unique_refs.len(), rolled.rows.len(), "{rolled:?}");
		assert!(
			rolled.rows.iter().all(|row| {
				row.context != service && identity_rest(&row.context, &service).is_none()
			}),
			"relations internal to the owner boundary must not count as coupling: {rolled:?}"
		);
	}

	#[test]
	fn symbol_graph_routes_directory_focus_to_identity_graph() {
		let temp = tempfile::tempdir().expect("tempdir");
		let src_dir = temp.path().join("src");
		fs::create_dir_all(&src_dir).expect("src dir");
		fs::write(src_dir.join("lib.rs"), "pub fn entry() {}\n").expect("write lib");
		let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
			roots: vec![temp.path().display().to_string()],
			project: None,
			cache_dir: None,
			live_refresh: None,
		})
		.expect("daemon");
		let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refreshed, ProtocolResponse::Command(_)));

		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::SymbolGraph(code_moniker_query::SymbolGraphQuery {
				workspace: None,
				focus: "src".to_string(),
				direction: code_moniker_query::UsageDirection::Both,
				relation: Vec::new(),
				min_count: 1,
				include_internal: true,
			}),
			consistency: code_moniker_query::Consistency::Current,
			page: Page::default(),
		})));
		let ProtocolResponse::Error(error) = response else {
			panic!("a directory focus must fail with routing guidance, got {response:?}");
		};
		assert_eq!(error.code, "focus_is_directory", "{error:?}");
		assert!(
			error.message.contains("identity.graph"),
			"the error must route to the scope graph, got {error:?}"
		);
	}

	#[test]
	fn identity_graph_rejects_unknown_prefix_with_valid_heads() {
		let temp = tempfile::tempdir().expect("tempdir");
		let src_dir = temp.path().join("src");
		fs::create_dir_all(&src_dir).expect("src dir");
		fs::write(src_dir.join("lib.rs"), "pub fn entry() {}\n").expect("write lib");
		let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
			roots: vec![temp.path().display().to_string()],
			project: None,
			cache_dir: None,
			live_refresh: None,
		})
		.expect("daemon");
		let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refreshed, ProtocolResponse::Command(_)));
		let mut graph = |prefix: &str| {
			daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
				query: Query::IdentityGraph(code_moniker_query::IdentityGraphQuery {
					workspace: None,
					prefix: prefix.to_string(),
					path: Vec::new(),
					min_count: 1,
				}),
				consistency: code_moniker_query::Consistency::Current,
				page: Page::default(),
			})))
		};

		let response = graph("dir:src");
		let ProtocolResponse::Error(error) = response else {
			panic!("a prefix matching no identity must fail loudly, got {response:?}");
		};
		assert_eq!(error.code, "prefix_not_found", "{error:?}");
		assert!(
			error.message.contains("lang:rs"),
			"the error must list valid heads, got {error:?}"
		);

		let response = graph("lang:rs/dir:src/module:lib/fn:entry()");
		assert!(
			matches!(response, ProtocolResponse::Query(_)),
			"an exact leaf identity is a valid scope, got {response:?}"
		);
	}

	#[test]
	fn identity_graph_separates_external_from_unresolved() {
		let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("../workspace/tests/fixtures/projects/java/multiprojet");
		let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
			roots: vec![fixture.display().to_string()],
			project: None,
			cache_dir: None,
			live_refresh: None,
		})
		.expect("daemon");
		let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refreshed, ProtocolResponse::Command(_)));
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::IdentityGraph(code_moniker_query::IdentityGraphQuery {
				workspace: None,
				prefix: String::new(),
				path: Vec::new(),
				min_count: 1,
			}),
			consistency: code_moniker_query::Consistency::Current,
			page: Page::default(),
		})));
		let ProtocolResponse::Query(query) = response else {
			panic!("expected query response");
		};
		let QueryResult::IdentityGraph(result) = query.result else {
			panic!("expected identity graph, got {:?}", query.result);
		};
		// The fixture explains every project-internal reference. Non-unique
		// candidates stay outside the graph but never masquerade as unresolved.
		assert!(result.unlinked.external > 0, "{:?}", result.unlinked);
		assert!(result.unlinked.candidate > 0, "{:?}", result.unlinked);
		assert_eq!(result.unlinked.unresolved, 0, "{:?}", result.unlinked);
		assert!(
			result.unlinked.unresolved_reasons.is_empty(),
			"{:?}",
			result.unlinked
		);
	}

	#[test]
	fn auto_policy_applies_live_edits_before_plain_queries() {
		let temp = tempfile::tempdir().expect("tempdir");
		let src = temp.path().join("src");
		fs::create_dir_all(&src).expect("src dir");
		let lib = src.join("lib.rs");
		fs::write(&lib, "pub fn before_auto_edit() {}\n").expect("write lib");
		let mut daemon = WorkspaceDaemon::new_with_config(DaemonWorkspaceConfig {
			roots: vec![temp.path().display().to_string()],
			project: None,
			cache_dir: None,
			live_refresh: Some("auto".to_string()),
		})
		.expect("daemon");
		let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refreshed, ProtocolResponse::Command(_)));

		fs::write(&lib, "pub fn after_auto_edit() {}\n").expect("rewrite lib");
		daemon
			.live
			.tx
			.send(WorkspaceLiveEvent::SourcesChanged(vec![lib.clone()]))
			.expect("send live event");

		match search_symbols(&mut daemon, "after_auto_edit") {
			QueryResult::SymbolList(symbols) => {
				assert_eq!(
					symbols.rows.len(),
					1,
					"auto policy should apply the edit before a plain query"
				);
			}
			other => panic!("expected symbols result, got {other:?}"),
		}

		fs::write(
			src.join("fresh_auto.rs"),
			"pub fn fresh_auto_created() {}\n",
		)
		.expect("create file");
		daemon
			.live
			.tx
			.send(WorkspaceLiveEvent::SourcesChanged(vec![
				src.join("fresh_auto.rs"),
			]))
			.expect("send create event");

		match search_symbols(&mut daemon, "fresh_auto_created") {
			QueryResult::SymbolList(symbols) => {
				assert_eq!(
					symbols.rows.len(),
					1,
					"auto policy should index created files before a plain query"
				);
			}
			other => panic!("expected symbols result, got {other:?}"),
		}
	}

	#[test]
	fn query_error_carries_structured_code_in_data() {
		let error = query_error(QueryError::new("workspace_loading", "still loading"));
		assert_eq!(error.message(), "still loading");
		let data = error.data().expect("error should carry structured data");
		let value: serde_json::Value = serde_json::from_str(data.get()).unwrap();
		assert_eq!(value["code"], "workspace_loading");
		assert_eq!(value["message"], "still loading");
	}

	#[test]
	fn initial_refresh_failure_is_a_typed_observable_workspace_state() {
		let temp = tempfile::tempdir().expect("tempdir");
		let workspace = temp.path().join("workspace");
		let unavailable = temp.path().join("workspace-unavailable");
		fs::create_dir_all(&workspace).expect("workspace");
		let mut daemon = WorkspaceDaemon::new(vec![workspace.clone()]).expect("daemon");
		fs::rename(&workspace, &unavailable).expect("make workspace unavailable");

		daemon
			.refresh_cancellable(WorkspaceCancellation::default())
			.expect_err("initial refresh must fail");
		let status = workspace_status_result(&daemon.roots, &daemon.registry);

		assert_eq!(status.phase, WorkspacePhase::Failed);
		let failure = status.failure.expect("typed failure");
		assert_eq!(failure.resource.as_deref(), Some("source_catalog"));
		assert!(!failure.message.is_empty());
	}

	#[test]
	fn failed_initial_index_rejects_data_queries_without_a_restart_loop() {
		let lifecycle = RwLock::new(WorkspaceLifecycle::failed("broken corpus"));
		let response = workspace_unavailable_response(
			ProtocolRequest::Query(Box::new(QueryRequest::new(Query::SymbolSearch(
				SymbolSearchQuery::default(),
			)))),
			&lifecycle,
		);

		let ProtocolResponse::Error(error) = response else {
			panic!("expected typed workspace failure")
		};
		assert_eq!(error.code, "workspace_load_failed");
		assert_eq!(error.message, "broken corpus");
	}

	#[test]
	fn failed_workspace_status_without_snapshot_carries_the_failure_summary() {
		let response = workspace_status_without_snapshot(
			&[PathBuf::from("/workspace")],
			WorkspaceLifecycle::failed("broken corpus"),
		);
		let QueryResult::WorkspaceStatus(status) = response.result else {
			panic!("expected workspace status")
		};

		assert_eq!(status.phase, WorkspacePhase::Failed);
		assert_eq!(status.stale_summary, "broken corpus");
		assert_eq!(status.roots[0].stale_summary, "broken corpus");
	}

	#[test]
	fn rules_config_root_searches_above_common_multi_root() {
		let temp = tempfile::tempdir().expect("tempdir");
		fs::write(temp.path().join(".code-moniker.toml"), "").expect("rules config");
		let first = temp.path().join("crates").join("first");
		let second = temp.path().join("crates").join("second");
		fs::create_dir_all(&first).expect("first");
		fs::create_dir_all(&second).expect("second");
		let roots = canonical_workspace_roots([&first, &second]).expect("roots");
		let common = temp
			.path()
			.join("crates")
			.canonicalize()
			.expect("canonical common");
		assert_eq!(common_workspace_root(&roots).expect("common root"), common);
		assert_eq!(
			rules_config_root(&roots).expect("rules config root"),
			temp.path().canonicalize().expect("canonical temp")
		);
	}

	#[test]
	fn aggregate_check_summary_reconciles_unspecified_srcsets_across_roots() {
		let root = |root: &str, total_violations, violations_by_srcset| RulesCheckRootResult {
			root: root.to_string(),
			verdict: RulesCheckVerdict::Fail,
			exit: "no_match".to_string(),
			summary: CheckSummaryDto {
				total_violations,
				violations_by_srcset,
				..Default::default()
			},
			violations: Vec::new(),
			errors: Vec::new(),
			rule_reports: Vec::new(),
			skip_reason: None,
		};
		let summary = helpers::aggregate_check_summary(&[
			root("legacy", 2, BTreeMap::new()),
			root("indexed", 1, BTreeMap::from([("main".to_string(), 1)])),
		]);

		assert_eq!(summary.total_violations, 3);
		assert_eq!(
			summary.violations_by_srcset,
			BTreeMap::from([("main".to_string(), 1), ("unspecified".to_string(), 2)])
		);
	}

	#[test]
	fn workspace_selector_rejects_ambiguous_basenames() {
		let temp = tempfile::tempdir().expect("tempdir");
		let first = temp.path().join("a").join("same");
		let second = temp.path().join("b").join("same");
		fs::create_dir_all(&first).expect("first");
		fs::create_dir_all(&second).expect("second");
		let roots = canonical_workspace_roots([&first, &second]).expect("roots");
		let error = selected_roots(&roots, Some("same")).expect_err("ambiguous selector");
		assert_eq!(error.code, "workspace_selector_ambiguous");
	}

	#[test]
	fn audit_excerpt_is_line_scoped_and_bounded() {
		let source = "one\n  receiver.call(\n    argument,\n  )\nfive\n";
		let excerpt = bounded_source_excerpt(source, (2, 4));

		assert_eq!(excerpt, "receiver.call( argument, )");
		assert!(excerpt.len() <= 240);
	}

	#[test]
	fn source_root_uses_declared_workspace_root() {
		let temp = tempfile::tempdir().expect("tempdir");
		let parent = temp.path().join("workspace");
		let child = parent.join("child");
		fs::create_dir_all(child.join("src")).expect("child src");
		let roots = canonical_workspace_roots([&parent, &child]).expect("roots");
		let canonical_child = child.canonicalize().expect("canonical child");
		let source_owned_by_parent = SourceFileRecord {
			id: SourceId::at(0),
			uri: String::new(),
			source_root: 0,
			path: canonical_child.join("src/lib.rs").display().to_string(),
			rel_path: "child/src/lib.rs".to_string(),
			anchor: String::new(),
			language: "rs".to_string(),
			text: String::new(),
		};
		let selected = roots.iter().collect::<Vec<_>>();
		let root = source_root(&roots, &selected, &source_owned_by_parent).expect("source root");
		assert_eq!(root, &roots[0]);

		let source_owned_by_child = SourceFileRecord {
			source_root: 1,
			..source_owned_by_parent
		};
		let root = source_root(&roots, &selected, &source_owned_by_child).expect("source root");
		assert_eq!(root, &canonical_child);
	}

	#[test]
	fn page_rows_rejects_cursor_from_another_generation() {
		let page = Page {
			cursor: Some(QueryCursor::new(1, Some(WorkspaceGeneration(1)))),
			limit: 1,
		};
		let error = page_rows(vec![1, 2, 3], page, Some(WorkspaceGeneration(2)))
			.expect_err("generation mismatch");
		assert_eq!(error.code, "cursor_generation_mismatch");
	}

	#[test]
	fn page_rows_rejects_offset_only_cursor_for_generated_snapshot() {
		let page = Page {
			cursor: Some(QueryCursor::new(1, None)),
			limit: 1,
		};
		let error = page_rows(vec![1, 2, 3], page, Some(WorkspaceGeneration(2)))
			.expect_err("missing generation");
		assert_eq!(error.code, "cursor_generation_mismatch");
	}

	#[test]
	fn usage_prefix_distinguishes_workspace_crates() {
		assert_eq!(path_prefix("crates/daemon/src/lib.rs"), "crates/daemon");
		assert_eq!(
			path_prefix("crates/workspace/src/live/watcher.rs"),
			"crates/workspace"
		);
		assert_eq!(path_prefix("src/a.rs"), "src");
		assert_eq!(path_prefix("src/b.rs"), "src");
	}

	#[test]
	fn symbol_search_lists_production_before_tests() {
		let temp = tempfile::tempdir().expect("tempdir");
		fs::create_dir_all(temp.path().join("src")).expect("create source directory");
		fs::create_dir_all(temp.path().join("benches")).expect("create bench directory");
		fs::write(
			temp.path().join("src/lib.rs"),
			r#"
#[cfg(test)]
mod tests {
	fn helper() {}

	#[test]
	fn early_test() {}
}

pub fn production_entry() {}
"#,
		)
		.expect("write fixture");
		fs::write(
			temp.path().join("benches/speed.rs"),
			"pub fn benchmark_helper() {}\n",
		)
		.expect("write bench fixture");
		let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
		let refresh = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refresh, ProtocolResponse::Command(_)));

		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::SymbolSearch(code_moniker_query::SymbolSearchQuery {
				shape: vec!["callable".to_string()],
				..Default::default()
			}),
			consistency: code_moniker_query::Consistency::Current,
			page: Page {
				cursor: None,
				limit: 1,
			},
		})));
		let ProtocolResponse::Query(response) = response else {
			panic!("expected query response");
		};
		let QueryResult::SymbolList(list) = response.result else {
			panic!("expected symbol list, got {:?}", response.result);
		};

		assert_eq!(list.rows.len(), 1);
		assert_eq!(
			list.rows[0].name, "production_entry()",
			"production symbols must precede test symbols on the default page"
		);
	}

	#[test]
	fn daemon_answers_status_and_symbol_search() {
		let temp = tempfile::tempdir().expect("tempdir");
		fs::write(
			temp.path().join("lib.rs"),
			"pub struct Customer;\nimpl Customer { pub fn id(&self) -> u64 { 42 } }\n",
		)
		.expect("write fixture");
		let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
		let status = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::WorkspaceStatus,
		))));
		match status {
			ProtocolResponse::Query(response) => {
				assert!(matches!(response.result, QueryResult::WorkspaceStatus(_)));
			}
			other => panic!("unexpected response: {other:?}"),
		}
		let refresh = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(
			matches!(refresh, ProtocolResponse::Command(_)),
			"unexpected response: {refresh:?}"
		);
		let search = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::SymbolSearch(code_moniker_query::SymbolSearchQuery {
				text: Some("Customer".to_string()),
				..Default::default()
			}),
		))));
		match search {
			ProtocolResponse::Query(response) => match response.result {
				QueryResult::SymbolList(list) => {
					assert!(list.rows.iter().any(|row| row.name == "Customer"));
				}
				other => panic!("unexpected result: {other:?}"),
			},
			other => panic!("unexpected response: {other:?}"),
		}
	}

	#[test]
	fn daemon_returns_bounded_syntax_trees_for_files_and_symbols() {
		let temp = tempfile::tempdir().expect("tempdir");
		fs::write(
			temp.path().join("lib.rs"),
			"pub fn greet(name: &str) -> String {\n    format!(\"hello {name}\")\n}\n",
		)
		.expect("write fixture");
		let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
		let refresh = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refresh, ProtocolResponse::Command(_)));

		let file_response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(
			QueryRequest::new(Query::SyntaxTree(SyntaxTreeQuery {
				workspace: None,
				focus: "lib.rs".to_string(),
				max_depth: 8,
				max_nodes: 100,
				named_only: true,
				include_text: true,
				max_text_chars: 40,
			})),
		)));
		let ProtocolResponse::Query(file_response) = file_response else {
			panic!("expected syntax query response, got {file_response:?}");
		};
		let QueryResult::SyntaxTree(file_tree) = file_response.result else {
			panic!("expected syntax tree result");
		};
		assert_eq!(file_tree.file, "lib.rs");
		assert_eq!(file_tree.language, "rs");
		assert_eq!(file_tree.root.kind, "source_file");
		assert!(!file_tree.truncated);
		assert!(syntax_node_contains(&file_tree.root, "function_item", None));
		assert!(syntax_node_contains(
			&file_tree.root,
			"identifier",
			Some("greet")
		));

		let QueryResult::SymbolList(symbols) = search_symbols(&mut daemon, "greet") else {
			panic!("expected symbol list");
		};
		let symbol = symbols
			.rows
			.iter()
			.find(|symbol| symbol.name.starts_with("greet"))
			.expect("greet symbol");
		let symbol_response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(
			QueryRequest::new(Query::SyntaxTree(SyntaxTreeQuery {
				workspace: None,
				focus: symbol.uri.clone(),
				max_depth: 1,
				max_nodes: 2,
				named_only: true,
				include_text: false,
				max_text_chars: 0,
			})),
		)));
		let ProtocolResponse::Query(symbol_response) = symbol_response else {
			panic!("expected symbol syntax response, got {symbol_response:?}");
		};
		let QueryResult::SyntaxTree(symbol_tree) = symbol_response.result else {
			panic!("expected symbol syntax tree result");
		};
		assert_eq!(symbol_tree.root.kind, "function_item");
		assert_eq!(symbol_tree.emitted_nodes, 2);
		assert!(symbol_tree.truncated);
		assert!(symbol_tree.focus_line_range.is_some());
	}

	#[test]
	fn daemon_syntax_tree_uses_language_sdk_injections_for_plpgsql() {
		let temp = tempfile::tempdir().expect("tempdir");
		let source = "CREATE FUNCTION account_balance(p_id bigint) RETURNS numeric\n\
			 LANGUAGE plpgsql AS $$\n\
			 <<\"account block\">>\n\
			 DECLARE total numeric;\n\
			 BEGIN\n\
			   SELECT sum(amount) INTO total FROM ledger_entry WHERE account_id = p_id;\n\
			   IF total IS NULL THEN RETURN 0; END IF;\n\
			   RETURN total;\n\
			 EXCEPTION WHEN OTHERS THEN RETURN -1;\n\
			 END \"account block\";\n\
			 $$;\n";
		fs::write(temp.path().join("account.sql"), source).expect("write PL/pgSQL fixture");
		let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
		let refresh = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refresh, ProtocolResponse::Command(_)));

		let QueryResult::SymbolList(symbols) = search_symbols(&mut daemon, "account_balance")
		else {
			panic!("expected SQL symbol list");
		};
		let function = symbols
			.rows
			.iter()
			.find(|symbol| symbol.kind == "function")
			.expect("account_balance function");
		let compact =
			code_moniker_workspace::code::compact_identity(&function.uri, "code+moniker://")
				.expect("compact SQL moniker");
		assert!(
			!compact.contains('/'),
			"fixture must cover root-level compact monikers: {compact}"
		);
		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::SyntaxTree(SyntaxTreeQuery {
				workspace: None,
				focus: compact,
				max_depth: 20,
				max_nodes: 500,
				named_only: true,
				include_text: true,
				max_text_chars: 40,
			}),
		))));
		let ProtocolResponse::Query(response) = response else {
			panic!("expected PL/pgSQL syntax response, got {response:?}");
		};
		let QueryResult::SyntaxTree(tree) = response.result else {
			panic!("expected PL/pgSQL syntax tree result");
		};
		assert!(
			!tree.has_error,
			"indexed quoted PL/pgSQL label must parse: {tree:#?}"
		);
		assert_eq!(tree.root.kind, "toplevel_stmt");
		assert!(syntax_node_contains(&tree.root, "CreateFunctionStmt", None));
		assert!(syntax_node_contains_language(
			&tree.root,
			"source_file",
			"plpgsql"
		));
		assert!(syntax_node_contains(&tree.root, "stmt_if", None));
		assert!(syntax_node_contains(&tree.root, "stmt_return", None));
		assert!(syntax_node_contains(&tree.root, "sql_expression", None));
		assert!(syntax_node_contains(&tree.root, "block_label", None));
		assert!(syntax_node_contains(
			&tree.root,
			"quoted_identifier",
			Some("\"account block\""),
		));
		let quoted_label =
			syntax_node_find(&tree.root, "quoted_identifier", Some("\"account block\""))
				.expect("quoted block label");
		let label_text = "\"account block\"";
		let label_start = source.find(label_text).expect("label text in source");
		assert_eq!(
			quoted_label.byte_range,
			(label_start, label_start + label_text.len())
		);
		assert_eq!(syntax_node_language_count(&tree.root, "plpgsql"), 1);
	}

	#[test]
	fn syntax_tree_disambiguates_one_line_csharp_symbols() {
		let temp = tempfile::tempdir().expect("tempdir");
		fs::write(
			temp.path().join("App.cs"),
			"class App { App() {} void Run() {} }\n",
		)
		.expect("write one-line nested fixture");
		let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
		let refresh = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refresh, ProtocolResponse::Command(_)));
		let QueryResult::SymbolList(symbols) = search_symbols(&mut daemon, "App") else {
			panic!("expected C# symbol list");
		};
		for (symbol_kind, expected_node_kind) in [
			("class", "class_declaration"),
			("constructor", "constructor_declaration"),
		] {
			let symbol = symbols
				.rows
				.iter()
				.find(|symbol| symbol.kind == symbol_kind)
				.unwrap_or_else(|| panic!("missing {symbol_kind} symbol: {symbols:?}"));
			let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(
				QueryRequest::new(Query::SyntaxTree(SyntaxTreeQuery {
					workspace: None,
					focus: symbol.uri.clone(),
					max_depth: 4,
					max_nodes: 20,
					named_only: true,
					include_text: false,
					max_text_chars: 0,
				})),
			)));
			let ProtocolResponse::Query(response) = response else {
				panic!("expected {symbol_kind} syntax response, got {response:?}");
			};
			let QueryResult::SyntaxTree(tree) = response.result else {
				panic!("expected {symbol_kind} syntax tree result");
			};
			assert_eq!(tree.root.kind, expected_node_kind);
		}

		let QueryResult::SymbolList(symbols) = search_symbols(&mut daemon, "Run") else {
			panic!("expected C# symbol list");
		};
		let method = symbols
			.rows
			.iter()
			.find(|symbol| symbol.name.starts_with("Run"))
			.expect("Run method");
		let nested_response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(
			QueryRequest::new(Query::SyntaxTree(SyntaxTreeQuery {
				workspace: None,
				focus: method.uri.clone(),
				max_depth: 4,
				max_nodes: 20,
				named_only: true,
				include_text: false,
				max_text_chars: 0,
			})),
		)));
		let ProtocolResponse::Query(nested_response) = nested_response else {
			panic!("expected nested syntax response, got {nested_response:?}");
		};
		let QueryResult::SyntaxTree(nested_tree) = nested_response.result else {
			panic!("expected nested syntax tree result");
		};
		assert_eq!(nested_tree.root.kind, "method_declaration");
	}

	#[test]
	fn syntax_tree_accepts_an_empty_memory_source() {
		let temp = tempfile::tempdir().expect("tempdir");
		let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
		let refresh = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		assert!(matches!(refresh, ProtocolResponse::Command(_)));
		replace_source_set(
			&mut daemon,
			WorkspaceSourceSetDto {
				srcset: "empty".to_string(),
				revision: None,
				documents: vec![WorkspaceSourceDocumentDto {
					uri: "empty.rs".to_string(),
					language: "rs".to_string(),
					content: String::new(),
				}],
			},
		);
		let empty_response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(
			QueryRequest::new(Query::SyntaxTree(SyntaxTreeQuery {
				workspace: None,
				focus: "empty.rs".to_string(),
				max_depth: 4,
				max_nodes: 20,
				named_only: true,
				include_text: true,
				max_text_chars: 40,
			})),
		)));
		let ProtocolResponse::Query(empty_response) = empty_response else {
			panic!("expected empty memory syntax response, got {empty_response:?}");
		};
		let QueryResult::SyntaxTree(empty_tree) = empty_response.result else {
			panic!("expected empty memory syntax tree result");
		};
		assert_eq!(empty_tree.file, "empty.rs");
		assert_eq!(empty_tree.root.kind, "source_file");
	}

	fn syntax_node_contains(node: &SyntaxNodeDto, kind: &str, text: Option<&str>) -> bool {
		syntax_node_find(node, kind, text).is_some()
	}

	fn syntax_node_find<'a>(
		node: &'a SyntaxNodeDto,
		kind: &str,
		text: Option<&str>,
	) -> Option<&'a SyntaxNodeDto> {
		if node.kind == kind && text.is_none_or(|text| node.text.as_deref() == Some(text)) {
			return Some(node);
		}
		node.children
			.iter()
			.find_map(|child| syntax_node_find(child, kind, text))
	}

	fn syntax_node_contains_language(node: &SyntaxNodeDto, kind: &str, language: &str) -> bool {
		(node.kind == kind && node.language.as_deref() == Some(language))
			|| node
				.children
				.iter()
				.any(|child| syntax_node_contains_language(child, kind, language))
	}

	fn syntax_node_language_count(node: &SyntaxNodeDto, language: &str) -> usize {
		usize::from(node.language.as_deref() == Some(language))
			+ node
				.children
				.iter()
				.map(|child| syntax_node_language_count(child, language))
				.sum::<usize>()
	}

	#[test]
	fn memory_source_set_replace_is_idempotent_and_removable() {
		let temp = tempfile::tempdir().expect("tempdir");
		fs::write(
			temp.path().join("local.rs"),
			"pub fn local_symbol_survives() {}\n",
		)
		.expect("write local source");
		let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
		let refresh = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		let ProtocolResponse::Command(refresh) = refresh else {
			panic!("expected initial refresh, got {refresh:?}");
		};

		let source_set = WorkspaceSourceSetDto {
			srcset: "database".to_string(),
			revision: Some("1".to_string()),
			documents: vec![
				WorkspaceSourceDocumentDto {
					uri: "schema/accounts.sql".to_string(),
					language: "sql".to_string(),
					content: "CREATE TABLE app.virtual_accounts (id bigint);\n".to_string(),
				},
				WorkspaceSourceDocumentDto {
					uri: "schema/audit.sql".to_string(),
					language: "sql".to_string(),
					content: "CREATE TABLE app.virtual_audit (id bigint);\n".to_string(),
				},
			],
		};
		let replace = replace_source_set(&mut daemon, source_set.clone());
		assert!(
			replace.generation.expect("replace generation").0
				> refresh.generation.expect("refresh generation").0
		);

		let QueryResult::SymbolList(accounts) =
			search_symbols_named(&mut daemon, "virtual_accounts")
		else {
			panic!("expected symbol list");
		};
		assert_eq!(accounts.total, 1, "{accounts:?}");
		assert!(
			accounts.rows[0].uri.contains("/srcset:database/"),
			"the existing srcset identity facet must carry the supplied source set: {accounts:?}"
		);
		assert!(accounts.rows[0].file.ends_with("schema/accounts.sql"));
		assert_eq!(
			accounts.rows[0]
				.source
				.as_ref()
				.expect("in-memory source snippet")
				.lines[0]
				.text,
			"CREATE TABLE app.virtual_accounts (id bigint);"
		);

		let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceRefresh,
		}));
		let ProtocolResponse::Command(refreshed) = refreshed else {
			panic!("expected full refresh, got {refreshed:?}");
		};
		assert_symbol_total(&mut daemon, "virtual_accounts", 1);

		let mut reordered = source_set.clone();
		reordered.documents.reverse();
		let duplicate = replace_source_set(&mut daemon, reordered);
		assert_eq!(duplicate.generation, refreshed.generation);

		replace_source_set(
			&mut daemon,
			WorkspaceSourceSetDto {
				srcset: "database".to_string(),
				revision: Some("2".to_string()),
				documents: vec![WorkspaceSourceDocumentDto {
					uri: "schema/accounts.sql".to_string(),
					language: "sql".to_string(),
					content: "CREATE TABLE app.virtual_customers (id bigint);\n".to_string(),
				}],
			},
		);
		assert_symbol_total(&mut daemon, "virtual_accounts", 0);
		assert_symbol_total(&mut daemon, "virtual_audit", 0);
		assert_symbol_total(&mut daemon, "virtual_customers", 1);
		assert_symbol_total(&mut daemon, "local_symbol_survives()", 1);

		let remove = remove_source_set(&mut daemon, "database");
		assert_symbol_total(&mut daemon, "virtual_customers", 0);
		assert_symbol_total(&mut daemon, "local_symbol_survives()", 1);

		let duplicate_remove = remove_source_set(&mut daemon, "database");
		assert_eq!(duplicate_remove.generation, remove.generation);

		let rules = temp.path().join("memory-lifecycle-rules.toml");
		fs::write(
			&rules,
			r#"
default_rules = false

[[rust.fn.where]]
id = "local-function-remains"
expr = "name =~ ."
message = "the local function remains visible"
"#,
		)
		.expect("lifecycle rules");
		assert_memory_root_absent_from_rules(&mut daemon, &rules);
	}

	#[test]
	fn memory_source_set_refreshes_linkage_from_local_sources() {
		let temp = tempfile::tempdir().expect("tempdir");
		let source_dir = temp.path().join("src/main/java/app");
		fs::create_dir_all(&source_dir).expect("create Java source directory");
		fs::write(
			source_dir.join("Local.java"),
			"package app; public class Local { Generated value; }\n",
		)
		.expect("write local source");
		let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
		daemon
			.refresh_cancellable(WorkspaceCancellation::default())
			.expect("initial refresh");

		replace_source_set(
			&mut daemon,
			WorkspaceSourceSetDto {
				srcset: "main".to_string(),
				revision: Some("1".to_string()),
				documents: vec![WorkspaceSourceDocumentDto {
					uri: "src/main/java/app/Generated.java".to_string(),
					language: "java".to_string(),
					content: "package app; public class Generated {}\n".to_string(),
				}],
			},
		);
		let snapshot = daemon
			.registry
			.queries()
			.snapshot()
			.expect("indexed snapshot");
		let target = snapshot
			.index
			.symbols
			.iter()
			.find(|symbol| symbol.name == "Generated" && symbol.kind == "class")
			.expect("virtual target");
		assert!(
			snapshot
				.linkage
				.resolved
				.iter()
				.any(|edge| edge.target == target.id),
			"an unchanged local reference must be reconsidered when its in-memory target appears; \
			 refs={:?}; unresolved={:?}; target={}",
			snapshot
				.index
				.references
				.iter()
				.map(|reference| reference.target_identity.to_string())
				.collect::<Vec<_>>(),
			snapshot.linkage.unresolved,
			target.identity
		);
	}

	#[test]
	fn memory_source_set_rejects_ambiguous_input() {
		for srcset in ["bad/name", ".", ".."] {
			let error = parse_memory_source_set(WorkspaceSourceSetDto {
				srcset: srcset.to_string(),
				revision: None,
				documents: Vec::new(),
			})
			.expect_err("invalid srcset");
			assert_eq!(error.code, "invalid_workspace_srcset");
		}

		let error = parse_memory_source_set(WorkspaceSourceSetDto {
			srcset: "generated".to_string(),
			revision: None,
			documents: vec![WorkspaceSourceDocumentDto {
				uri: "generated.data".to_string(),
				language: "unknown".to_string(),
				content: String::new(),
			}],
		})
		.expect_err("invalid language");
		assert_eq!(error.code, "unsupported_workspace_source_language");

		let document = WorkspaceSourceDocumentDto {
			uri: "generated.rs".to_string(),
			language: "rs".to_string(),
			content: String::new(),
		};
		let error = parse_memory_source_set(WorkspaceSourceSetDto {
			srcset: "generated".to_string(),
			revision: None,
			documents: vec![document.clone(), document],
		})
		.expect_err("duplicate URI");
		assert_eq!(error.code, "duplicate_workspace_source_uri");
	}

	#[test]
	fn memory_source_set_publishes_its_new_generation() {
		let temp = tempfile::tempdir().expect("tempdir");
		let (events, mut rx) = tokio::sync::broadcast::channel(4);
		let mut daemon = WorkspaceDaemon::with_events(
			DaemonWorkspaceConfig {
				roots: vec![temp.path().display().to_string()],
				project: None,
				cache_dir: None,
				live_refresh: Some("on-demand".to_string()),
			},
			events,
		)
		.expect("daemon");
		daemon
			.refresh_cancellable(WorkspaceCancellation::default())
			.expect("initial refresh");
		let response = replace_source_set(
			&mut daemon,
			WorkspaceSourceSetDto {
				srcset: "generated".to_string(),
				revision: Some("1".to_string()),
				documents: vec![WorkspaceSourceDocumentDto {
					uri: "generated.rs".to_string(),
					language: "rs".to_string(),
					content: "pub fn generated() {}\n".to_string(),
				}],
			},
		);
		let event = rx.try_recv().expect("refreshed event");
		assert_eq!(event.kind, WorkspaceEventKind::Refreshed);
		assert_eq!(event.generation, response.generation);
	}

	#[test]
	fn memory_source_set_replace_rolls_back_after_refresh_failure() {
		let temp = tempfile::tempdir().expect("tempdir");
		let workspace = temp.path().join("workspace");
		let unavailable = temp.path().join("workspace-unavailable");
		fs::create_dir_all(&workspace).expect("workspace");
		let mut daemon = WorkspaceDaemon::new(vec![workspace.clone()]).expect("daemon");
		let source_set = WorkspaceSourceSetDto {
			srcset: "generated".to_string(),
			revision: Some("1".to_string()),
			documents: vec![WorkspaceSourceDocumentDto {
				uri: "generated.rs".to_string(),
				language: "rs".to_string(),
				content: "pub struct RetriedPublication;\n".to_string(),
			}],
		};

		fs::rename(&workspace, &unavailable).expect("make workspace unavailable");
		let failed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
			command: Command::WorkspaceSourceSetReplace {
				source_set: source_set.clone(),
			},
		}));
		assert!(
			matches!(failed, ProtocolResponse::Error(_)),
			"the unavailable workspace must reject publication: {failed:?}"
		);

		fs::rename(&unavailable, &workspace).expect("restore workspace");
		let retried = replace_source_set(&mut daemon, source_set);
		assert!(
			retried.generation.is_some(),
			"replaying a failed publication must rebuild and publish it"
		);
		assert_symbol_total(&mut daemon, "RetriedPublication", 1);
	}

	#[test]
	fn memory_source_set_has_workspace_level_multi_root_identity() {
		let temp = tempfile::tempdir().expect("tempdir");
		let first = temp.path().join("first");
		let second = temp.path().join("second");
		fs::create_dir_all(&first).expect("first root");
		fs::create_dir_all(&second).expect("second root");
		let source_set = WorkspaceSourceSetDto {
			srcset: "generated".to_string(),
			revision: Some("1".to_string()),
			documents: vec![WorkspaceSourceDocumentDto {
				uri: "generated.rs".to_string(),
				language: "rs".to_string(),
				content: "pub struct WorkspaceOwned;\n".to_string(),
			}],
		};
		let coordinates = |roots: Vec<PathBuf>| {
			let mut daemon = WorkspaceDaemon::new(roots).expect("daemon");
			daemon
				.refresh_cancellable(WorkspaceCancellation::default())
				.expect("initial refresh");
			replace_source_set(&mut daemon, source_set.clone());
			let QueryResult::SymbolList(symbols) =
				search_symbols_named(&mut daemon, "WorkspaceOwned")
			else {
				panic!("expected symbol list");
			};
			let symbol = symbols.rows.first().expect("workspace-owned symbol");
			(symbol.root.clone(), symbol.uri.clone())
		};

		let forward = coordinates(vec![first.clone(), second.clone()]);
		let reversed = coordinates(vec![second, first]);
		assert_eq!(forward, reversed);
		assert_eq!(forward.0, MEMORY_SOURCE_ROOT_LABEL);
	}

	#[test]
	fn memory_source_set_runs_through_unscoped_indexed_rules() {
		let temp = tempfile::tempdir().expect("tempdir");
		let rules = temp.path().join("memory-rules.toml");
		fs::write(
			&rules,
			r#"
default_rules = false

[[rust.shape.type.where]]
id = "memory-type-is-visible"
expr = "name != 'WorkspaceOwned'"
message = "the indexed rule must observe the memory source"
"#,
		)
		.expect("rules");
		let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
		daemon
			.refresh_cancellable(WorkspaceCancellation::default())
			.expect("initial refresh");
		replace_source_set(
			&mut daemon,
			WorkspaceSourceSetDto {
				srcset: "generated".to_string(),
				revision: Some("1".to_string()),
				documents: vec![WorkspaceSourceDocumentDto {
					uri: "generated.rs".to_string(),
					language: "rs".to_string(),
					content: "pub struct WorkspaceOwned;\n".to_string(),
				}],
			},
		);

		let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::RulesCheck(RulesCheckQuery {
				workspace: None,
				profile: None,
				rules: Some(rules.display().to_string()),
				file: Vec::new(),
				report: true,
			}),
		))));
		let ProtocolResponse::Query(response) = response else {
			panic!("expected rules response, got {response:?}");
		};
		let QueryResult::RulesCheck(result) = response.result else {
			panic!("expected rules result, got {:?}", response.result);
		};
		assert_eq!(result.summary.files_scanned, 1, "{result:?}");
		assert_eq!(result.summary.total_violations, 1, "{result:?}");
		assert_eq!(result.violations[0].root, MEMORY_SOURCE_ROOT);
		assert_eq!(
			result.violations[0].rule_id,
			"rust.shape.type.memory-type-is-visible"
		);
	}

	#[test]
	fn memory_source_set_limits_bound_each_publication_and_global_usage() {
		let limits = MemorySourceLimits {
			max_source_sets: 1,
			max_documents_per_set: 1,
			max_uri_bytes: 8,
			max_document_bytes: 8,
			max_source_set_bytes: 20,
			max_total_bytes: 20,
		};
		let cache = LocalResourceCache::default();
		let first = MemorySourceSet {
			srcset: "first".to_string(),
			revision: None,
			documents: vec![MemorySourceDocument {
				uri: "a.rs".to_string(),
				lang: Lang::Rs,
				content: "fn a()".to_string(),
			}],
		};
		validate_memory_source_set_limits(&cache, &first, limits).expect("first publication fits");
		cache.replace_memory_source_set(first);

		let second = MemorySourceSet {
			srcset: "second".to_string(),
			revision: None,
			documents: vec![MemorySourceDocument {
				uri: "b.rs".to_string(),
				lang: Lang::Rs,
				content: "fn b()".to_string(),
			}],
		};
		let error = validate_memory_source_set_limits(&cache, &second, limits)
			.expect_err("global active-set budget");
		assert_eq!(error.code, "workspace_source_set_limit_exceeded");

		let oversized = MemorySourceSet {
			srcset: "first".to_string(),
			revision: None,
			documents: vec![MemorySourceDocument {
				uri: "long-uri.rs".to_string(),
				lang: Lang::Rs,
				content: "fn too_large()".to_string(),
			}],
		};
		let error = validate_memory_source_set_limits(&cache, &oversized, limits)
			.expect_err("per-publication budget");
		assert_eq!(error.code, "workspace_source_set_limit_exceeded");
	}

	#[tokio::test]
	async fn rpc_server_answers_query_and_streams_events() {
		use code_moniker_query::DaemonRpcClient;
		use code_moniker_query::{WorkspaceEventDto, WorkspaceEventKind};
		use jsonrpsee::ws_client::WsClientBuilder;

		let temp = tempfile::tempdir().expect("tempdir");
		fs::write(temp.path().join("lib.rs"), "pub struct Customer;\n").expect("seed fixture");
		let (events, _) = tokio::sync::broadcast::channel(16);
		let daemon = WorkspaceDaemon::with_events(
			DaemonWorkspaceConfig {
				roots: vec![temp.path().display().to_string()],
				project: None,
				cache_dir: None,
				live_refresh: None,
			},
			events.clone(),
		)
		.expect("daemon");
		let build = producer_identity();
		let mut service = test_rpc_service(daemon, vec![temp.path().to_path_buf()], events.clone());
		service.handshake.build = build.clone();
		let daemon_handle = Arc::clone(&service.daemon);
		let server = Server::builder()
			.build("127.0.0.1:0")
			.await
			.expect("server binds");
		let addr = server.local_addr().expect("addr");
		let handle = server.start(service.into_rpc());

		let client = WsClientBuilder::default()
			.build(format!("ws://{addr}"))
			.await
			.expect("client connects");

		let (release_lock, lock_holder) = hold_workspace_lock(daemon_handle);
		let response = tokio::time::timeout(
			std::time::Duration::from_secs(1),
			client.query(QueryRequest::new(Query::SyntaxParse(
				code_moniker_query::SyntaxParseQuery {
					language: "rs".to_string(),
					source: "fn rpc_answer() -> u32 { 42 }".to_string(),
					uri: None,
					max_depth: 6,
					max_nodes: 100,
					named_only: true,
					include_text: false,
					max_text_chars: 80,
				},
			))),
		)
		.await
		.expect("syntax.parse must bypass the held workspace lock")
		.expect("syntax.parse RPC");
		release_lock.send(()).expect("release workspace lock");
		lock_holder.join().expect("workspace lock holder");
		assert!(matches!(response.result, QueryResult::SyntaxTree(_)));

		let response = client
			.query(QueryRequest::new(Query::WorkspaceStatus))
			.await
			.expect("query");
		let QueryResult::WorkspaceStatus(status) = response.result else {
			panic!("expected workspace status")
		};
		assert_eq!(status.producer, build);

		let mut subscription = client.subscribe_events().await.expect("subscribe");
		let replaced = client
			.command(CommandRequest {
				command: Command::WorkspaceSourceSetReplace {
					source_set: WorkspaceSourceSetDto {
						srcset: "generated".to_string(),
						revision: Some("rpc-1".to_string()),
						documents: vec![WorkspaceSourceDocumentDto {
							uri: "generated.rs".to_string(),
							language: "rs".to_string(),
							content: "pub struct RpcGenerated;\n".to_string(),
						}],
					},
				},
			})
			.await
			.expect("replace source set over RPC");
		let refreshed = subscription
			.next()
			.await
			.expect("refreshed event present")
			.expect("refreshed event decoded");
		assert_eq!(refreshed.kind, WorkspaceEventKind::Refreshed);
		assert_eq!(refreshed.generation, replaced.generation);

		events
			.send(WorkspaceEventDto {
				kind: WorkspaceEventKind::Notes,
				generation: None,
				stale_summary: None,
			})
			.expect("publish event");
		let event = subscription
			.next()
			.await
			.expect("event present")
			.expect("event decoded");
		assert_eq!(event.kind, WorkspaceEventKind::Notes);

		handle.stop().ok();
	}
}
