use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::lang::{build_manifest::Manifest, kinds};
use rustc_hash::FxHashSet;

use crate::linkage::catalog::LinkageCandidate;
use crate::linkage::catalog::LinkageQuery;
use crate::linkage::language::{LanguageLinkageStrategy, generic::GenericLanguageLinkageStrategy};

pub(super) struct TsLanguageLinkageStrategy;

impl LanguageLinkageStrategy for TsLanguageLinkageStrategy {
	fn matches(&self, query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
		GenericLanguageLinkageStrategy.matches(query, candidate)
			|| external_package_symbol_match(query, candidate)
	}
}

fn external_package_symbol_match(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	if query
		.target_first
		.is_none_or(|segment| segment.kind != kinds::EXTERNAL_PKG)
	{
		return false;
	}
	let Some(query_name) = query.call_name.map(str::as_bytes).or_else(|| {
		query
			.target_last
			.map(|segment| bare_callable_name(segment.name))
	}) else {
		return false;
	};
	let candidate_name = candidate.call_name.or_else(|| {
		candidate
			.last_segment
			.map(|segment| bare_callable_name(segment.name))
	});
	candidate_name == Some(query_name)
}

pub(super) fn package_prefix(target: &Moniker) -> Option<String> {
	let head = target.as_view().segments().next()?;
	if head.kind != kinds::EXTERNAL_PKG {
		return None;
	}
	std::str::from_utf8(head.name).ok().map(str::to_string)
}

pub(super) fn builtin_external_root(root: &str) -> bool {
	root == "code-moniker-ts-runtime" || root.starts_with("node:")
}

pub(super) fn source_declares_external_package(
	manifest: Manifest,
	deps: &FxHashSet<String>,
	package_prefix: &str,
	_query_confidence: Option<&str>,
	_workspace_declares_package: impl Fn(&str) -> bool,
) -> bool {
	if manifest != Manifest::PackageJson {
		return false;
	}
	deps.contains(&format!("{}\0{package_prefix}", manifest.tag()))
}
