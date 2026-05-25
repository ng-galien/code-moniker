use crate::workspace::linkage::candidate::LinkageCandidate;
use crate::workspace::linkage::query::LinkageQuery;
use crate::workspace::linkage::strategy::LanguageLinkageStrategy;

pub(super) struct GenericLanguageLinkageStrategy;

impl LanguageLinkageStrategy for GenericLanguageLinkageStrategy {
	fn matches(&self, query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
		candidate.moniker.bind_match(query.target) || query.target.bind_match(candidate.moniker)
	}
}
