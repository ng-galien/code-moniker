use code_moniker_core::core::code_graph::DefRecord;
use code_moniker_core::core::moniker::Moniker;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::linkage::binding::{
	ExternalOrigin, ReferenceLinkageDecision, ResolutionDecision, ResolutionScope,
};
use crate::linkage::catalog::SymbolSet;
use crate::linkage::resolve::{DecisionSelection, LinkageRefiner};
use crate::snapshot::{RecordTable, ReferenceRecord, ResolutionEvidence};
use crate::source::CodeIndexMaterial;

use super::CIncludeVisibility;

pub(in crate::linkage) fn classify_c_unindexed_external_dependencies(
	linkage: &LinkageRefiner<'_>,
	visibility: &CIncludeVisibility,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	selection: DecisionSelection<'_>,
) {
	for &decision_idx in selection.indices() {
		let decision = &mut decisions[decision_idx];
		let Some(reference_idx) = decision.refinement_pending_reference_idx() else {
			continue;
		};
		if !selection.includes(decision.reference()) {
			continue;
		}
		let reference = &references[reference_idx];
		let Some(location) = linkage.locations.get(reference_idx) else {
			continue;
		};
		if reference.kind == "imports_module" {
			if reference.receiver.as_deref() == Some("c_build_dependency") {
				*decision = ReferenceLinkageDecision::external(
					ExternalOrigin::Dependency,
					reference_idx,
					reference.id,
				);
			}
			continue;
		}
		if !visibility.depends_on_unindexed_external(location.source_file) {
			continue;
		}
		*decision = ReferenceLinkageDecision::dynamic(
			crate::snapshot::DynamicReason::ExternalDependencyUnindexed,
			reference_idx,
			reference.id,
			SymbolSet::new(),
		);
	}
}

pub(in crate::linkage) fn classify_c_preprocessor_tokens(
	linkage: &LinkageRefiner<'_>,
	visibility: &CIncludeVisibility,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	selection: DecisionSelection<'_>,
) {
	let transformed_call_ranges = collect_c_transformed_macro_arguments(
		linkage, visibility, decisions, references, selection,
	);
	let expanded_macro_tokens = collect_c_expanded_macro_tokens(linkage, decisions, references);
	classify_c_pending_macro_tokens(
		linkage,
		visibility,
		decisions,
		references,
		&transformed_call_ranges,
		&expanded_macro_tokens,
		selection,
	);
}

struct ExpandedMacroTokens {
	invocation_end: u32,
	tokens: FxHashSet<Vec<u8>>,
}

