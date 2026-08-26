use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use code_moniker_query::{
	EffectiveCapabilityDto, EffectiveCapabilityState, Query, QueryError, RuntimeDependencyDto,
	RuntimeDependencyFailureDto, RuntimeDependencyResolutionSource, RuntimeDependencyRootDto,
	RuntimeDependencyRootState, RuntimeDependencyState, WorkspaceStatus,
};
use code_moniker_workspace::git_runtime::{
	GitDiagnostic, GitDiagnosticState, GitResolutionSource, GitRootState,
	SUPPORTED_GIT_VERSION_RANGE, process_git_runtime,
};

use crate::helpers::selected_roots;

const GIT_ROOT_REPROBE_BACKOFF_MS: u64 = 1_000;

pub fn probe_runtime_dependencies(roots: &[PathBuf]) {
	process_git_runtime().probe(roots);
}

pub fn augment_workspace_status(
	status: &mut WorkspaceStatus,
	roots: &[PathBuf],
	process_scope: &str,
) {
	let diagnostic = process_git_runtime().diagnostic_for(roots);
	status.runtime_dependencies = vec![git_dependency_dto(&diagnostic, roots, process_scope)];
	status.effective_capabilities = git_effective_capabilities(&diagnostic, roots);
}

pub fn gate_git_query(query: &Query, roots: &[PathBuf]) -> Result<(), QueryError> {
	if !query_has_git_change_portion(query) {
		return Ok(());
	}
	let result = gate_git_roots(&query_roots(query, roots)?);
	if query_requires_git(query) {
		result
	} else {
		Ok(())
	}
}

pub(crate) fn optional_git_change_failure(
	query: &Query,
	roots: &[PathBuf],
) -> Result<Option<QueryError>, QueryError> {
	if !matches!(query, Query::ChangeContext(_)) {
		return Ok(None);
	}
	let selected = query_roots(query, roots)?;
	let diagnostic = process_git_runtime().diagnostic_for(&selected);
	if diagnostic.state == GitDiagnosticState::Available
		&& diagnostic
			.roots
			.iter()
			.any(|root| root.state == GitRootState::Worktree)
	{
		Ok(None)
	} else {
		Ok(Some(git_unavailable_error(&diagnostic)))
	}
}

pub(crate) fn query_requires_git(query: &Query) -> bool {
	matches!(query, Query::ChangeReview(_))
}

pub(crate) fn query_has_git_change_portion(query: &Query) -> bool {
	matches!(query, Query::ChangeReview(_) | Query::ChangeContext(_))
}

fn query_roots(query: &Query, roots: &[PathBuf]) -> Result<Vec<PathBuf>, QueryError> {
	let selector = match query {
		Query::ChangeReview(query) => query.workspace.as_deref(),
		Query::ChangeContext(query) => query.workspace.as_deref(),
		_ => return Ok(Vec::new()),
	};
	selected_roots(roots, selector).map(|selected| selected.into_iter().cloned().collect())
}

pub(crate) fn git_change_dependency(
	selector: Option<&str>,
	roots: &[PathBuf],
	process_scope: &str,
) -> Result<RuntimeDependencyDto, QueryError> {
	let selected = selected_roots(roots, selector)?
		.into_iter()
		.cloned()
		.collect::<Vec<_>>();
	let runtime = process_git_runtime();
	let diagnostic = if runtime.diagnostic_for(&selected).state == GitDiagnosticState::Checking {
		runtime.probe_if_needed(&selected)
	} else {
		runtime.diagnostic_for(&selected)
	};
	let mut dependency = git_dependency_dto(&diagnostic, &selected, process_scope);
	if diagnostic.state == GitDiagnosticState::Available
		&& !diagnostic
			.roots
			.iter()
			.any(|root| root.state == GitRootState::Worktree)
	{
		let failure = diagnostic
			.roots
			.iter()
			.find_map(|root| root.detail.failure.as_ref())
			.map(|failure| RuntimeDependencyFailureDto {
				category: failure.category.clone(),
				message: failure.message.clone(),
			})
			.unwrap_or(RuntimeDependencyFailureDto {
				category: "not_repository".to_string(),
				message: "no selected workspace root is a Git worktree".to_string(),
			});
		dependency.state = if failure.category == "timed_out" {
			RuntimeDependencyState::TimedOut
		} else {
			RuntimeDependencyState::Unavailable
		};
		dependency.failure = Some(failure);
	}
	Ok(dependency)
}

