//! Moniker predicates shared by CLI argument parsing and graph filtering.

use code_moniker_core::core::moniker::Moniker;

#[derive(Clone, Debug)]
pub enum Predicate {
	Eq(Moniker),
	Lt(Moniker),
	Le(Moniker),
	Gt(Moniker),
	Ge(Moniker),
	AncestorOf(Moniker),
	DescendantOf(Moniker),
	Bind(Moniker),
}

impl Predicate {
	pub fn matches(&self, m: &Moniker) -> bool {
		match self {
			Self::Eq(o) => m == o,
			Self::Lt(o) => m < o,
			Self::Le(o) => m <= o,
			Self::Gt(o) => m > o,
			Self::Ge(o) => m >= o,
			Self::AncestorOf(o) => m.is_ancestor_of(o),
			Self::DescendantOf(o) => o.is_ancestor_of(m),
			Self::Bind(o) => m.bind_match(o),
		}
	}
}
