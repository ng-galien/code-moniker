use code_moniker_core::lang::Lang;

use crate::linkage::candidate::LinkageCandidate;
use crate::linkage::query::LinkageQuery;

mod generic;
mod java;

pub(super) trait LanguageLinkageStrategy: Sync {
	fn matches(&self, query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool;
}

static GENERIC_STRATEGY: generic::GenericLanguageLinkageStrategy =
	generic::GenericLanguageLinkageStrategy;
static JAVA_STRATEGY: java::JavaLanguageLinkageStrategy = java::JavaLanguageLinkageStrategy;

pub(super) fn language_strategy(lang: Lang) -> &'static dyn LanguageLinkageStrategy {
	match lang {
		Lang::Java => &JAVA_STRATEGY,
		Lang::Ts | Lang::Rs | Lang::Python | Lang::Go | Lang::Cs | Lang::Sql => &GENERIC_STRATEGY,
	}
}
