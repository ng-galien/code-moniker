use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::lang::kinds;

use crate::linkage::catalog::{LinkageCandidate, LinkageQuery};
use crate::linkage::language::generic_matches;
use crate::snapshot::{DynamicReason, RecordTable, ReferenceId, ReferenceRecord};
use crate::source::CodeIndexMaterial;

pub(super) fn matches(query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
	if sql_callable_query(query) {
		return sql_callable_matches(query, candidate);
	}
	if sql_object_query(query) {
		return sql_object_matches(query, candidate);
	}
	generic_matches(query, candidate)
}

fn sql_callable_query(query: &LinkageQuery<'_>) -> bool {
	query
		.target_last
		.is_some_and(|segment| matches!(segment.kind, kinds::FUNCTION | b"procedure"))
}

fn sql_object_query(query: &LinkageQuery<'_>) -> bool {
	query.target_last.is_some_and(|segment| {
		matches!(
			segment.kind,
			kinds::TABLE | kinds::VIEW | kinds::TYPE | kinds::COLUMN
		)
	})
}

fn sql_object_matches(query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
	let Some(target) = query.target_last else {
		return false;
	};
	let Some(candidate_segment) = candidate.last_segment else {
		return false;
	};
	if !sql_object_kinds_match(target.kind, candidate_segment.kind, query.reference_kind)
		|| !identifier_matches(target.name, candidate_segment.name)
	{
		return false;
	}
	if query_schema(query).is_some_and(|schema| {
		!candidate_schema(candidate).is_some_and(|candidate| identifier_matches(schema, candidate))
	}) {
		return false;
	}
	if target.kind == kinds::COLUMN {
		return relation_owner(query.target)
			.zip(relation_owner(candidate.moniker))
			.is_some_and(|(target, candidate)| identifier_matches(target, candidate));
	}
	true
}

fn sql_object_kinds_match(target: &[u8], candidate: &[u8], reference_kind: &str) -> bool {
	match target {
		kinds::TABLE => {
			candidate == kinds::TABLE || (reference_kind == "reads" && candidate == kinds::VIEW)
		}
		kinds::VIEW => candidate == kinds::VIEW,
		kinds::TYPE => candidate == kinds::TYPE,
		kinds::COLUMN => candidate == kinds::COLUMN,
		_ => false,
	}
}

fn relation_owner(moniker: &code_moniker_core::core::moniker::Moniker) -> Option<&[u8]> {
	let segments = moniker.as_view().segments().collect::<Vec<_>>();
	let [.., owner, column] = segments.as_slice() else {
		return None;
	};
	(column.kind == kinds::COLUMN && matches!(owner.kind, kinds::TABLE | kinds::VIEW))
		.then_some(owner.name)
}

fn sql_callable_matches(query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
	let Some(target) = query.target_last else {
		return false;
	};
	let Some(candidate_segment) = candidate.last_segment else {
		return false;
	};
	if candidate_segment.kind != target.kind {
		return false;
	}
	let target_name = query
		.call_name
		.map(str::as_bytes)
		.unwrap_or_else(|| bare_callable_name(target.name));
	let candidate_name = candidate
		.call_name
		.unwrap_or_else(|| bare_callable_name(candidate_segment.name));
	identifier_matches(target_name, candidate_name)
		&& call_arity_matches(
			query.call_arity,
			candidate.call_arity,
			candidate_segment.name,
		) && call_types_match(target.name, candidate_segment.name, candidate.call_arity)
		&& query_schema(query).is_none_or(|schema| {
			candidate_schema(candidate)
				.is_some_and(|candidate| identifier_matches(schema, candidate))
		})
}