pub(crate) fn apply_change_failure(dependency: &mut RuntimeDependencyDto, failure: &QueryError) {
	let category = failure
		.category
		.as_deref()
		.unwrap_or(failure.code.as_str())
		.to_string();
	dependency.state = if category == "timed_out" {
		RuntimeDependencyState::TimedOut
	} else {
		RuntimeDependencyState::Unavailable
	};
	dependency.failure = Some(RuntimeDependencyFailureDto {
		category,
		message: failure.message.clone(),
	});
}

pub(crate) fn git_acquisition_error(
	error: &code_moniker_workspace::git_runtime::GitRuntimeError,
) -> QueryError {
	let code = if error.category == "timed_out" {
		"runtime_dependency_timed_out"
	} else {
		"runtime_dependency_unavailable"
	};
	QueryError::new(code, error.message.clone()).with_category(error.category.clone())
}

pub(crate) fn gate_git_roots(roots: &[PathBuf]) -> Result<(), QueryError> {
	let runtime = process_git_runtime();
	let current = runtime.diagnostic_for(roots);
	let retryable_root = current.roots.iter().any(|root| {
		root.state == GitRootState::RepositoryOnly
			|| root
				.detail
				.failure
				.as_ref()
				.is_some_and(|failure| git_failure_is_retryable(failure.category.as_str()))
	});
	let retry_due = git_reprobe_due(&current, unix_timestamp_ms());
	let diagnostic = if current.state == GitDiagnosticState::Checking {
		runtime.probe_if_needed(roots)
	} else if retry_due && (current.state == GitDiagnosticState::TimedOut || retryable_root) {
		runtime.probe(roots)
	} else {
		current
	};
	if diagnostic.state == GitDiagnosticState::Available
		&& diagnostic
			.roots
			.iter()
			.any(|root| root.state == GitRootState::Worktree)
	{
		return Ok(());
	}
	Err(git_unavailable_error(&diagnostic))
}

fn git_failure_is_retryable(category: &str) -> bool {
	!matches!(
		category,
		"invalid_configuration" | "incompatible_version" | "malformed_version" | "malformed_output"
	)
}

fn git_reprobe_due(diagnostic: &GitDiagnostic, now_unix_ms: u64) -> bool {
	let Some(checked_at) = diagnostic.checked_at_unix_ms else {
		return true;
	};
	let completed_at = checked_at.saturating_add(diagnostic.duration_ms.unwrap_or(0));
	now_unix_ms.saturating_sub(completed_at) >= GIT_ROOT_REPROBE_BACKOFF_MS
}

fn unix_timestamp_ms() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
		.unwrap_or(0)
}

fn git_dependency_dto(
	diagnostic: &GitDiagnostic,
	roots: &[PathBuf],
	process_scope: &str,
) -> RuntimeDependencyDto {
	RuntimeDependencyDto {
		name: "git".to_string(),
		process_scope: process_scope.to_string(),
		state: dependency_state(diagnostic.state),
		resolution_source: diagnostic.resolution_source.map(resolution_source),
		executable: diagnostic
			.executable
			.as_ref()
			.map(|path| path.display().to_string()),
		version: diagnostic.version.clone(),
		supported_version_range: SUPPORTED_GIT_VERSION_RANGE.to_string(),
		compatible: diagnostic.compatible,
		failure: diagnostic
			.failure
			.as_ref()
			.map(|failure| RuntimeDependencyFailureDto {
				category: failure.category.clone(),
				message: failure.message.clone(),
			}),
		checked_at_unix_ms: diagnostic.checked_at_unix_ms,
		duration_ms: diagnostic.duration_ms,
		roots: dependency_roots(diagnostic, roots),
	}
}

