use std::sync::Arc;

use code_moniker_core::core::code_graph::DefRecord;
use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::core::moniker::{Moniker, Segment};
use code_moniker_core::lang::kinds;
use rustc_hash::FxHashMap;

use crate::linkage::catalog::LinkageQuery;
use crate::linkage::catalog::{SymbolOrdinal, SymbolOrdinalCatalog, SymbolSet};
use crate::source::{CodeIndexMaterial, IndexedSourceFile};

#[derive(Clone)]
pub(in crate::linkage) struct LinkageCandidate<'a> {
	pub(in crate::linkage) moniker: &'a Moniker,
	pub(in crate::linkage) last_segment: Option<Segment<'a>>,
	pub(in crate::linkage) segment_count: usize,
	pub(in crate::linkage) call_name: Option<&'a [u8]>,
	pub(in crate::linkage) call_arity: Option<usize>,
	pub(in crate::linkage) source_file: usize,
}

struct CandidateFileShard {
	file: Arc<IndexedSourceFile>,
	by_location: Vec<Option<SymbolOrdinal>>,
}

pub(in crate::linkage) struct CandidateCatalog {
	files: Vec<CandidateFileShard>,
	symbols: SymbolOrdinalCatalog,
	locations: Vec<Option<(u32, u32)>>,
	indexes: CandidateIndexes,
}

impl CandidateCatalog {
	pub(in crate::linkage) fn new(material: &CodeIndexMaterial) -> Self {
		let mut catalog = Self {
			files: Vec::with_capacity(material.files.len()),
			symbols: SymbolOrdinalCatalog::default(),
			locations: Vec::new(),
			indexes: CandidateIndexes::new(),
		};
		for (file_idx, file) in material.files.iter().enumerate() {
			push_file(&mut catalog, file_idx, Arc::clone(file));
		}
		catalog
	}

	pub(in crate::linkage) fn refresh_files(&mut self, material: &CodeIndexMaterial) {
		for (file_idx, file) in material.files.iter().enumerate() {
			if file_idx >= self.files.len() {
				push_file(self, file_idx, Arc::clone(file));
				continue;
			}
			if !Arc::ptr_eq(&self.files[file_idx].file, file) {
				refresh_file(self, file_idx, Arc::clone(file));
			}
		}
	}

	pub(in crate::linkage) fn symbols(&self) -> &SymbolOrdinalCatalog {
		&self.symbols
	}

	pub(in crate::linkage) fn candidate(
		&self,
		symbol: SymbolOrdinal,
	) -> Option<LinkageCandidate<'_>> {
		let (file_idx, def_idx) = self.locations.get(symbol.index()).copied().flatten()?;
		let shard = self.files.get(file_idx as usize)?;
		let def = shard.file.graph.def_at(def_idx as usize);
		Some(candidate(file_idx as usize, def))
	}

	pub(in crate::linkage) fn indexes(&self) -> &CandidateIndexes {
		&self.indexes
	}

	pub(in crate::linkage) fn candidate_for_symbol_id(
		&self,
		id: &crate::snapshot::SymbolId,
	) -> Option<(SymbolOrdinal, LinkageCandidate<'_>)> {
		let symbol = self.symbols.ordinal(id)?;
		Some((symbol, self.candidate(symbol)?))
	}

	pub(in crate::linkage) fn query_keys_for_symbol(
		&self,
		symbol: SymbolOrdinal,
	) -> Option<Vec<Vec<u8>>> {
		self.candidate(symbol)
			.map(|candidate| candidate_keys(&candidate))
	}

	pub(in crate::linkage) fn symbol_at(
		&self,
		file_idx: usize,
		def_idx: usize,
	) -> Option<SymbolOrdinal> {
		self.files
			.get(file_idx)?
			.by_location
			.get(def_idx)
			.copied()
			.flatten()
	}
}

pub(in crate::linkage) struct CandidateIndexes {
	by_moniker: FxHashMap<Moniker, SymbolOrdinal>,
	by_name: FxHashMap<Vec<u8>, SymbolSet>,
	by_source_name: FxHashMap<usize, FxHashMap<Vec<u8>, SymbolSet>>,
}