pub(super) fn call_has_strong_evidence(query: &LinkageQuery<'_>) -> bool {
	let Some(target) = query.target_last else {
		return false;
	};
	query_schema(query).is_some()
		&& callable_slots(target.name).is_some_and(|slots| {
			slots
				.into_iter()
				.all(|slot| call_slot_type(slot).is_some_and(|r#type| r#type != b"_"))
		})
}

fn query_schema<'a>(query: &'a LinkageQuery<'_>) -> Option<&'a [u8]> {
	query
		.target_segments()
		.find(|segment| segment.kind == b"schema")
		.map(|segment| segment.name)
}

fn candidate_schema<'a>(candidate: &'a LinkageCandidate<'_>) -> Option<&'a [u8]> {
	candidate
		.moniker
		.as_view()
		.segments()
		.find(|segment| segment.kind == b"schema")
		.map(|segment| segment.name)
}

fn identifier_matches(left: &[u8], right: &[u8]) -> bool {
	left == right
}

pub(super) fn classify_open_references(
	material: &CodeIndexMaterial,
	decisions: &mut [crate::linkage::binding::ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	changed_references: Option<&rustc_hash::FxHashSet<ReferenceId>>,
) {
	for decision in decisions {
		let Some(reference_idx) = decision.semantic_pending_reference_idx() else {
			continue;
		};
		if changed_references.is_some_and(|changed| !changed.contains(decision.reference())) {
			continue;
		}
		let reference = &references[reference_idx];
		if !super::reference_is_language(material, reference, b"sql") {
			continue;
		}
		if reference.kind == "calls"
			&& decision
				.linkage_targets()
				.is_some_and(|targets| !targets.is_empty())
		{
			continue;
		}
		let reason = match reference.kind.as_str() {
			"calls" => Some(DynamicReason::ExternalDependencyUnindexed),
			"uses_type"
				if matches!(
					reference.confidence.as_deref(),
					Some("name_match" | "resolved")
				) =>
			{
				Some(DynamicReason::InsufficientLocalFacts)
			}
			_ => None,
		};
		let Some(reason) = reason else { continue };
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
}

fn call_arity_matches(call: Option<usize>, required: Option<usize>, callable_name: &[u8]) -> bool {
	match (call, required, callable_slot_count(callable_name)) {
		(Some(call), Some(required), Some(_)) if callable_is_variadic(callable_name) => {
			required <= call
		}
		(Some(call), Some(required), Some(maximum)) => required <= call && call <= maximum,
		_ => false,
	}
}

fn call_types_match(
	call_name: &[u8],
	candidate_name: &[u8],
	required_arity: Option<usize>,
) -> bool {
	let Some(call_slots) = callable_slots(call_name) else {
		return true;
	};
	let Some(candidate_slots) = callable_slots(candidate_name) else {
		return false;
	};
	let variadic = callable_is_variadic(candidate_name);
	let mut matched = vec![false; candidate_slots.len()];
	let mut named_argument_seen = false;
	for (position, call_slot) in call_slots.into_iter().enumerate() {
		let call_name = call_slot_name(call_slot);
		named_argument_seen |= call_name.is_some();
		if named_argument_seen && call_name.is_none() {
			return false;
		}
		let candidate_position = if let Some(name) = call_name {
			let Some(position) = candidate_slots
				.iter()
				.position(|slot| definition_slot_name(slot) == Some(name))
			else {
				return false;
			};
			position
		} else if position < candidate_slots.len() {
			position
		} else if variadic {
			candidate_slots.len().saturating_sub(1)
		} else {
			return false;
		};
		let expanded_variadic = variadic
			&& call_name.is_none()
			&& candidate_position == candidate_slots.len().saturating_sub(1);
		if matched[candidate_position] && !expanded_variadic {
			return false;
		}
		matched[candidate_position] = true;
		let Some(call_type) = call_slot_type(call_slot) else {
			continue;
		};
		if call_type == b"_" {
			continue;
		}
		let Some(mut candidate_type) = candidate_slots
			.get(candidate_position)
			.copied()
			.and_then(definition_slot_type)
		else {
			return false;
		};
		if candidate_type.ends_with(b"...") {
			candidate_type = &candidate_type[..candidate_type.len() - 3];
			if expanded_variadic && candidate_type.ends_with(b"[]") {
				candidate_type = &candidate_type[..candidate_type.len() - 2];
			}
		}
		if !sql_type_matches(call_type, candidate_type) {
			return false;
		}
	}
	matched
		.iter()
		.take(required_arity.unwrap_or(candidate_slots.len()))
		.all(|matched| *matched)
}

fn callable_slots(name: &[u8]) -> Option<Vec<&[u8]>> {
	let open = name.iter().position(|byte| *byte == b'(')?;
	let body = name.get(open + 1..name.len().checked_sub(1)?)?;
	if body.is_empty() {
		return Some(Vec::new());
	}
	let mut slots = Vec::new();
	let mut start = 0;
	let mut depth = 0usize;
	let mut quoted = false;
	for (index, byte) in body.iter().copied().enumerate() {
		match byte {
			b'"' => quoted = !quoted,
			b'(' | b'[' if !quoted => depth += 1,
			b')' | b']' if !quoted => depth = depth.saturating_sub(1),
			b',' if !quoted && depth == 0 => {
				slots.push(&body[start..index]);
				start = index + 1;
			}
			_ => {}
		}
	}
	slots.push(&body[start..]);
	Some(slots)
}

fn top_level_colon(slot: &[u8]) -> Option<usize> {
	let mut depth = 0usize;
	let mut quoted = false;
	for (index, byte) in slot.iter().copied().enumerate() {
		match byte {
			b'"' => quoted = !quoted,
			b'(' | b'[' if !quoted => depth += 1,
			b')' | b']' if !quoted => depth = depth.saturating_sub(1),
			b':' if !quoted && depth == 0 => return Some(index),
			_ => {}
		}
	}
	None
}

fn call_slot_name(slot: &[u8]) -> Option<&[u8]> {
	top_level_colon(slot).map(|colon| &slot[..colon])
}

fn call_slot_type(slot: &[u8]) -> Option<&[u8]> {
	top_level_colon(slot)
		.map(|colon| &slot[colon + 1..])
		.or(Some(slot))
}

fn definition_slot_name(slot: &[u8]) -> Option<&[u8]> {
	top_level_colon(slot).map(|colon| &slot[..colon])
}

fn definition_slot_type(slot: &[u8]) -> Option<&[u8]> {
	top_level_colon(slot)
		.map(|colon| &slot[colon + 1..])
		.or(Some(slot))
}

fn sql_type_matches(call: &[u8], candidate: &[u8]) -> bool {
	let call = canonical_type(call);
	let candidate = canonical_type(candidate);
	call == candidate || implicit_type_cast(&call, &candidate) || polymorphic_type(&candidate)
}

fn implicit_type_cast(call: &[u8], candidate: &[u8]) -> bool {
	matches!(
		(call, candidate),
		(b"int2", b"int4" | b"int8" | b"numeric")
			| (b"int4", b"int8" | b"numeric")
			| (b"int8", b"numeric")
			| (b"float4", b"float8")
	)
}

fn canonical_type(r#type: &[u8]) -> Vec<u8> {
	let mut normalized = r#type
		.iter()
		.map(u8::to_ascii_lowercase)
		.filter(|byte| !byte.is_ascii_whitespace())
		.collect::<Vec<_>>();
	if normalized.starts_with(b"pg_catalog.") {
		normalized.drain(..b"pg_catalog.".len());
	}
	let array = normalized.ends_with(b"[]");
	let base_end = if array {
		normalized.len() - 2
	} else {
		normalized.len()
	};
	let base = &normalized[..base_end];
	let base = base
		.iter()
		.position(|byte| *byte == b'(')
		.map(|open| &base[..open])
		.unwrap_or(base);
	let canonical = match base {
		b"int" | b"integer" => b"int4".as_slice(),
		b"bigint" => b"int8".as_slice(),
		b"smallint" => b"int2".as_slice(),
		b"boolean" => b"bool".as_slice(),
		b"real" => b"float4".as_slice(),
		b"doubleprecision" => b"float8".as_slice(),
		b"decimal" => b"numeric".as_slice(),
		b"charactervarying" => b"varchar".as_slice(),
		_ => base,
	};
	let mut out = canonical.to_vec();
	if array {
		out.extend_from_slice(b"[]");
	}
	out
}

fn polymorphic_type(candidate: &[u8]) -> bool {
	matches!(
		candidate,
		b"anyelement"
			| b"anyarray"
			| b"anynonarray"
			| b"anyenum"
			| b"anycompatible"
			| b"anycompatiblearray"
			| b"anycompatiblenonarray"
			| b"record"
	)
}

fn callable_slot_count(name: &[u8]) -> Option<usize> {
	callable_slots(name).map(|slots| slots.len())
}

fn callable_is_variadic(name: &[u8]) -> bool {
	callable_slots(name)
		.and_then(|slots| slots.last().copied())
		.and_then(definition_slot_type)
		.is_some_and(|r#type| r#type.ends_with(b"..."))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn default_arity_range_is_bounded_by_callable_slots() {
		let callable = b"finish(value:int4)";
		assert!(call_arity_matches(Some(0), Some(0), callable));
		assert!(call_arity_matches(Some(1), Some(0), callable));
		assert!(!call_arity_matches(Some(2), Some(0), callable));
	}

	#[test]
	fn callable_slot_count_ignores_nested_type_commas() {
		assert_eq!(
			callable_slot_count(b"pick(value:numeric(10,2),label:text)"),
			Some(2)
		);
	}

	#[test]
	fn variadic_calls_accept_expanded_arguments() {
		let callable = b"concat_all(items:text[]...)";
		assert!(call_arity_matches(Some(0), Some(0), callable));
		assert!(call_arity_matches(Some(3), Some(0), callable));
		assert!(call_types_match(
			b"concat_all(text,text,text)",
			callable,
			Some(0)
		));
		assert!(!call_types_match(b"concat_all(int4)", callable, Some(0)));
	}

	#[test]
	fn typed_calls_filter_same_arity_overloads() {
		assert!(call_types_match(
			b"pick(int4)",
			b"pick(value:int4)",
			Some(1)
		));
		assert!(!call_types_match(
			b"pick(int4)",
			b"pick(value:text)",
			Some(1)
		));
		assert!(call_types_match(b"pick(_)", b"pick(value:text)", Some(1)));
		assert!(call_types_match(
			b"pick(value:int4)",
			b"pick(value:int4,label:text)",
			Some(1)
		));
		assert!(call_types_match(
			b"pick(int4)",
			b"pick(value:int8)",
			Some(1)
		));
		assert!(!call_types_match(
			b"pick(other:int4)",
			b"pick(value:int4)",
			Some(1)
		));
		assert!(!call_types_match(
			b"pick(optional:int4)",
			b"pick(required:text,optional:int4)",
			Some(1)
		));
	}
}