fn dependency_roots(
	diagnostic: &GitDiagnostic,
	roots: &[PathBuf],
) -> Vec<RuntimeDependencyRootDto> {
	if diagnostic.state == GitDiagnosticState::Checking && diagnostic.roots.is_empty() {
		return roots
			.iter()
			.map(|root| RuntimeDependencyRootDto {
				root: root.display().to_string(),
				state: RuntimeDependencyRootState::Unavailable,
				repository_root: None,
				failure: None,
				message: "Git diagnostic is still checking".to_string(),
			})
			.collect();
	}
	diagnostic
		.roots
		.iter()
		.map(|root| RuntimeDependencyRootDto {
			root: root.root.display().to_string(),
			state: root_state(root.state),
			repository_root: root
				.repository_root
				.as_ref()
				.map(|path| path.display().to_string()),
			failure: root
				.detail
				.failure
				.as_ref()
				.map(|failure| RuntimeDependencyFailureDto {
					category: failure.category.clone(),
					message: failure.message.clone(),
				}),
			message: root.detail.message.clone(),
		})
		.collect()
}

fn git_effective_capabilities(
	diagnostic: &GitDiagnostic,
	roots: &[PathBuf],
) -> Vec<EffectiveCapabilityDto> {
	let worktrees = diagnostic
		.roots
		.iter()
		.filter(|root| root.state == GitRootState::Worktree)
		.count();
	let change_state = match diagnostic.state {
		GitDiagnosticState::Checking => EffectiveCapabilityState::Checking,
		GitDiagnosticState::Available if worktrees == roots.len() && worktrees > 0 => {
			EffectiveCapabilityState::Available
		}
		GitDiagnosticState::Available if worktrees > 0 => EffectiveCapabilityState::Degraded,
		_ => EffectiveCapabilityState::Unavailable,
	};
	let change_reason = change_capability_reason(diagnostic, worktrees, roots.len());
	let context_state = if change_state == EffectiveCapabilityState::Available {
		EffectiveCapabilityState::Available
	} else {
		EffectiveCapabilityState::Degraded
	};
	let coupling_state = match diagnostic.state {
		GitDiagnosticState::Available if worktrees == roots.len() && worktrees > 0 => {
			EffectiveCapabilityState::Available
		}
		_ => EffectiveCapabilityState::Degraded,
	};
	vec![
		effective_capability(
			"change.review",
			change_state,
			"workspace_roots",
			change_reason.clone(),
		),
		effective_capability(
			"change.context",
			context_state,
			"workspace_roots",
			change_reason.map(|reason| format!("change evidence is unavailable: {reason}")),
		),
		effective_capability(
			"metrics.coupling",
			coupling_state,
			"workspace_roots",
			(coupling_state == EffectiveCapabilityState::Degraded).then(|| {
				"Git revision metadata is unavailable; coupling facts remain available".to_string()
			}),
		),
		EffectiveCapabilityDto {
			name: "diff-impact.compare".to_string(),
			state: EffectiveCapabilityState::Available,
			dependency: None,
			scope: "virtual_source_sets".to_string(),
			reason: Some("daemon-side comparison does not launch Git".to_string()),
		},
	]
}

fn effective_capability(
	name: &str,
	state: EffectiveCapabilityState,
	scope: &str,
	reason: Option<String>,
) -> EffectiveCapabilityDto {
	EffectiveCapabilityDto {
		name: name.to_string(),
		state,
		dependency: Some("git".to_string()),
		scope: scope.to_string(),
		reason,
	}
}