impl CandidateIndexes {
	fn new() -> Self {
		Self {
			by_moniker: FxHashMap::default(),
			by_name: FxHashMap::default(),
			by_source_name: FxHashMap::default(),
		}
	}

	fn push_candidate(&mut self, symbol: SymbolOrdinal, candidate: &LinkageCandidate<'_>) {
		self.by_moniker.insert(candidate.moniker.clone(), symbol);
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

	fn remove_candidate(&mut self, symbol: SymbolOrdinal, candidate: &LinkageCandidate<'_>) {
		if self
			.by_moniker
			.get(candidate.moniker)
			.is_some_and(|existing| *existing == symbol)
		{
			self.by_moniker.remove(candidate.moniker);
		}
		for key in candidate_keys(candidate) {
			if let Some(set) = self.by_name.get_mut(&key) {
				set.remove(symbol);
				if set.is_empty() {
					self.by_name.remove(&key);
				}
			}
			if let Some(source) = self.by_source_name.get_mut(&candidate.source_file) {
				if let Some(set) = source.get_mut(&key) {
					set.remove(symbol);
					if set.is_empty() {
						source.remove(&key);
					}
				}
				if source.is_empty() {
					self.by_source_name.remove(&candidate.source_file);
				}
			}
		}
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

fn push_file(catalog: &mut CandidateCatalog, file_idx: usize, file: Arc<IndexedSourceFile>) {
	let mut shard = CandidateFileShard {
		by_location: vec![None; file.graph.def_count()],
		file,
	};
	index_shard(catalog, file_idx, &mut shard);
	catalog.files.push(shard);
}

fn refresh_file(catalog: &mut CandidateCatalog, file_idx: usize, file: Arc<IndexedSourceFile>) {
	let old_shard = std::mem::replace(
		&mut catalog.files[file_idx],
		CandidateFileShard {
			file: Arc::clone(&file),
			by_location: Vec::new(),
		},
	);
	let old_ordinals = unindex_shard(catalog, file_idx, &old_shard);
	let mut shard = CandidateFileShard {
		by_location: vec![None; file.graph.def_count()],
		file,
	};
	index_shard(catalog, file_idx, &mut shard);
	catalog.files[file_idx] = shard;
	for ordinal in old_ordinals {
		let rebound_here = catalog
			.locations
			.get(ordinal.index())
			.copied()
			.flatten()
			.is_some_and(|(slot, _)| slot as usize == file_idx);
		if !rebound_here {
			catalog.symbols.retire(ordinal);
			if let Some(slot) = catalog.locations.get_mut(ordinal.index()) {
				*slot = None;
			}
		}
	}
}

fn index_shard(catalog: &mut CandidateCatalog, file_idx: usize, shard: &mut CandidateFileShard) {
	let file = Arc::clone(&shard.file);
	for (def_idx, def) in file.graph.defs().enumerate() {
		if !is_linkage_candidate_def(def) {
			continue;
		}
		let symbol_id = file.identity.symbol_id(file_idx, def_idx);
		let symbol_identity = file.identity.moniker_uri(&def.moniker);
		let symbol = catalog.symbols.push(symbol_id, symbol_identity);
		if catalog.locations.len() <= symbol.index() {
			catalog.locations.resize(symbol.index() + 1, None);
		}
		catalog.locations[symbol.index()] = Some((file_idx as u32, def_idx as u32));
		shard.by_location[def_idx] = Some(symbol);
		catalog
			.indexes
			.push_candidate(symbol, &candidate(file_idx, def));
	}
}

fn unindex_shard(
	catalog: &mut CandidateCatalog,
	file_idx: usize,
	shard: &CandidateFileShard,
) -> Vec<SymbolOrdinal> {
	let mut old_ordinals = Vec::new();
	for (def_idx, slot) in shard.by_location.iter().enumerate() {
		let Some(symbol) = *slot else {
			continue;
		};
		old_ordinals.push(symbol);
		catalog.symbols.unbind_id(symbol);
		let def = shard.file.graph.def_at(def_idx);
		catalog
			.indexes
			.remove_candidate(symbol, &candidate(file_idx, def));
	}
	old_ordinals
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
