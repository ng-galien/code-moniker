use code_moniker_core::core::moniker::Segment;
use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::lang::c::RECOVERED_BODY_SIGNATURE;
use code_moniker_core::lang::{Lang, kinds};

use crate::linkage::catalog::LinkageQuery;
use crate::linkage::catalog::{CandidateCatalog, LinkageCandidate, SymbolOrdinal, SymbolSet};
use crate::linkage::language::generic_matches;
use crate::linkage::source_groups::{LinkPermission, SourceGroupPolicy};

mod includes;
mod refinement;

pub(in crate::linkage) use includes::CIncludeVisibility;
pub(in crate::linkage) use refinement::{
	classify_c_preprocessor_tokens, classify_c_unindexed_external_dependencies,
	refine_c_include_visibility,
};

// Leave the prototype in place when a same-name body lacks a known signature
// and could therefore also supply the link.
pub(in crate::linkage) fn definition_for_local_c_prototype(
	query: &LinkageQuery<'_>,
	catalog: &CandidateCatalog,
	groups: &SourceGroupPolicy,
	local_targets: &SymbolSet,
) -> Option<SymbolOrdinal> {
	if query.reference_kind != "calls" || local_targets.len() != 1 {
		return None;
	}
	let source = query.material.files.get(query.source_file)?;
	if !is_c_translation_unit(source) {
		return None;
	}
	let local = local_targets.iter().next()?;
	let prototype = catalog.function_linkage(local)?;
	if !prototype.declaration_only {
		return None;
	}
	let signature = prototype.signature.as_deref()?;
	if signature.contains(&b' ') {
		return None;
	}
	compatible_definition(query, catalog, groups, signature, false, |_| true)
}

pub(super) fn definition_for_included_prototype(
	query: &LinkageQuery<'_>,
	catalog: &CandidateCatalog,
	groups: &SourceGroupPolicy,
	visibility: &CIncludeVisibility,
	prototype: SymbolOrdinal,
) -> Option<SymbolOrdinal> {
	let candidate = catalog.candidate(prototype)?;
	let header = query.material.files.get(candidate.source_file)?;
	let linkage = catalog.function_linkage(prototype)?;
	if !linkage.declaration_only
		|| candidate.visibility != kinds::VIS_PUBLIC
		|| header.rel_path.extension().is_none_or(|ext| ext != "h")
	{
		return None;
	}
	let signature = linkage.signature.as_deref()?;
	compatible_definition(query, catalog, groups, signature, true, |file| {
		visibility
			.topology
			.directly_includes(file, candidate.source_file)
	})
}

fn compatible_definition(
	query: &LinkageQuery<'_>,
	catalog: &CandidateCatalog,
	groups: &SourceGroupPolicy,
	signature: &[u8],
	allow_recovered_body: bool,
	has_provenance: impl Fn(usize) -> bool,
) -> Option<SymbolOrdinal> {
	let source = query.material.files.get(query.source_file)?;
	let name = query.call_name?.as_bytes();
	let candidates = catalog.indexes().symbols_by_language_key(Lang::C, name)?;
	let mut definition = None;
	for symbol in candidates.iter() {
		let Some(candidate) = catalog.candidate(symbol) else {
			continue;
		};
		let Some(file) = query.material.files.get(candidate.source_file) else {
			continue;
		};
		if candidate.source_file == query.source_file
			|| candidate.visibility != kinds::VIS_PUBLIC
			|| candidate
				.last_segment
				.is_none_or(|segment| segment.kind != kinds::FUNC)
			|| candidate.call_name != Some(name)
			|| file.source_root != source.source_root
			|| !has_provenance(candidate.source_file)
			|| groups
				.link_permission(query.material, query.source_file, candidate.source_file)
				.is_some_and(|permission| permission != LinkPermission::Allowed)
		{
			continue;
		}
		let linkage = catalog.function_linkage(symbol)?;
		if linkage.declaration_only {
			continue;
		}
		let other_signature = linkage.signature.as_deref()?;
		let other_signature = match other_signature.strip_prefix(&[RECOVERED_BODY_SIGNATURE]) {
			Some(known) if allow_recovered_body => known,
			Some(_) => return None,
			None => other_signature,
		};
		if other_signature == signature {
			if definition.is_some() || !is_c_translation_unit(file) {
				return None;
			}
			definition = Some(symbol);
		}
	}
	definition
}

fn is_c_translation_unit(file: &crate::source::IndexedSourceFile) -> bool {
	file.lang == Lang::C && file.rel_path.extension().is_some_and(|ext| ext == "c")
}

// C translation-unit visibility is recorded as `module` and checked before
// any generic or program-wide name matching.
pub(super) fn matches(query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
	if candidate.visibility == b"module"
		&& candidate.source_file != query.source_file
		&& !source_imports_candidate_file(query, candidate)
	{
		return false;
	}
	if libc_target_matches_workspace_function(query, candidate) {
		return true;
	}
	if imported_macro_matches_call(query, candidate) {
		return true;
	}
	if imported_constant_matches_read(query, candidate) {
		return true;
	}
	generic_matches(query, candidate) || c_program_linkage_target_matches_def(query, candidate)
}

pub(super) fn matches_include_candidate(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	libc_target_matches_workspace_function(query, candidate)
		|| macro_matches_call(query, candidate)
		|| constant_matches_read(query, candidate)
		|| object_macro_matches_type_modifier(query, candidate)
		|| generic_matches(query, candidate)
		|| normalized_c_target_matches_def(query, candidate)
}

