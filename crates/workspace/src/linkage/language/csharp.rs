use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::lang::kinds;

use crate::linkage::catalog::{LinkageCandidate, LinkageQuery};
use crate::linkage::language::generic_matches;
use crate::snapshot::{DynamicReason, ReferenceRecord};
use crate::source::CodeIndexMaterial;

pub(super) fn matches(query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
	generic_matches(query, candidate) || csharp_name_target_matches_def(query, candidate)
}

fn csharp_name_target_matches_def(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	let Some(target) = query.target_last else {
		return false;
	};
	let Some(candidate_segment) = candidate.last_segment else {
		return false;
	};
	if is_type_kind(target.kind) && is_type_kind(candidate_segment.kind) {
		return target.name == candidate_segment.name
			|| (query.reference_kind == "annotates"
				&& candidate_segment
					.name
					.strip_suffix(b"Attribute")
					.is_some_and(|short_name| short_name == target.name));
	}
	if !is_target_callable_kind(target.kind) || !is_def_callable_kind(candidate_segment.kind) {
		return false;
	}
	let target_name = query
		.call_name
		.map(str::as_bytes)
		.unwrap_or_else(|| bare_callable_name(target.name));
	let candidate_name = candidate
		.call_name
		.unwrap_or_else(|| bare_callable_name(candidate_segment.name));
	target_name == candidate_name && call_arity_matches(query.call_arity, candidate.call_arity)
}

fn call_arity_matches(call: Option<usize>, definition: Option<usize>) -> bool {
	match (call, definition) {
		(Some(call), Some(definition)) => call == definition,
		_ => true,
	}
}

fn is_type_kind(kind: &[u8]) -> bool {
	matches!(
		kind,
		kinds::CLASS | kinds::INTERFACE | kinds::STRUCT | kinds::RECORD | kinds::ENUM | b"delegate"
	)
}

fn is_target_callable_kind(kind: &[u8]) -> bool {
	matches!(kind, kinds::FUNCTION | kinds::METHOD | kinds::CONSTRUCTOR)
}

fn is_def_callable_kind(kind: &[u8]) -> bool {
	matches!(kind, kinds::METHOD | kinds::CONSTRUCTOR)
}

pub(super) fn classify_open_reference(
	material: &CodeIndexMaterial,
	decision: &mut crate::linkage::binding::ReferenceLinkageDecision,
	reference_idx: usize,
	reference: &ReferenceRecord,
) {
	let imported_external = reference.confidence.as_deref() == Some("imported")
		&& material
			.reference_target(&reference.id)
			.is_some_and(super::external_target_shape);
	let reason = if imported_external {
		Some(DynamicReason::ExternalDependencyUnindexed)
	} else if reference.confidence.as_deref() == Some("name_match")
		&& matches!(
			reference.kind.as_str(),
			"method_call"
				| "calls" | "uses_type"
				| "typed_as" | "annotates"
				| "instantiates"
				| "extends"
		) {
		Some(DynamicReason::InsufficientLocalFacts)
	} else {
		None
	};
	let Some(reason) = reason else { return };
	let candidates = decision
		.linkage_targets()
		.cloned()
		.unwrap_or_else(crate::linkage::catalog::SymbolSet::new);
	*decision = crate::linkage::binding::ReferenceLinkageDecision::dynamic(
		reason,
		reference_idx,
		reference.id,
		candidates,
	);
}
