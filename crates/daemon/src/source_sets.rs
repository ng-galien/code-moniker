use std::collections::HashSet;

use code_moniker_core::lang::Lang;
use code_moniker_query::{
	CommandResponse, QueryError, WorkspaceEventDto, WorkspaceEventKind, WorkspaceSourceSetDto,
};
use code_moniker_workspace::snapshot::{
	MemorySourceRefreshMetrics, MemorySourceRefreshMode, WorkspaceRequest,
};
use code_moniker_workspace::source::{
	LocalResourceCache, MemorySourceDocument, MemorySourceSet, MemorySourceSetUpdate,
};

use crate::daemon::WorkspaceDaemon;
use crate::lifecycle::{generation, workspace_status_result, workspace_transition_result};

pub(crate) const MEMORY_SOURCE_LIMITS: MemorySourceLimits = MemorySourceLimits {
	max_source_sets: 128,
	max_documents_per_set: 10_000,
	max_uri_bytes: 4 * 1024,
	max_document_bytes: 16 * 1024 * 1024,
	max_source_set_bytes: 64 * 1024 * 1024,
	max_total_bytes: 256 * 1024 * 1024,
};

#[derive(Clone, Copy)]
pub(crate) struct MemorySourceLimits {
	pub(crate) max_source_sets: usize,
	pub(crate) max_documents_per_set: usize,
	pub(crate) max_uri_bytes: usize,
	pub(crate) max_document_bytes: usize,
	pub(crate) max_source_set_bytes: usize,
	pub(crate) max_total_bytes: usize,
}

pub(super) fn parse_memory_source_set(
	dto: WorkspaceSourceSetDto,
) -> Result<MemorySourceSet, QueryError> {
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
			content: document.content.into(),
		});
	}
	documents.sort_by(|left, right| left.uri.cmp(&right.uri));
	Ok(MemorySourceSet {
		srcset: dto.srcset,
		revision: dto.revision,
		documents,
	})
}

pub(super) fn validate_memory_source_set_limits(
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

pub(super) fn memory_source_limit_error(message: String) -> QueryError {
	QueryError::new("workspace_source_set_limit_exceeded", message)
}

pub(super) fn validate_srcset(srcset: &str) -> Result<(), QueryError> {
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

pub(super) fn refresh_memory_source_set(
	daemon: &mut WorkspaceDaemon,
	mut update: MemorySourceSetUpdate,
	message: String,
) -> Result<CommandResponse, QueryError> {
	if !update.changed {
		tracing::info!(
			memory.srcset = update.srcset,
			memory.refresh.mode = "noop",
			memory.documents.total = update.document_total,
			memory.documents.added = 0,
			memory.documents.modified = 0,
			memory.documents.removed = 0,
			memory.documents.unchanged = update.delta.unchanged,
			memory.extraction.jobs = 0,
			memory.extraction.completed = 0,
			memory.linkage.invocations = 0,
			"memory source set refresh completed"
		);
		return Ok(CommandResponse {
			generation: generation(&daemon.registry),
			message: format!("{message}: unchanged"),
			status: Some(Box::new(workspace_status_result(
				&daemon.roots,
				&daemon.registry,
			))),
		});
	}
	let initial_collection = update.previous.is_none()
		&& update.delta.added.len() == update.document_total
		&& update.delta.modified.is_empty()
		&& update.delta.removed.is_empty();
	let complete_bulk_load = initial_collection
		&& daemon
			.registry
			.queries()
			.snapshot()
			.is_none_or(|snapshot| snapshot.catalog.sources.is_empty());
	let mode = if complete_bulk_load {
		MemorySourceRefreshMode::Bulk
	} else {
		MemorySourceRefreshMode::Incremental
	};
	let checkpoint = daemon.cache.checkpoint_memory_source_update(&mut update);
	let document_total = update.document_total;
	let added = update.delta.added.len();
	let modified = update.delta.modified.len();
	let removed = update.delta.removed.len();
	let unchanged = update.delta.unchanged;
	let refresh_metrics = MemorySourceRefreshMetrics {
		mode,
		documents_total: document_total,
		added,
		modified,
		removed,
		unchanged,
		extraction_jobs: 0,
		extraction_workers: 0,
		linkage_invocations: 0,
	};
	let paths = update.paths;
	let request = WorkspaceRequest::new("daemon-memory-source-set")
		.with_memory_source_refresh(refresh_metrics);
	let transition = if complete_bulk_load {
		daemon.registry.commands().refresh(request)
	} else {
		daemon.registry.commands().refresh_paths(request, paths)
	};
	if let Err(error) = workspace_transition_result(transition) {
		daemon.cache.restore_checkpoint(checkpoint);
		return Err(error);
	}
	let generation = generation(&daemon.registry);
	let (extraction_jobs, extraction_workers) = daemon
		.registry
		.queries()
		.snapshot()
		.map(|snapshot| {
			(
				snapshot.index.timings.extraction_jobs,
				snapshot.index.timings.extraction_workers,
			)
		})
		.unwrap_or_default();
	tracing::info!(
		memory.srcset = update.srcset,
		memory.refresh.mode = mode.as_str(),
		memory.documents.total = document_total,
		memory.documents.added = added,
		memory.documents.modified = modified,
		memory.documents.removed = removed,
		memory.documents.unchanged = unchanged,
		memory.extraction.jobs = extraction_jobs,
		memory.extraction.completed = extraction_jobs,
		memory.extraction.workers = extraction_workers,
		memory.linkage.invocations = 1,
		workspace.generation = generation.map_or(0, |value| value.0),
		"memory source set refresh completed"
	);
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
