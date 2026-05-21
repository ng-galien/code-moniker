use code_moniker_core::core::code_graph::RefRecord;
use code_moniker_core::lang::Lang;

use super::{DefLocation, LinkKey, SessionIndex};
use crate::workspace::index::IndexedFile;

mod java;

pub(super) trait LinkageStrategy: Sync {
	fn allow_generic_candidates(&self, _ctx: &LinkageQuery<'_>) -> bool {
		true
	}

	fn candidate_keys(&self, _ctx: &LinkageQuery<'_>, _out: &mut CandidateKeys) {}

	fn candidate_defs(&self, _ctx: &LinkageQuery<'_>, _out: &mut Vec<CandidateDef>) {}

	fn classify_unresolved(&self, _ctx: &LinkageQuery<'_>) -> UnresolvedClassification {
		UnresolvedClassification::Actionable
	}
}

pub(super) struct LinkageQuery<'a> {
	pub(super) index: &'a SessionIndex,
	pub(super) reference: &'a RefRecord,
	pub(super) source_file_idx: usize,
	pub(super) source_file: &'a IndexedFile,
}

#[derive(Default)]
pub(super) struct CandidateKeys {
	pub(super) exact: Vec<LinkKey>,
	pub(super) projectless: Vec<LinkKey>,
}

pub(super) struct CandidateDef {
	pub(super) loc: DefLocation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum UnresolvedClassification {
	Actionable,
	External,
	Suppressed,
}

static GENERIC: GenericLinkageStrategy = GenericLinkageStrategy;
static JAVA: java::JavaLinkageStrategy = java::JavaLinkageStrategy;
static STRATEGIES: [(Lang, &dyn LinkageStrategy); 1] = [(Lang::Java, &JAVA)];

pub(super) fn for_lang(lang: Lang) -> &'static dyn LinkageStrategy {
	STRATEGIES
		.iter()
		.find(|(strategy_lang, _)| *strategy_lang == lang)
		.map(|(_, strategy)| *strategy)
		.unwrap_or(&GENERIC)
}

struct GenericLinkageStrategy;

impl LinkageStrategy for GenericLinkageStrategy {}