fn object_macro_matches_type_modifier(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	let Some(target) = query.target_last else {
		return false;
	};
	let Some(candidate_segment) = candidate.last_segment else {
		return false;
	};
	matches!(
		query.reference_kind,
		"uses_type" | "typed_as" | "returns_type"
	) && target.kind == kinds::TYPE
		&& candidate_segment.kind == kinds::CONST
		&& target.name == candidate_segment.name
}

fn imported_macro_matches_call(query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
	macro_matches_call(query, candidate) && source_imports_candidate_file(query, candidate)
}

fn macro_matches_call(query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
	let target_is_function = query
		.target_last
		.is_some_and(|segment| segment.kind == kinds::FUNC);
	let candidate_is_macro = candidate
		.last_segment
		.is_some_and(|segment| segment.kind == b"macro");
	target_is_function
		&& candidate_is_macro
		&& query.call_name.map(str::as_bytes) == candidate.call_name
		&& query.call_arity == candidate.call_arity
}

fn imported_constant_matches_read(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	constant_matches_read(query, candidate) && source_imports_candidate_file(query, candidate)
}

fn constant_matches_read(query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
	let Some(target) = query.target_last else {
		return false;
	};
	let Some(candidate_segment) = candidate.last_segment else {
		return false;
	};
	query.reference_kind == "reads"
		&& target.kind == kinds::VAR
		&& matches!(candidate_segment.kind, kinds::CONST | b"enum_constant")
		&& target.name == candidate_segment.name
}

fn source_imports_candidate_file(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	let Some(source) = query.material.files.get(query.source_file) else {
		return false;
	};
	let Some(candidate_file) = query.material.files.get(candidate.source_file) else {
		return false;
	};
	source.graph.refs().any(|reference| {
		reference.kind == kinds::IMPORTS_MODULE && reference.target == *candidate_file.graph.root()
	})
}

// The C extern namespace spans the whole program while extraction anchors
// fallbacks on the current file: compare with directory and module segments
// erased. An include target ends on a module segment, so terminal modules
// survive the erasure and match header roots wherever they live.
fn c_program_linkage_target_matches_def(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	if !candidate
		.last_segment
		.is_some_and(|segment| matches!(segment.kind, kinds::FUNC | kinds::VAR))
	{
		return false;
	}
	normalized_c_target_matches_def(query, candidate)
}

fn normalized_c_target_matches_def(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	let target_segments = query.target_segments().collect::<Vec<_>>();
	let keep_terminal_module = target_segments
		.last()
		.is_some_and(|segment| segment.kind == kinds::MODULE);
	if keep_terminal_module {
		return false;
	}
	let target = normalized_c_segments(target_segments, keep_terminal_module);
	let cand = normalized_c_segments(
		candidate.moniker.as_view().segments().collect::<Vec<_>>(),
		keep_terminal_module,
	);
	if target.is_empty() || target.len() != cand.len() {
		return false;
	}
	target.iter().zip(cand.iter()).enumerate().all(
		|(index, (target_segment, candidate_segment))| {
			let terminal = index == target.len() - 1;
			c_segment_matches(
				query,
				candidate,
				*target_segment,
				*candidate_segment,
				terminal,
			)
		},
	)
}

fn libc_target_matches_workspace_function(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	let libc_target = query
		.target_first
		.is_some_and(|segment| segment.kind == kinds::SDK && segment.name == b"c")
		&& query
			.target_segments()
			.nth(1)
			.is_some_and(|segment| segment.kind == kinds::PATH && segment.name == b"libc");
	let c_candidate = candidate
		.moniker
		.as_view()
		.segments()
		.any(|segment| segment.kind == kinds::LANG && segment.name == b"c");
	libc_target
		&& c_candidate
		&& candidate
			.last_segment
			.is_some_and(|segment| segment.kind == kinds::FUNC)
		&& query.call_name.map(str::as_bytes) == candidate.call_name
		&& query.call_arity == candidate.call_arity
}

fn normalized_c_segments(
	segments: Vec<Segment<'_>>,
	keep_terminal_module: bool,
) -> Vec<Segment<'_>> {
	let last_index = segments.len().saturating_sub(1);
	segments
		.into_iter()
		.enumerate()
		.filter(|(index, segment)| {
			if segment.kind == kinds::DIR || segment.kind == b"srcset" {
				return false;
			}
			if segment.kind == kinds::MODULE {
				return keep_terminal_module && *index == last_index;
			}
			true
		})
		.map(|(_, segment)| segment)
		.collect()
}

fn c_segment_matches(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
	target: Segment<'_>,
	candidate_segment: Segment<'_>,
	terminal: bool,
) -> bool {
	if target.kind != candidate_segment.kind
		&& !(!terminal
			&& target.kind == b"type"
			&& matches!(candidate_segment.kind, b"struct" | b"enum"))
	{
		return false;
	}
	if terminal && is_c_callable_kind(target.kind) {
		if let Some(call_name) = query.call_name {
			return Some(call_name.as_bytes()) == candidate.call_name
				&& query.call_arity == candidate.call_arity;
		}
		return bare_callable_name(target.name) == bare_callable_name(candidate_segment.name);
	}
	bare_callable_name(target.name) == bare_callable_name(candidate_segment.name)
}

fn is_c_callable_kind(kind: &[u8]) -> bool {
	matches!(kind, kinds::FUNC | b"macro")
}
