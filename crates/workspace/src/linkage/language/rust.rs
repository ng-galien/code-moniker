use code_moniker_core::core::moniker::Segment;
use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::lang::kinds;

use crate::linkage::catalog::LinkageCandidate;
use crate::linkage::catalog::LinkageQuery;
use crate::linkage::language::LanguageLinkageStrategy;

pub(super) struct RustLanguageLinkageStrategy;

impl LanguageLinkageStrategy for RustLanguageLinkageStrategy {
	fn matches(&self, query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
		candidate.moniker.bind_match(query.target)
			|| query.target.bind_match(candidate.moniker)
			|| rust_path_target_matches_def(query, candidate)
			|| rust_contextual_name_matches_def(query, candidate)
	}
}

fn rust_path_target_matches_def(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	if query.confidence != Some(confidence(kinds::CONF_NAME_MATCH))
		&& query.confidence != Some(confidence(kinds::CONF_IMPORTED))
	{
		return false;
	}
	let target_segments = normalized_rust_segments(query.target_segments());
	let candidate_segments = normalized_rust_segments(candidate.moniker.as_view().segments());
	if target_segments.len() != candidate_segments.len() || target_segments.is_empty() {
		return false;
	}
	target_segments
		.iter()
		.zip(candidate_segments.iter())
		.all(|(target, candidate_segment)| rust_path_segment_matches(*target, *candidate_segment))
}

#[derive(Clone, Copy)]
struct NormalizedSegment<'a> {
	kind: &'a [u8],
	name: &'a [u8],
}

fn normalized_rust_segments<'a>(
	segments: impl IntoIterator<Item = Segment<'a>>,
) -> Vec<NormalizedSegment<'a>> {
	let segments = segments.into_iter().collect::<Vec<_>>();
	let mut normalized = Vec::with_capacity(segments.len());
	let mut idx = 0;
	while idx < segments.len() {
		if is_implicit_rust_crate_root_module(&segments, idx) {
			idx += 1;
			continue;
		}
		if idx + 1 < segments.len()
			&& segments[idx].kind == kinds::DIR
			&& segments[idx + 1].kind == kinds::MODULE
			&& segments[idx + 1].name == b"mod"
		{
			normalized.push(NormalizedSegment {
				kind: kinds::MODULE,
				name: segments[idx].name,
			});
			idx += 2;
			continue;
		}
		normalized.push(NormalizedSegment {
			kind: segments[idx].kind,
			name: segments[idx].name,
		});
		idx += 1;
	}
	normalized
}