fn collect_c_expanded_macro_tokens(
	linkage: &LinkageRefiner<'_>,
	decisions: &[ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
) -> FxHashMap<crate::snapshot::SymbolId, Vec<ExpandedMacroTokens>> {
	let mut expanded = FxHashMap::<_, Vec<ExpandedMacroTokens>>::default();
	for decision in decisions {
		let reference = &references[decision.reference_idx()];
		if reference.kind != "calls" {
			continue;
		}
		let Some(location) = linkage.locations.get(decision.reference_idx()) else {
			continue;
		};
		let Some(invocation_range) = linkage
			.material
			.files
			.get(location.source_file)
			.and_then(|file| file.graph.ref_at(location.reference).position)
		else {
			continue;
		};
		let Some(targets) = decision.linkage_targets() else {
			continue;
		};
		for target in targets.iter() {
			let Some(candidate) = linkage.candidates.candidate(target) else {
				continue;
			};
			if candidate
				.last_segment
				.is_none_or(|segment| segment.kind != b"macro")
			{
				continue;
			}
			let Some(file) = linkage.material.files.get(candidate.source_file) else {
				continue;
			};
			let Some((start, end)) = file.graph.locate(candidate.moniker) else {
				continue;
			};
			let Some(body) = file.source.as_bytes().get(start as usize..end as usize) else {
				continue;
			};
			let tokens = c_identifier_tokens(body);
			if !tokens.is_empty() {
				expanded
					.entry(reference.source_symbol)
					.or_default()
					.push(ExpandedMacroTokens {
						invocation_end: invocation_range.1,
						tokens,
					});
			}
		}
	}
	expanded
}

fn c_identifier_tokens(source: &[u8]) -> FxHashSet<Vec<u8>> {
	let mut tokens = FxHashSet::default();
	let mut start = None;
	for (index, byte) in source.iter().copied().enumerate() {
		let identifier = byte == b'_' || byte.is_ascii_alphanumeric();
		match (start, identifier) {
			(None, true) if byte == b'_' || byte.is_ascii_alphabetic() => start = Some(index),
			(Some(token_start), false) => {
				tokens.insert(source[token_start..index].to_vec());
				start = None;
			}
			_ => {}
		}
	}
	if let Some(token_start) = start {
		tokens.insert(source[token_start..].to_vec());
	}
	tokens
}

fn collect_c_transformed_macro_arguments(
	linkage: &LinkageRefiner<'_>,
	visibility: &CIncludeVisibility,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	selection: DecisionSelection<'_>,
) -> FxHashMap<crate::snapshot::SymbolId, Vec<(u32, u32)>> {
	let mut transformed = FxHashMap::<crate::snapshot::SymbolId, Vec<(u32, u32)>>::default();
	let mut structural_macros = FxHashMap::<Moniker, Vec<usize>>::default();
	let mut pending_macro_calls = Vec::new();
	for &decision_slot in selection.indices() {
		let decision = &decisions[decision_slot];
		let reference = &references[decision.reference_idx()];
		if reference.kind != "calls" {
			continue;
		}
		if !selection.includes(decision.reference()) {
			continue;
		}
		let Some(location) = linkage.locations.get(decision.reference_idx()) else {
			continue;
		};
		let Some(file) = linkage.material.files.get(location.source_file) else {
			continue;
		};
		let Some(range) = file.graph.ref_at(location.reference).position else {
			continue;
		};
		let argument_ranges = c_call_argument_ranges(&file.source, range);
		let mut targets = SymbolSet::new();
		if let Some(linked) = decision.linkage_targets() {
			for target in linked.iter() {
				targets.insert(target);
			}
		}
		let visible_macros = reference
			.call_name
			.as_deref()
			.map(|name| {
				compatible_macro_candidates(
					linkage,
					visibility.macros_named(
						location.source_file,
						name.as_bytes(),
						linkage.candidates,
					),
					argument_ranges.len(),
				)
			})
			.unwrap_or_default();
		for target in visible_macros.iter() {
			targets.insert(target);
		}
		if let Some(reference_idx) = decision.refinement_pending_reference_idx()
			&& !visible_macros.is_empty()
		{
			pending_macro_calls.push((decision_slot, reference_idx, reference.id, visible_macros));
		}
		let mut structural_arguments = FxHashSet::default();
		for target in targets.iter() {
			let Some(candidate) = linkage.candidates.candidate(target) else {
				continue;
			};
			let indexes = structural_macros
				.entry(candidate.moniker.clone())
				.or_insert_with(|| macro_structural_parameters(linkage.material, &candidate));
			structural_arguments.extend(indexes.iter().copied());
		}
		if structural_arguments.is_empty() {
			continue;
		}
		let ranges = transformed.entry(reference.source_symbol).or_default();
		for argument in structural_arguments {
			if let Some(range) = argument_ranges.get(argument) {
				ranges.push(*range);
			}
		}
	}
	for (decision_slot, reference_idx, reference_id, candidates) in pending_macro_calls {
		decisions[decision_slot] = ReferenceLinkageDecision::dynamic(
			crate::snapshot::DynamicReason::PreprocessorExpansion,
			reference_idx,
			reference_id,
			candidates,
		);
	}
	transformed
}

fn classify_c_pending_macro_tokens(
	linkage: &LinkageRefiner<'_>,
	visibility: &CIncludeVisibility,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	transformed_call_ranges: &FxHashMap<crate::snapshot::SymbolId, Vec<(u32, u32)>>,
	expanded_macro_tokens: &FxHashMap<crate::snapshot::SymbolId, Vec<ExpandedMacroTokens>>,
	selection: DecisionSelection<'_>,
) {
	for &decision_idx in selection.indices() {
		let decision = &mut decisions[decision_idx];
		let Some(reference_idx) = decision.refinement_pending_reference_idx() else {
			continue;
		};
		if !selection.includes(decision.reference()) {
			continue;
		}
		let reference = &references[reference_idx];
		if !matches!(reference.kind.as_str(), "reads" | "uses_type")
			|| reference.confidence.as_deref() != Some("name_match")
		{
			continue;
		}
		let Some(location) = linkage.locations.get(reference_idx) else {
			continue;
		};
		let Some(file) = linkage.material.files.get(location.source_file) else {
			continue;
		};
		let raw_reference = file.graph.ref_at(location.reference);
		let Some(read_range) = raw_reference.position else {
			continue;
		};
		if reference.kind == "uses_type"
			&& let Some(invocation_range) = c_invocation_range(&file.source, read_range)
			&& let Some(name) = raw_reference
				.target
				.as_view()
				.segments()
				.last()
				.map(|segment| segment.name)
		{
			let arguments = c_call_argument_ranges(&file.source, invocation_range);
			let candidates = compatible_macro_candidates(
				linkage,
				visibility.macros_named(location.source_file, name, linkage.candidates),
				arguments.len(),
			);
			if !candidates.is_empty() {
				*decision = ReferenceLinkageDecision::dynamic(
					crate::snapshot::DynamicReason::PreprocessorExpansion,
					reference_idx,
					reference.id,
					candidates,
				);
				continue;
			}
		}
		let in_token_macro = transformed_call_ranges
			.get(&reference.source_symbol)
			.is_some_and(|ranges| {
				ranges
					.iter()
					.any(|range| read_range.0 >= range.0 && read_range.1 <= range.1)
			});
		let expanded_by_prior_macro = raw_reference
			.target
			.as_view()
			.segments()
			.last()
			.is_some_and(|target| {
				expanded_macro_tokens
					.get(&reference.source_symbol)
					.is_some_and(|macros| {
						macros.iter().any(|expanded| {
							expanded.invocation_end <= read_range.0
								&& expanded.tokens.contains(target.name)
						})
					})
			});
		if !in_token_macro && !expanded_by_prior_macro {
			continue;
		}
		*decision = ReferenceLinkageDecision::dynamic(
			crate::snapshot::DynamicReason::PreprocessorExpansion,
			reference_idx,
			reference.id,
			SymbolSet::new(),
		);
	}
}

fn compatible_macro_candidates(
	linkage: &LinkageRefiner<'_>,
	candidates: SymbolSet,
	actual_arity: usize,
) -> SymbolSet {
	let mut compatible = SymbolSet::new();
	for symbol in candidates.iter() {
		if linkage
			.candidates
			.candidate(symbol)
			.is_some_and(|candidate| {
				macro_accepts_arity(linkage.material, &candidate, actual_arity)
			}) {
			compatible.insert(symbol);
		}
	}
	compatible
}

fn macro_accepts_arity(
	material: &CodeIndexMaterial,
	candidate: &crate::linkage::catalog::LinkageCandidate<'_>,
	actual_arity: usize,
) -> bool {
	let Some(expected_arity) = candidate.call_arity else {
		return false;
	};
	if actual_arity <= expected_arity {
		return actual_arity == expected_arity;
	}
	let Some(file) = material.files.get(candidate.source_file) else {
		return false;
	};
	let Some(definition) = file
		.graph
		.defs()
		.find(|definition| definition.moniker == *candidate.moniker)
	else {
		return false;
	};
	let Some((start, _)) = definition.position else {
		return false;
	};
	let Some(tail) = file.source.get(start as usize..) else {
		return false;
	};
	let mut logical = String::new();
	for line in tail.lines() {
		logical.push_str(line);
		if !line.trim_end().ends_with('\\') {
			break;
		}
	}
	let variadic = logical
		.split_once(')')
		.is_some_and(|(parameters, _)| macro_parameter_list_is_variadic(parameters));
	macro_arity_is_compatible(expected_arity, variadic, actual_arity)
}

pub(super) fn macro_parameter_list_is_variadic(parameters: &str) -> bool {
	mask_c_literals_and_comments(parameters).contains("...")
}

pub(super) fn macro_arity_is_compatible(expected: usize, variadic: bool, actual: usize) -> bool {
	actual == expected || (variadic && actual >= expected)
}

fn macro_structural_parameters(
	material: &CodeIndexMaterial,
	candidate: &crate::linkage::catalog::LinkageCandidate<'_>,
) -> Vec<usize> {
	if candidate
		.last_segment
		.is_none_or(|segment| segment.kind != b"macro")
	{
		return Vec::new();
	}
	let Some(file) = material.files.get(candidate.source_file) else {
		return Vec::new();
	};
	let Some(definition) = file
		.graph
		.defs()
		.find(|definition| definition.moniker == *candidate.moniker)
	else {
		return Vec::new();
	};
	macro_definition_structural_parameters(file, definition)
}

fn macro_definition_structural_parameters(
	file: &crate::source::IndexedSourceFile,
	definition: &DefRecord,
) -> Vec<usize> {
	let Some((start, _)) = definition.position else {
		return Vec::new();
	};
	let Some(tail) = file.source.get(start as usize..) else {
		return Vec::new();
	};
	let mut logical = String::new();
	for line in tail.lines() {
		logical.push_str(line);
		if !line.trim_end().ends_with('\\') {
			break;
		}
	}
	let body = logical
		.find(')')
		.and_then(|end| logical.get(end + 1..))
		.unwrap_or("");
	let parameters = definition
		.signature
		.split(|byte| *byte == b',')
		.filter(|parameter| !parameter.is_empty())
		.map(|parameter| String::from_utf8_lossy(parameter).into_owned())
		.collect::<Vec<_>>();
	macro_body_structural_parameters(body, &parameters)
}

pub(super) fn macro_body_structural_parameters(body: &str, parameters: &[String]) -> Vec<usize> {
	let body = mask_c_literals_and_comments(body);
	let bytes = body.as_bytes();
	let mut structural = FxHashSet::default();
	let mut index = 0;
	while index < bytes.len() {
		match bytes[index] {
			b'#' if bytes.get(index + 1) == Some(&b'#') => {
				if let Some(identifier) = identifier_before(bytes, index) {
					mark_structural_parameter(identifier, parameters, &mut structural);
				}
				if let Some(identifier) = identifier_after(bytes, index + 2) {
					mark_structural_parameter(identifier, parameters, &mut structural);
				}
				index += 1;
			}
			b'#' | b'.' => {
				if let Some(identifier) = identifier_after(bytes, index + 1) {
					mark_structural_parameter(identifier, parameters, &mut structural);
				}
			}
			b'-' if bytes.get(index + 1) == Some(&b'>') => {
				if let Some(identifier) = identifier_after(bytes, index + 2) {
					mark_structural_parameter(identifier, parameters, &mut structural);
				}
			}
			_ => {}
		}
		index += 1;
	}
	let mut structural = structural.into_iter().collect::<Vec<_>>();
	structural.sort_unstable();
	structural
}

fn mark_structural_parameter(
	identifier: &str,
	parameters: &[String],
	structural: &mut FxHashSet<usize>,
) {
	if let Some(index) = parameters
		.iter()
		.position(|parameter| parameter == identifier)
	{
		structural.insert(index);
	}
}

fn identifier_after(bytes: &[u8], mut index: usize) -> Option<&str> {
	while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
		index += 1;
	}
	let start = index;
	if !bytes
		.get(index)
		.is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
	{
		return None;
	}
	index += 1;
	while bytes
		.get(index)
		.is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
	{
		index += 1;
	}
	std::str::from_utf8(&bytes[start..index]).ok()
}

