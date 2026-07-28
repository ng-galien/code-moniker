use crate::linkage::binding::{
	ExternalOrigin, ReferenceLinkageDecision, ResolutionDecision, ResolutionScope, UnknownReason,
};
use crate::linkage::catalog::CandidateCatalog;
use crate::linkage::catalog::{LinkageQuery, ReferenceLocation, SymbolSet};
use crate::linkage::resolve::manifest::{GlobalTargetAuthority, GlobalTargetQueries};
use crate::linkage::resolve::{
	CrateForwards, GlobalScopeResolver, LocalScopeResolver, ManifestPolicy, WorkspacePackageIndex,
};
use crate::linkage::source_groups::SourceGroupPolicy;
use crate::snapshot::{ReferenceRecord, ResolutionEvidence};
use crate::source::CodeIndexMaterial;
use code_moniker_core::lang::Lang;

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
		if reference.confidence.as_deref() == Some("unresolved")
			&& self
				.material
				.files
				.get(location.source_file)
				.is_some_and(|file| file.lang == Lang::Python)
		{
			return site.unknown(UnknownReason::IncompleteExtractorMetadata);
		}
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
		let global_targets = prefer_definitions_over_reexport_aliases(
			self.material,
			policies.candidates,
			global_targets,
		);
		let global_targets = confirm_name_match_targets(policies.candidates, query, global_targets);
		let global_decision = policies.manifests.evaluate_global_targets(
			GlobalTargetQueries {
				candidate: query,
				authority: query,
			},
			global_targets,
			policies.candidates,
			|target_file| {
				policies.source_groups.link_permission(
					self.material,
					query.source_file,
					target_file,
				)
			},
			GlobalTargetAuthority::Direct,
		);
		global_decision.for_reference(
			site.reference_idx,
			site.reference,
			global_resolution_evidence(query),
		)
	}

	fn resolve_forwarded_global(
		&self,
		original: &LinkageQuery<'_>,
		forwarded: &LinkageQuery<'_>,
		site: ReferenceSite<'_>,
		policies: &LinkagePolicies<'_>,
	) -> Option<ReferenceLinkageDecision> {
		if !policies.manifests.declares_external_target(original) {
			return None;
		}
		let targets = self.global.resolve(forwarded, policies.candidates);
		let targets =
			prefer_definitions_over_reexport_aliases(self.material, policies.candidates, targets);
		let targets = confirm_name_match_targets(policies.candidates, forwarded, targets);
		let policy = policies.manifests.evaluate_global_targets(
			GlobalTargetQueries {
				candidate: forwarded,
				authority: original,
			},
			targets,
			policies.candidates,
			|target_file| {
				policies.source_groups.link_permission(
					self.material,
					original.source_file,
					target_file,
				)
			},
			GlobalTargetAuthority::Forwarded,
		);
		policy.for_reference(
			site.reference_idx,
			site.reference,
			global_resolution_evidence(forwarded),
		)
	}
}

// A rust `pub use` façade indexes a path alias that rivals the definition
// it re-exports in global name binding; a mixed set keeps the concrete
// definitions only. Scoped to rust candidates: python reuses the path kind
// for ordinary module-level bindings, which are legitimate definitions.
fn prefer_definitions_over_reexport_aliases(
	material: &CodeIndexMaterial,
	catalog: &CandidateCatalog,
	targets: SymbolSet,
) -> SymbolSet {
	if targets.len() < 2 {
		return targets;
	}
	let mut concrete = SymbolSet::new();
	for symbol in targets.iter() {
		let is_alias = catalog.candidate(symbol).is_some_and(|candidate| {
			candidate
				.last_segment
				.is_some_and(|segment| segment.kind == code_moniker_core::lang::kinds::PATH)
				&& material
					.files
					.get(candidate.source_file)
					.is_some_and(|file| file.lang == Lang::Rs)
		});
		if !is_alias {
			concrete.insert(symbol);
		}
	}
	if concrete.is_empty() || concrete.len() == targets.len() {
		targets
	} else {
		concrete
	}
}

