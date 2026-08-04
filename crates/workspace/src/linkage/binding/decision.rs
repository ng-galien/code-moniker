use crate::linkage::catalog::SymbolSet;
use crate::snapshot::{
	CandidateReason, CandidateScope, DynamicReason, ReferenceId, ResolutionEvidence,
	UnresolvedReason,
};
use code_moniker_core::core::moniker::Moniker;

pub(in crate::linkage) use crate::snapshot::ExternalReferenceOrigin as ExternalOrigin;

// This is a discriminated union of mutually exclusive outcomes, not a stateful
// class; LCOM cohesion across variant-specific constructors is not meaningful.
// code-moniker: ignore[rust.shape.type.smell-god-type-local-metrics]
#[derive(Clone)]
pub(in crate::linkage) enum ReferenceLinkageDecision {
	Unique {
		resolution: ResolutionDecision,
	},
	Candidate {
		reason: CandidateReason,
		resolution: ResolutionDecision,
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
	Dynamic {
		reason: DynamicReason,
		reference: ReferenceId,
		reference_idx: usize,
		candidates: SymbolSet,
	},
	Unknown {
		reason: UnknownReason,
		reference: ReferenceId,
		reference_idx: usize,
	},
}

#[derive(Clone)]
pub(in crate::linkage) struct ResolutionDecision {
	pub(in crate::linkage) scope: ResolutionScope,
	pub(in crate::linkage) evidence: ResolutionEvidence,
	pub(in crate::linkage) reference: ReferenceId,
	pub(in crate::linkage) reference_idx: usize,
	pub(in crate::linkage) targets: SymbolSet,
}

impl ResolutionDecision {
	pub(in crate::linkage) fn new(
		scope: ResolutionScope,
		evidence: ResolutionEvidence,
		reference: ReferenceId,
		reference_idx: usize,
		targets: SymbolSet,
	) -> Self {
		Self {
			scope,
			evidence,
			reference,
			reference_idx,
			targets,
		}
	}

