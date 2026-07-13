use crate::linkage::binding::{
	ExternalOrigin, ReferenceLinkageDecision, ResolutionScope, UnknownReason,
};
use crate::linkage::catalog::CandidateCatalog;
use crate::linkage::catalog::{LinkageQuery, ReferenceLocation, SymbolSet};
use crate::linkage::resolve::{
	CrateForwards, GlobalScopeResolver, LocalScopeResolver, ManifestPolicy, WorkspacePackageIndex,
};
use crate::linkage::source_groups::SourceGroupPolicy;
use crate::snapshot::ReferenceRecord;
use crate::source::CodeIndexMaterial;

pub(in crate::linkage) struct LinkagePolicies<'a> {
	pub(in crate::linkage) candidates: &'a CandidateCatalog,
	pub(in crate::linkage) manifests: &'a ManifestPolicy,
	pub(in crate::linkage) source_groups: &'a SourceGroupPolicy,
	pub(in crate::linkage) packages: &'a WorkspacePackageIndex,
	pub(in crate::linkage) forwards: &'a CrateForwards,
}

#[derive(Clone, Copy)]
struct ReferenceSite<'a> {
	reference_idx: usize,
	reference: &'a ReferenceRecord,
}

impl ReferenceSite<'_> {
	fn unknown(&self, reason: UnknownReason) -> ReferenceLinkageDecision {
		ReferenceLinkageDecision::unknown(reason, self.reference_idx, self.reference.id)
	}
}

pub(in crate::linkage) struct ReferenceResolver<'a> {
	material: &'a CodeIndexMaterial,
	local: LocalScopeResolver,
	global: GlobalScopeResolver,
}

impl<'a> ReferenceResolver<'a> {
	pub(in crate::linkage) fn new(material: &'a CodeIndexMaterial) -> Self {
		Self {
			material,
			local: LocalScopeResolver,
			global: GlobalScopeResolver,
		}
	}

	pub(in crate::linkage) fn resolve_reference(
		&self,
		reference_idx: usize,
		reference: &ReferenceRecord,
		location: Option<ReferenceLocation>,
		policies: &LinkagePolicies<'_>,
	) -> ReferenceLinkageDecision {
		let site = ReferenceSite {
			reference_idx,
			reference,
		};
		let Some(location) = location else {
			return site.unknown(UnknownReason::MissingQuery);
		};
		let Some(query) = LinkageQuery::at(reference, self.material, location) else {
			return site.unknown(UnknownReason::MissingQuery);
		};

		resolve_scopes(self, &query, site, policies)
	}

	fn resolve_global(
		&self,
		query: &LinkageQuery<'_>,
		site: ReferenceSite<'_>,
		policies: &LinkagePolicies<'_>,
	) -> Option<ReferenceLinkageDecision> {
		let global_targets = self.global.resolve(query, policies.candidates);
		let global_targets = confirm_name_match_targets(policies.candidates, query, global_targets);
		let global_decision = policies.manifests.evaluate_global_targets(
			query,
			global_targets,
			policies.candidates,
			|target_file| {
				policies.source_groups.link_permission(
					self.material,
					query.source_file,
					target_file,
				)
			},
		);
		global_decision.for_reference(site.reference_idx, site.reference)
	}
}

// The extractor already committed to "this import root is not part of the
// project" by anchoring the target on external_pkg. When nothing internal
// matched and no manifest could confirm it, honour that claim instead of
// reporting a hole: the reference is external, whatever the build system.
fn resolve_scopes(
	resolver: &ReferenceResolver<'_>,
	query: &LinkageQuery<'_>,
	site: ReferenceSite<'_>,
	policies: &LinkagePolicies<'_>,
) -> ReferenceLinkageDecision {
	let local_targets = resolver.local.resolve(query, policies.candidates);
	if !local_targets.is_empty() {
		return ReferenceLinkageDecision::resolved(
			ResolutionScope::Local,
			site.reference_idx,
			site.reference.id,
			local_targets,
		);
	}
	if let Some(decision) = resolver.resolve_global(query, site, policies) {
		return decision;
	}
	if let Some(forwarded) = policies.forwards.rewrite(query.target) {
		let forwarded_query = query.with_target(&forwarded);
		if let Some(decision) = resolver.resolve_global(&forwarded_query, site, policies) {
			return decision;
		}
	}
	if external_fallthrough(query, policies) {
		return ReferenceLinkageDecision::external(
			ExternalOrigin::Dependency,
			site.reference_idx,
			site.reference.id,
		);
	}
	site.unknown(UnknownReason::NoCandidate)
}

