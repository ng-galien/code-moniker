use code_moniker_core::core::moniker::Moniker;

use crate::workspace::linkage::candidate::LinkageCandidate;
use crate::workspace::linkage::strategy::{LanguageLinkageStrategy, language_strategy};
use crate::workspace::snapshot::ReferenceRecord;
use crate::workspace::source::CodeIndexMaterial;

pub(super) struct LinkageQuery<'a> {
	pub(super) material: &'a CodeIndexMaterial,
	pub(super) target: &'a Moniker,
	pub(super) call_name: Option<&'a str>,
	pub(super) call_arity: Option<usize>,
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
			call_name: reference.call_name.as_deref(),
			call_arity: reference.call_arity,
			source_file,
			strategy: language_strategy(lang),
		})
	}

	pub(super) fn matches(&self, candidate: &LinkageCandidate<'_>) -> bool {
		self.strategy.matches(self, candidate)
	}
}
