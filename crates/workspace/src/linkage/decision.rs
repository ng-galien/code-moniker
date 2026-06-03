use crate::snapshot::{
	ExternalReference, LinkageEdge, LinkageSnapshotReport, ReferenceId, ReferenceRecord,
	ResourceGeneration, SymbolId, UnresolvedReference,
};
use crate::source::LocalIdentityResolver;
use code_moniker_core::core::moniker::Moniker;
use std::sync::Arc;

pub(super) use crate::snapshot::ExternalReferenceOrigin as ExternalOrigin;

pub(super) fn project_decisions(
	decisions: &[ReferenceLinkageDecision],
	references: &[ReferenceRecord],
	identity: &LocalIdentityResolver,
) -> LinkageReportProjection {
	LinkageReportProjection::from_decisions(decisions, references, identity)
}

#[derive(Clone)]
pub(super) enum ReferenceLinkageDecision {
	Resolved {
		scope: ResolutionScope,
		reference: ReferenceId,
		reference_idx: usize,
		targets: Vec<SymbolId>,
	},
	External {
		origin: ExternalOrigin,
		reference: ReferenceId,
		reference_idx: usize,
		target: Option<Moniker>,
	},
	Blocked {
		reason: BlockReason,
		reference: ReferenceId,
		reference_idx: usize,
	},
	Unknown {
		reason: UnknownReason,
		reference: ReferenceId,
		reference_idx: usize,
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
		reference_idx: usize,
		reference: ReferenceId,
		targets: Vec<SymbolId>,
	) -> Self {
		Self::Resolved {
			scope,
			reference,
			reference_idx,
			targets,
		}
	}

	pub(super) fn unknown(
		reason: UnknownReason,
		reference_idx: usize,
		reference: ReferenceId,
	) -> Self {
		Self::Unknown {
			reason,
			reference,
			reference_idx,
		}
	}

	pub(super) fn manifest_blocked(reference_idx: usize, reference: ReferenceId) -> Self {
		Self::Blocked {
			reason: BlockReason::ManifestPolicy,
			reference,
			reference_idx,
		}
	}

	pub(super) fn external(
		origin: ExternalOrigin,
		reference_idx: usize,
		reference: ReferenceId,
	) -> Self {
		Self::External {
			origin,
			reference,
			reference_idx,
			target: None,
		}
	}

	pub(super) fn external_target(
		origin: ExternalOrigin,
		reference_idx: usize,
		reference: ReferenceId,
		target: Moniker,
	) -> Self {
		Self::External {
			origin,
			reference,
			reference_idx,
			target: Some(target),
		}
	}

	pub(super) fn reference_idx(&self) -> usize {
		match self {
			Self::Resolved { reference_idx, .. }
			| Self::External { reference_idx, .. }
			| Self::Blocked { reference_idx, .. }
			| Self::Unknown { reference_idx, .. } => *reference_idx,
		}
	}

	pub(super) fn reference(&self) -> &ReferenceId {
		match self {
			Self::Resolved { reference, .. }
			| Self::External { reference, .. }
			| Self::Blocked { reference, .. }
			| Self::Unknown { reference, .. } => reference,
		}
	}

	pub(super) fn resolved_targets(&self) -> Option<&[SymbolId]> {
		match self {
			Self::Resolved { targets, .. } => Some(targets),
			Self::External { .. } | Self::Blocked { .. } | Self::Unknown { .. } => None,
		}
	}

	pub(super) fn set_reference_idx(&mut self, next_reference_idx: usize) {
		match &mut *self {
			Self::Resolved { reference_idx, .. }
			| Self::External { reference_idx, .. }
			| Self::Blocked { reference_idx, .. }
			| Self::Unknown { reference_idx, .. } => *reference_idx = next_reference_idx,
		}
	}
}

#[derive(Default)]
pub(super) struct LinkageReportProjection {
	resolved: ResolvedLinkProjection,
	external: ExternalLinkProjection,
	unresolved: UnresolvedLinkProjection,
}

impl LinkageReportProjection {
	fn from_decisions(
		decisions: &[ReferenceLinkageDecision],
		references: &[ReferenceRecord],
		identity: &LocalIdentityResolver,
	) -> Self {
		let capacity = LinkageProjectionCapacity::from_decisions(decisions);
		decisions
			.iter()
			.map(|decision| {
				LinkageDecisionProjection::from_decision(decision, references, identity)
			})
			.fold(Self::with_capacity(capacity), Self::collect)
	}

	fn with_capacity(capacity: LinkageProjectionCapacity) -> Self {
		Self {
			resolved: ResolvedLinkProjection::with_capacity(capacity.resolved_edges),
			external: ExternalLinkProjection::with_capacity(capacity.external_refs),
			unresolved: UnresolvedLinkProjection::with_capacity(capacity.unresolved_refs),
		}
	}

	fn collect(mut self, decision: LinkageDecisionProjection) -> Self {
		match decision {
			LinkageDecisionProjection::Resolved(resolved) => self.resolved.collect(resolved),
			LinkageDecisionProjection::External(external) => self.external.collect(external),
			LinkageDecisionProjection::ManifestBlocked(reference) => {
				self.unresolved.collect_manifest_blocked(reference)
			}
			LinkageDecisionProjection::Unresolved(reference) => self.unresolved.collect(reference),
		}
		self
	}

