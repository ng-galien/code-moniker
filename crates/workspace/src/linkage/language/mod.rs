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
mod sql;
mod ts;

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

pub(super) fn c_include_matches_candidate(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	c::matches_include_candidate(query, candidate)
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

pub(super) fn rust_sdk_method_fallback(query: &LinkageQuery<'_>) -> Option<Moniker> {
	rust::sdk_method_fallback(query)
}

pub(super) fn classify_open_references(
	material: &CodeIndexMaterial,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	changed_references: Option<&FxHashSet<ReferenceId>>,
) {
	let mut python_present = false;
	let mut csharp_present = false;
	let mut sql_present = false;
	for file in &material.files {
		match file.lang {
			Lang::Python => python_present = true,
			Lang::Cs => csharp_present = true,
			Lang::Sql => sql_present = true,
			_ => {}
		}
	}
	if python_present {
		python::classify_open_references(material, decisions, references, changed_references);
	}
	if csharp_present {
		csharp::classify_open_references(material, decisions, references, changed_references);
	}
	if sql_present {
		sql::classify_open_references(material, decisions, references, changed_references);
	}
}

fn reference_is_language(
	material: &CodeIndexMaterial,
	reference: &ReferenceRecord,
	language: &[u8],
) -> bool {
	material
		.symbol_moniker(&reference.source_symbol)
		.is_some_and(|source| {
			source.as_view().segments().any(|segment| {
				segment.kind == code_moniker_core::lang::kinds::LANG && segment.name == language
			})
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