fn identifier_before(bytes: &[u8], mut index: usize) -> Option<&str> {
	while index > 0 && bytes[index - 1].is_ascii_whitespace() {
		index -= 1;
	}
	let end = index;
	while index > 0 && (bytes[index - 1].is_ascii_alphanumeric() || bytes[index - 1] == b'_') {
		index -= 1;
	}
	if index == end {
		return None;
	}
	std::str::from_utf8(&bytes[index..end]).ok()
}

fn c_invocation_range(source: &str, range: (u32, u32)) -> Option<(u32, u32)> {
	let start = range.0 as usize;
	let end = (range.1 as usize).min(source.len());
	let search_end = start.saturating_add(16 * 1024).min(source.len());
	let tail = source.get(start..search_end)?;
	let open = source
		.get(start..end)
		.and_then(|reference| reference.find('('))
		.or_else(|| {
			let after = source.get(end..search_end)?;
			let whitespace = after.len() - after.trim_start().len();
			(after.as_bytes().get(whitespace) == Some(&b'(')).then_some(end - start + whitespace)
		})?;
	let masked = mask_c_literals_and_comments(tail);
	let mut depth = 0usize;
	for (index, byte) in masked.as_bytes().iter().enumerate().skip(open) {
		match byte {
			b'(' => depth += 1,
			b')' => {
				depth = depth.saturating_sub(1);
				if depth == 0 {
					return Some((start as u32, (start + index + 1) as u32));
				}
			}
			_ => {}
		}
	}
	None
}