fn is_implicit_rust_crate_root_module(segments: &[Segment<'_>], idx: usize) -> bool {
	idx > 0
		&& segments[idx - 1].kind == kinds::DIR
		&& segments[idx - 1].name == b"src"
		&& segments[idx].kind == kinds::MODULE
		&& matches!(segments[idx].name, b"lib" | b"main")
}

fn rust_path_segment_matches(
	target: NormalizedSegment<'_>,
	candidate: NormalizedSegment<'_>,
) -> bool {
	if target.kind == candidate.kind {
		return bare_callable_name(target.name) == bare_callable_name(candidate.name);
	}
	if target.kind == kinds::MODULE && candidate.kind == kinds::DIR {
		return bare_callable_name(target.name) == bare_callable_name(candidate.name);
	}
	if is_rust_callable_kind(target.kind) && is_rust_callable_kind(candidate.kind) {
		return bare_callable_name(target.name) == bare_callable_name(candidate.name);
	}
	target.kind == kinds::PATH
		&& is_rust_path_target_kind(candidate.kind)
		&& bare_callable_name(target.name) == bare_callable_name(candidate.name)
}

fn rust_contextual_name_matches_def(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	if !can_use_contextual_name_match(query) {
		return false;
	}
	let Some(target) = query.target_last else {
		return false;
	};
	let Some(candidate_segment) = candidate.last_segment else {
		return false;
	};
	if !rust_name_matches(query, candidate, target, candidate_segment) {
		return false;
	}
	rust_kind_can_satisfy(query, target.kind, candidate_segment.kind)
}

fn can_use_contextual_name_match(query: &LinkageQuery<'_>) -> bool {
	if is_qualified_local_rust_call(query) {
		return false;
	}
	if is_rust_call_ref(query.reference_kind.as_bytes()) {
		return true;
	}
	if query.confidence == Some(confidence(kinds::CONF_NAME_MATCH))
		|| query.confidence == Some(confidence(kinds::CONF_IMPORTED))
	{
		return true;
	}
	query.confidence == Some(confidence(kinds::CONF_EXTERNAL))
		&& external_root(query).is_some_and(|root| !is_builtin_external_root(root))
}

fn is_qualified_local_rust_call(query: &LinkageQuery<'_>) -> bool {
	query.reference_kind.as_bytes() == kinds::CALLS
		&& query.target_segment_count > 1
		&& query
			.target_first
			.is_some_and(|first| first.kind != kinds::EXTERNAL_PKG)
}

fn rust_name_matches(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
	target: Segment<'_>,
	candidate_segment: Segment<'_>,
) -> bool {
	if is_rust_call_ref(query.reference_kind.as_bytes()) {
		return query
			.call_name
			.is_some_and(|name| Some(name.as_bytes()) == candidate.call_name)
			&& query.call_arity == candidate.call_arity;
	}
	bare_callable_name(target.name) == candidate_name(candidate, candidate_segment)
}

fn rust_kind_can_satisfy(
	query: &LinkageQuery<'_>,
	target_kind: &[u8],
	candidate_kind: &[u8],
) -> bool {
	if is_rust_call_ref(query.reference_kind.as_bytes()) {
		return is_rust_callable_kind(candidate_kind);
	}
	if target_kind == kinds::PATH {
		return is_rust_path_target_kind(candidate_kind);
	}
	target_kind == candidate_kind
}

fn candidate_name<'a>(
	candidate: &'a LinkageCandidate<'a>,
	candidate_segment: Segment<'a>,
) -> &'a [u8] {
	candidate
		.call_name
		.unwrap_or_else(|| bare_callable_name(candidate_segment.name))
}

fn external_root<'a>(query: &'a LinkageQuery<'_>) -> Option<&'a [u8]> {
	query
		.target
		.as_view()
		.segments()
		.next()
		.and_then(|head| (head.kind == kinds::EXTERNAL_PKG).then_some(head.name))
}

fn is_builtin_external_root(root: &[u8]) -> bool {
	matches!(root, b"std" | b"core" | b"alloc" | b"proc_macro")
}

pub(super) fn builtin_external_root(root: &str) -> bool {
	matches!(root, "std" | "core" | "alloc" | "proc_macro")
}

pub(super) fn proc_macro_annotation(query: &LinkageQuery<'_>) -> bool {
	query
		.material
		.files
		.get(query.source_file)
		.is_some_and(|file| file.lang == code_moniker_core::lang::Lang::Rs)
		&& query.reference_kind.as_bytes() == kinds::ANNOTATES
		&& query.confidence == Some(confidence(kinds::CONF_NAME_MATCH))
}

fn is_rust_exportable_kind(kind: &[u8]) -> bool {
	matches!(
		kind,
		kinds::MODULE
			| kinds::STRUCT
			| kinds::ENUM
			| kinds::TRAIT
			| kinds::TYPE
			| kinds::FN
			| kinds::CONST
			| kinds::STATIC
			| kinds::ENUM_CONSTANT
	)
}

fn is_rust_callable_kind(kind: &[u8]) -> bool {
	kind == kinds::FN || kind == kinds::METHOD
}

fn is_rust_call_ref(kind: &[u8]) -> bool {
	kind == kinds::CALLS || kind == kinds::METHOD_CALL
}

fn is_rust_path_target_kind(kind: &[u8]) -> bool {
	kind == kinds::PATH || is_rust_exportable_kind(kind) || is_rust_callable_kind(kind)
}

fn confidence(value: &[u8]) -> &str {
	std::str::from_utf8(value)
		.unwrap_or_else(|err| panic!("confidence constants must be utf-8: {err}"))
}
