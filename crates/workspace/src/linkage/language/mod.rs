use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;
use code_moniker_core::lang::build_manifest::Manifest;
use rustc_hash::FxHashSet;

use crate::linkage::binding::ReferenceLinkageDecision;
use crate::linkage::catalog::LinkageQuery;
use crate::linkage::catalog::{CandidateCatalog, LinkageCandidate};
use crate::snapshot::{RecordTable, ReferenceRecord};
use crate::source::CodeIndexMaterial;

mod c;
mod csharp;
mod generic;
mod go;
mod java;
mod python;
mod rust;
mod sql;
mod ts;

pub(super) trait LanguageLinkageStrategy: Sync {
	fn matches(&self, query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool;
}

static C_STRATEGY: c::CLanguageLinkageStrategy = c::CLanguageLinkageStrategy;
static CSHARP_STRATEGY: csharp::CsharpLanguageLinkageStrategy =
	csharp::CsharpLanguageLinkageStrategy;
static GO_STRATEGY: go::GoLanguageLinkageStrategy = go::GoLanguageLinkageStrategy;
static JAVA_STRATEGY: java::JavaLanguageLinkageStrategy = java::JavaLanguageLinkageStrategy;
static PYTHON_STRATEGY: python::PythonLanguageLinkageStrategy =
	python::PythonLanguageLinkageStrategy;
static RUST_STRATEGY: rust::RustLanguageLinkageStrategy = rust::RustLanguageLinkageStrategy;
static SQL_STRATEGY: sql::SqlLanguageLinkageStrategy = sql::SqlLanguageLinkageStrategy;
static TS_STRATEGY: ts::TsLanguageLinkageStrategy = ts::TsLanguageLinkageStrategy;

pub(super) fn language_strategy(lang: Lang) -> &'static dyn LanguageLinkageStrategy {
	match lang {
		Lang::Java => &JAVA_STRATEGY,
		Lang::Python => &PYTHON_STRATEGY,
		Lang::Rs => &RUST_STRATEGY,
		Lang::Ts => &TS_STRATEGY,
		Lang::Go => &GO_STRATEGY,
		Lang::C => &C_STRATEGY,
		Lang::Cs => &CSHARP_STRATEGY,
		Lang::Sql => &SQL_STRATEGY,
	}
}

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
	source.lang == target.lang && language_strategy(source.lang).matches(query, candidate)
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

pub(super) struct SemanticContext<'a> {
	pub(super) material: &'a CodeIndexMaterial,
	pub(super) candidates: &'a CandidateCatalog,
	pub(super) locations: &'a crate::linkage::catalog::ReferenceLocations,
	pub(super) source_groups: &'a crate::linkage::source_groups::SourceGroupPolicy,
}

pub(super) fn enhance_reference_semantics(
	context: &SemanticContext<'_>,
	extends_of: &rustc_hash::FxHashMap<
		code_moniker_core::core::moniker::Moniker,
		code_moniker_core::core::moniker::Moniker,
	>,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	changed_references: Option<&rustc_hash::FxHashSet<crate::snapshot::ReferenceId>>,
) {
	java::enhance_reference_semantics(
		context,
		extends_of,
		decisions,
		references,
		changed_references,
	);
}
