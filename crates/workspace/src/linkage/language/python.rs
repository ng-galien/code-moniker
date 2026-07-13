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
	let target_segments = query.target_segments().collect::<Vec<_>>();
	let candidate_segments =
		normalized_python_segments(candidate.moniker.as_view().segments().collect::<Vec<_>>());
	if target_segments.len() != candidate_segments.len() || target_segments.is_empty() {
		return false;
	}
	target_segments
		.iter()
		.zip(candidate_segments.iter())
		.all(|(target, candidate_segment)| {
			python_segment_matches(query, candidate, *target, *candidate_segment)
		})
}

// A Python package is imported by its bare name, but its definitions live in
// package:X/module:__init__ — collapse that pair to module:X so `import
// httpx` and `httpx.get(...)` line up with the __init__ reexports.
fn normalized_python_segments(segments: Vec<Segment<'_>>) -> Vec<Segment<'_>> {
	let mut normalized: Vec<Segment<'_>> = Vec::with_capacity(segments.len());
	let mut idx = 0;
	while idx < segments.len() {
		if segments[idx].kind == kinds::PACKAGE
			&& idx + 1 < segments.len()
			&& segments[idx + 1].kind == kinds::MODULE
			&& segments[idx + 1].name == b"__init__"
		{
			normalized.push(Segment {
				kind: kinds::MODULE,
				name: segments[idx].name,
			});
			idx += 2;
			continue;
		}
		normalized.push(segments[idx]);
		idx += 1;
	}
	normalized
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
	if target.kind == kinds::PATH
		&& is_python_path_target_kind(candidate_segment.kind)
		&& target.name == candidate_segment.name
	{
		return true;
	}
	if is_python_callable_kind(target.kind)
		&& candidate_segment.kind == kinds::PATH
		&& bare_callable_name(target.name) == candidate_segment.name
	{
		return true;
	}
	target.kind == kinds::LOCAL
		&& candidate_segment.kind == kinds::PARAM
		&& target.name == candidate_segment.name
}

fn python_segment_name_matches(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
	target: Segment<'_>,
	candidate_segment: Segment<'_>,
) -> bool {
	if is_python_callable_kind(target.kind) && is_python_callable_kind(candidate_segment.kind) {
		if query.call_name.is_none() {
			return bare_callable_name(target.name) == bare_callable_name(candidate_segment.name);
		}
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
