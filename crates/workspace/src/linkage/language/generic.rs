use crate::linkage::language::LanguageLinkageStrategy;
use crate::linkage::resolution::LinkageCandidate;
use crate::linkage::resolution::LinkageQuery;

pub(super) struct GenericLanguageLinkageStrategy;

impl LanguageLinkageStrategy for GenericLanguageLinkageStrategy {
	fn matches(&self, query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
		candidate.moniker.bind_match(query.target) || query.target.bind_match(candidate.moniker)
	}
}
