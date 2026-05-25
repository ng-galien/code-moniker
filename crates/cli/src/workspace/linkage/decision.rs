use crate::workspace::snapshot::{LinkageEdge, ReferenceRecord, SymbolId, UnresolvedReference};

#[derive(Default)]
pub(super) struct LinkageOutcome {
	pub(super) resolved_refs: usize,
	pub(super) external_refs: usize,
	pub(super) manifest_blocked_refs: usize,
	pub(super) unresolved_refs: usize,
	pub(super) ambiguous_refs: usize,
	pub(super) resolved: Vec<LinkageEdge>,
	pub(super) unresolved: Vec<UnresolvedReference>,
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

impl FromIterator<ReferenceLinkageDecision> for LinkageOutcome {
	fn from_iter<T: IntoIterator<Item = ReferenceLinkageDecision>>(iter: T) -> Self {
		let contributions = iter
			.into_iter()
			.map(LinkageContribution::from)
			.collect::<Vec<_>>();
		Self {
			resolved_refs: contributions
				.iter()
				.filter(|contribution| matches!(contribution, LinkageContribution::Resolved { .. }))
				.count(),
			external_refs: contributions
				.iter()
				.filter(|contribution| matches!(contribution, LinkageContribution::External))
				.count(),
			manifest_blocked_refs: contributions
				.iter()
				.filter(|contribution| {
					matches!(contribution, LinkageContribution::ManifestBlocked(_))
				})
				.count(),
			unresolved_refs: contributions
				.iter()
				.filter(|contribution| matches!(contribution, LinkageContribution::Unresolved(_)))
				.count(),
			ambiguous_refs: contributions
				.iter()
				.filter(|contribution| {
					matches!(
						contribution,
						LinkageContribution::Resolved {
							ambiguous: true,
							..
						}
					)
				})
				.count(),
			resolved: contributions
				.iter()
				.flat_map(LinkageContribution::resolved_edges)
				.cloned()
				.collect(),
			unresolved: contributions
				.iter()
				.filter_map(LinkageContribution::unresolved_reference)
				.cloned()
				.collect(),
		}
	}
}

enum LinkageContribution {
	Resolved {
		ambiguous: bool,
		edges: Vec<LinkageEdge>,
	},
	External,
	ManifestBlocked(UnresolvedReference),
	Unresolved(UnresolvedReference),
}

impl From<ReferenceLinkageDecision> for LinkageContribution {
	fn from(decision: ReferenceLinkageDecision) -> Self {
		match decision {
			ReferenceLinkageDecision::Resolved {
				scope: _scope,
				reference,
				targets,
			} => {
				let ambiguous = targets.len() > 1;
				Self::Resolved {
					ambiguous,
					edges: targets
						.into_iter()
						.map(|target| LinkageEdge::new(reference.id.clone(), target))
						.collect(),
				}
			}
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

impl LinkageContribution {
	fn resolved_edges(&self) -> &[LinkageEdge] {
		match self {
			Self::Resolved { edges, .. } => edges,
			Self::External | Self::ManifestBlocked(_) | Self::Unresolved(_) => &[],
		}
	}

	fn unresolved_reference(&self) -> Option<&UnresolvedReference> {
		match self {
			Self::ManifestBlocked(reference) | Self::Unresolved(reference) => Some(reference),
			Self::Resolved { .. } | Self::External => None,
		}
	}
}

fn unresolved_reference(reference: ReferenceRecord) -> UnresolvedReference {
	UnresolvedReference::new(reference.id, reference.target_identity)
}
