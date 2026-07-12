use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;
use code_moniker_core::lang::build_manifest::Manifest;
use rustc_hash::FxHashSet;

use crate::linkage::binding::ReferenceLinkageDecision;
use crate::linkage::catalog::LinkageQuery;
use crate::linkage::catalog::{CandidateCatalog, LinkageCandidate};
use crate::snapshot::{RecordTable, ReferenceRecord};
use crate::source::CodeIndexMaterial;

mod generic;
mod java;
mod python;
mod rust;
mod ts;

pub(super) trait LanguageLinkageStrategy: Sync {
	fn matches(&self, query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool;
}

static GENERIC_STRATEGY: generic::GenericLanguageLinkageStrategy =
	generic::GenericLanguageLinkageStrategy;
static JAVA_STRATEGY: java::JavaLanguageLinkageStrategy = java::JavaLanguageLinkageStrategy;
static PYTHON_STRATEGY: python::PythonLanguageLinkageStrategy =
	python::PythonLanguageLinkageStrategy;
static RUST_STRATEGY: rust::RustLanguageLinkageStrategy = rust::RustLanguageLinkageStrategy;
static TS_STRATEGY: ts::TsLanguageLinkageStrategy = ts::TsLanguageLinkageStrategy;

pub(super) fn language_strategy(lang: Lang) -> &'static dyn LanguageLinkageStrategy {
	match lang {
		Lang::Java => &JAVA_STRATEGY,
		Lang::Python => &PYTHON_STRATEGY,
		Lang::Rs => &RUST_STRATEGY,
		Lang::Ts => &TS_STRATEGY,
		Lang::Go | Lang::Cs | Lang::Sql => &GENERIC_STRATEGY,
	}
}

pub(super) fn matches_candidate(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	query
		.material
		.files
		.get(query.source_file)
		.is_some_and(|file| language_strategy(file.lang).matches(query, candidate))
}

pub(super) fn manifest_for_lang(lang: Lang) -> Option<Manifest> {
	match lang {
		Lang::Ts => Some(Manifest::PackageJson),
		Lang::Rs => Some(Manifest::Cargo),
		Lang::Java => Some(Manifest::PomXml),
		Lang::Python => Some(Manifest::Pyproject),
		Lang::Go => Some(Manifest::GoMod),
		Lang::Cs => Some(Manifest::Csproj),
		Lang::Sql => None,
	}
}

pub(super) fn builtin_external_root(lang: Lang, root: &str) -> bool {
	match lang {
		Lang::Rs => rust::builtin_external_root(root),
		Lang::Java => java::builtin_external_root(root),
		Lang::Ts => ts::builtin_external_root(root),
		_ => false,
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

pub(super) struct SemanticContext<'a> {
	pub(super) material: &'a CodeIndexMaterial,
	pub(super) candidates: &'a CandidateCatalog,
	pub(super) locations: &'a crate::linkage::catalog::ReferenceLocations,
	pub(super) source_groups: &'a crate::linkage::source_groups::SourceGroupPolicy,
}

pub(super) fn enhance_reference_semantics(
	context: &SemanticContext<'_>,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	changed_references: Option<&rustc_hash::FxHashSet<crate::snapshot::ReferenceId>>,
) {
	java::enhance_reference_semantics(context, decisions, references, changed_references);
}
