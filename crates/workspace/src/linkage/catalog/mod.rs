mod candidate;
mod ordinals;
mod query;

pub(in crate::linkage) use candidate::{CandidateCatalog, LinkageCandidate, query_keys};
pub(in crate::linkage) use ordinals::{
	ReferenceOrdinal, ReferenceSet, SymbolOrdinal, SymbolOrdinalCatalog, SymbolSet,
};
pub(in crate::linkage) use query::{LinkageQuery, ReferenceLocation, ReferenceLocations};