fn change_capability_reason(
	diagnostic: &GitDiagnostic,
	worktrees: usize,
	root_count: usize,
) -> Option<String> {
	match diagnostic.state {
		GitDiagnosticState::Checking => Some("Git diagnostic is still checking".to_string()),
		GitDiagnosticState::Available if worktrees == root_count && worktrees > 0 => None,
		GitDiagnosticState::Available if worktrees > 0 => Some(format!(
			"Git is usable for {worktrees} of {root_count} workspace roots"
		)),
		GitDiagnosticState::Available => Some("no workspace root is a Git worktree".to_string()),
		_ => diagnostic
			.failure
			.as_ref()
			.map(|failure| failure.message.clone()),
	}
}

fn git_unavailable_error(diagnostic: &GitDiagnostic) -> QueryError {
	if diagnostic.state == GitDiagnosticState::Incompatible {
		if let Some(failure) = diagnostic.failure.as_ref().or_else(|| {
			diagnostic
				.roots
				.iter()
				.filter_map(|root| root.detail.failure.as_ref())
				.find(|failure| failure.category == "incompatible_version")
		}) {
			return query_error_from_failure("runtime_dependency_incompatible", failure);
		}
		return QueryError::new(
			"runtime_dependency_incompatible",
			"Git version is incompatible",
		);
	}
	if let Some(failure) = diagnostic
		.roots
		.iter()
		.filter_map(|root| root.detail.failure.as_ref())
		.find(|failure| failure.category == "timed_out")
	{
		return query_error_from_failure("runtime_dependency_timed_out", failure);
	}
	if let Some(failure) = diagnostic
		.roots
		.iter()
		.filter_map(|root| root.detail.failure.as_ref())
		.find(|failure| failure.category != "not_repository")
	{
		return query_error_from_failure("runtime_dependency_unavailable", failure);
	}
	let (code, fallback) = match diagnostic.state {
		GitDiagnosticState::Checking => (
			"runtime_dependency_checking",
			"Git diagnostic is still checking",
		),
		GitDiagnosticState::Incompatible => (
			"runtime_dependency_incompatible",
			"Git version is incompatible",
		),
		GitDiagnosticState::TimedOut => {
			("runtime_dependency_timed_out", "Git diagnostic timed out")
		}
		GitDiagnosticState::Available => (
			"git_worktree_unavailable",
			"no selected workspace root is a Git worktree",
		),
		GitDiagnosticState::Unavailable => (
			"runtime_dependency_unavailable",
			"Git runtime dependency is unavailable",
		),
	};
	if let Some(failure) = &diagnostic.failure {
		return query_error_from_failure(code, failure);
	}
	QueryError::new(code, fallback)
}

fn query_error_from_failure(
	code: &str,
	failure: &code_moniker_workspace::git_runtime::GitFailure,
) -> QueryError {
	QueryError::new(code, failure.message.as_str()).with_category(failure.category.as_str())
}

fn dependency_state(state: GitDiagnosticState) -> RuntimeDependencyState {
	match state {
		GitDiagnosticState::Checking => RuntimeDependencyState::Checking,
		GitDiagnosticState::Available => RuntimeDependencyState::Available,
		GitDiagnosticState::Unavailable => RuntimeDependencyState::Unavailable,
		GitDiagnosticState::Incompatible => RuntimeDependencyState::Incompatible,
		GitDiagnosticState::TimedOut => RuntimeDependencyState::TimedOut,
	}
}

fn resolution_source(source: GitResolutionSource) -> RuntimeDependencyResolutionSource {
	match source {
		GitResolutionSource::ExplicitConfiguration => {
			RuntimeDependencyResolutionSource::ExplicitConfiguration
		}
		GitResolutionSource::InheritedPath => RuntimeDependencyResolutionSource::InheritedPath,
	}
}

