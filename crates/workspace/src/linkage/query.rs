use code_moniker_core::core::moniker::Moniker;

use crate::linkage::candidate::LinkageCandidate;
use crate::linkage::strategy::{LanguageLinkageStrategy, language_strategy};
use crate::snapshot::ReferenceRecord;
use crate::source::CodeIndexMaterial;

pub(super) struct LinkageQuery<'a> {
	pub(super) material: &'a CodeIndexMaterial,
	pub(super) target: &'a Moniker,
	pub(super) reference_kind: &'a str,
	pub(super) call_name: Option<&'a str>,
	pub(super) call_arity: Option<usize>,
	pub(super) confidence: Option<&'a str>,
	pub(super) source_file: usize,
	strategy: &'static dyn LanguageLinkageStrategy,
}

impl<'a> LinkageQuery<'a> {
	pub(super) fn new(
		reference: &'a ReferenceRecord,
		material: &'a CodeIndexMaterial,
	) -> Option<Self> {
		let target = material.reference_targets.get(&reference.id)?;
		let source_file = material.identity.source_index(&reference.source)?;
		let lang = material.files.get(source_file)?.lang;
		Some(Self {
			material,
			target,
			reference_kind: reference.kind.as_str(),
			call_name: reference.call_name.as_deref(),
			call_arity: reference.call_arity,
			confidence: reference.confidence.as_deref(),
			source_file,
			strategy: language_strategy(lang),
		})
	}

	pub(super) fn matches(&self, candidate: &LinkageCandidate<'_>) -> bool {
		self.strategy.matches(self, candidate)
	}
}
