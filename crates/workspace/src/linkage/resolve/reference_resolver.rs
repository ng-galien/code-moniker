use crate::linkage::binding::{
	ExternalOrigin, ReferenceLinkageDecision, ResolutionDecision, ResolutionScope, UnknownReason,
};
use crate::linkage::catalog::CandidateCatalog;
use crate::linkage::catalog::{LinkageQuery, ReferenceLocation, SymbolSet};
use crate::linkage::language::{
	confirm_name_match_targets, global_resolution_evidence, local_resolution_evidence,
	prefer_concrete_definitions,
};
use crate::linkage::resolve::manifest::{GlobalTargetAuthority, GlobalTargetQueries};
use crate::linkage::resolve::{
	BindingForwards, ManifestPolicy, WorkspacePackageIndex, resolve_global_scope,
	resolve_local_scope,
};
use crate::linkage::source_groups::SourceGroupPolicy;
use crate::snapshot::{DynamicReason, ReferenceRecord, ResolutionEvidence};
use crate::source::CodeIndexMaterial;
use code_moniker_core::lang::Lang;

pub(in crate::linkage) struct LinkagePolicies<'a> {
	pub(in crate::linkage) candidates: &'a CandidateCatalog,
	pub(in crate::linkage) manifests: &'a ManifestPolicy,
	pub(in crate::linkage) source_groups: &'a SourceGroupPolicy,
	pub(in crate::linkage) packages: &'a WorkspacePackageIndex,
	pub(in crate::linkage) forwards: &'a BindingForwards,
	pub(in crate::linkage) java_on_demand: &'a crate::linkage::resolve::JavaOnDemandImports,
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
	policies: &'a LinkagePolicies<'a>,
}

impl<'a> ReferenceResolver<'a> {
	pub(in crate::linkage) fn new(
		material: &'a CodeIndexMaterial,
		policies: &'a LinkagePolicies<'a>,
	) -> Self {
		Self { material, policies }
	}

	pub(in crate::linkage) fn resolve_reference(
		&self,
		reference_idx: usize,
		reference: &ReferenceRecord,
		location: Option<ReferenceLocation>,
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

		resolve_scopes(self, &query, site)
	}

	fn resolve_global(
		&self,
		query: &LinkageQuery<'_>,
		site: ReferenceSite<'_>,
	) -> Option<ReferenceLinkageDecision> {
		let policies = self.policies;
		let global_targets = resolve_global_scope(query, policies.candidates);
		let global_targets =
			prefer_concrete_definitions(self.material, policies.candidates, global_targets);
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
	) -> Option<ReferenceLinkageDecision> {
		let policies = self.policies;
		let authority = if external_tagged(original) {
			original
		} else {
			forwarded
		};
		if !policies.manifests.authorizes_forwarded_target(authority) {
			return None;
		}
		let targets = resolve_global_scope(forwarded, policies.candidates);
		let targets = prefer_concrete_definitions(self.material, policies.candidates, targets);
		let targets = confirm_name_match_targets(policies.candidates, forwarded, targets);
		let policy = policies.manifests.evaluate_global_targets(
			GlobalTargetQueries {
				candidate: forwarded,
				authority,
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

	fn resolve_forwarded(
		&self,
		original: &LinkageQuery<'_>,
		forwarded: &LinkageQuery<'_>,
		site: ReferenceSite<'_>,
	) -> Option<ReferenceLinkageDecision> {
		if external_tagged(original) || external_tagged(forwarded) {
			self.resolve_forwarded_global(original, forwarded, site)
		} else {
			self.resolve_global(forwarded, site)
		}
	}

	fn resolve_java_on_demand(
		&self,
		query: &LinkageQuery<'_>,
		site: ReferenceSite<'_>,
	) -> Option<ReferenceLinkageDecision> {
		let policies = self.policies;
		let targets = policies
			.java_on_demand
			.matching_targets(query, policies.candidates);
		if targets.is_empty() {
			return None;
		}
		let policy = policies.manifests.evaluate_global_targets(
			GlobalTargetQueries {
				candidate: query,
				authority: query,
			},
			targets,
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
		policy.for_reference(
			site.reference_idx,
			site.reference,
			ResolutionEvidence::GlobalBinding,
		)
	}
}

fn resolve_scopes(
	resolver: &ReferenceResolver<'_>,
	query: &LinkageQuery<'_>,
	site: ReferenceSite<'_>,
) -> ReferenceLinkageDecision {
	let policies = resolver.policies;
	// Import and reexport path defs are local binding sites, not canonical
	// targets. Follow their recorded binding before the ordinary local lookup;
	// otherwise the synthetic alias wins merely because it shares the source
	// file with the reference.
	if let Some(decision) = resolve_rust_forwards(resolver, query, site) {
		return decision;
	}
	let local_targets = resolve_local_scope(query, policies.candidates);
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
	if let Some(decision) = resolver.resolve_global(query, site) {
		return decision;
	}
	if let Some(decision) = resolver.resolve_java_on_demand(query, site) {
		return decision;
	}
	if let Some(forwarded) = policies.forwards.rewrite(query.target) {
		let forwarded_query = query.with_target(&forwarded);
		if let Some(decision) = resolver.resolve_global(&forwarded_query, site) {
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

fn resolve_rust_forwards(
	resolver: &ReferenceResolver<'_>,
	query: &LinkageQuery<'_>,
	site: ReferenceSite<'_>,
) -> Option<ReferenceLinkageDecision> {
	let forwarded = resolver.policies.forwards.rewrite_rust_named(query.target);
	if forwarded.is_empty() {
		return None;
	}
	if forwarded.len() == 1 {
		let forwarded_query = query.with_target(&forwarded[0]);
		return resolver.resolve_forwarded(query, &forwarded_query, site);
	}

	let mut candidates = SymbolSet::new();
	let mut all_resolved = true;
	for target in &forwarded {
		let forwarded_query = query.with_target(target);
		match resolver.resolve_forwarded(query, &forwarded_query, site) {
			Some(ReferenceLinkageDecision::Unique { resolution })
			| Some(ReferenceLinkageDecision::Candidate { resolution, .. }) => {
				for target in resolution.targets.iter() {
					candidates.insert(target);
				}
			}
			Some(_) | None => all_resolved = false,
		}
	}
	if all_resolved && candidates.len() == 1 {
		return Some(ReferenceLinkageDecision::resolved(ResolutionDecision::new(
			ResolutionScope::Global,
			ResolutionEvidence::GlobalBinding,
			site.reference.id,
			site.reference_idx,
			candidates,
		)));
	}
	Some(ReferenceLinkageDecision::dynamic(
		DynamicReason::ConditionalCompilation,
		site.reference_idx,
		site.reference.id,
		candidates,
	))
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