fn root_state(state: GitRootState) -> RuntimeDependencyRootState {
	match state {
		GitRootState::Worktree => RuntimeDependencyRootState::Worktree,
		GitRootState::RepositoryOnly => RuntimeDependencyRootState::RepositoryOnly,
		GitRootState::NotRepository => RuntimeDependencyRootState::NotRepository,
		GitRootState::Unavailable => RuntimeDependencyRootState::Unavailable,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use code_moniker_workspace::git_runtime::{GitFailure, GitRootDetail, GitRootDiagnostic};

	#[test]
	fn checking_diagnostic_projects_selected_roots_without_blocking_status() {
		let roots = vec![PathBuf::from("alpha"), PathBuf::from("beta")];
		let dependency = git_dependency_dto(&GitDiagnostic::checking(&roots), &roots, "daemon");
		assert_eq!(dependency.state, RuntimeDependencyState::Checking);
		assert_eq!(dependency.roots.len(), 2);
		assert!(
			dependency
				.roots
				.iter()
				.all(|root| root.state == RuntimeDependencyRootState::Unavailable)
		);
	}

	#[test]
	fn mixed_worktrees_degrade_change_capabilities_without_disabling_coupling_facts() {
		let roots = vec![PathBuf::from("alpha"), PathBuf::from("beta")];
		let diagnostic = GitDiagnostic {
			state: GitDiagnosticState::Available,
			resolution_source: Some(GitResolutionSource::InheritedPath),
			executable: Some(PathBuf::from("/resolved/git")),
			version: Some("git version 2.50.1".to_string()),
			compatible: Some(true),
			failure: None,
			checked_at_unix_ms: Some(1),
			duration_ms: Some(2),
			roots: vec![
				GitRootDiagnostic {
					root: roots[0].clone(),
					state: GitRootState::Worktree,
					repository_root: Some(roots[0].clone()),
					detail: GitRootDetail {
						failure: None,
						message: "available".to_string(),
					},
				},
				GitRootDiagnostic {
					root: roots[1].clone(),
					state: GitRootState::NotRepository,
					repository_root: None,
					detail: GitRootDetail {
						failure: None,
						message: "not a repository".to_string(),
					},
				},
			],
		};
		let capabilities = git_effective_capabilities(&diagnostic, &roots);
		assert_eq!(capabilities[0].state, EffectiveCapabilityState::Degraded);
		assert_eq!(capabilities[1].state, EffectiveCapabilityState::Degraded);
		assert_eq!(capabilities[2].state, EffectiveCapabilityState::Degraded);
		assert_eq!(capabilities[3].state, EffectiveCapabilityState::Available);
		assert_eq!(capabilities[3].dependency, None);
	}

	#[test]
	fn timeout_is_typed_and_never_disables_daemon_side_diff_impact() {
		let diagnostic = GitDiagnostic {
			state: GitDiagnosticState::TimedOut,
			resolution_source: Some(GitResolutionSource::ExplicitConfiguration),
			executable: Some(PathBuf::from("/resolved/git")),
			version: None,
			compatible: None,
			failure: Some(GitFailure {
				category: "timed_out".to_string(),
				message: "Git command timed out after 2000 ms".to_string(),
			}),
			checked_at_unix_ms: Some(1),
			duration_ms: Some(2_000),
			roots: vec![],
		};
		assert_eq!(
			git_unavailable_error(&diagnostic).code,
			"runtime_dependency_timed_out"
		);
		let capabilities = git_effective_capabilities(&diagnostic, &[]);
		assert_eq!(capabilities[0].state, EffectiveCapabilityState::Unavailable);
		assert_eq!(capabilities[3].state, EffectiveCapabilityState::Available);
	}

	#[test]
	fn change_review_gate_uses_only_the_requested_root() {
		let roots = vec![PathBuf::from("alpha"), PathBuf::from("beta")];
		let query = Query::ChangeReview(code_moniker_query::ChangeReviewQuery {
			workspace: Some("beta".to_string()),
		});
		assert_eq!(query_roots(&query, &roots).unwrap(), vec![roots[1].clone()]);
		assert!(query_requires_git(&query));
		let context = Query::ChangeContext(code_moniker_query::ChangeContextQuery {
			workspace: Some("beta".to_string()),
			focus: "src/lib.rs".to_string(),
			..Default::default()
		});
		assert!(!query_requires_git(&context));
		assert!(query_has_git_change_portion(&context));
	}

	#[test]
	fn root_timeout_keeps_its_typed_gate_error_when_git_itself_is_available() {
		let root = PathBuf::from("slow");
		let diagnostic = GitDiagnostic {
			state: GitDiagnosticState::Available,
			resolution_source: Some(GitResolutionSource::InheritedPath),
			executable: Some(PathBuf::from("/resolved/git")),
			version: Some("git version 2.50.1".to_string()),
			compatible: Some(true),
			failure: None,
			checked_at_unix_ms: Some(1),
			duration_ms: Some(2_000),
			roots: vec![GitRootDiagnostic {
				root,
				state: GitRootState::Unavailable,
				repository_root: None,
				detail: GitRootDetail {
					failure: Some(GitFailure {
						category: "timed_out".to_string(),
						message: "Git root probe timed out".to_string(),
					}),
					message: "Git root probe timed out".to_string(),
				},
			}],
		};
		assert_eq!(
			git_unavailable_error(&diagnostic).code,
			"runtime_dependency_timed_out"
		);
	}

	#[test]
	fn incompatible_git_keeps_its_dedicated_code_with_non_empty_roots() {
		let failure = GitFailure {
			category: "incompatible_version".to_string(),
			message: "Git 2.21.0 is outside the supported range >=2.22.0".to_string(),
		};
		let diagnostic = GitDiagnostic {
			state: GitDiagnosticState::Incompatible,
			resolution_source: Some(GitResolutionSource::InheritedPath),
			executable: Some(PathBuf::from("/resolved/git")),
			version: Some("git version 2.21.0".to_string()),
			compatible: Some(false),
			failure: Some(failure.clone()),
			checked_at_unix_ms: Some(1),
			duration_ms: Some(1),
			roots: vec![GitRootDiagnostic {
				root: PathBuf::from("workspace"),
				state: GitRootState::Unavailable,
				repository_root: None,
				detail: GitRootDetail {
					failure: Some(failure),
					message: "Git version is incompatible".to_string(),
				},
			}],
		};
		let error = git_unavailable_error(&diagnostic);
		assert_eq!(error.code, "runtime_dependency_incompatible");
		assert_eq!(error.category.as_deref(), Some("incompatible_version"));
	}

	#[test]
	fn acquisition_failure_preserves_a_non_timeout_category() {
		let error = git_acquisition_error(&code_moniker_workspace::git_runtime::GitRuntimeError {
			category: "output_limit".to_string(),
			message: "Git diff exceeded its output limit".to_string(),
		});
		assert_eq!(error.code, "runtime_dependency_unavailable");
		assert_eq!(error.category.as_deref(), Some("output_limit"));
	}

	#[test]
	fn operational_git_failures_retry_but_static_contract_failures_do_not() {
		for category in [
			"timed_out",
			"resolver_thread_unavailable",
			"reader_thread_unavailable",
			"spawn_failed",
			"pipe_unavailable",
			"wait_failed",
			"output_read_failed",
			"process_isolation_failed",
			"resolution_failed",
			"not_repository",
		] {
			assert!(
				git_failure_is_retryable(category),
				"{category} should retry"
			);
		}
		for category in [
			"invalid_configuration",
			"incompatible_version",
			"malformed_version",
			"malformed_output",
		] {
			assert!(
				!git_failure_is_retryable(category),
				"{category} should remain cached"
			);
		}
	}

	#[test]
	fn git_reprobe_cooldown_starts_after_a_slow_probe_completes() {
		let diagnostic = GitDiagnostic {
			state: GitDiagnosticState::TimedOut,
			resolution_source: None,
			executable: None,
			version: None,
			compatible: None,
			failure: None,
			checked_at_unix_ms: Some(1_000),
			duration_ms: Some(2_000),
			roots: vec![],
		};
		assert!(!git_reprobe_due(&diagnostic, 3_999));
		assert!(git_reprobe_due(&diagnostic, 4_000));
	}
}