// A name-backed resolution is only trustworthy when language semantics back
// it: the source's own package wins outright (Java resolves it before any
// import), then the source's own srcset breaks main/test homonym ties —
// better an honest narrowing than a coin-flip multi-link.
fn confirm_name_match_targets(
	candidates: &CandidateCatalog,
	query: &LinkageQuery<'_>,
	targets: SymbolSet,
) -> SymbolSet {
	if targets.len() <= 1 {
		return targets;
	}
	let targets = match query.confidence {
		Some("name_match") => restrict_to_source_package(candidates, query, targets),
		Some("imported") => targets,
		_ => return targets,
	};
	let source_srcset = file_srcset(query.material, query.source_file);
	prefer_same_srcset(candidates, &source_srcset, targets)
}

fn restrict_to_source_package(
	candidates: &CandidateCatalog,
	query: &LinkageQuery<'_>,
	targets: SymbolSet,
) -> SymbolSet {
	let source_packages = file_package_chain(query.material, query.source_file);
	if source_packages.is_empty() {
		return targets;
	}
	let mut same_package = SymbolSet::new();
	for symbol in targets.iter() {
		let Some(candidate) = candidates.candidate(symbol) else {
			continue;
		};
		if moniker_package_chain(candidate.moniker) == source_packages {
			same_package.insert(symbol);
		}
	}
	same_package
}

// Same package and several candidates left: an identically named class in
// main and in test of the same package (a common test idiom) — the source's
// own source set is the closer compilation scope, pick it when it answers.
fn prefer_same_srcset(
	candidates: &CandidateCatalog,
	source_srcset: &[u8],
	targets: SymbolSet,
) -> SymbolSet {
	if source_srcset.is_empty() || targets.len() <= 1 {
		return targets;
	}
	let mut same_srcset = SymbolSet::new();
	for symbol in targets.iter() {
		let Some(candidate) = candidates.candidate(symbol) else {
			continue;
		};
		if moniker_srcset(candidate.moniker) == source_srcset {
			same_srcset.insert(symbol);
		}
	}
	if same_srcset.is_empty() {
		targets
	} else {
		same_srcset
	}
}

fn file_srcset(material: &CodeIndexMaterial, file_idx: usize) -> Vec<u8> {
	let Some(file) = material.files.get(file_idx) else {
		return Vec::new();
	};
	if file.graph.def_count() == 0 {
		return Vec::new();
	}
	moniker_srcset(&file.graph.def_at(0).moniker)
}

fn moniker_srcset(moniker: &code_moniker_core::core::moniker::Moniker) -> Vec<u8> {
	moniker
		.as_view()
		.segments()
		.find(|segment| segment.kind == b"srcset")
		.map(|segment| segment.name.to_vec())
		.unwrap_or_default()
}

fn file_package_chain(material: &CodeIndexMaterial, file_idx: usize) -> Vec<Vec<u8>> {
	let Some(file) = material.files.get(file_idx) else {
		return Vec::new();
	};
	if file.graph.def_count() == 0 {
		return Vec::new();
	}
	moniker_package_chain(&file.graph.def_at(0).moniker)
}

fn moniker_package_chain(moniker: &code_moniker_core::core::moniker::Moniker) -> Vec<Vec<u8>> {
	moniker
		.as_view()
		.segments()
		.filter(|segment| segment.kind == code_moniker_core::lang::kinds::PACKAGE)
		.map(|segment| segment.name.to_vec())
		.collect()
}

fn external_fallthrough(query: &LinkageQuery<'_>, policies: &LinkagePolicies<'_>) -> bool {
	policies.packages.is_foreign(query)
		|| (external_tagged(query)
			&& !crate::linkage::resolve::manifest::source_has_manifest_entry(
				policies.manifests,
				query.source_file,
			))
}

fn external_tagged(query: &LinkageQuery<'_>) -> bool {
	query
		.target_first
		.is_some_and(|segment| segment.kind == code_moniker_core::lang::kinds::EXTERNAL_PKG)
}