fn global_resolution_evidence(query: &LinkageQuery<'_>) -> ResolutionEvidence {
	if query.reference_kind == "calls"
		&& query
			.material
			.files
			.get(query.source_file)
			.is_some_and(|file| file.lang == Lang::Sql)
	{
		if crate::linkage::language::sql_call_has_strong_evidence(query) {
			ResolutionEvidence::GlobalBinding
		} else {
			ResolutionEvidence::NameMatch
		}
	} else {
		ResolutionEvidence::GlobalBinding
	}
}

fn resolve_scopes(
	resolver: &ReferenceResolver<'_>,
	query: &LinkageQuery<'_>,
	site: ReferenceSite<'_>,
	policies: &LinkagePolicies<'_>,
) -> ReferenceLinkageDecision {
	let local_targets = resolver.local.resolve(query, policies.candidates);
	if !local_targets.is_empty() {
		let evidence = local_resolution_evidence(query, policies.candidates, &local_targets);
		return ReferenceLinkageDecision::resolved(ResolutionDecision::new(
			ResolutionScope::Local,
			evidence,
			site.reference.id,
			site.reference_idx,
			local_targets,
		));
	}
	if let Some(forwarded) = policies.forwards.rewrite_rust_named(query.target) {
		let forwarded_query = query.with_target(&forwarded);
		if let Some(decision) =
			resolver.resolve_forwarded_global(query, &forwarded_query, site, policies)
		{
			return decision;
		}
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
	if let Some(target) = crate::linkage::language::rust_sdk_method_fallback(query) {
		return ReferenceLinkageDecision::external_target(
			ExternalOrigin::Sdk,
			site.reference_idx,
			site.reference.id,
			target,
		);
	}
	if sdk_tagged(query) {
		return ReferenceLinkageDecision::external(
			ExternalOrigin::Sdk,
			site.reference_idx,
			site.reference.id,
		);
	}
	if external_fallthrough(query, policies) {
		return ReferenceLinkageDecision::external(
			ExternalOrigin::Dependency,
			site.reference_idx,
			site.reference.id,
		);
	}
	if external_tagged(query)
		&& !crate::linkage::resolve::manifest::source_has_manifest_entry(
			policies.manifests,
			query.source_file,
		) {
		return ReferenceLinkageDecision::external(
			ExternalOrigin::UnknownExternal,
			site.reference_idx,
			site.reference.id,
		);
	}
	site.unknown(UnknownReason::NoCandidate)
}

fn local_resolution_evidence(
	query: &LinkageQuery<'_>,
	candidates: &CandidateCatalog,
	targets: &SymbolSet,
) -> ResolutionEvidence {
	let lang = query
		.material
		.files
		.get(query.source_file)
		.map(|file| file.lang);
	if lang == Some(Lang::Sql) && query.reference_kind == "calls" {
		return if crate::linkage::language::sql_call_has_strong_evidence(query) {
			ResolutionEvidence::LocalBinding
		} else {
			ResolutionEvidence::NameMatch
		};
	}
	if lang != Some(Lang::Sql) && query.confidence != Some("name_match") {
		return ResolutionEvidence::LocalBinding;
	}
	if !matches!(lang, Some(Lang::Cs | Lang::Sql)) {
		return ResolutionEvidence::LocalBinding;
	}
	let exact = candidates.indexes().symbol_by_moniker(query.target);
	if exact.is_some_and(|exact| targets.iter().any(|target| target == exact)) {
		ResolutionEvidence::LocalBinding
	} else {
		ResolutionEvidence::NameMatch
	}
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
}

fn sdk_tagged(query: &LinkageQuery<'_>) -> bool {
	query
		.target_first
		.is_some_and(|segment| segment.kind == code_moniker_core::lang::kinds::SDK)
}

fn external_tagged(query: &LinkageQuery<'_>) -> bool {
	query
		.target_first
		.is_some_and(|segment| segment.kind == code_moniker_core::lang::kinds::EXTERNAL_PKG)
}
