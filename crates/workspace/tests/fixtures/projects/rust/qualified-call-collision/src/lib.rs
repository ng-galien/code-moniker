pub mod check {
	pub mod path {
		pub struct Pattern;
		pub struct Moniker;

		pub fn matches(pattern: &Pattern, m: &Moniker) -> bool {
			pattern.accepts(m)
		}

		impl Pattern {
			pub fn accepts(&self, _m: &Moniker) -> bool {
				true
			}
		}
	}
}

pub mod linkage {
	pub mod language {
		pub mod generic {
			pub struct GenericLanguageLinkageStrategy;
			pub struct LinkageCandidate;
			pub struct LinkageQuery;

			pub trait LanguageLinkageStrategy {
				fn matches(&self, query: &LinkageQuery, candidate: &LinkageCandidate) -> bool;
			}

			impl LanguageLinkageStrategy for GenericLanguageLinkageStrategy {
				fn matches(&self, _query: &LinkageQuery, _candidate: &LinkageCandidate) -> bool {
					true
				}
			}
		}
	}
}

pub fn uses_qualified_path_matches(
	pattern: &check::path::Pattern,
	m: &check::path::Moniker,
) -> bool {
	crate::check::path::matches(pattern, m)
}