pub(super) fn c_call_argument_ranges(source: &str, call_range: (u32, u32)) -> Vec<(u32, u32)> {
	let call_start = call_range.0 as usize;
	let call_end = (call_range.1 as usize).min(source.len());
	let Some(call) = source.get(call_start..call_end) else {
		return Vec::new();
	};
	let masked = mask_c_literals_and_comments(call);
	let bytes = masked.as_bytes();
	let Some(open) = bytes.iter().position(|byte| *byte == b'(') else {
		return Vec::new();
	};
	let mut ranges = Vec::new();
	let mut argument_start = open + 1;
	let mut parens = 1usize;
	for index in open + 1..bytes.len() {
		match bytes[index] {
			b'(' => parens += 1,
			b')' if parens == 1 => {
				push_c_argument_range(&mut ranges, bytes, call_start, argument_start, index);
				break;
			}
			b')' => parens = parens.saturating_sub(1),
			b',' if parens == 1 => {
				push_c_argument_range(&mut ranges, bytes, call_start, argument_start, index);
				argument_start = index + 1;
			}
			_ => {}
		}
	}
	ranges
}

fn push_c_argument_range(
	ranges: &mut Vec<(u32, u32)>,
	bytes: &[u8],
	call_start: usize,
	mut start: usize,
	mut end: usize,
) {
	while start < end && bytes[start].is_ascii_whitespace() {
		start += 1;
	}
	while end > start && bytes[end - 1].is_ascii_whitespace() {
		end -= 1;
	}
	if start < end {
		ranges.push(((call_start + start) as u32, (call_start + end) as u32));
	}
}

