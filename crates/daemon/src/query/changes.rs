use std::path::PathBuf;

use code_moniker_query::{
	ChangeReviewFile, ChangeReviewQuery, ChangeReviewRef, ChangeReviewResult, ChangeReviewSide,
	ChangeReviewSummary, ChangeReviewSymbol, DiffImpactCompareQuery, DiffImpactFile,
	DiffImpactFileStatus, DiffImpactRef, DiffImpactResult, DiffImpactSide, DiffImpactSummary,
	DiffImpactSymbol, QueryError, QueryResponse, QueryResult, WorkspaceGeneration,
	symbol_is_test_artifact,
};
use code_moniker_workspace::snapshot::WorkspaceSnapshot;
use code_moniker_workspace::source::{LocalResourceCache, MemorySourceSet};

use crate::helpers::{DEFAULT_SCHEME, selected_roots};
use crate::source_sets::{
	MEMORY_SOURCE_LIMITS, memory_source_limit_error, parse_memory_source_set,
	validate_memory_source_set_limits,
};

pub(crate) fn change_review_response(
	cache: &LocalResourceCache,
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: ChangeReviewQuery,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let _ = selected_roots(roots, query.workspace.as_deref())?;
	let material = cache
		.index_material(snapshot.index.generation)
		.ok_or_else(|| {
			QueryError::new(
				"change_review_unavailable",
				"code index material is unavailable for semantic change review",
			)
		})?;
	let review_span = tracing::info_span!(
		"workspace.change_review",
		index.generation = snapshot.index.generation.value(),
	);
	let review = review_span
		.in_scope(|| code_moniker_workspace::changes::build_semantic_review(material.as_ref()));
	let result = change_review_dto(&review);
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::ChangeReview(Box::new(result)),
		next_cursor: None,
	})
}

pub(crate) fn diff_impact_compare_response(
	query: DiffImpactCompareQuery,
) -> Result<QueryResponse, QueryError> {
	if query.scope.trim().is_empty() {
		return Err(QueryError::new(
			"invalid_diff_impact_scope",
			"diff-impact comparison scope must not be empty",
		));
	}
	let base = parse_memory_source_set(query.base)?;
	let head = parse_memory_source_set(query.head)?;
	if base.srcset != head.srcset {
		return Err(QueryError::new(
			"diff_impact_source_set_mismatch",
			"base and head must use the same virtual source-set identity",
		));
	}
	validate_virtual_diff_impact_limits(&base, &head)?;
	for file in &query.files {
		validate_diff_impact_file(file)?;
	}
	let files = query
		.files
		.into_iter()
		.map(|file| code_moniker_workspace::changes::semantic::virtual_diff_impact::VirtualDiffImpactFile {
			status: match file.status {
				DiffImpactFileStatus::Added => code_moniker_workspace::changes::semantic::virtual_diff_impact::VirtualDiffImpactFileStatus::Added,
				DiffImpactFileStatus::Modified => code_moniker_workspace::changes::semantic::virtual_diff_impact::VirtualDiffImpactFileStatus::Modified,
				DiffImpactFileStatus::Deleted => code_moniker_workspace::changes::semantic::virtual_diff_impact::VirtualDiffImpactFileStatus::Deleted,
				DiffImpactFileStatus::Renamed => code_moniker_workspace::changes::semantic::virtual_diff_impact::VirtualDiffImpactFileStatus::Renamed,
			},
			old_uri: file.old_uri,
			new_uri: file.new_uri,
			old_hunks: file.old_hunks.into_iter().map(|span| (span.start, span.end)).collect(),
			new_hunks: file.new_hunks.into_iter().map(|span| (span.start, span.end)).collect(),
		})
		.collect();
	let srcset = base.srcset.clone();
	let impact =
		code_moniker_workspace::changes::semantic::virtual_diff_impact::build_virtual_diff_impact(
			code_moniker_workspace::changes::semantic::virtual_diff_impact::VirtualDiffImpactInput {
				scope: query.scope,
				project: query.project,
				srcset,
				base: virtual_diff_impact_documents(base),
				head: virtual_diff_impact_documents(head),
				files,
			},
		)
		.map_err(|message| QueryError::new("diff_impact_compare_failed", message))?;
	Ok(QueryResponse {
		generation: None,
		result: QueryResult::DiffImpact(Box::new(diff_impact_dto(&impact))),
		next_cursor: None,
	})
}

