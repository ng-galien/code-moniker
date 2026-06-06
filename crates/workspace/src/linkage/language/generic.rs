use crate::linkage::catalog::LinkageCandidate;
use crate::linkage::catalog::LinkageQuery;
use crate::linkage::language::LanguageLinkageStrategy;

pub(super) struct GenericLanguageLinkageStrategy;

impl LanguageLinkageStrategy for GenericLanguageLinkageStrategy {
	fn matches(&self, query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
		candidate.moniker.bind_match(query.target) || query.target.bind_match(candidate.moniker)
	}
}
