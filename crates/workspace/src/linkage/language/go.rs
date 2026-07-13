use code_moniker_core::core::moniker::Segment;
use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::lang::kinds;

use crate::linkage::catalog::LinkageCandidate;
use crate::linkage::catalog::LinkageQuery;
use crate::linkage::language::{LanguageLinkageStrategy, generic::GenericLanguageLinkageStrategy};

pub(super) struct GoLanguageLinkageStrategy;

impl LanguageLinkageStrategy for GoLanguageLinkageStrategy {
	fn matches(&self, query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
		GenericLanguageLinkageStrategy.matches(query, candidate)
			|| go_package_target_matches_def(query, candidate)
	}
}

// Go package scope spans every file of a directory, but extraction anchors
// same-package fallbacks on the current file's `module:` segment — compare
// with module segments erased. A bare method fallback also matches a method
// nested under its receiver type when call name and arity agree.
fn go_package_target_matches_def(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	let target = normalized_go_segments(query.target_segments().collect::<Vec<_>>());
	let mut cand =
		normalized_go_segments(candidate.moniker.as_view().segments().collect::<Vec<_>>());
	if target.is_empty() || cand.is_empty() {
		return false;
	}
	if cand.len() == target.len() + 1
		&& query.call_name.is_some()
		&& target
			.last()
			.is_some_and(|segment| segment.kind == kinds::METHOD)
		&& is_go_owner_type_kind(cand[cand.len() - 2].kind)
	{
		cand.remove(cand.len() - 2);
	}
	if target.len() != cand.len() {
		return false;
	}
	target.iter().zip(cand.iter()).enumerate().all(
		|(index, (target_segment, candidate_segment))| {
			let terminal = index == target.len() - 1;
			go_segment_matches(
				query,
				candidate,
				*target_segment,
				*candidate_segment,
				terminal,
			)
		},
	)
}

fn normalized_go_segments(segments: Vec<Segment<'_>>) -> Vec<Segment<'_>> {
	segments
		.into_iter()
		.filter(|segment| segment.kind != kinds::MODULE && segment.kind != b"srcset")
		.collect()
}

fn go_segment_matches(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
	target: Segment<'_>,
	candidate_segment: Segment<'_>,
	terminal: bool,
) -> bool {
	if target.kind != candidate_segment.kind {
		return false;
	}
	if terminal && is_go_callable_kind(target.kind) {
		if let Some(call_name) = query.call_name {
			return Some(call_name.as_bytes()) == candidate.call_name
				&& query.call_arity == candidate.call_arity;
		}
		return bare_callable_name(target.name) == bare_callable_name(candidate_segment.name);
	}
	bare_callable_name(target.name) == bare_callable_name(candidate_segment.name)
}

fn is_go_owner_type_kind(kind: &[u8]) -> bool {
	matches!(kind, kinds::STRUCT | kinds::INTERFACE | kinds::TYPE)
}

fn is_go_callable_kind(kind: &[u8]) -> bool {
	matches!(kind, kinds::FUNC | kinds::METHOD)
}
