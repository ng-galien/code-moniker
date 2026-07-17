use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::lang::kinds;

use crate::linkage::catalog::{LinkageCandidate, LinkageQuery};
use crate::linkage::language::{LanguageLinkageStrategy, generic::GenericLanguageLinkageStrategy};

pub(super) struct SqlLanguageLinkageStrategy;

impl LanguageLinkageStrategy for SqlLanguageLinkageStrategy {
	fn matches(&self, query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
		if sql_callable_query(query) {
			return sql_callable_matches(query, candidate);
		}
		GenericLanguageLinkageStrategy.matches(query, candidate)
	}
}

fn sql_callable_query(query: &LinkageQuery<'_>) -> bool {
	query
		.target_last
		.is_some_and(|segment| matches!(segment.kind, kinds::FUNCTION | b"procedure"))
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
		) && query_schema(query).is_none_or(|schema| {
		candidate_schema(candidate).is_some_and(|candidate| identifier_matches(schema, candidate))
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

fn call_arity_matches(call: Option<usize>, required: Option<usize>, callable_name: &[u8]) -> bool {
	match (call, required, callable_slot_count(callable_name)) {
		(Some(call), Some(required), Some(maximum)) => required <= call && call <= maximum,
		_ => false,
	}
}

fn callable_slot_count(name: &[u8]) -> Option<usize> {
	let open = name.iter().position(|byte| *byte == b'(')?;
	let body = name.get(open + 1..name.len().checked_sub(1)?)?;
	if body.is_empty() {
		return Some(0);
	}
	let mut count = 1;
	let mut depth = 0usize;
	let mut quoted = false;
	for byte in body {
		match *byte {
			b'"' => quoted = !quoted,
			b'(' if !quoted => depth += 1,
			b')' if !quoted => depth = depth.saturating_sub(1),
			b',' if !quoted && depth == 0 => count += 1,
			_ => {}
		}
	}
	Some(count)
}

pub(super) fn builtin_external_root(root: &str) -> bool {
	root == "pg_catalog"
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
}
