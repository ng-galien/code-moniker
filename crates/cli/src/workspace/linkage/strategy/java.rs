use code_moniker_core::core::moniker::Segment;
use code_moniker_core::lang::kinds;

use crate::workspace::linkage::candidate::LinkageCandidate;
use crate::workspace::linkage::query::LinkageQuery;
use crate::workspace::linkage::strategy::LanguageLinkageStrategy;

pub(super) struct JavaLanguageLinkageStrategy;

impl LanguageLinkageStrategy for JavaLanguageLinkageStrategy {
	fn matches(&self, query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
		candidate.moniker.bind_match(query.target)
			|| query.target.bind_match(candidate.moniker)
			|| java_path_target_matches_type_def(query, candidate)
	}
}

fn java_path_target_matches_type_def(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	let target_segments = query.target.as_view().segments().collect::<Vec<_>>();
	let candidate_segments = candidate.moniker.as_view().segments().collect::<Vec<_>>();
	if target_segments.len() != candidate_segments.len() || target_segments.is_empty() {
		return false;
	}
	if query.target.as_view().project() != candidate.moniker.as_view().project() {
		return false;
	}
	target_segments
		.iter()
		.zip(candidate_segments.iter())
		.all(|(target, candidate_segment)| {
			java_segment_matches(query, candidate, *target, *candidate_segment)
		})
}

fn java_segment_matches(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
	target: Segment<'_>,
	candidate_segment: Segment<'_>,
) -> bool {
	if is_java_source_set_segment(target) && is_java_source_set_segment(candidate_segment) {
		return java_source_set_can_read(target.name, candidate_segment.name);
	}
	if target.kind == candidate_segment.kind {
		return java_segment_name_matches(query, candidate, target, candidate_segment);
	}
	target.kind == kinds::PATH
		&& is_java_type_kind(candidate_segment.kind)
		&& target.name == candidate_segment.name
}

fn java_segment_name_matches(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
	target: Segment<'_>,
	candidate_segment: Segment<'_>,
) -> bool {
	if is_java_callable_kind(target.kind) && is_java_callable_kind(candidate_segment.kind) {
		return query
			.call_name
			.is_some_and(|name| Some(name.as_bytes()) == candidate.call_name)
			&& query.call_arity == candidate.call_arity;
	}
	target.name == candidate_segment.name
}

fn is_java_type_kind(kind: &[u8]) -> bool {
	matches!(
		kind,
		kinds::CLASS | kinds::INTERFACE | kinds::RECORD | kinds::ENUM | kinds::ANNOTATION_TYPE
	)
}

fn is_java_callable_kind(kind: &[u8]) -> bool {
	matches!(kind, kinds::METHOD | kinds::CONSTRUCTOR)
}

fn is_java_source_set_segment(segment: Segment<'_>) -> bool {
	segment.kind == b"srcset"
}

fn java_source_set_can_read(source_set: &[u8], candidate_source_set: &[u8]) -> bool {
	source_set == candidate_source_set || (source_set == b"test" && candidate_source_set == b"main")
}