pub(crate) fn validate_diff_impact_file(
	file: &code_moniker_query::DiffImpactCompareFile,
) -> Result<(), QueryError> {
	let valid_paths = match file.status {
		DiffImpactFileStatus::Added => file.old_uri.is_none() && file.new_uri.is_some(),
		DiffImpactFileStatus::Deleted => file.old_uri.is_some() && file.new_uri.is_none(),
		DiffImpactFileStatus::Modified => file.old_uri.is_some() && file.old_uri == file.new_uri,
		DiffImpactFileStatus::Renamed => {
			file.old_uri.is_some() && file.new_uri.is_some() && file.old_uri != file.new_uri
		}
	};
	if !valid_paths {
		return Err(QueryError::new(
			"invalid_diff_impact_file",
			"diff-impact file paths do not match its added, modified, deleted, or renamed status",
		));
	}
	if file.rename_score.is_some_and(|score| score > 100) {
		return Err(QueryError::new(
			"invalid_diff_impact_rename_score",
			"diff-impact rename score must be between 0 and 100",
		));
	}
	if file
		.old_hunks
		.iter()
		.chain(&file.new_hunks)
		.any(|span| span.start == 0 || span.end < span.start)
	{
		return Err(QueryError::new(
			"invalid_diff_impact_span",
			"diff-impact line spans must be one-based and end at or after their start",
		));
	}
	Ok(())
}

fn validate_virtual_diff_impact_limits(
	base: &MemorySourceSet,
	head: &MemorySourceSet,
) -> Result<(), QueryError> {
	let cache = LocalResourceCache::default();
	validate_memory_source_set_limits(&cache, base, MEMORY_SOURCE_LIMITS)?;
	validate_memory_source_set_limits(&cache, head, MEMORY_SOURCE_LIMITS)?;
	let total_bytes = base.size_bytes().saturating_add(head.size_bytes());
	if total_bytes > MEMORY_SOURCE_LIMITS.max_total_bytes {
		return Err(memory_source_limit_error(format!(
			"the diff-impact comparison uses {total_bytes} bytes of source text; the limit is {}",
			MEMORY_SOURCE_LIMITS.max_total_bytes
		)));
	}
	Ok(())
}

