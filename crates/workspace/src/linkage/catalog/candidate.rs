use code_moniker_core::core::code_graph::DefRecord;
use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::core::moniker::{Moniker, Segment};
use code_moniker_core::lang::kinds;
use rustc_hash::FxHashMap;

use crate::linkage::catalog::LinkageQuery;
use crate::linkage::catalog::{SymbolOrdinal, SymbolOrdinalCatalog, SymbolSet};
use crate::source::CodeIndexMaterial;

#[derive(Clone)]
pub(in crate::linkage) struct LinkageCandidate<'a> {
	pub(in crate::linkage) moniker: &'a Moniker,
	pub(in crate::linkage) last_segment: Option<Segment<'a>>,
	pub(in crate::linkage) segment_count: usize,
	pub(in crate::linkage) call_name: Option<&'a [u8]>,
	pub(in crate::linkage) call_arity: Option<usize>,
	pub(in crate::linkage) source_file: usize,
}

pub(in crate::linkage) struct CandidateCatalog<'a> {
	candidates: Vec<LinkageCandidate<'a>>,
	symbols: SymbolOrdinalCatalog,
	indexes: CandidateIndexes<'a>,
}

impl<'a> CandidateCatalog<'a> {
	pub(in crate::linkage) fn new(material: &'a CodeIndexMaterial) -> Self {
		CandidateCatalogBuilder::new().build(material)
	}

	pub(in crate::linkage) fn symbols(&self) -> &SymbolOrdinalCatalog {
		&self.symbols
	}

	pub(in crate::linkage) fn candidate(
		&self,
		symbol: SymbolOrdinal,
	) -> Option<&LinkageCandidate<'a>> {
		self.candidates.get(symbol.index())
	}

	pub(in crate::linkage) fn indexes(&self) -> &CandidateIndexes<'a> {
		&self.indexes
	}

	pub(in crate::linkage) fn candidate_for_symbol_id(
		&self,
		id: &crate::snapshot::SymbolId,
	) -> Option<(SymbolOrdinal, &LinkageCandidate<'a>)> {
		let symbol = self.symbols.ordinal(id)?;
		Some((symbol, self.candidate(symbol)?))
	}

	pub(in crate::linkage) fn query_keys_for_symbol(
		&self,
		symbol: SymbolOrdinal,
	) -> Option<Vec<Vec<u8>>> {
		self.candidate(symbol).map(candidate_keys)
	}
}

pub(in crate::linkage) struct CandidateIndexes<'a> {
	by_location: Vec<Vec<Option<SymbolOrdinal>>>,
	by_moniker: FxHashMap<&'a Moniker, SymbolOrdinal>,
	by_name: FxHashMap<Vec<u8>, SymbolSet>,
	by_source_name: FxHashMap<usize, FxHashMap<Vec<u8>, SymbolSet>>,
}

impl<'a> CandidateIndexes<'a> {
	fn new() -> Self {
		Self {
			by_location: Vec::new(),
			by_moniker: FxHashMap::default(),
			by_name: FxHashMap::default(),
			by_source_name: FxHashMap::default(),
		}
	}

	fn begin_file(&mut self, def_count: usize) {
		self.by_location.push(vec![None; def_count]);
	}

	fn push_location(&mut self, file_idx: usize, def_idx: usize, symbol: SymbolOrdinal) {
		if let Some(file) = self.by_location.get_mut(file_idx) {
			if let Some(slot) = file.get_mut(def_idx) {
				*slot = Some(symbol);
			}
		}
	}

	fn push_candidate(&mut self, symbol: SymbolOrdinal, candidate: &LinkageCandidate<'a>) {
		self.by_moniker.insert(candidate.moniker, symbol);
		for key in candidate_keys(candidate) {
			self.by_name.entry(key.clone()).or_default().insert(symbol);
			self.by_source_name
				.entry(candidate.source_file)
				.or_default()
				.entry(key)
				.or_default()
				.insert(symbol);
		}
	}

	pub(in crate::linkage) fn symbol_at(
		&self,
		file_idx: usize,
		def_idx: usize,
	) -> Option<SymbolOrdinal> {
		self.by_location
			.get(file_idx)?
			.get(def_idx)
			.copied()
			.flatten()
	}

	pub(in crate::linkage) fn symbol_by_moniker(&self, moniker: &Moniker) -> Option<SymbolOrdinal> {
		self.by_moniker.get(moniker).copied()
	}

	pub(in crate::linkage) fn source_candidate_keys(
		&self,
		source_file: usize,
	) -> Option<impl Iterator<Item = &[u8]>> {
		self.by_source_name
			.get(&source_file)
			.map(|keys| keys.keys().map(|key| key.as_slice()))
	}

