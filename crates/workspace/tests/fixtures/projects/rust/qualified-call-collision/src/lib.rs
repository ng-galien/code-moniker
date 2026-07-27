pub mod cross_file;

use crate::cross_file::{CrossFileClone, DerivedClone};

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

pub struct CloneCollision;

impl CloneCollision {
	pub fn clone(&self) -> Self {
		Self
	}
}

pub fn clone_sdk_path(path: &std::path::PathBuf) -> std::path::PathBuf {
	path.clone()
}

pub fn clone_local(value: &CloneCollision) -> CloneCollision {
	value.clone()
}

pub fn clone_cross_file(value: &CrossFileClone) -> CrossFileClone {
	value.clone()
}

pub fn clone_cross_file_derived(value: &DerivedClone) -> DerivedClone {
	value.clone()
}
