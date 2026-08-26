use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};
use std::sync::{Arc, Condvar, Mutex, OnceLock, RwLock, TryLockError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const GIT_BINARY_ENV: &str = "CODE_MONIKER_GIT_BINARY";
pub const MINIMUM_GIT_VERSION: (u32, u32, u32) = (2, 22, 0);
pub const SUPPORTED_GIT_VERSION_RANGE: &str = ">=2.22.0";

const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_OUTPUT_LIMIT: usize = 32 * 1024 * 1024;
const PROBE_OUTPUT_LIMIT: usize = 64 * 1024;
const RESOLUTION_RETRY_BACKOFF: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitDiagnosticState {
	Checking,
	Available,
	Unavailable,
	Incompatible,
	TimedOut,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitResolutionSource {
	ExplicitConfiguration,
	InheritedPath,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitRootState {
	Worktree,
	RepositoryOnly,
	NotRepository,
	Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitFailure {
	pub category: String,
	pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitRootDiagnostic {
	pub root: PathBuf,
	pub state: GitRootState,
	pub repository_root: Option<PathBuf>,
	pub failure: Option<GitFailure>,
	pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitDiagnostic {
	pub state: GitDiagnosticState,
	pub resolution_source: Option<GitResolutionSource>,
	pub executable: Option<PathBuf>,
	pub version: Option<String>,
	pub compatible: Option<bool>,
	pub failure: Option<GitFailure>,
	pub checked_at_unix_ms: Option<u64>,
	pub duration_ms: Option<u64>,
	pub roots: Vec<GitRootDiagnostic>,
}

impl GitDiagnostic {
	pub fn checking(roots: &[PathBuf]) -> Self {
		Self {
			state: GitDiagnosticState::Checking,
			resolution_source: None,
			executable: None,
			version: None,
			compatible: None,
			failure: None,
			checked_at_unix_ms: None,
			duration_ms: None,
			roots: roots
				.iter()
				.map(|root| GitRootDiagnostic {
					root: root.clone(),
					state: GitRootState::Unavailable,
					repository_root: None,
					failure: None,
					message: "Git diagnostic is still checking".to_string(),
				})
				.collect(),
		}
	}
}

#[derive(Clone, Debug)]
pub struct GitRuntimeConfig {
	pub explicit_binary: Option<PathBuf>,
	pub probe_timeout: Duration,
	pub command_timeout: Duration,
	pub output_limit: usize,
}

impl GitRuntimeConfig {
	pub fn from_environment() -> Self {
		Self {
			explicit_binary: std::env::var_os(GIT_BINARY_ENV).map(PathBuf::from),
			probe_timeout: DEFAULT_PROBE_TIMEOUT,
			command_timeout: DEFAULT_COMMAND_TIMEOUT,
			output_limit: DEFAULT_OUTPUT_LIMIT,
		}
	}
}

impl Default for GitRuntimeConfig {
	fn default() -> Self {
		Self::from_environment()
	}
}

#[derive(Clone, Debug)]
pub struct GitRuntime {
	config: GitRuntimeConfig,
	diagnostic: Arc<RwLock<GitDiagnostic>>,
	root_diagnostics: Arc<RwLock<Vec<GitRootDiagnostic>>>,
	probe_gate: Arc<Mutex<()>>,
	resolver: Arc<GitResolver>,
}

#[derive(Debug)]
pub struct GitOutput {
	pub stdout: Vec<u8>,
	pub stderr: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{category}: {message}")]
pub struct GitRuntimeError {
	pub category: String,
	pub message: String,
}

#[derive(Clone, Debug)]
struct ResolvedGit {
	executable: PathBuf,
	source: GitResolutionSource,
}

#[derive(Debug, Default)]
struct GitResolver {
	state: Mutex<GitResolverState>,
	ready: Condvar,
}

#[derive(Debug, Default)]
struct GitResolverState {
	started: bool,
	result: Option<Result<ResolvedGit, GitRuntimeError>>,
	retry_after: Option<Instant>,
}

impl GitResolver {
	fn invalidate(&self) {
		let mut state = self
			.state
			.lock()
			.unwrap_or_else(|poisoned| poisoned.into_inner());
		if state.result.is_some() {
			*state = GitResolverState::default();
		}
	}
}

pub fn process_git_runtime() -> &'static GitRuntime {
	static RUNTIME: OnceLock<GitRuntime> = OnceLock::new();
	RUNTIME.get_or_init(|| GitRuntime::from_environment(&[]))
}

impl GitRuntime {
	pub fn new(config: GitRuntimeConfig, roots: &[PathBuf]) -> Self {
		let checking = GitDiagnostic::checking(roots);
		Self {
			config,
			diagnostic: Arc::new(RwLock::new(GitDiagnostic::checking(&[]))),
			root_diagnostics: Arc::new(RwLock::new(checking.roots)),
			probe_gate: Arc::new(Mutex::new(())),
			resolver: Arc::new(GitResolver::default()),
		}
	}

	pub fn from_environment(roots: &[PathBuf]) -> Self {
		Self::new(GitRuntimeConfig::from_environment(), roots)
	}

	pub fn diagnostic(&self) -> GitDiagnostic {
		let mut diagnostic = self
			.diagnostic
			.read()
			.unwrap_or_else(|error| error.into_inner())
			.clone();
		diagnostic.roots = self
			.root_diagnostics
			.read()
			.unwrap_or_else(|error| error.into_inner())
			.clone();
		diagnostic
	}

	pub fn diagnostic_for(&self, roots: &[PathBuf]) -> GitDiagnostic {
		if roots.is_empty() {
			return self.diagnostic();
		}
		let diagnostic = self
			.diagnostic
			.read()
			.unwrap_or_else(|error| error.into_inner())
			.clone();
		let root_diagnostics = self
			.root_diagnostics
			.read()
			.unwrap_or_else(|error| error.into_inner());
		compose_root_diagnostic(diagnostic, &root_diagnostics, roots)
	}

	pub fn probe(&self, roots: &[PathBuf]) -> GitDiagnostic {
		probe_runtime(self, roots, true)
	}

	pub fn probe_if_needed(&self, roots: &[PathBuf]) -> GitDiagnostic {
		probe_runtime(self, roots, false)
	}

	pub fn run(&self, cwd: &Path, args: &[&str]) -> Result<GitOutput, GitRuntimeError> {
		run_runtime(
			self,
			cwd,
			args,
			self.config.command_timeout,
			self.config.output_limit,
		)
	}

	pub fn text(&self, cwd: &Path, args: &[&str]) -> Result<String, GitRuntimeError> {
		output_text(self.run(cwd, args)?)
	}
}

fn compose_root_diagnostic(
	mut diagnostic: GitDiagnostic,
	cached_roots: &[GitRootDiagnostic],
	roots: &[PathBuf],
) -> GitDiagnostic {
	let missing_root = roots
		.iter()
		.any(|root| !cached_roots.iter().any(|candidate| candidate.root == *root));
	if diagnostic.state == GitDiagnosticState::Available && missing_root {
		diagnostic.state = GitDiagnosticState::Checking;
	}
	let process_diagnostic = diagnostic.clone();
	diagnostic.roots = roots
		.iter()
		.map(|root| {
			cached_roots
				.iter()
				.find(|candidate| candidate.root == *root)
				.cloned()
				.unwrap_or_else(|| root_for_process_diagnostic(&process_diagnostic, root))
		})
		.collect();
	diagnostic
}

pub fn git_fast_text(cwd: &Path, args: &[&str]) -> Result<String, GitRuntimeError> {
	let runtime = process_git_runtime();
	let output = run_runtime(
		runtime,
		cwd,
		args,
		runtime.config.probe_timeout,
		PROBE_OUTPUT_LIMIT,
	)?;
	output_text(output)
}

pub fn run_git_executable_bounded(
	executable: &Path,
	cwd: &Path,
	args: &[&str],
	timeout: Duration,
	output_limit: usize,
) -> Result<GitOutput, GitRuntimeError> {
	if !executable.is_absolute() {
		return Err(GitRuntimeError {
			category: "invalid_configuration".to_string(),
			message: "supervised Git executable must be an absolute path".to_string(),
		});
	}
	if timeout.is_zero() || output_limit == 0 {
		return Err(GitRuntimeError {
			category: "invalid_configuration".to_string(),
			message: "supervised Git timeout and output limit must be positive".to_string(),
		});
	}
	run_bounded(executable, cwd, args, timeout, output_limit)
}

fn run_runtime(
	runtime: &GitRuntime,
	cwd: &Path,
	args: &[&str],
	timeout: Duration,
	output_limit: usize,
) -> Result<GitOutput, GitRuntimeError> {
	let resolved = usable_git(runtime)?;
	let result = run_bounded(&resolved.executable, cwd, args, timeout, output_limit);
	if let Err(error) = &result {
		if error.category == "command_failed" && is_not_repository_failure(&error.message) {
			record_not_repository(runtime, cwd, error);
		} else if error.category != "command_failed" {
			record_execution_failure(runtime, &resolved, cwd, error);
		}
	}
	result
}

fn probe_runtime(runtime: &GitRuntime, roots: &[PathBuf], force: bool) -> GitDiagnostic {
	let started = Instant::now();
	let deadline = started + runtime.config.probe_timeout.saturating_mul(2);
	let _probe = loop {
		match runtime.probe_gate.try_lock() {
			Ok(probe) => break probe,
			Err(TryLockError::Poisoned(poisoned)) => break poisoned.into_inner(),
			Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
				std::thread::sleep(Duration::from_millis(10));
			}
			Err(TryLockError::WouldBlock) => {
				return probe_gate_timeout(runtime, roots, started.elapsed());
			}
		}
	};
	if !force {
		let diagnostic = runtime.diagnostic_for(roots);
		if diagnostic.state != GitDiagnosticState::Checking {
			return diagnostic;
		}
	}
	let checked_at_unix_ms = unix_timestamp_ms();
	let mut diagnostic = match resolve_with_deadline(runtime, deadline) {
		Ok(resolved) => probe_resolved(&runtime.config, &resolved, roots, deadline),
		Err(error) => unavailable_diagnostic(
			error,
			roots,
			if runtime.config.explicit_binary.is_some() {
				GitResolutionSource::ExplicitConfiguration
			} else {
				GitResolutionSource::InheritedPath
			},
		),
	};
	diagnostic.checked_at_unix_ms = Some(checked_at_unix_ms);
	diagnostic.duration_ms = Some(duration_ms(started.elapsed()));
	let mut process_diagnostic = diagnostic.clone();
	process_diagnostic.roots.clear();
	*runtime
		.diagnostic
		.write()
		.unwrap_or_else(|error| error.into_inner()) = process_diagnostic;
	if !roots.is_empty() {
		let mut cached_roots = runtime
			.root_diagnostics
			.write()
			.unwrap_or_else(|error| error.into_inner());
		for root in &diagnostic.roots {
			if let Some(cached) = cached_roots
				.iter_mut()
				.find(|cached| cached.root == root.root)
			{
				*cached = root.clone();
			} else {
				cached_roots.push(root.clone());
			}
		}
	}
	diagnostic
}

fn root_for_process_diagnostic(diagnostic: &GitDiagnostic, root: &Path) -> GitRootDiagnostic {
	if let Some(failure) = &diagnostic.failure {
		return unavailable_root_diagnostic(root, &failure.category, &failure.message);
	}
	GitDiagnostic::checking(&[root.to_path_buf()])
		.roots
		.pop()
		.expect("one checking root")
}

fn probe_gate_timeout(runtime: &GitRuntime, roots: &[PathBuf], elapsed: Duration) -> GitDiagnostic {
	let current = runtime.diagnostic();
	GitDiagnostic {
		state: GitDiagnosticState::TimedOut,
		resolution_source: current.resolution_source,
		executable: current.executable,
		version: current.version,
		compatible: current.compatible,
		failure: Some(GitFailure {
			category: "timed_out".to_string(),
			message: "Git diagnostic timed out while waiting for another probe".to_string(),
		}),
		checked_at_unix_ms: Some(unix_timestamp_ms()),
		duration_ms: Some(duration_ms(elapsed)),
		roots: unavailable_roots(
			roots,
			"timed_out",
			"Git diagnostic timed out while waiting for another probe",
		),
	}
}

fn resolve_with_deadline(
	runtime: &GitRuntime,
	deadline: Instant,
) -> Result<ResolvedGit, GitRuntimeError> {
	resolve_with_deadline_using(
		Arc::clone(&runtime.resolver),
		runtime.config.clone(),
		deadline,
		resolve_config,
	)
}

fn resolve_with_deadline_using<F>(
	resolver_state: Arc<GitResolver>,
	config: GitRuntimeConfig,
	deadline: Instant,
	resolver: F,
) -> Result<ResolvedGit, GitRuntimeError>
where
	F: FnOnce(&GitRuntimeConfig) -> Result<ResolvedGit, GitRuntimeError> + Send + 'static,
{
	let mut state = resolver_state
		.state
		.lock()
		.unwrap_or_else(|poisoned| poisoned.into_inner());
	if state.result.as_ref().is_some_and(Result::is_err)
		&& state
			.retry_after
			.is_some_and(|retry_after| Instant::now() >= retry_after)
	{
		*state = GitResolverState::default();
	}
	if !state.started {
		state.started = true;
		let shared = Arc::clone(&resolver_state);
		let spawned = std::thread::Builder::new()
			.name("code-moniker-git-resolver".to_string())
			.spawn(move || {
				let result = resolver(&config);
				let mut state = shared
					.state
					.lock()
					.unwrap_or_else(|poisoned| poisoned.into_inner());
				state.retry_after = result
					.is_err()
					.then(|| Instant::now() + RESOLUTION_RETRY_BACKOFF);
				state.result = Some(result);
				shared.ready.notify_all();
			});
		if let Err(error) = spawned {
			*state = GitResolverState::default();
			return Err(GitRuntimeError {
				category: "resolver_thread_unavailable".to_string(),
				message: format!("cannot start Git resolver thread: {error}"),
			});
		}
	}
	loop {
		if let Some(result) = &state.result {
			return result.clone();
		}
		let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
			return Err(GitRuntimeError {
				category: "timed_out".to_string(),
				message: "Git executable resolution timed out".to_string(),
			});
		};
		let (next, timeout) = resolver_state
			.ready
			.wait_timeout(state, remaining)
			.unwrap_or_else(|poisoned| poisoned.into_inner());
		state = next;
		if timeout.timed_out() && state.result.is_none() {
			return Err(GitRuntimeError {
				category: "timed_out".to_string(),
				message: "Git executable resolution timed out".to_string(),
			});
		}
	}
}

fn resolve_config(config: &GitRuntimeConfig) -> Result<ResolvedGit, GitRuntimeError> {
	if let Some(explicit) = &config.explicit_binary {
		if !explicit.is_absolute() {
			return Err(GitRuntimeError {
				category: "invalid_configuration".to_string(),
				message: format!("{GIT_BINARY_ENV} must name an absolute executable path"),
			});
		}
		return resolve_candidate(explicit, GitResolutionSource::ExplicitConfiguration);
	}
	let path = std::env::var_os("PATH").ok_or_else(|| GitRuntimeError {
		category: "path_unavailable".to_string(),
		message: "cannot resolve Git because PATH is unavailable".to_string(),
	})?;
	resolve_from_path(&path)
}

fn usable_git(runtime: &GitRuntime) -> Result<ResolvedGit, GitRuntimeError> {
	let diagnostic = runtime.diagnostic();
	if diagnostic.state == GitDiagnosticState::Checking {
		runtime.probe(&[]);
	}
	let diagnostic = runtime.diagnostic();
	match (
		diagnostic.state,
		diagnostic.executable,
		diagnostic.resolution_source,
	) {
		(GitDiagnosticState::Available, Some(executable), Some(source)) => {
			Ok(ResolvedGit { executable, source })
		}
		_ => Err(GitRuntimeError {
			category: diagnostic
				.failure
				.as_ref()
				.map(|failure| failure.category.clone())
				.unwrap_or_else(|| "unavailable".to_string()),
			message: diagnostic
				.failure
				.map(|failure| failure.message)
				.unwrap_or_else(|| "Git runtime dependency is unavailable".to_string()),
		}),
	}
}

fn probe_resolved(
	config: &GitRuntimeConfig,
	resolved: &ResolvedGit,
	roots: &[PathBuf],
	deadline: Instant,
) -> GitDiagnostic {
	let version = match probe_version(config, resolved, deadline) {
		Ok(version) => version,
		Err((state, error)) => {
			return failed_resolved_diagnostic(
				resolved,
				state,
				&error.category,
				error.message,
				roots,
			);
		}
	};
	let Some(parsed) = parse_git_version(&version) else {
		return failed_resolved_diagnostic(
			resolved,
			GitDiagnosticState::Unavailable,
			"malformed_version",
			format!("Git returned an unrecognized version response: {version:?}"),
			roots,
		);
	};
	compatible_diagnostic(config, resolved, roots, version, parsed, deadline)
}

fn probe_version(
	config: &GitRuntimeConfig,
	resolved: &ResolvedGit,
	deadline: Instant,
) -> Result<String, (GitDiagnosticState, GitRuntimeError)> {
	let timeout = remaining_probe_timeout(deadline, config.probe_timeout).ok_or_else(|| {
		(
			GitDiagnosticState::TimedOut,
			GitRuntimeError {
				category: "timed_out".to_string(),
				message: "Git diagnostic time budget was exhausted before version probing"
					.to_string(),
			},
		)
	})?;
	let output = run_bounded(
		&resolved.executable,
		Path::new("."),
		&["--version"],
		timeout,
		PROBE_OUTPUT_LIMIT,
	)
	.map_err(|error| {
		let state = if error.category == "timed_out" {
			GitDiagnosticState::TimedOut
		} else {
			GitDiagnosticState::Unavailable
		};
		(state, error)
	})?;
	String::from_utf8(output.stdout).map_or_else(
		|error| {
			Err((
				GitDiagnosticState::Unavailable,
				GitRuntimeError {
					category: "malformed_output".to_string(),
					message: format!("Git returned a non-UTF-8 version response: {error}"),
				},
			))
		},
		|version| Ok(version.trim().to_string()),
	)
}

fn compatible_diagnostic(
	config: &GitRuntimeConfig,
	resolved: &ResolvedGit,
	roots: &[PathBuf],
	version: String,
	parsed: (u32, u32, u32),
	deadline: Instant,
) -> GitDiagnostic {
	let compatible = parsed >= MINIMUM_GIT_VERSION;
	let root_diagnostics = if compatible {
		roots
			.iter()
			.map(|root| probe_root(config, resolved, root, deadline))
			.collect()
	} else {
		unavailable_roots(roots, "incompatible_version", "Git version is incompatible")
	};
	GitDiagnostic {
		state: if compatible {
			GitDiagnosticState::Available
		} else {
			GitDiagnosticState::Incompatible
		},
		resolution_source: Some(resolved.source),
		executable: Some(resolved.executable.clone()),
		version: Some(version.clone()),
		compatible: Some(compatible),
		failure: (!compatible).then(|| GitFailure {
			category: "incompatible_version".to_string(),
			message: format!(
				"Git {version:?} does not satisfy supported range {SUPPORTED_GIT_VERSION_RANGE}"
			),
		}),
		checked_at_unix_ms: None,
		duration_ms: None,
		roots: root_diagnostics,
	}
}

fn probe_root(
	config: &GitRuntimeConfig,
	resolved: &ResolvedGit,
	root: &Path,
	deadline: Instant,
) -> GitRootDiagnostic {
	let Some(timeout) = remaining_probe_timeout(deadline, config.probe_timeout) else {
		return exhausted_root_diagnostic(root);
	};
	let output = run_bounded(
		&resolved.executable,
		root,
		&["rev-parse", "--is-inside-work-tree", "--is-bare-repository"],
		timeout,
		PROBE_OUTPUT_LIMIT,
	);
	match output {
		Ok(output) => available_root_diagnostic(config, resolved, root, &output.stdout, deadline),
		Err(error)
			if error.category == "command_failed" && is_not_repository_failure(&error.message) =>
		{
			GitRootDiagnostic {
				root: root.to_path_buf(),
				state: GitRootState::NotRepository,
				repository_root: None,
				failure: Some(GitFailure {
					category: "not_repository".to_string(),
					message: sanitize(&error.message),
				}),
				message: "root is not inside a Git worktree".to_string(),
			}
		}
		Err(error) => GitRootDiagnostic {
			root: root.to_path_buf(),
			state: GitRootState::Unavailable,
			repository_root: None,
			failure: Some(GitFailure {
				category: error.category,
				message: error.message.to_owned(),
			}),
			message: error.message,
		},
	}
}

fn is_not_repository_failure(message: &str) -> bool {
	message
		.to_ascii_lowercase()
		.contains("not a git repository")
}

fn available_root_diagnostic(
	config: &GitRuntimeConfig,
	resolved: &ResolvedGit,
	root: &Path,
	stdout: &[u8],
	deadline: Instant,
) -> GitRootDiagnostic {
	let Ok(text) = std::str::from_utf8(stdout) else {
		return unavailable_root_diagnostic(
			root,
			"malformed_output",
			"Git repository probe returned non-UTF-8 output",
		);
	};
	let lines = text.lines().map(str::trim).collect::<Vec<_>>();
	let (inside, bare) = match lines.as_slice() {
		["true", "false"] => (true, false),
		["false", "true"] => (false, true),
		_ => {
			return unavailable_root_diagnostic(
				root,
				"malformed_output",
				&format!("Git repository probe returned unexpected output: {text:?}"),
			);
		}
	};
	let repository_root = if inside {
		let Some(timeout) = remaining_probe_timeout(deadline, config.probe_timeout) else {
			return exhausted_root_diagnostic(root);
		};
		let output = match run_bounded(
			&resolved.executable,
			root,
			&["rev-parse", "--show-toplevel"],
			timeout,
			PROBE_OUTPUT_LIMIT,
		) {
			Ok(output) => output,
			Err(error) => {
				return GitRootDiagnostic {
					root: root.to_path_buf(),
					state: GitRootState::Unavailable,
					repository_root: None,
					failure: Some(GitFailure {
						category: error.category,
						message: error.message.to_owned(),
					}),
					message: error.message,
				};
			}
		};
		let Ok(path) = String::from_utf8(output.stdout) else {
			return unavailable_root_diagnostic(
				root,
				"malformed_output",
				"Git worktree root probe returned non-UTF-8 output",
			);
		};
		let path = PathBuf::from(path.trim());
		if !path.is_absolute() {
			return unavailable_root_diagnostic(
				root,
				"malformed_output",
				"Git worktree root probe did not return an absolute path",
			);
		}
		Some(path)
	} else {
		None
	};
	GitRootDiagnostic {
		root: root.to_path_buf(),
		state: if inside {
			GitRootState::Worktree
		} else if bare {
			GitRootState::RepositoryOnly
		} else {
			GitRootState::NotRepository
		},
		repository_root,
		failure: (!inside && !bare).then(|| GitFailure {
			category: "not_repository".to_string(),
			message: "root is not a Git repository".to_string(),
		}),
		message: if inside {
			"Git worktree is available".to_string()
		} else if bare {
			"Git repository has no worktree".to_string()
		} else {
			"root is not a Git repository".to_string()
		},
	}
}

fn unavailable_root_diagnostic(root: &Path, category: &str, message: &str) -> GitRootDiagnostic {
	GitRootDiagnostic {
		root: root.to_path_buf(),
		state: GitRootState::Unavailable,
		repository_root: None,
		failure: Some(GitFailure {
			category: category.to_string(),
			message: sanitize(message),
		}),
		message: sanitize(message),
	}
}

fn remaining_probe_timeout(deadline: Instant, command_timeout: Duration) -> Option<Duration> {
	deadline
		.checked_duration_since(Instant::now())
		.map(|remaining| remaining.min(command_timeout))
		.filter(|remaining| !remaining.is_zero())
}

fn exhausted_root_diagnostic(root: &Path) -> GitRootDiagnostic {
	GitRootDiagnostic {
		root: root.to_path_buf(),
		state: GitRootState::Unavailable,
		repository_root: None,
		failure: Some(GitFailure {
			category: "timed_out".to_string(),
			message: "Git diagnostic time budget was exhausted".to_string(),
		}),
		message: "Git diagnostic time budget was exhausted".to_string(),
	}
}

fn record_execution_failure(
	runtime: &GitRuntime,
	resolved: &ResolvedGit,
	cwd: &Path,
	error: &GitRuntimeError,
) {
	let executable_failed = match error.category.as_str() {
		"not_found" => !resolved.executable.is_file(),
		"permission_denied" => executable_permission_lost(&resolved.executable),
		_ => false,
	};
	if !executable_failed {
		record_root_execution_failure(runtime, cwd, error);
		return;
	}
	runtime.resolver.invalidate();
	let mut diagnostic = runtime
		.diagnostic
		.write()
		.unwrap_or_else(|poisoned| poisoned.into_inner());
	diagnostic.resolution_source = Some(resolved.source);
	diagnostic.executable = Some(resolved.executable.clone());
	diagnostic.checked_at_unix_ms = Some(unix_timestamp_ms());
	diagnostic.duration_ms = None;
	apply_execution_failure(&mut diagnostic, error);
	for root in runtime
		.root_diagnostics
		.write()
		.unwrap_or_else(|poisoned| poisoned.into_inner())
		.iter_mut()
	{
		apply_root_failure(root, error);
	}
}

#[cfg(unix)]
fn executable_permission_lost(executable: &Path) -> bool {
	use std::os::unix::fs::PermissionsExt;

	std::fs::metadata(executable)
		.map(|metadata| !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0)
		.unwrap_or(true)
}

#[cfg(windows)]
fn executable_permission_lost(_executable: &Path) -> bool {
	true
}

fn record_root_execution_failure(runtime: &GitRuntime, cwd: &Path, error: &GitRuntimeError) {
	for root in runtime
		.root_diagnostics
		.write()
		.unwrap_or_else(|poisoned| poisoned.into_inner())
		.iter_mut()
	{
		if cwd.starts_with(&root.root) || root.root.starts_with(cwd) {
			apply_root_failure(root, error);
		}
	}
}

fn record_not_repository(runtime: &GitRuntime, cwd: &Path, error: &GitRuntimeError) {
	for root in runtime
		.root_diagnostics
		.write()
		.unwrap_or_else(|poisoned| poisoned.into_inner())
		.iter_mut()
	{
		if cwd.starts_with(&root.root) || root.root.starts_with(cwd) {
			root.state = GitRootState::NotRepository;
			root.repository_root = None;
			root.failure = Some(GitFailure {
				category: "not_repository".to_string(),
				message: sanitize(&error.message),
			});
			root.message = "root is not inside a Git worktree".to_string();
		}
	}
}

fn apply_root_failure(root: &mut GitRootDiagnostic, error: &GitRuntimeError) {
	root.state = GitRootState::Unavailable;
	root.repository_root = None;
	root.failure = Some(GitFailure {
		category: error.category.clone(),
		message: error.message.clone(),
	});
	root.message.clone_from(&error.message);
}

fn apply_execution_failure(diagnostic: &mut GitDiagnostic, error: &GitRuntimeError) {
	diagnostic.state = if error.category == "timed_out" {
		GitDiagnosticState::TimedOut
	} else {
		GitDiagnosticState::Unavailable
	};
	let category = error.category.to_owned();
	let message = error.message.to_owned();
	diagnostic.failure = Some(GitFailure {
		category: category.clone(),
		message: message.clone(),
	});
	for root in &mut diagnostic.roots {
		root.state = GitRootState::Unavailable;
		root.failure = Some(GitFailure {
			category: category.to_owned(),
			message: message.to_owned(),
		});
		root.message.clone_from(&message);
	}
}

fn resolve_from_path(path: &OsStr) -> Result<ResolvedGit, GitRuntimeError> {
	let executable = if cfg!(windows) { "git.exe" } else { "git" };
	let mut permission_failure = None;
	for directory in std::env::split_paths(path) {
		if directory.as_os_str().is_empty() {
			continue;
		}
		let candidate = directory.join(executable);
		match resolve_candidate(&candidate, GitResolutionSource::InheritedPath) {
			Ok(resolved) => return Ok(resolved),
			Err(error) if error.category == "permission_denied" => {
				permission_failure.get_or_insert(error);
			}
			Err(error) if error.category == "not_found" => {}
			Err(_) => {}
		}
	}
	if let Some(error) = permission_failure {
		return Err(error);
	}
	Err(GitRuntimeError {
		category: "not_found".to_string(),
		message: format!("cannot resolve Git: {executable} is not present in inherited PATH"),
	})
}

fn resolve_candidate(
	candidate: &Path,
	source: GitResolutionSource,
) -> Result<ResolvedGit, GitRuntimeError> {
	let metadata = std::fs::metadata(candidate).map_err(|error| GitRuntimeError {
		category: resolution_error_category(&error).to_string(),
		message: format!(
			"cannot inspect configured Git executable {}: {error}",
			candidate.display()
		),
	})?;
	if !metadata.is_file() {
		return Err(GitRuntimeError {
			category: "not_found".to_string(),
			message: format!(
				"configured Git executable {} is not a file",
				candidate.display()
			),
		});
	}
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		if metadata.permissions().mode() & 0o111 == 0 {
			return Err(GitRuntimeError {
				category: "permission_denied".to_string(),
				message: format!(
					"configured Git executable {} is not executable",
					candidate.display()
				),
			});
		}
	}
	let executable = candidate.canonicalize().map_err(|error| GitRuntimeError {
		category: resolution_error_category(&error).to_string(),
		message: format!(
			"cannot resolve absolute Git executable path {}: {error}",
			candidate.display()
		),
	})?;
	Ok(ResolvedGit { executable, source })
}

fn resolution_error_category(error: &std::io::Error) -> &'static str {
	match error.kind() {
		std::io::ErrorKind::NotFound => "not_found",
		std::io::ErrorKind::PermissionDenied => "permission_denied",
		_ => "resolution_failed",
	}
}

#[cfg(unix)]
macro_rules! windows_job_ref {
	($job:ident) => {
		()
	};
}

#[cfg(windows)]
macro_rules! windows_job_ref {
	($job:ident) => {
		Some(&$job)
	};
}

fn run_bounded(
	executable: &Path,
	cwd: &Path,
	args: &[&str],
	timeout: Duration,
	output_limit: usize,
) -> Result<GitOutput, GitRuntimeError> {
	let mut command = Command::new(executable);
	command
		.current_dir(cwd)
		.env("GIT_OPTIONAL_LOCKS", "0")
		.env("LC_ALL", "C")
		.env("LANG", "C")
		.args(args)
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped());
	#[cfg(unix)]
	{
		use std::os::unix::process::CommandExt;
		command.process_group(0);
	}
	#[cfg(windows)]
	{
		use std::os::windows::process::CommandExt;
		use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;
		command.creation_flags(CREATE_SUSPENDED);
	}
	let mut child = command
		.spawn()
		.map_err(|error| spawn_error(executable, error))?;
	#[cfg(windows)]
	let process_job = match WindowsProcessJob::attach(&child) {
		Ok(job) => job,
		Err(error) => {
			let _ = child.kill();
			let _ = child.wait();
			return Err(error);
		}
	};
	#[cfg(windows)]
	if let Err(error) = resume_suspended_process(child.id()) {
		terminate_child(&mut child, Some(&process_job));
		return Err(error);
	}
	let Some(stdout) = child.stdout.take() else {
		terminate_child(&mut child, windows_job_ref!(process_job));
		return Err(GitRuntimeError {
			category: "pipe_unavailable".to_string(),
			message: "Git stdout pipe was not created".to_string(),
		});
	};
	let Some(stderr) = child.stderr.take() else {
		terminate_child(&mut child, windows_job_ref!(process_job));
		return Err(GitRuntimeError {
			category: "pipe_unavailable".to_string(),
			message: "Git stderr pipe was not created".to_string(),
		});
	};
	let (stdout_sender, stdout_reader) = sync_channel(1);
	let (stderr_sender, stderr_reader) = sync_channel(1);
	let mut stdout_thread = match std::thread::Builder::new()
		.name("code-moniker-git-stdout".to_string())
		.spawn(move || {
			let _ = stdout_sender.send(read_bounded(stdout, output_limit));
		}) {
		Ok(thread) => Some(thread),
		Err(error) => {
			terminate_child(&mut child, windows_job_ref!(process_job));
			return Err(reader_thread_error("stdout", error));
		}
	};
	let mut stderr_thread = match std::thread::Builder::new()
		.name("code-moniker-git-stderr".to_string())
		.spawn(move || {
			let _ = stderr_sender.send(read_bounded(stderr, output_limit));
		}) {
		Ok(thread) => Some(thread),
		Err(error) => {
			terminate_child(&mut child, windows_job_ref!(process_job));
			join_reader_threads(&mut stdout_thread, &mut None);
			return Err(reader_thread_error("stderr", error));
		}
	};
	let started = Instant::now();
	let mut status = None;
	let mut stdout_result = None;
	let mut stderr_result = None;
	loop {
		if let Err(error) = poll_reader(&stdout_reader, &mut stdout_result, "stdout") {
			terminate_child(&mut child, windows_job_ref!(process_job));
			join_reader_threads(&mut stdout_thread, &mut stderr_thread);
			return Err(error);
		}
		if let Err(error) = poll_reader(&stderr_reader, &mut stderr_result, "stderr") {
			terminate_child(&mut child, windows_job_ref!(process_job));
			join_reader_threads(&mut stdout_thread, &mut stderr_thread);
			return Err(error);
		}
		if stdout_result
			.as_ref()
			.is_some_and(|(_, truncated)| *truncated)
			|| stderr_result
				.as_ref()
				.is_some_and(|(_, truncated)| *truncated)
		{
			terminate_child(&mut child, windows_job_ref!(process_job));
			join_reader_threads(&mut stdout_thread, &mut stderr_thread);
			return Err(GitRuntimeError {
				category: "output_limit".to_string(),
				message: format!("Git command exceeded the {output_limit}-byte output limit"),
			});
		}
		if status.is_none() {
			status = match child.try_wait() {
				Ok(status) => status,
				Err(error) => {
					terminate_child(&mut child, windows_job_ref!(process_job));
					join_reader_threads(&mut stdout_thread, &mut stderr_thread);
					return Err(GitRuntimeError {
						category: "wait_failed".to_string(),
						message: format!("cannot wait for Git command: {error}"),
					});
				}
			};
		}
		if status.is_some() && stdout_result.is_some() && stderr_result.is_some() {
			break;
		}
		if started.elapsed() >= timeout {
			terminate_child(&mut child, windows_job_ref!(process_job));
			join_reader_threads(&mut stdout_thread, &mut stderr_thread);
			return Err(GitRuntimeError {
				category: "timed_out".to_string(),
				message: format!("Git command timed out after {} ms", duration_ms(timeout)),
			});
		}
		std::thread::sleep(Duration::from_millis(10));
	}
	join_reader_threads(&mut stdout_thread, &mut stderr_thread);
	let status = status.expect("completed Git command has an exit status");
	let (stdout, stdout_truncated) = stdout_result.expect("completed stdout reader");
	let (stderr, stderr_truncated) = stderr_result.expect("completed stderr reader");
	if stdout_truncated || stderr_truncated {
		return Err(GitRuntimeError {
			category: "output_limit".to_string(),
			message: format!("Git command exceeded the {output_limit}-byte output limit"),
		});
	}
	if !status.success() {
		let message = String::from_utf8_lossy(&stderr).trim().to_string();
		return Err(GitRuntimeError {
			category: "command_failed".to_string(),
			message: if message.is_empty() {
				format!("Git command exited with status {status}")
			} else {
				format!("Git command failed: {}", sanitize(&message))
			},
		});
	}
	Ok(GitOutput { stdout, stderr })
}

fn reader_thread_error(stream: &str, error: std::io::Error) -> GitRuntimeError {
	GitRuntimeError {
		category: "reader_thread_unavailable".to_string(),
		message: format!("cannot start Git {stream} reader thread: {error}"),
	}
}

fn join_reader_threads(
	stdout: &mut Option<std::thread::JoinHandle<()>>,
	stderr: &mut Option<std::thread::JoinHandle<()>>,
) {
	if let Some(thread) = stdout.take() {
		let _ = thread.join();
	}
	if let Some(thread) = stderr.take() {
		let _ = thread.join();
	}
}

#[cfg(unix)]
fn terminate_child(child: &mut std::process::Child, _job: ()) {
	#[cfg(unix)]
	unsafe {
		libc::kill(-(child.id() as i32), libc::SIGKILL);
	}
	let _ = child.kill();
	let _ = child.wait();
}

#[cfg(windows)]
fn terminate_child(child: &mut std::process::Child, job: Option<&WindowsProcessJob>) {
	job.inspect(|job| job.terminate());
	let _ = child.kill();
	let _ = child.wait();
}

#[cfg(windows)]
struct WindowsProcessJob {
	handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl WindowsProcessJob {
	// SAFETY: This narrow FFI adapter configures the Windows Job Object API in one place.
	// code-moniker: ignore[smell-feature-envy-local]
	fn attach(child: &std::process::Child) -> Result<Self, GitRuntimeError> {
		use std::os::windows::io::AsRawHandle;
		use windows_sys::Win32::Foundation::CloseHandle;
		use windows_sys::Win32::System::JobObjects::{
			AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
			JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
			SetInformationJobObject,
		};
		let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
		if handle.is_null() {
			return Err(process_isolation_error("cannot create Windows Job Object"));
		}
		let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
		limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
		let configured = unsafe {
			SetInformationJobObject(
				handle,
				JobObjectExtendedLimitInformation,
				std::ptr::from_ref(&limits).cast(),
				u32::try_from(std::mem::size_of_val(&limits)).unwrap_or(u32::MAX),
			)
		};
		if configured == 0 {
			unsafe { CloseHandle(handle) };
			return Err(process_isolation_error(
				"cannot configure Windows Job Object tree termination",
			));
		}
		let assigned = unsafe { AssignProcessToJobObject(handle, child.as_raw_handle().cast()) };
		if assigned == 0 {
			unsafe { CloseHandle(handle) };
			return Err(process_isolation_error(
				"cannot assign suspended Git process to Windows Job Object",
			));
		}
		Ok(Self { handle })
	}

	fn terminate(&self) {
		unsafe {
			windows_sys::Win32::System::JobObjects::TerminateJobObject(self.handle, 1);
		}
	}
}

#[cfg(windows)]
fn resume_suspended_process(process_id: u32) -> Result<(), GitRuntimeError> {
	use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
	use windows_sys::Win32::System::Diagnostics::ToolHelp::{
		CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
	};
	use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

	let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
	if snapshot == INVALID_HANDLE_VALUE {
		return Err(process_isolation_error(
			"cannot enumerate suspended Git process threads",
		));
	}
	let mut entry = THREADENTRY32 {
		dwSize: u32::try_from(std::mem::size_of::<THREADENTRY32>()).unwrap_or(u32::MAX),
		..THREADENTRY32::default()
	};
	let mut found = unsafe { Thread32First(snapshot, &raw mut entry) } != 0;
	let mut resumed = false;
	while found {
		if entry.th32OwnerProcessID == process_id {
			let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
			if thread.is_null() {
				unsafe { CloseHandle(snapshot) };
				return Err(process_isolation_error(
					"cannot open suspended Git process thread",
				));
			}
			let previous_count = unsafe { ResumeThread(thread) };
			unsafe { CloseHandle(thread) };
			if previous_count == u32::MAX {
				unsafe { CloseHandle(snapshot) };
				return Err(process_isolation_error(
					"cannot resume isolated Git process thread",
				));
			}
			resumed = true;
			break;
		}
		found = unsafe { Thread32Next(snapshot, &raw mut entry) } != 0;
	}
	unsafe { CloseHandle(snapshot) };
	if !resumed {
		return Err(process_isolation_error(
			"cannot find suspended Git process thread",
		));
	}
	Ok(())
}

#[cfg(windows)]
fn process_isolation_error(context: &str) -> GitRuntimeError {
	GitRuntimeError {
		category: "process_isolation_failed".to_string(),
		message: format!("{context}: {}", std::io::Error::last_os_error()),
	}
}

#[cfg(windows)]
impl Drop for WindowsProcessJob {
	fn drop(&mut self) {
		unsafe {
			windows_sys::Win32::Foundation::CloseHandle(self.handle);
		}
	}
}

fn read_bounded(mut reader: impl Read, limit: usize) -> Result<(Vec<u8>, bool), GitRuntimeError> {
	let mut output = Vec::with_capacity(limit.min(64 * 1024));
	let mut truncated = false;
	let mut chunk = [0_u8; 8192];
	loop {
		match reader.read(&mut chunk) {
			Ok(0) => break,
			Err(error) => {
				return Err(GitRuntimeError {
					category: "output_read_failed".to_string(),
					message: format!("cannot read Git command output: {error}"),
				});
			}
			Ok(count) => {
				let remaining = limit.saturating_sub(output.len());
				output.extend_from_slice(&chunk[..count.min(remaining)]);
				truncated = count > remaining;
				if truncated {
					break;
				}
			}
		}
	}
	Ok((output, truncated))
}

fn poll_reader(
	reader: &Receiver<Result<(Vec<u8>, bool), GitRuntimeError>>,
	result: &mut Option<(Vec<u8>, bool)>,
	stream: &str,
) -> Result<(), GitRuntimeError> {
	if result.is_some() {
		return Ok(());
	}
	match reader.try_recv() {
		Ok(Ok(output)) => {
			*result = Some(output);
			Ok(())
		}
		Ok(Err(error)) => Err(error),
		Err(TryRecvError::Empty) => Ok(()),
		Err(TryRecvError::Disconnected) => Err(GitRuntimeError {
			category: "output_read_failed".to_string(),
			message: format!("Git {stream} reader failed"),
		}),
	}
}

fn spawn_error(executable: &Path, error: std::io::Error) -> GitRuntimeError {
	let category = match error.kind() {
		std::io::ErrorKind::NotFound => "not_found",
		std::io::ErrorKind::PermissionDenied => "permission_denied",
		_ => "spawn_failed",
	};
	GitRuntimeError {
		category: category.to_string(),
		message: format!(
			"cannot launch Git executable {}: {error}",
			executable.display()
		),
	}
}

fn parse_git_version(text: &str) -> Option<(u32, u32, u32)> {
	let raw = text.strip_prefix("git version ")?;
	let mut parts = raw.split('.');
	let major = parts.next()?.parse().ok()?;
	let minor = parts.next()?.parse().ok()?;
	let patch_segment = parts.next()?;
	let digit_count = patch_segment
		.chars()
		.take_while(|character| character.is_ascii_digit())
		.count();
	if digit_count == 0 {
		return None;
	}
	let suffix = &patch_segment[digit_count..];
	if !suffix.is_empty() && !suffix.starts_with(char::is_whitespace) {
		return None;
	}
	let patch = patch_segment[..digit_count].parse().ok()?;
	Some((major, minor, patch))
}

fn failed_resolved_diagnostic(
	resolved: &ResolvedGit,
	state: GitDiagnosticState,
	category: &str,
	message: String,
	roots: &[PathBuf],
) -> GitDiagnostic {
	GitDiagnostic {
		state,
		resolution_source: Some(resolved.source),
		executable: Some(resolved.executable.clone()),
		version: None,
		compatible: None,
		failure: Some(GitFailure {
			category: category.to_string(),
			message: sanitize(&message),
		}),
		checked_at_unix_ms: None,
		duration_ms: None,
		roots: unavailable_roots(roots, category, &message),
	}
}

fn unavailable_diagnostic(
	error: GitRuntimeError,
	roots: &[PathBuf],
	source: GitResolutionSource,
) -> GitDiagnostic {
	let category = error.category.clone();
	GitDiagnostic {
		state: if category == "timed_out" {
			GitDiagnosticState::TimedOut
		} else {
			GitDiagnosticState::Unavailable
		},
		resolution_source: Some(source),
		executable: None,
		version: None,
		compatible: None,
		failure: Some(GitFailure {
			category: error.category,
			message: sanitize(&error.message),
		}),
		checked_at_unix_ms: None,
		duration_ms: None,
		roots: unavailable_roots(roots, &category, &error.message),
	}
}

fn unavailable_roots(roots: &[PathBuf], category: &str, message: &str) -> Vec<GitRootDiagnostic> {
	roots
		.iter()
		.map(|root| GitRootDiagnostic {
			root: root.clone(),
			state: GitRootState::Unavailable,
			repository_root: None,
			failure: Some(GitFailure {
				category: category.to_string(),
				message: sanitize(message),
			}),
			message: sanitize(message),
		})
		.collect()
}

fn sanitize(message: &str) -> String {
	let mut output = message.to_string();
	while let Some(scheme) = output.find("://") {
		let credentials_start = output[..scheme]
			.rfind(|character: char| character.is_ascii_whitespace())
			.map_or(0, |index| index + 1);
		let rest = &output[scheme + 3..];
		let Some(at) = rest.find('@') else {
			break;
		};
		let credentials_end = scheme + 3 + at;
		output.replace_range(credentials_start..credentials_end, "<redacted>");
	}
	output
}

fn output_text(output: GitOutput) -> Result<String, GitRuntimeError> {
	String::from_utf8(output.stdout).map_err(|error| GitRuntimeError {
		category: "malformed_output".to_string(),
		message: format!("Git returned non-UTF-8 stdout: {error}"),
	})
}

fn unix_timestamp_ms() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(duration_ms)
		.unwrap_or(0)
}

fn duration_ms(duration: Duration) -> u64 {
	u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_supported_git_version_variants() {
		assert_eq!(parse_git_version("git version 2.22.0"), Some((2, 22, 0)));
		assert_eq!(
			parse_git_version("git version 2.47.1.windows.1"),
			Some((2, 47, 1))
		);
		assert_eq!(
			parse_git_version("git version 2.50.1 (Apple Git-155)"),
			Some((2, 50, 1))
		);
		assert_eq!(parse_git_version("git version 2.22.0evil"), None);
		assert_eq!(parse_git_version("not git"), None);
	}

	#[test]
	fn output_read_errors_are_not_treated_as_eof() {
		struct FailingReader;

		impl Read for FailingReader {
			fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
				Err(std::io::Error::other("broken pipe reader"))
			}
		}

		let error = read_bounded(FailingReader, 1024).expect_err("read failure must propagate");
		assert_eq!(error.category, "output_read_failed");
	}

	#[test]
	fn explicit_configuration_must_be_absolute_and_does_not_fall_back_to_path() {
		let runtime = GitRuntime::new(
			GitRuntimeConfig {
				explicit_binary: Some(PathBuf::from("git")),
				..GitRuntimeConfig::default()
			},
			&[],
		);
		let diagnostic = runtime.probe(&[]);
		assert_eq!(diagnostic.state, GitDiagnosticState::Unavailable);
		assert_eq!(
			diagnostic.failure.unwrap().category,
			"invalid_configuration"
		);
	}

	#[test]
	fn executable_resolution_is_part_of_the_probe_deadline() {
		use std::sync::atomic::{AtomicUsize, Ordering};

		let config = GitRuntimeConfig {
			probe_timeout: Duration::from_millis(50),
			..GitRuntimeConfig::default()
		};
		let resolver = Arc::new(GitResolver::default());
		let invocations = Arc::new(AtomicUsize::new(0));
		let started = Instant::now();
		let first_invocations = Arc::clone(&invocations);
		let error = resolve_with_deadline_using(
			Arc::clone(&resolver),
			config.clone(),
			started + Duration::from_millis(50),
			move |_| {
				first_invocations.fetch_add(1, Ordering::SeqCst);
				std::thread::sleep(Duration::from_millis(250));
				Err(GitRuntimeError {
					category: "not_found".to_string(),
					message: "delayed missing Git".to_string(),
				})
			},
		)
		.expect_err("slow executable resolution must time out");
		assert_eq!(error.category, "timed_out");
		assert!(started.elapsed() < Duration::from_secs(1));
		let second_invocations = Arc::clone(&invocations);
		let second = resolve_with_deadline_using(
			resolver,
			config,
			Instant::now() + Duration::from_millis(50),
			move |_| {
				second_invocations.fetch_add(1, Ordering::SeqCst);
				unreachable!("a repeated request must join the in-flight resolver")
			},
		)
		.expect_err("the shared slow resolution must retain its deadline");
		assert_eq!(second.category, "timed_out");
		assert_eq!(invocations.load(Ordering::SeqCst), 1);
	}

	#[test]
	fn inherited_path_ignores_empty_segments() {
		let missing = std::env::temp_dir().join("code-moniker-no-git-here");
		let path = std::env::join_paths([missing.as_os_str(), OsStr::new("")]).expect("PATH");
		let error =
			resolve_from_path(&path).expect_err("empty PATH entries must not select cwd/git");
		assert_eq!(error.category, "not_found");
	}

	#[cfg(unix)]
	#[test]
	fn inherited_path_skips_a_non_executable_git_candidate() {
		use std::os::unix::fs::PermissionsExt;

		let temp = tempfile::tempdir().expect("tempdir");
		let denied = temp.path().join("denied");
		let available = temp.path().join("available");
		std::fs::create_dir(&denied).expect("denied directory");
		std::fs::create_dir(&available).expect("available directory");
		std::fs::write(denied.join("git"), "not executable").expect("denied git");
		let valid_git = available.join("git");
		std::fs::write(&valid_git, "#!/bin/sh\nexit 0\n").expect("valid git");
		let mut permissions = std::fs::metadata(&valid_git)
			.expect("metadata")
			.permissions();
		permissions.set_mode(0o755);
		std::fs::set_permissions(&valid_git, permissions).expect("permissions");
		let path = std::env::join_paths([denied, available]).expect("PATH");
		let resolved = resolve_from_path(&path).expect("later executable Git candidate");
		assert_eq!(
			resolved.executable,
			valid_git.canonicalize().expect("canonical Git")
		);
	}

	#[cfg(unix)]
	#[test]
	fn cached_path_candidate_is_invalidated_after_execute_permission_is_removed() {
		use std::os::unix::fs::PermissionsExt;

		let temp = tempfile::tempdir().expect("tempdir");
		let first = temp.path().join("first");
		let second = temp.path().join("second");
		std::fs::create_dir(&first).expect("first directory");
		std::fs::create_dir(&second).expect("second directory");
		for path in [first.join("git"), second.join("git")] {
			std::fs::write(&path, "#!/bin/sh\nexit 0\n").expect("fake git");
			let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
			permissions.set_mode(0o755);
			std::fs::set_permissions(path, permissions).expect("permissions");
		}
		let first_git = first.join("git").canonicalize().expect("first Git");
		let runtime = GitRuntime::new(GitRuntimeConfig::default(), &[]);
		{
			let mut state = runtime
				.resolver
				.state
				.lock()
				.unwrap_or_else(|poisoned| poisoned.into_inner());
			state.result = Some(Ok(ResolvedGit {
				executable: first_git.clone(),
				source: GitResolutionSource::InheritedPath,
			}));
		}
		let mut permissions = std::fs::metadata(&first_git)
			.expect("metadata")
			.permissions();
		permissions.set_mode(0o644);
		std::fs::set_permissions(&first_git, permissions).expect("permissions");
		record_execution_failure(
			&runtime,
			&ResolvedGit {
				executable: first_git,
				source: GitResolutionSource::InheritedPath,
			},
			temp.path(),
			&GitRuntimeError {
				category: "permission_denied".to_string(),
				message: "cached Git is no longer executable".to_string(),
			},
		);
		assert!(
			runtime
				.resolver
				.state
				.lock()
				.unwrap_or_else(|poisoned| poisoned.into_inner())
				.result
				.is_none()
		);
		let path = std::env::join_paths([first, second.clone()]).expect("PATH");
		let resolved = resolve_from_path(&path).expect("later executable Git candidate");
		assert_eq!(
			resolved.executable,
			second.join("git").canonicalize().unwrap()
		);
	}

	#[cfg(unix)]
	#[test]
	fn hanging_git_is_terminated_and_reaped() {
		use std::os::unix::fs::PermissionsExt;

		let temp = tempfile::tempdir().expect("tempdir");
		let git = temp.path().join("fake git");
		std::fs::write(&git, "#!/bin/sh\nsleep 10\n").expect("fake git");
		let mut permissions = std::fs::metadata(&git).expect("metadata").permissions();
		permissions.set_mode(0o755);
		std::fs::set_permissions(&git, permissions).expect("permissions");
		let runtime = GitRuntime::new(
			GitRuntimeConfig {
				explicit_binary: Some(git),
				probe_timeout: Duration::from_millis(100),
				command_timeout: Duration::from_millis(100),
				output_limit: 1024,
			},
			&[],
		);
		let started = Instant::now();
		let diagnostic = runtime.probe(&[]);
		assert_eq!(diagnostic.state, GitDiagnosticState::TimedOut);
		assert!(started.elapsed() < Duration::from_secs(2));
	}

	#[cfg(unix)]
	#[test]
	fn descendant_holding_pipes_cannot_outlive_the_command_deadline() {
		use std::os::unix::fs::PermissionsExt;

		let temp = tempfile::tempdir().expect("tempdir");
		let git = temp.path().join("fake git");
		std::fs::write(&git, "#!/bin/sh\n(sleep 10) &\nexit 0\n").expect("fake git");
		let mut permissions = std::fs::metadata(&git).expect("metadata").permissions();
		permissions.set_mode(0o755);
		std::fs::set_permissions(&git, permissions).expect("permissions");
		let started = Instant::now();
		let error = run_bounded(
			&git,
			temp.path(),
			&["status"],
			Duration::from_millis(100),
			1024,
		)
		.expect_err("inherited pipes must remain subject to the deadline");
		assert_eq!(error.category, "timed_out");
		assert!(started.elapsed() < Duration::from_secs(2));
	}

	#[cfg(unix)]
	#[test]
	fn one_command_timeout_does_not_poison_process_or_root_diagnostics() {
		use std::os::unix::fs::PermissionsExt;

		let temp = tempfile::tempdir().expect("tempdir");
		let git = temp.path().join("fake git");
		std::fs::write(
			&git,
			"#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'git version 2.50.1'; elif [ \"$1\" = status ]; then sleep 10; elif [ \"$2\" = --show-toplevel ]; then pwd; else printf 'true\\nfalse\\n'; fi\n",
		)
		.expect("fake git");
		let mut permissions = std::fs::metadata(&git).expect("metadata").permissions();
		permissions.set_mode(0o755);
		std::fs::set_permissions(&git, permissions).expect("permissions");
		let roots = vec![temp.path().to_path_buf()];
		let runtime = GitRuntime::new(
			GitRuntimeConfig {
				explicit_binary: Some(git),
				probe_timeout: Duration::from_secs(2),
				command_timeout: Duration::from_millis(100),
				output_limit: 1024,
			},
			&roots,
		);
		assert_eq!(runtime.probe(&roots).state, GitDiagnosticState::Available);
		let error = runtime
			.run(temp.path(), &["status"])
			.expect_err("status must time out");
		assert_eq!(error.category, "timed_out");
		assert_eq!(runtime.diagnostic().state, GitDiagnosticState::Available);
		let root_diagnostic = runtime.diagnostic_for(&roots);
		assert_eq!(root_diagnostic.state, GitDiagnosticState::Available);
		assert_eq!(root_diagnostic.roots[0].state, GitRootState::Unavailable);
		assert_eq!(
			root_diagnostic.roots[0]
				.failure
				.as_ref()
				.map(|failure| failure.category.as_str()),
			Some("timed_out")
		);
	}

	#[cfg(unix)]
	#[test]
	fn rejected_git_argument_does_not_poison_root_availability() {
		use std::os::unix::fs::PermissionsExt;

		let temp = tempfile::tempdir().expect("tempdir");
		let git = temp.path().join("fake git");
		std::fs::write(
			&git,
			"#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'git version 2.50.1'; elif [ \"$2\" = --show-toplevel ]; then pwd; elif [ \"$3\" = bad-ref ]; then echo 'fatal: bad revision' >&2; exit 128; else printf 'true\\nfalse\\n'; fi\n",
		)
		.expect("fake git");
		let mut permissions = std::fs::metadata(&git).expect("metadata").permissions();
		permissions.set_mode(0o755);
		std::fs::set_permissions(&git, permissions).expect("permissions");
		let roots = vec![temp.path().to_path_buf()];
		let runtime = GitRuntime::new(
			GitRuntimeConfig {
				explicit_binary: Some(git),
				probe_timeout: Duration::from_secs(5),
				command_timeout: Duration::from_secs(1),
				output_limit: 1024,
			},
			&roots,
		);
		assert_eq!(runtime.probe(&roots).roots[0].state, GitRootState::Worktree);

		let error = runtime
			.text(temp.path(), &["rev-parse", "--verify", "bad-ref"])
			.expect_err("the invalid revision must still fail");
		assert_eq!(error.category, "command_failed");
		assert_eq!(
			runtime.diagnostic_for(&roots).roots[0].state,
			GitRootState::Worktree
		);
	}

	#[cfg(unix)]
	#[test]
	fn repository_loss_invalidates_the_cached_root_diagnostic() {
		use std::os::unix::fs::PermissionsExt;

		let temp = tempfile::tempdir().expect("tempdir");
		let root = temp.path().join("workspace");
		std::fs::create_dir(&root).expect("workspace");
		std::fs::create_dir(root.join(".git")).expect("git directory");
		let git = temp.path().join("fake git");
		std::fs::write(
			&git,
			"#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'git version 2.50.1'; elif [ ! -d \"$PWD/.git\" ]; then echo 'fatal: not a git repository' >&2; exit 128; elif [ \"$2\" = --show-toplevel ]; then pwd; else printf 'true\\nfalse\\n'; fi\n",
		)
		.expect("fake git");
		let mut permissions = std::fs::metadata(&git).expect("metadata").permissions();
		permissions.set_mode(0o755);
		std::fs::set_permissions(&git, permissions).expect("permissions");
		let roots = vec![root.clone()];
		let runtime = GitRuntime::new(
			GitRuntimeConfig {
				explicit_binary: Some(git),
				probe_timeout: Duration::from_secs(5),
				command_timeout: Duration::from_secs(1),
				output_limit: 1024,
			},
			&roots,
		);
		assert_eq!(runtime.probe(&roots).roots[0].state, GitRootState::Worktree);

		std::fs::remove_dir(root.join(".git")).expect("remove git directory");
		let error = runtime
			.text(&root, &["status"])
			.expect_err("the command must observe repository loss");
		assert_eq!(error.category, "command_failed");
		let diagnostic = runtime.diagnostic_for(&roots);
		assert_eq!(diagnostic.roots[0].state, GitRootState::NotRepository);
		assert_eq!(
			diagnostic.roots[0]
				.failure
				.as_ref()
				.map(|failure| failure.category.as_str()),
			Some("not_repository")
		);
	}

	#[cfg(unix)]
	#[test]
	fn permission_failure_is_scoped_to_the_affected_root() {
		use std::os::unix::fs::PermissionsExt;

		let temp = tempfile::tempdir().expect("tempdir");
		let git = temp.path().join("fake git");
		std::fs::write(
			&git,
			"#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'git version 2.50.1'; elif [ \"$2\" = --show-toplevel ]; then pwd; else printf 'true\\nfalse\\n'; fi\n",
		)
		.expect("fake git");
		let mut permissions = std::fs::metadata(&git).expect("metadata").permissions();
		permissions.set_mode(0o755);
		std::fs::set_permissions(&git, permissions).expect("permissions");
		let roots = vec![temp.path().join("one"), temp.path().join("two")];
		for root in &roots {
			std::fs::create_dir(root).expect("root");
		}
		let runtime = GitRuntime::new(
			GitRuntimeConfig {
				explicit_binary: Some(git.clone()),
				probe_timeout: Duration::from_secs(2),
				command_timeout: Duration::from_millis(100),
				output_limit: 1024,
			},
			&roots,
		);
		assert_eq!(runtime.probe(&roots).state, GitDiagnosticState::Available);
		record_execution_failure(
			&runtime,
			&ResolvedGit {
				executable: git,
				source: GitResolutionSource::ExplicitConfiguration,
			},
			&roots[0],
			&GitRuntimeError {
				category: "permission_denied".to_string(),
				message: "cannot enter selected root".to_string(),
			},
		);
		assert_eq!(runtime.diagnostic().state, GitDiagnosticState::Available);
		let diagnostic = runtime.diagnostic_for(&roots);
		assert_eq!(diagnostic.roots[0].state, GitRootState::Unavailable);
		assert_eq!(diagnostic.roots[1].state, GitRootState::Worktree);
	}

	#[cfg(unix)]
	#[test]
	fn concurrent_root_selection_stays_within_one_probe_budget() {
		use std::os::unix::fs::PermissionsExt;

		let temp = tempfile::tempdir().expect("tempdir");
		let git = temp.path().join("slow git");
		std::fs::write(&git, "#!/bin/sh\nsleep 10\n").expect("fake git");
		let mut permissions = std::fs::metadata(&git).expect("metadata").permissions();
		permissions.set_mode(0o755);
		std::fs::set_permissions(&git, permissions).expect("permissions");
		let roots = vec![temp.path().join("one"), temp.path().join("two")];
		for root in &roots {
			std::fs::create_dir(root).expect("root");
		}
		let runtime = Arc::new(GitRuntime::new(
			GitRuntimeConfig {
				explicit_binary: Some(git),
				probe_timeout: Duration::from_millis(100),
				command_timeout: Duration::from_millis(100),
				output_limit: 1024,
			},
			&roots,
		));
		let background = {
			let runtime = Arc::clone(&runtime);
			let roots = roots.clone();
			std::thread::spawn(move || runtime.probe(&roots))
		};
		std::thread::sleep(Duration::from_millis(20));
		let started = Instant::now();
		let selected = runtime.probe(&roots[..1]);
		assert_eq!(selected.state, GitDiagnosticState::TimedOut);
		assert!(started.elapsed() < Duration::from_millis(350));
		background.join().expect("background probe");
	}

	#[cfg(unix)]
	#[test]
	fn all_roots_diagnostic_is_projected_for_a_selected_root() {
		use std::os::unix::fs::PermissionsExt;

		let temp = tempfile::tempdir().expect("tempdir");
		let git = temp.path().join("fake git");
		std::fs::write(
			&git,
			"#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'git version 2.50.1'; else printf 'false\\nfalse\\n'; fi\n",
		)
		.expect("fake git");
		let mut permissions = std::fs::metadata(&git).expect("metadata").permissions();
		permissions.set_mode(0o755);
		std::fs::set_permissions(&git, permissions).expect("permissions");
		let roots = vec![temp.path().join("one"), temp.path().join("two")];
		for root in &roots {
			std::fs::create_dir(root).expect("root");
		}
		let runtime = GitRuntime::new(
			GitRuntimeConfig {
				explicit_binary: Some(git),
				probe_timeout: Duration::from_secs(5),
				command_timeout: Duration::from_millis(100),
				output_limit: 1024,
			},
			&roots,
		);
		runtime.probe(&roots);
		let selected = runtime.diagnostic_for(&roots[..1]);
		assert_eq!(selected.roots.len(), 1);
		assert_eq!(selected.roots[0].root, roots[0]);
	}

	#[cfg(unix)]
	#[test]
	fn repository_probe_preserves_non_repository_command_failures() {
		use std::os::unix::fs::PermissionsExt;

		let temp = tempfile::tempdir().expect("tempdir");
		let git = temp.path().join("fake git");
		std::fs::write(
			&git,
			"#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'git version 2.50.1'; else echo 'fatal: detected dubious ownership' >&2; exit 128; fi\n",
		)
		.expect("fake git");
		let mut permissions = std::fs::metadata(&git).expect("metadata").permissions();
		permissions.set_mode(0o755);
		std::fs::set_permissions(&git, permissions).expect("permissions");
		let roots = vec![temp.path().to_path_buf()];
		let runtime = GitRuntime::new(
			GitRuntimeConfig {
				explicit_binary: Some(git),
				probe_timeout: Duration::from_secs(5),
				command_timeout: Duration::from_millis(100),
				output_limit: 1024,
			},
			&roots,
		);
		let diagnostic = runtime.probe(&roots);
		assert_eq!(diagnostic.roots[0].state, GitRootState::Unavailable);
		assert_eq!(
			diagnostic.roots[0]
				.failure
				.as_ref()
				.map(|failure| failure.category.as_str()),
			Some("command_failed")
		);
	}

	#[cfg(unix)]
	#[test]
	fn root_probes_share_one_global_diagnostic_budget() {
		use std::os::unix::fs::PermissionsExt;

		let temp = tempfile::tempdir().expect("tempdir");
		let git = temp.path().join("fake git");
		std::fs::write(
			&git,
			"#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'git version 2.50.1'; else sleep 10; fi\n",
		)
		.expect("fake git");
		let mut permissions = std::fs::metadata(&git).expect("metadata").permissions();
		permissions.set_mode(0o755);
		std::fs::set_permissions(&git, permissions).expect("permissions");
		let roots = vec![
			temp.path().join("one"),
			temp.path().join("two"),
			temp.path().join("three"),
		];
		for root in &roots {
			std::fs::create_dir(root).expect("root");
		}
		let runtime = GitRuntime::new(
			GitRuntimeConfig {
				explicit_binary: Some(git),
				probe_timeout: Duration::from_secs(2),
				command_timeout: Duration::from_millis(100),
				output_limit: 1024,
			},
			&roots,
		);
		let started = Instant::now();
		let diagnostic = runtime.probe(&roots);
		assert_eq!(diagnostic.state, GitDiagnosticState::Available);
		assert!(
			diagnostic
				.roots
				.iter()
				.all(|root| root.state == GitRootState::Unavailable)
		);
		assert!(diagnostic.roots.iter().all(|root| {
			root.failure
				.as_ref()
				.map(|failure| failure.category.as_str())
				== Some("timed_out")
		}));
		assert!(started.elapsed() < Duration::from_secs(5));
	}

	#[cfg(unix)]
	#[test]
	fn root_diagnostics_are_cached_per_root() {
		use std::os::unix::fs::PermissionsExt;

		let temp = tempfile::tempdir().expect("tempdir");
		let git = temp.path().join("fake git");
		std::fs::write(
			&git,
			"#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'git version 2.50.1'; else printf 'false\\nfalse\\n'; fi\n",
		)
		.expect("fake git");
		let mut permissions = std::fs::metadata(&git).expect("metadata").permissions();
		permissions.set_mode(0o755);
		std::fs::set_permissions(&git, permissions).expect("permissions");
		let first = vec![temp.path().join("first")];
		let second = vec![temp.path().join("second")];
		std::fs::create_dir(&first[0]).expect("first root");
		std::fs::create_dir(&second[0]).expect("second root");
		let runtime = GitRuntime::new(
			GitRuntimeConfig {
				explicit_binary: Some(git),
				probe_timeout: Duration::from_secs(5),
				command_timeout: Duration::from_millis(100),
				output_limit: 1024,
			},
			&first,
		);
		runtime.probe(&first);
		assert_eq!(runtime.diagnostic_for(&first).roots[0].root, first[0]);
		assert_eq!(
			runtime.diagnostic_for(&second).state,
			GitDiagnosticState::Checking
		);
		runtime.probe(&second);
		assert_eq!(runtime.diagnostic_for(&first).roots[0].root, first[0]);
		assert_eq!(runtime.diagnostic_for(&second).roots[0].root, second[0]);
	}

	#[cfg(unix)]
	#[test]
	fn targeted_reprobe_refreshes_the_aggregate_diagnostic() {
		use std::os::unix::fs::PermissionsExt;

		let temp = tempfile::tempdir().expect("tempdir");
		let git = temp.path().join("fake git");
		std::fs::write(
			&git,
			"#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'git version 2.50.1'; elif [ \"$2\" = --show-toplevel ]; then pwd; else printf 'true\\nfalse\\n'; fi\n",
		)
		.expect("fake git");
		let mut permissions = std::fs::metadata(&git).expect("metadata").permissions();
		permissions.set_mode(0o755);
		std::fs::set_permissions(&git, permissions).expect("permissions");
		let roots = vec![temp.path().join("one"), temp.path().join("two")];
		for root in &roots {
			std::fs::create_dir(root).expect("root");
		}
		let runtime = GitRuntime::new(
			GitRuntimeConfig {
				explicit_binary: Some(git),
				probe_timeout: Duration::from_secs(5),
				command_timeout: Duration::from_secs(1),
				output_limit: 1024,
			},
			&roots,
		);
		runtime.probe(&roots);
		record_root_execution_failure(
			&runtime,
			&roots[0],
			&GitRuntimeError {
				category: "timed_out".to_string(),
				message: "temporary root failure".to_string(),
			},
		);
		assert_eq!(
			runtime.diagnostic_for(&roots).roots[0].state,
			GitRootState::Unavailable
		);

		runtime.probe(&roots[..1]);
		let aggregate = runtime.diagnostic_for(&roots);
		assert_eq!(aggregate.roots[0].state, GitRootState::Worktree);
		assert_eq!(aggregate.roots[1].state, GitRootState::Worktree);
	}

	#[cfg(windows)]
	#[test]
	fn hanging_windows_process_is_terminated_and_reaped() {
		let system_root = std::env::var_os("SystemRoot").expect("SystemRoot");
		let ping = PathBuf::from(system_root).join("System32").join("ping.exe");
		let started = Instant::now();
		let error = run_bounded(
			&ping,
			Path::new("."),
			&["127.0.0.1", "-n", "30"],
			Duration::from_millis(100),
			1024,
		)
		.expect_err("the Windows process must time out");
		assert_eq!(error.category, "timed_out");
		assert!(started.elapsed() < Duration::from_secs(2));
	}
}