	pub(super) fn into_report(
		self,
		generation: ResourceGeneration,
		index_generation: ResourceGeneration,
	) -> LinkageSnapshotReport {
		LinkageSnapshotReport {
			generation,
			index_generation,
			resolved_refs: self.resolved.resolved_refs,
			external_refs: self.external.external_refs,
			manifest_blocked_refs: self.unresolved.manifest_blocked_refs,
			unresolved_refs: self.unresolved.unresolved_refs,
			ambiguous_refs: self.resolved.ambiguous_refs,
			resolved: self.resolved.edges,
			external: self.external.references,
			manifest_blocked: self.unresolved.manifest_blocked_references,
			unresolved: self.unresolved.references,
		}
	}
}

struct LinkageProjectionCapacity {
	resolved_edges: usize,
	external_refs: usize,
	unresolved_refs: usize,
}

impl LinkageProjectionCapacity {
	fn from_decisions(decisions: &[ReferenceLinkageDecision]) -> Self {
		decisions.iter().fold(
			Self {
				resolved_edges: 0,
				external_refs: 0,
				unresolved_refs: 0,
			},
			|mut capacity, decision| {
				match decision {
					ReferenceLinkageDecision::Resolved { targets, .. } => {
						capacity.resolved_edges += targets.len();
					}
					ReferenceLinkageDecision::Blocked { .. }
					| ReferenceLinkageDecision::Unknown { .. } => {
						capacity.unresolved_refs += 1;
					}
					ReferenceLinkageDecision::External { .. } => {
						capacity.external_refs += 1;
					}
				}
				capacity
			},
		)
	}
}

enum LinkageDecisionProjection {
	Resolved(ResolvedReferenceProjection),
	External(ExternalReference),
	ManifestBlocked(UnresolvedReference),
	Unresolved(UnresolvedReference),
}

impl LinkageDecisionProjection {
	fn from_decision(
		decision: &ReferenceLinkageDecision,
		references: &[ReferenceRecord],
		identity: &LocalIdentityResolver,
	) -> Self {
		match decision {
			ReferenceLinkageDecision::Resolved {
				reference_idx,
				targets,
				..
			} => Self::Resolved(ResolvedReferenceProjection::new(
				&references[*reference_idx],
				targets.clone(),
			)),
			ReferenceLinkageDecision::Blocked {
				reason: BlockReason::ManifestPolicy,
				reference_idx,
				..
			} => Self::ManifestBlocked(unresolved_reference(&references[*reference_idx])),
			ReferenceLinkageDecision::Blocked { reference_idx, .. } => {
				Self::Unresolved(unresolved_reference(&references[*reference_idx]))
			}
			ReferenceLinkageDecision::Unknown { reference_idx, .. } => {
				Self::Unresolved(unresolved_reference(&references[*reference_idx]))
			}
			ReferenceLinkageDecision::External {
				origin,
				reference_idx,
				target,
				..
			} => Self::External(external_reference(
				&references[*reference_idx],
				*origin,
				target.as_ref(),
				identity,
			)),
		}
	}
}

struct ResolvedReferenceProjection {
	ambiguous: bool,
	edges: Vec<LinkageEdge>,
}

impl ResolvedReferenceProjection {
	fn new(reference: &ReferenceRecord, targets: Vec<SymbolId>) -> Self {
		let ambiguous = targets.len() > 1;
		let mut edges = Vec::with_capacity(targets.len());
		edges.extend(
			targets
				.into_iter()
				.map(|target| LinkageEdge::new(reference.id.clone(), target)),
		);
		Self { ambiguous, edges }
	}
}

#[derive(Default)]
struct ResolvedLinkProjection {
	resolved_refs: usize,
	ambiguous_refs: usize,
	edges: Vec<LinkageEdge>,
}

impl ResolvedLinkProjection {
	fn with_capacity(capacity: usize) -> Self {
		Self {
			resolved_refs: 0,
			ambiguous_refs: 0,
			edges: Vec::with_capacity(capacity),
		}
	}

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
	references: Vec<ExternalReference>,
}

impl ExternalLinkProjection {
	fn with_capacity(capacity: usize) -> Self {
		Self {
			external_refs: 0,
			references: Vec::with_capacity(capacity),
		}
	}

	fn collect(&mut self, reference: ExternalReference) {
		self.external_refs += 1;
		self.references.push(reference);
	}
}

#[derive(Default)]
struct UnresolvedLinkProjection {
	manifest_blocked_refs: usize,
	unresolved_refs: usize,
	manifest_blocked_references: Vec<UnresolvedReference>,
	references: Vec<UnresolvedReference>,
}

impl UnresolvedLinkProjection {
	fn with_capacity(capacity: usize) -> Self {
		Self {
			manifest_blocked_refs: 0,
			unresolved_refs: 0,
			manifest_blocked_references: Vec::with_capacity(capacity),
			references: Vec::with_capacity(capacity),
		}
	}

	fn collect_manifest_blocked(&mut self, reference: UnresolvedReference) {
		self.manifest_blocked_refs += 1;
		self.manifest_blocked_references.push(reference);
	}

	fn collect(&mut self, reference: UnresolvedReference) {
		self.unresolved_refs += 1;
		self.references.push(reference);
	}
}

fn unresolved_reference(reference: &ReferenceRecord) -> UnresolvedReference {
	UnresolvedReference::new(reference.id.clone(), Arc::clone(&reference.target_identity))
}

fn external_reference(
	reference: &ReferenceRecord,
	origin: ExternalOrigin,
	target: Option<&Moniker>,
	identity: &LocalIdentityResolver,
) -> ExternalReference {
	ExternalReference::new(
		reference.id.clone(),
		target
			.map(|target| identity.moniker_uri(target))
			.unwrap_or_else(|| reference.target_identity.to_string()),
		origin,
	)
}
