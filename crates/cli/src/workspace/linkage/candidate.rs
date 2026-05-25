use code_moniker_core::core::moniker::Moniker;

use crate::workspace::linkage::query::LinkageQuery;
use crate::workspace::snapshot::SymbolId;
use crate::workspace::source::CodeIndexMaterial;

pub(super) struct LinkageCandidate<'a> {
	pub(super) symbol: &'a SymbolId,
	pub(super) moniker: &'a Moniker,
	pub(super) call_name: Option<&'a [u8]>,
	pub(super) call_arity: Option<usize>,
	pub(super) source_file: usize,
}

pub(super) struct CandidateCatalog<'a> {
	material: &'a CodeIndexMaterial,
}

impl<'a> CandidateCatalog<'a> {
	pub(super) fn new(material: &'a CodeIndexMaterial) -> Self {
		Self { material }
	}

	pub(super) fn local_matches(&self, query: &LinkageQuery<'_>) -> Vec<LinkageCandidate<'a>> {
		self.matches(|candidate| {
			candidate.source_file == query.source_file && query.matches(candidate)
		})
	}

	pub(super) fn global_matches(&self, query: &LinkageQuery<'_>) -> Vec<LinkageCandidate<'a>> {
		self.matches(|candidate| {
			candidate.source_file != query.source_file && query.matches(candidate)
		})
	}

	fn matches(&self, accept: impl Fn(&LinkageCandidate<'_>) -> bool) -> Vec<LinkageCandidate<'a>> {
		self.material
			.symbol_monikers
			.iter()
			.filter_map(|(symbol, moniker)| self.candidate(symbol, moniker))
			.filter(accept)
			.collect()
	}

	fn candidate(
		&self,
		symbol: &'a SymbolId,
		moniker: &'a Moniker,
	) -> Option<LinkageCandidate<'a>> {
		let (source_file, def_idx) = self.material.identity.symbol_location(symbol)?;
		let def = self
			.material
			.files
			.get(source_file)?
			.graph
			.defs()
			.nth(def_idx)?;
		Some(LinkageCandidate {
			symbol,
			moniker,
			call_name: (!def.call_name.is_empty()).then_some(def.call_name.as_slice()),
			call_arity: def.call_arity,
			source_file,
		})
	}
}

pub(super) fn candidate_symbols(candidates: Vec<LinkageCandidate<'_>>) -> Vec<SymbolId> {
	candidates
		.into_iter()
		.map(|candidate| candidate.symbol.clone())
		.collect()
}
