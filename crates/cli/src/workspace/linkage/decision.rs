use crate::workspace::snapshot::{
	LinkageEdge, LinkageGraphReport, ReferenceRecord, ResourceGeneration, SymbolId,
	UnresolvedReference,
};

#[derive(Default)]
pub(super) struct LinkageDecisionLog {
	decisions: Vec<ReferenceLinkageDecision>,
}

impl LinkageDecisionLog {
	pub(super) fn new(decisions: Vec<ReferenceLinkageDecision>) -> Self {
		Self { decisions }
	}

	pub(super) fn project_report(
		self,
		generation: ResourceGeneration,
		index_generation: ResourceGeneration,
	) -> LinkageGraphReport {
		LinkageReportProjection::from_decisions(self.decisions)
			.into_report(generation, index_generation)
	}
}

pub(super) enum ReferenceLinkageDecision {
	Resolved {
		scope: ResolutionScope,
		reference: ReferenceRecord,
		targets: Vec<SymbolId>,
	},
	External {
		origin: ExternalOrigin,
		reference: ReferenceRecord,
	},
	Blocked {
		reason: BlockReason,
		reference: ReferenceRecord,
	},
	Unknown {
		reason: UnknownReason,
		reference: ReferenceRecord,
	},
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(super) enum ResolutionScope {
	Local,
	Global,
	Builtin,
	Injected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(super) enum ExternalOrigin {
	Dependency,
	UnknownExternal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(super) enum BlockReason {
	ManifestPolicy,
	Visibility,
	LanguageBoundary,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(super) enum UnknownReason {
	MissingQuery,
	NoCandidate,
	Ambiguous(Vec<SymbolId>),
	UnsupportedLanguageRule,
	IncompleteExtractorMetadata,
}

impl ReferenceLinkageDecision {
	pub(super) fn resolved(
		scope: ResolutionScope,
		reference: &ReferenceRecord,
		targets: Vec<SymbolId>,
	) -> Self {
		Self::Resolved {
			scope,
			reference: reference.clone(),
			targets,
		}
	}

	pub(super) fn unknown(reason: UnknownReason, reference: &ReferenceRecord) -> Self {
		Self::Unknown {
			reason,
			reference: reference.clone(),
		}
	}

	pub(super) fn manifest_blocked(reference: &ReferenceRecord) -> Self {
		Self::Blocked {
			reason: BlockReason::ManifestPolicy,
			reference: reference.clone(),
		}
	}

	pub(super) fn external(origin: ExternalOrigin, reference: &ReferenceRecord) -> Self {
		Self::External {
			origin,
			reference: reference.clone(),
		}
	}
}

#[derive(Default)]
struct LinkageReportProjection {
	resolved: ResolvedLinkProjection,
	external: ExternalLinkProjection,
	unresolved: UnresolvedLinkProjection,
}

impl LinkageReportProjection {
	fn from_decisions(decisions: Vec<ReferenceLinkageDecision>) -> Self {
		decisions
			.into_iter()
			.map(LinkageDecisionProjection::from)
			.fold(Self::default(), Self::collect)
	}

	fn collect(mut self, decision: LinkageDecisionProjection) -> Self {
		match decision {
			LinkageDecisionProjection::Resolved(resolved) => self.resolved.collect(resolved),
			LinkageDecisionProjection::External => self.external.collect(),
			LinkageDecisionProjection::ManifestBlocked(reference) => {
				self.unresolved.collect_manifest_blocked(reference)
			}
			LinkageDecisionProjection::Unresolved(reference) => self.unresolved.collect(reference),
		}
		self
	}

	fn into_report(
		self,
		generation: ResourceGeneration,
		index_generation: ResourceGeneration,
	) -> LinkageGraphReport {
		LinkageGraphReport {
			generation,
			index_generation,
			resolved_refs: self.resolved.resolved_refs,
			external_refs: self.external.external_refs,
			manifest_blocked_refs: self.unresolved.manifest_blocked_refs,
			unresolved_refs: self.unresolved.unresolved_refs,
			ambiguous_refs: self.resolved.ambiguous_refs,
			resolved: self.resolved.edges,
			unresolved: self.unresolved.references,
		}
	}
}

enum LinkageDecisionProjection {
	Resolved(ResolvedReferenceProjection),
	External,
	ManifestBlocked(UnresolvedReference),
	Unresolved(UnresolvedReference),
}

impl From<ReferenceLinkageDecision> for LinkageDecisionProjection {
	fn from(decision: ReferenceLinkageDecision) -> Self {
		match decision {
			ReferenceLinkageDecision::Resolved {
				scope: _scope,
				reference,
				targets,
			} => Self::Resolved(ResolvedReferenceProjection::new(reference, targets)),
			ReferenceLinkageDecision::Blocked {
				reason: BlockReason::ManifestPolicy,
				reference,
			} => Self::ManifestBlocked(unresolved_reference(reference)),
			ReferenceLinkageDecision::Blocked {
				reason: _reason,
				reference,
			} => Self::Unresolved(unresolved_reference(reference)),
			ReferenceLinkageDecision::Unknown {
				reason: _reason,
				reference,
			} => Self::Unresolved(unresolved_reference(reference)),
			ReferenceLinkageDecision::External {
				origin: _origin,
				reference: _reference,
			} => Self::External,
		}
	}
}

struct ResolvedReferenceProjection {
	ambiguous: bool,
	edges: Vec<LinkageEdge>,
}

impl ResolvedReferenceProjection {
	fn new(reference: ReferenceRecord, targets: Vec<SymbolId>) -> Self {
		Self {
			ambiguous: targets.len() > 1,
			edges: targets
				.into_iter()
				.map(|target| LinkageEdge::new(reference.id.clone(), target))
				.collect(),
		}
	}
}

#[derive(Default)]
struct ResolvedLinkProjection {
	resolved_refs: usize,
	ambiguous_refs: usize,
	edges: Vec<LinkageEdge>,
}

impl ResolvedLinkProjection {
	fn collect(&mut self, resolved: ResolvedReferenceProjection) {
		self.resolved_refs += 1;
		if resolved.ambiguous {
			self.ambiguous_refs += 1;
		}
		self.edges.extend(resolved.edges);
	}
}

#[derive(Default)]
struct ExternalLinkProjection {
	external_refs: usize,
}

impl ExternalLinkProjection {
	fn collect(&mut self) {
		self.external_refs += 1;
	}
}

#[derive(Default)]
struct UnresolvedLinkProjection {
	manifest_blocked_refs: usize,
	unresolved_refs: usize,
	references: Vec<UnresolvedReference>,
}

impl UnresolvedLinkProjection {
	fn collect_manifest_blocked(&mut self, reference: UnresolvedReference) {
		self.manifest_blocked_refs += 1;
		self.references.push(reference);
	}

	fn collect(&mut self, reference: UnresolvedReference) {
		self.unresolved_refs += 1;
		self.references.push(reference);
	}
}

fn unresolved_reference(reference: ReferenceRecord) -> UnresolvedReference {
	UnresolvedReference::new(reference.id, reference.target_identity)
}