	fn candidate_reason(&self) -> Option<CandidateReason> {
		if self.evidence == ResolutionEvidence::NameMatch {
			Some(CandidateReason::WeakNameMatch)
		} else if self.targets.len() > 1 {
			Some(CandidateReason::MultipleTargets)
		} else {
			None
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::linkage) enum ResolutionScope {
	Local,
	Global,
	Builtin,
	Injected,
	Unknown,
}

impl From<ResolutionScope> for CandidateScope {
	fn from(scope: ResolutionScope) -> Self {
		match scope {
			ResolutionScope::Local => Self::Local,
			ResolutionScope::Global => Self::Global,
			ResolutionScope::Builtin => Self::Builtin,
			ResolutionScope::Injected => Self::Injected,
			ResolutionScope::Unknown => Self::Unknown,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::linkage) enum BlockReason {
	ManifestPolicy,
	Visibility,
	LanguageBoundary,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::linkage) enum UnknownReason {
	MissingQuery,
	NoCandidate,
	Ambiguous,
	UnsupportedLanguageRule,
	IncompleteExtractorMetadata,
}

impl BlockReason {
	pub(super) fn unresolved_reason(&self) -> UnresolvedReason {
		match self {
			Self::ManifestPolicy => UnresolvedReason::ManifestBlocked,
			Self::Visibility => UnresolvedReason::Visibility,
			Self::LanguageBoundary => UnresolvedReason::LanguageBoundary,
		}
	}

	pub(in crate::linkage) fn from_unresolved_reason(reason: UnresolvedReason) -> Option<Self> {
		match reason {
			UnresolvedReason::ManifestBlocked => Some(Self::ManifestPolicy),
			UnresolvedReason::Visibility => Some(Self::Visibility),
			UnresolvedReason::LanguageBoundary => Some(Self::LanguageBoundary),
			_ => None,
		}
	}
}

impl UnknownReason {
	pub(super) fn unresolved_reason(&self) -> UnresolvedReason {
		match self {
			Self::MissingQuery => UnresolvedReason::MissingQuery,
			Self::NoCandidate => UnresolvedReason::NoCandidate,
			Self::Ambiguous => UnresolvedReason::Ambiguous,
			Self::UnsupportedLanguageRule => UnresolvedReason::UnsupportedLanguageRule,
			Self::IncompleteExtractorMetadata => UnresolvedReason::IncompleteExtractorMetadata,
		}
	}

	pub(in crate::linkage) fn from_unresolved_reason(reason: UnresolvedReason) -> Option<Self> {
		match reason {
			UnresolvedReason::MissingQuery => Some(Self::MissingQuery),
			UnresolvedReason::NoCandidate => Some(Self::NoCandidate),
			UnresolvedReason::Ambiguous => Some(Self::Ambiguous),
			UnresolvedReason::UnsupportedLanguageRule => Some(Self::UnsupportedLanguageRule),
			UnresolvedReason::IncompleteExtractorMetadata => {
				Some(Self::IncompleteExtractorMetadata)
			}
			UnresolvedReason::ManifestBlocked
			| UnresolvedReason::Visibility
			| UnresolvedReason::LanguageBoundary => None,
		}
	}
}

impl ReferenceLinkageDecision {
	pub(in crate::linkage) fn resolved(resolution: ResolutionDecision) -> Self {
		if resolution.targets.is_empty() {
			return Self::unknown(
				UnknownReason::NoCandidate,
				resolution.reference_idx,
				resolution.reference,
			);
		}
		match resolution.candidate_reason() {
			Some(reason) => Self::candidate(reason, resolution),
			None => Self::Unique { resolution },
		}
	}

	pub(in crate::linkage) fn candidate(
		reason: CandidateReason,
		resolution: ResolutionDecision,
	) -> Self {
		Self::Candidate { reason, resolution }
	}

	pub(in crate::linkage) fn dynamic(
		reason: DynamicReason,
		reference_idx: usize,
		reference: ReferenceId,
		candidates: SymbolSet,
	) -> Self {
		Self::Dynamic {
			reason,
			reference,
			reference_idx,
			candidates,
		}
	}

	pub(in crate::linkage) fn unknown(
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

	pub(in crate::linkage) fn manifest_blocked(
		reference_idx: usize,
		reference: ReferenceId,
	) -> Self {
		Self::blocked(BlockReason::ManifestPolicy, reference_idx, reference)
	}

	pub(in crate::linkage) fn blocked(
		reason: BlockReason,
		reference_idx: usize,
		reference: ReferenceId,
	) -> Self {
		Self::Blocked {
			reason,
			reference,
			reference_idx,
		}
	}

	pub(in crate::linkage) fn external(
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

	pub(in crate::linkage) fn external_target(
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

	pub(in crate::linkage) fn reference_idx(&self) -> usize {
		match self {
			Self::Unique { resolution } | Self::Candidate { resolution, .. } => {
				resolution.reference_idx
			}
			Self::External { reference_idx, .. }
			| Self::Blocked { reference_idx, .. }
			| Self::Dynamic { reference_idx, .. }
			| Self::Unknown { reference_idx, .. } => *reference_idx,
		}
	}

	pub(in crate::linkage) fn type_refinement_reference_idx(&self) -> Option<usize> {
		match self {
			Self::Candidate { resolution, .. } => Some(resolution.reference_idx),
			_ => self.refinement_pending_reference_idx(),
		}
	}

	pub(in crate::linkage) fn refinement_pending_reference_idx(&self) -> Option<usize> {
		match self {
			Self::Candidate {
				reason: CandidateReason::WeakNameMatch,
				resolution,
			} => Some(resolution.reference_idx),
			Self::Unknown {
				reason: UnknownReason::NoCandidate | UnknownReason::IncompleteExtractorMetadata,
				reference_idx,
				..
			} => Some(*reference_idx),
			_ => None,
		}
	}

	pub(in crate::linkage) fn reference(&self) -> &ReferenceId {
		match self {
			Self::Unique { resolution } | Self::Candidate { resolution, .. } => {
				&resolution.reference
			}
			Self::External { reference, .. }
			| Self::Blocked { reference, .. }
			| Self::Dynamic { reference, .. }
			| Self::Unknown { reference, .. } => reference,
		}
	}

	pub(in crate::linkage) fn linkage_targets(&self) -> Option<&SymbolSet> {
		match self {
			Self::Unique { resolution } | Self::Candidate { resolution, .. } => {
				Some(&resolution.targets)
			}
			Self::Dynamic { candidates, .. } => Some(candidates),
			Self::External { .. } | Self::Blocked { .. } | Self::Unknown { .. } => None,
		}
	}

	pub(in crate::linkage) fn set_reference_idx(&mut self, next_reference_idx: usize) {
		match &mut *self {
			Self::Unique { resolution } | Self::Candidate { resolution, .. } => {
				resolution.reference_idx = next_reference_idx;
			}
			Self::External { reference_idx, .. }
			| Self::Blocked { reference_idx, .. }
			| Self::Dynamic { reference_idx, .. }
			| Self::Unknown { reference_idx, .. } => *reference_idx = next_reference_idx,
		}
	}

	pub(in crate::linkage) fn set_reference(
		&mut self,
		next_reference: ReferenceId,
		next_reference_idx: usize,
	) {
		match &mut *self {
			Self::Unique { resolution } | Self::Candidate { resolution, .. } => {
				resolution.reference = next_reference;
				resolution.reference_idx = next_reference_idx;
			}
			Self::External {
				reference,
				reference_idx,
				..
			}
			| Self::Blocked {
				reference,
				reference_idx,
				..
			}
			| Self::Dynamic {
				reference,
				reference_idx,
				..
			}
			| Self::Unknown {
				reference,
				reference_idx,
				..
			} => {
				*reference = next_reference;
				*reference_idx = next_reference_idx;
			}
		}
	}
}
