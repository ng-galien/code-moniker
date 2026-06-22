use code_moniker_core::core::moniker::Segment;
use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::lang::kinds;

use crate::linkage::catalog::LinkageCandidate;
use crate::linkage::catalog::LinkageQuery;
use crate::linkage::language::{LanguageLinkageStrategy, generic::GenericLanguageLinkageStrategy};

pub(super) struct PythonLanguageLinkageStrategy;

impl LanguageLinkageStrategy for PythonLanguageLinkageStrategy {
	fn matches(&self, query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
		GenericLanguageLinkageStrategy.matches(query, candidate)
			|| python_path_target_matches_def(query, candidate)
	}
}

fn python_path_target_matches_def(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	if query.target_segment_count != candidate.segment_count || query.target_segment_count == 0 {
		return false;
	}
	query
		.target_segments()
		.zip(candidate.moniker.as_view().segments())
		.all(|(target, candidate_segment)| {
			python_segment_matches(query, candidate, target, candidate_segment)
		})
}

fn python_segment_matches(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
	target: Segment<'_>,
	candidate_segment: Segment<'_>,
) -> bool {
	if target.kind == candidate_segment.kind {
		return python_segment_name_matches(query, candidate, target, candidate_segment);
	}
	target.kind == kinds::PATH
		&& is_python_path_target_kind(candidate_segment.kind)
		&& target.name == candidate_segment.name
}

fn python_segment_name_matches(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
	target: Segment<'_>,
	candidate_segment: Segment<'_>,
) -> bool {
	if is_python_callable_kind(target.kind) && is_python_callable_kind(candidate_segment.kind) {
		return query
			.call_name
			.is_some_and(|name| Some(name.as_bytes()) == candidate.call_name)
			&& query.call_arity == candidate.call_arity;
	}
	bare_callable_name(target.name) == bare_callable_name(candidate_segment.name)
}

fn is_python_path_target_kind(kind: &[u8]) -> bool {
	matches!(kind, kinds::CLASS | kinds::TYPE) || is_python_callable_kind(kind)
}

fn is_python_callable_kind(kind: &[u8]) -> bool {
	matches!(
		kind,
		kinds::FUNCTION | kinds::ASYNC_FUNCTION | kinds::METHOD
	)
}
