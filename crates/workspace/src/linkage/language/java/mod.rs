use code_moniker_core::core::moniker::{Moniker, Segment};
use code_moniker_core::lang::{build_manifest::Manifest, kinds};
use rayon::prelude::*;
use rustc_hash::FxHashSet;

use crate::linkage::binding::ReferenceLinkageDecision;
use crate::linkage::catalog::LinkageQuery;
use crate::linkage::catalog::{CandidateCatalog, LinkageCandidate};
use crate::linkage::language::LanguageLinkageStrategy;
use crate::snapshot::{ReferenceId, ReferenceRecord};
use crate::source::CodeIndexMaterial;

mod lombok;

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
	if query.target_segment_count != candidate.segment_count || query.target_segment_count == 0 {
		return false;
	}
	query
		.target_segments()
		.zip(candidate.moniker.as_view().segments())
		.all(|(target, candidate_segment)| {
			java_segment_matches(query, candidate, target, candidate_segment)
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
		&& (is_java_type_kind(candidate_segment.kind)
			|| is_java_static_value_kind(candidate_segment.kind))
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

fn is_java_static_value_kind(kind: &[u8]) -> bool {
	matches!(kind, kinds::ENUM_CONSTANT | kinds::FIELD)
}

fn is_java_source_set_segment(segment: Segment<'_>) -> bool {
	segment.kind == b"srcset"
}

fn java_source_set_can_read(source_set: &[u8], candidate_source_set: &[u8]) -> bool {
	source_set == candidate_source_set || (source_set == b"test" && candidate_source_set == b"main")
}

pub(super) fn package_prefix(target: &Moniker) -> Option<String> {
	let mut pieces = Vec::new();
	let mut in_java = false;
	for segment in target.as_view().segments() {
		if segment.kind == kinds::LANG && segment.name == b"java" {
			in_java = true;
			continue;
		}
		if !in_java {
			continue;
		}
		if segment.kind == kinds::PACKAGE {
			pieces.push(std::str::from_utf8(segment.name).ok()?);
		} else if !pieces.is_empty() {
			break;
		}
	}
	(!pieces.is_empty()).then(|| pieces.join("."))
}

pub(super) fn builtin_external_root(root: &str) -> bool {
	root == "java"
}

pub(super) fn source_declares_external_package(
	manifest: Manifest,
	deps: &FxHashSet<String>,
	package_prefix: &str,
	query_confidence: Option<&str>,
	workspace_declares_package: impl Fn(&str) -> bool,
) -> bool {
	if manifest != Manifest::PomXml {
		return false;
	}
	let has_external_dependency = deps.iter().any(|dep| !workspace_declares_package(dep));
	if query_confidence == Some(confidence(kinds::CONF_IMPORTED)) && has_external_dependency {
		return true;
	}
	deps.iter().any(|dep| {
		!workspace_declares_package(dep)
			&& dependency_group(dep).is_some_and(|group| {
				package_prefix == group
					|| package_prefix
						.strip_prefix(group)
						.is_some_and(|tail| tail.starts_with('.'))
			})
	})
}

fn dependency_group(package: &str) -> Option<&str> {
	let coord = package
		.strip_prefix(Manifest::PomXml.tag())?
		.strip_prefix('\0')?;
	coord.split_once(':').map(|(group, _)| group)
}

pub(super) fn enhance_reference_semantics(
	material: &CodeIndexMaterial,
	candidates: &CandidateCatalog<'_>,
	decisions: &mut [ReferenceLinkageDecision],
	references: &[ReferenceRecord],
	changed_references: Option<&FxHashSet<ReferenceId>>,
) {
	let lombok = lombok::LombokSemantics::build(material, candidates, references);
	if lombok.is_empty() {
		return;
	}
	let replacements = decisions
		.par_iter()
		.enumerate()
		.filter_map(|(idx, decision)| {
			if changed_references.is_some_and(|changed| !changed.contains(decision.reference())) {
				return None;
			}
			lombok
				.resolve_reference(decision, references)
				.map(|replacement| (idx, replacement))
		})
		.collect::<Vec<_>>();
	for (idx, replacement) in replacements {
		decisions[idx] = replacement;
	}
}

fn confidence(value: &[u8]) -> &str {
	std::str::from_utf8(value).expect("confidence constants are utf-8")
}