fn virtual_diff_impact_documents(
	source_set: MemorySourceSet,
) -> Vec<code_moniker_workspace::changes::semantic::virtual_diff_impact::VirtualDiffImpactDocument>
{
	source_set
		.documents
		.into_iter()
		.map(|document| {
			code_moniker_workspace::changes::semantic::virtual_diff_impact::VirtualDiffImpactDocument {
				uri: document.uri,
				lang: document.lang,
				content: document.content.to_string(),
			}
		})
		.collect()
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

fn diff_impact_dto(
	impact: &code_moniker_workspace::changes::semantic::review::SemanticReview,
) -> DiffImpactResult {
	DiffImpactResult {
		scope: impact.scope.clone(),
		summary: DiffImpactSummary {
			files: impact.files.len(),
			analyzable_files: impact.files.iter().filter(|facts| facts.analyzable).count(),
			symbol_changes: impact.symbol_changes.len(),
			ref_changes: impact.ref_changes.len(),
			retargeted_refs: impact
				.ref_changes
				.iter()
				.filter(|change| change.kind.is_retarget())
				.count(),
			residual_files: impact
				.files
				.iter()
				.filter(|facts| !facts.coverage.explained())
				.count(),
		},
		files: impact.files.iter().map(diff_impact_file).collect(),
		symbol_changes: impact
			.symbol_changes
			.iter()
			.map(diff_impact_symbol)
			.collect(),
		ref_changes: impact.ref_changes.iter().map(diff_impact_ref).collect(),
		diagnostics: impact.diagnostics.clone(),
	}
}

fn diff_impact_file(
	facts: &code_moniker_workspace::changes::semantic::review::FileFacts,
) -> DiffImpactFile {
	let path = facts
		.rollup
		.new_path
		.as_ref()
		.or(facts.rollup.old_path.as_ref())
		.map(|path| path.display().to_string())
		.unwrap_or_default();
	DiffImpactFile {
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
		test_artifact: symbol_is_test_artifact("", &path, ""),
	}
}

fn diff_impact_symbol(
	change: &code_moniker_workspace::changes::semantic::model::SymbolChange,
) -> DiffImpactSymbol {
	DiffImpactSymbol {
		kind: change.kind.label().to_string(),
		confidence: change.confidence.label().to_string(),
		body_changed: change.facets.body_changed,
		signature_changed: change.facets.signature_changed,
		visibility_changed: change.facets.visibility_changed,
		header_changed: change.facets.header_changed,
		file_moved: change.facets.file_moved,
		old: change.old.as_ref().map(diff_impact_side),
		new: change.new.as_ref().map(diff_impact_side),
	}
}

fn diff_impact_side(
	side: &code_moniker_workspace::changes::semantic::model::SymbolSide,
) -> DiffImpactSide {
	let identity = code_moniker_core::core::uri::to_uri(
		&side.moniker,
		&code_moniker_core::core::uri::UriConfig {
			scheme: DEFAULT_SCHEME,
		},
	);
	let compact_identity =
		code_moniker_workspace::code::compact_identity(&identity, DEFAULT_SCHEME)
			.unwrap_or_else(|| identity.clone());
	let file = side.file_path.display().to_string();
	DiffImpactSide {
		test_artifact: symbol_is_test_artifact(&side.kind, &file, &identity),
		identity,
		compact_identity,
		file,
		kind: side.kind.clone(),
		name: side.name.clone(),
		visibility: side.visibility.clone(),
		lines: side.line_range,
	}
}

fn diff_impact_ref(
	change: &code_moniker_workspace::changes::semantic::model::RefChange,
) -> DiffImpactRef {
	let config = code_moniker_core::core::uri::UriConfig {
		scheme: DEFAULT_SCHEME,
	};
	let render = |target: &Option<code_moniker_core::core::moniker::Moniker>| {
		target
			.as_ref()
			.map(|target| code_moniker_core::core::uri::to_uri(target, &config))
	};
	let old_target = render(&change.old_target);
	let new_target = render(&change.new_target);
	let compact = |target: &Option<String>| {
		target.as_ref().map(|target| {
			code_moniker_workspace::code::compact_identity(target, DEFAULT_SCHEME)
				.unwrap_or_else(|| target.clone())
		})
	};
	DiffImpactRef {
		kind: change.kind.label().to_string(),
		file: change.file_path.display().to_string(),
		ref_kind: change.ref_kind.clone(),
		old_target_compact: compact(&old_target),
		new_target_compact: compact(&new_target),
		old_target,
		new_target,
		old_lines: change.old_line_range,
		new_lines: change.new_line_range,
	}
}

fn change_review_file(
	facts: &code_moniker_workspace::changes::semantic::review::FileFacts,
) -> ChangeReviewFile {
	let path = facts
		.rollup
		.new_path
		.as_ref()
		.or(facts.rollup.old_path.as_ref())
		.map(|path| path.display().to_string())
		.unwrap_or_default();
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
		test_artifact: symbol_is_test_artifact("", &path, ""),
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
	let identity = code_moniker_core::core::uri::to_uri(
		&side.moniker,
		&code_moniker_core::core::uri::UriConfig {
			scheme: DEFAULT_SCHEME,
		},
	);
	let file = side.file_path.display().to_string();
	ChangeReviewSide {
		test_artifact: symbol_is_test_artifact(&side.kind, &file, &identity),
		identity,
		file,
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
