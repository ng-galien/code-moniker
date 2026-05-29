use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;
use code_moniker_core::lang::build_manifest::Manifest;
use rustc_hash::FxHashSet;

use crate::linkage::candidate::LinkageCandidate;
use crate::linkage::decision::ReferenceLinkageDecision;
use crate::linkage::query::LinkageQuery;
use crate::snapshot::ReferenceRecord;
use crate::source::CodeIndexMaterial;

mod generic;
mod java;
mod rust;

pub(super) trait LanguageLinkageStrategy: Sync {
	fn matches(&self, query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool;
}

static GENERIC_STRATEGY: generic::GenericLanguageLinkageStrategy =
	generic::GenericLanguageLinkageStrategy;
static JAVA_STRATEGY: java::JavaLanguageLinkageStrategy = java::JavaLanguageLinkageStrategy;
static RUST_STRATEGY: rust::RustLanguageLinkageStrategy = rust::RustLanguageLinkageStrategy;

pub(super) fn language_strategy(lang: Lang) -> &'static dyn LanguageLinkageStrategy {
	match lang {
		Lang::Java => &JAVA_STRATEGY,
		Lang::Rs => &RUST_STRATEGY,
		Lang::Ts | Lang::Python | Lang::Go | Lang::Cs | Lang::Sql => &GENERIC_STRATEGY,
	}
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
		_ => false,
	}
}

pub(super) fn package_prefix_for_target(lang: Lang, target: &Moniker) -> Option<String> {
	match lang {
		Lang::Java => java::package_prefix(target),
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
		_ => false,
	}
}

pub(super) fn proc_macro_annotation(query: &LinkageQuery<'_>) -> bool {
	rust::proc_macro_annotation(query)
}

pub(super) fn enhance_reference_semantics(
	material: &CodeIndexMaterial,
	decisions: &mut [ReferenceLinkageDecision],
	references: &[ReferenceRecord],
) {
	java::enhance_reference_semantics(material, decisions, references);
}
