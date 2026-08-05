use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;
use code_moniker_core::lang::build_manifest::Manifest;
use rustc_hash::FxHashSet;

use crate::linkage::binding::ReferenceLinkageDecision;
use crate::linkage::catalog::LinkageCandidate;
use crate::linkage::catalog::LinkageQuery;
use crate::snapshot::{RecordTable, ReferenceId, ReferenceRecord};
use crate::source::CodeIndexMaterial;

mod c;
mod csharp;
mod go;
mod java;
mod python;
mod rust;
mod selection;
mod sql;
mod ts;

pub(in crate::linkage) use c::{
	CIncludeVisibility, classify_c_preprocessor_tokens, classify_c_unindexed_external_dependencies,
	refine_c_include_visibility,
};
pub(in crate::linkage) use python::binding_invalidation_sources;
pub(in crate::linkage) use python::{BindingTarget, PythonBindings};
pub(in crate::linkage) use selection::{
	confirm_name_match_targets, global_resolution_evidence, local_resolution_evidence,
	prefer_concrete_definitions,
};

pub(super) fn matches_candidate(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	let Some(source) = query.material.files.get(query.source_file) else {
		return false;
	};
	let Some(target) = query.material.files.get(candidate.source_file) else {
		return false;
	};
	if source.lang != target.lang {
		return false;
	}
	match source.lang {
		Lang::Java => java::matches(query, candidate),
		Lang::Python => python::matches(query, candidate),
		Lang::Rs => rust::matches(query, candidate),
		Lang::Ts => ts::matches(query, candidate),
		Lang::Go => go::matches(query, candidate),
		Lang::C => c::matches(query, candidate),
		Lang::Cs => csharp::matches(query, candidate),
		Lang::Sql => sql::matches(query, candidate),
	}
}

fn generic_matches(query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
	candidate.moniker.bind_match(query.target) || query.target.bind_match(candidate.moniker)
}

pub(super) fn sql_call_has_strong_evidence(query: &LinkageQuery<'_>) -> bool {
	sql::call_has_strong_evidence(query)
}

pub(super) fn manifest_for_lang(lang: Lang) -> Option<Manifest> {
	match lang {
		Lang::Ts => Some(Manifest::PackageJson),
		Lang::Rs => Some(Manifest::Cargo),
		Lang::Java => Some(Manifest::PomXml),
		Lang::Python => Some(Manifest::Pyproject),
		Lang::Go => Some(Manifest::GoMod),
		Lang::Cs => Some(Manifest::Csproj),
		Lang::C | Lang::Sql => None,
	}
}

pub(super) fn package_prefix_for_target(lang: Lang, target: &Moniker) -> Option<String> {
	match lang {
		Lang::Java => java::package_prefix(target),
		Lang::Ts => ts::package_prefix(target),
		_ => None,
	}
}

pub(super) fn source_declares_external_package(
	lang: Lang,
	manifest: Manifest,
	deps: &FxHashSet<String>,
	package_prefix: &str,
	query_confidence: Option<&str>,
	workspace_declares_package: impl Fn(&str) -> bool,
) -> bool {
	match lang {
		Lang::Java => java::source_declares_external_package(
			manifest,
			deps,
			package_prefix,
			query_confidence,
			workspace_declares_package,
		),
		Lang::Ts => ts::source_declares_external_package(
			manifest,
			deps,
			package_prefix,
			query_confidence,
			workspace_declares_package,
		),
		_ => false,
	}
}

pub(super) fn proc_macro_annotation(query: &LinkageQuery<'_>) -> bool {
	rust::proc_macro_annotation(query)
}

pub(super) fn rust_external_crate_target_matches_def(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
	lib_path: &std::path::Path,
) -> bool {
	rust::external_crate_target_matches_def(query, candidate, lib_path)
}

pub(super) fn rust_sdk_callable_fallback(query: &LinkageQuery<'_>) -> Option<Moniker> {
	rust::sdk_callable_fallback(query)
}

pub(super) fn classify_open_references(
	material: &CodeIndexMaterial,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	changed_references: Option<&FxHashSet<ReferenceId>>,
) {
	for decision in decisions {
		let Some(reference_idx) = decision.refinement_pending_reference_idx() else {
			continue;
		};
		if changed_references.is_some_and(|changed| !changed.contains(decision.reference())) {
			continue;
		}
		let reference = &references[reference_idx];
		match reference_language(material, reference) {
			Some(Lang::Python) => {
				python::classify_open_reference(material, decision, reference_idx, reference)
			}
			Some(Lang::Cs) => {
				csharp::classify_open_reference(material, decision, reference_idx, reference)
			}
			Some(Lang::Sql) => sql::classify_open_reference(decision, reference_idx, reference),
			_ => {}
		}
	}
}

pub(super) fn refine_external_reexports(
	material: &CodeIndexMaterial,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	changed_references: Option<&FxHashSet<ReferenceId>>,
	decision_indices: &[usize],
) {
	ts::refine_external_reexports(
		material,
		decisions,
		references,
		changed_references,
		decision_indices,
	);
}

pub(super) fn classify_runtime_imports(
	material: &CodeIndexMaterial,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	changed_references: Option<&FxHashSet<ReferenceId>>,
	decision_indices: &[usize],
) {
	python::classify_runtime_imports(
		material,
		decisions,
		references,
		changed_references,
		decision_indices,
	);
}

fn reference_language(material: &CodeIndexMaterial, reference: &ReferenceRecord) -> Option<Lang> {
	material
		.symbol_moniker(&reference.source_symbol)
		.and_then(|source| {
			source
				.as_view()
				.segments()
				.find(|segment| segment.kind == code_moniker_core::lang::kinds::LANG)
				.and_then(|segment| std::str::from_utf8(segment.name).ok())
				.and_then(Lang::from_tag)
		})
}

fn external_target_shape(target: &Moniker) -> bool {
	target.as_view().segments().next().is_some_and(|segment| {
		matches!(
			segment.kind,
			code_moniker_core::lang::kinds::EXTERNAL_PKG | code_moniker_core::lang::kinds::SDK
		)
	})
}