fn mask_c_literals_and_comments(source: &str) -> String {
	#[derive(Clone, Copy)]
	enum State {
		Code,
		Quoted(u8),
		LineComment,
		BlockComment,
	}
	let bytes = source.as_bytes();
	let mut masked = bytes.to_vec();
	let mut state = State::Code;
	let mut index = 0;
	while index < bytes.len() {
		match state {
			State::Code if matches!(bytes[index], b'\'' | b'"') => {
				state = State::Quoted(bytes[index]);
				masked[index] = b' ';
			}
			State::Code if bytes[index..].starts_with(b"//") => {
				state = State::LineComment;
				masked[index] = b' ';
			}
			State::Code if bytes[index..].starts_with(b"/*") => {
				state = State::BlockComment;
				masked[index] = b' ';
			}
			State::Quoted(quote) => {
				masked[index] = b' ';
				if bytes[index] == b'\\' && index + 1 < bytes.len() {
					index += 1;
					masked[index] = b' ';
				} else if bytes[index] == quote {
					state = State::Code;
				}
			}
			State::LineComment => masked[index] = b' ',
			State::BlockComment => {
				masked[index] = b' ';
				if bytes[index..].starts_with(b"*/") {
					state = State::Code;
				}
			}
			State::Code => {}
		}
		index += 1;
	}
	String::from_utf8(masked).unwrap_or_default()
}