	pub(in crate::linkage) fn symbols_by_key(&self, key: &[u8]) -> Option<&SymbolSet> {
		self.by_name.get(key)
	}

	pub(in crate::linkage) fn symbols_by_source_key(
		&self,
		source_file: usize,
		key: &[u8],
	) -> Option<&SymbolSet> {
		self.by_source_name.get(&source_file)?.get(key)
	}
}

struct CandidateCatalogBuilder<'a> {
	catalog: CandidateCatalog<'a>,
}

impl<'a> CandidateCatalogBuilder<'a> {
	fn new() -> Self {
		Self {
			catalog: CandidateCatalog {
				candidates: Vec::new(),
				symbols: SymbolOrdinalCatalog::default(),
				indexes: CandidateIndexes::new(),
			},
		}
	}

	fn build(mut self, material: &'a CodeIndexMaterial) -> CandidateCatalog<'a> {
		for (file_idx, file) in material.files.iter().enumerate() {
			self.catalog.indexes.begin_file(file.graph.def_count());
			for (def_idx, def) in file.graph.defs().enumerate() {
				if !is_linkage_candidate_def(def) {
					continue;
				}
				let symbol_id = file.identity.symbol_id(file_idx, def_idx);
				let symbol_identity = file.identity.moniker_uri(&def.moniker);
				let symbol = self.catalog.symbols.push(symbol_id, symbol_identity);
				self.catalog
					.indexes
					.push_location(file_idx, def_idx, symbol);
				self.push_candidate(symbol, candidate(file_idx, def));
			}
		}
		self.catalog
	}

	fn push_candidate(&mut self, symbol: SymbolOrdinal, candidate: LinkageCandidate<'a>) {
		self.catalog.indexes.push_candidate(symbol, &candidate);
		self.catalog.candidates.push(candidate);
	}
}

fn candidate(file_idx: usize, def: &DefRecord) -> LinkageCandidate<'_> {
	let segment_summary = candidate_segment_summary(&def.moniker);
	LinkageCandidate {
		moniker: &def.moniker,
		last_segment: segment_summary.last,
		segment_count: segment_summary.count,
		call_name: (!def.call_name.is_empty()).then_some(def.call_name.as_ref()),
		call_arity: def.call_arity,
		source_file: file_idx,
	}
}

struct CandidateSegmentSummary<'a> {
	last: Option<Segment<'a>>,
	count: usize,
}

fn candidate_segment_summary(moniker: &Moniker) -> CandidateSegmentSummary<'_> {
	let mut summary = CandidateSegmentSummary {
		last: None,
		count: 0,
	};
	for segment in moniker.as_view().segments() {
		summary.last = Some(segment);
		summary.count += 1;
	}
	summary
}

pub(in crate::linkage) fn query_keys(query: &LinkageQuery<'_>) -> Vec<Vec<u8>> {
	let mut keys = Vec::new();
	for_query_key(query, |key| keys.push(key.to_vec()));
	keys
}

fn for_query_key(query: &LinkageQuery<'_>, mut visit: impl FnMut(&[u8])) {
	let mut first = None;
	if let Some(name) = query.call_name {
		let key = name.as_bytes();
		if !key.is_empty() {
			first = Some(key);
			visit(key);
		}
	}
	if let Some(name) = query
		.target_last
		.map(|segment| bare_callable_name(segment.name))
	{
		if !name.is_empty() && first != Some(name) {
			visit(name);
		}
	}
}

fn candidate_keys(candidate: &LinkageCandidate<'_>) -> Vec<Vec<u8>> {
	let mut keys = Vec::new();
	if let Some(name) = candidate.call_name {
		push_key(&mut keys, name);
	}
	if let Some(segment) = candidate.last_segment {
		push_key(&mut keys, bare_callable_name(segment.name));
	}
	keys
}

fn push_key(keys: &mut Vec<Vec<u8>>, key: &[u8]) {
	if key.is_empty() || keys.iter().any(|existing| existing.as_slice() == key) {
		return;
	}
	keys.push(key.to_vec());
}

fn is_linkage_candidate_def(def: &DefRecord) -> bool {
	if matches!(def.kind.as_ref(), kinds::COMMENT) {
		return false;
	}
	!has_position_backed_anonymous_name(&def.moniker)
}

fn has_position_backed_anonymous_name(moniker: &Moniker) -> bool {
	moniker
		.as_view()
		.segments()
		.last()
		.is_some_and(|segment| segment.name.starts_with(b"__cb_"))
}