pub(in crate::linkage) fn refine_c_include_visibility(
	linkage: &LinkageRefiner<'_>,
	visibility: &CIncludeVisibility,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	selection: DecisionSelection<'_>,
) {
	for &decision_idx in selection.indices() {
		let decision = &mut decisions[decision_idx];
		let Some(reference_idx) = decision.refinement_pending_reference_idx() else {
			continue;
		};
		if !selection.includes(decision.reference()) {
			continue;
		}
		let reference = &references[reference_idx];
		let Some(location) = linkage.locations.get(reference_idx) else {
			continue;
		};
		let Some(query) =
			crate::linkage::catalog::LinkageQuery::at(reference, linkage.material, location)
		else {
			continue;
		};
		let targets = visibility.candidates(&query, linkage.candidates);
		if targets.is_empty() {
			continue;
		}
		*decision = ReferenceLinkageDecision::resolved(ResolutionDecision::new(
			ResolutionScope::Global,
			ResolutionEvidence::GlobalBinding,
			reference.id,
			reference_idx,
			targets,
		));
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn macro_structural_detection_ignores_literals_and_comments() {
		let parameters = vec!["token".to_string(), "value".to_string()];

		assert!(
			macro_body_structural_parameters(
				"printf(\"# .value ->value\", value) /* #value */",
				&parameters,
			)
			.is_empty()
		);
		assert_eq!(
			macro_body_structural_parameters("TOKEN_##token + value", &parameters),
			vec![0]
		);
		assert_eq!(
			macro_body_structural_parameters("# value", &parameters),
			vec![1]
		);
		assert_eq!(
			macro_body_structural_parameters("record->value", &parameters),
			vec![1]
		);
	}

	#[test]
	fn macro_argument_ranges_preserve_nested_argument_boundaries() {
		let source = "MIXED(FAST, call(ordinary, 2))";

		assert_eq!(
			c_call_argument_ranges(source, (0, source.len() as u32)),
			vec![(6, 10), (12, 29)]
		);
	}

	#[test]
	fn macro_arity_rejects_fixed_mismatches_and_accepts_variadics() {
		assert!(!macro_arity_is_compatible(1, false, 2));
		assert!(!macro_arity_is_compatible(2, false, 1));
		assert!(!macro_arity_is_compatible(2, true, 1));
		assert!(macro_arity_is_compatible(1, true, 3));
		assert!(!macro_parameter_list_is_variadic("x /* ... */"));
		assert!(macro_parameter_list_is_variadic("x, ..."));
	}

	#[test]
	fn macro_commas_in_brackets_still_split_preprocessor_arguments() {
		let source = "ONE(array[i, j])";

		assert_eq!(
			c_call_argument_ranges(source, (0, source.len() as u32)),
			vec![(4, 11), (13, 15)]
		);
	}
}
