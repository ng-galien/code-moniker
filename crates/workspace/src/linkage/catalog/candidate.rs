use std::sync::Arc;

use code_moniker_core::core::code_graph::DefRecord;
use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::core::moniker::{Moniker, Segment};
use code_moniker_core::lang::Lang;
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
	pub(in crate::linkage) visibility: &'a [u8],
	pub(in crate::linkage) source_file: usize,
}

struct CandidateFileShard {
	file: Arc<IndexedSourceFile>,
	by_location: Vec<Option<SymbolOrdinal>>,
}

type NameIndex = FxHashMap<Vec<u8>, SymbolSet>;

pub(in crate::linkage) struct CandidateCatalog {
	files: Vec<CandidateFileShard>,
	symbols: Arc<SymbolOrdinalCatalog>,
	locations: FxHashMap<SymbolOrdinal, (u32, u32)>,
	indexes: CandidateIndexes,
}

impl CandidateCatalog {
	pub(in crate::linkage) fn new(
		material: &CodeIndexMaterial,
		symbols: Arc<SymbolOrdinalCatalog>,
	) -> Self {
		let mut catalog = Self {
			files: Vec::with_capacity(material.files.len()),
			symbols,
			locations: FxHashMap::default(),
			indexes: CandidateIndexes::new(),
		};
		for (file_idx, file) in material.files.iter().enumerate() {
			push_file(&mut catalog, file_idx, Arc::clone(file));
		}
		catalog
	}

	pub(in crate::linkage) fn refresh_files(
		&mut self,
		material: &CodeIndexMaterial,
		symbols: Arc<SymbolOrdinalCatalog>,
	) {
		self.symbols = symbols;
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

	pub(in crate::linkage) fn candidate(
		&self,
		symbol: SymbolOrdinal,
	) -> Option<LinkageCandidate<'_>> {
		let (file_idx, def_idx) = self.locations.get(&symbol).copied()?;
		let shard = self.files.get(file_idx as usize)?;
		let def = shard.file.graph.def_at(def_idx as usize);
		Some(candidate(file_idx as usize, def))
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
	by_language_name: FxHashMap<Lang, NameIndex>,
	by_source_name: FxHashMap<usize, NameIndex>,
}

impl CandidateIndexes {
	fn new() -> Self {
		Self {
			by_moniker: FxHashMap::default(),
			by_language_name: FxHashMap::default(),
			by_source_name: FxHashMap::default(),
		}
	}

	fn push_candidate(
		&mut self,
		lang: Lang,
		symbol: SymbolOrdinal,
		candidate: &LinkageCandidate<'_>,
	) {
		self.by_moniker.insert(candidate.moniker.clone(), symbol);
		for key in candidate_keys(candidate) {
			if is_global_candidate(candidate) {
				insert_partitioned_name(&mut self.by_language_name, lang, &key, symbol);
			}
			insert_partitioned_name(
				&mut self.by_source_name,
				candidate.source_file,
				&key,
				symbol,
			);
		}
	}

	fn remove_candidate(
		&mut self,
		language: Lang,
		ordinal: SymbolOrdinal,
		entry: &LinkageCandidate<'_>,
	) {
		if self
			.by_moniker
			.get(entry.moniker)
			.is_some_and(|existing| *existing == ordinal)
		{
			self.by_moniker.remove(entry.moniker);
		}
		for key in candidate_keys(entry) {
			if is_global_candidate(entry) {
				remove_partitioned_name(&mut self.by_language_name, language, &key, ordinal);
			}
			remove_partitioned_name(&mut self.by_source_name, entry.source_file, &key, ordinal);
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

	pub(in crate::linkage) fn symbols_by_language_key(
		&self,
		lang: Lang,
		key: &[u8],
	) -> Option<&SymbolSet> {
		self.by_language_name.get(&lang)?.get(key)
	}

	pub(in crate::linkage) fn symbols_by_source_key(
		&self,
		source_file: usize,
		key: &[u8],
	) -> Option<&SymbolSet> {
		self.by_source_name.get(&source_file)?.get(key)
	}
}

fn insert_name(index: &mut NameIndex, key: &[u8], symbol: SymbolOrdinal) {
	index.entry(key.to_vec()).or_default().insert(symbol);
}

fn remove_name(index: &mut NameIndex, key: &[u8], symbol: SymbolOrdinal) {
	if let Some(set) = index.get_mut(key) {
		set.remove(symbol);
		if set.is_empty() {
			index.remove(key);
		}
	}
}

fn insert_partitioned_name<K: Copy + Eq + std::hash::Hash>(
	index: &mut FxHashMap<K, NameIndex>,
	partition: K,
	key: &[u8],
	symbol: SymbolOrdinal,
) {
	insert_name(index.entry(partition).or_default(), key, symbol);
}

fn remove_partitioned_name<K: Copy + Eq + std::hash::Hash>(
	index: &mut FxHashMap<K, NameIndex>,
	partition: K,
	key: &[u8],
	symbol: SymbolOrdinal,
) {
	let Some(names) = index.get_mut(&partition) else {
		return;
	};
	remove_name(names, key, symbol);
	if names.is_empty() {
		index.remove(&partition);
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
	unindex_shard(catalog, file_idx, &old_shard);
	let mut shard = CandidateFileShard {
		by_location: vec![None; file.graph.def_count()],
		file,
	};
	index_shard(catalog, file_idx, &mut shard);
	catalog.files[file_idx] = shard;
}

fn index_shard(catalog: &mut CandidateCatalog, file_idx: usize, shard: &mut CandidateFileShard) {
	let file = Arc::clone(&shard.file);
	for (def_idx, def) in file.graph.defs().enumerate() {
		if !is_linkage_candidate_def(def) {
			continue;
		}
		let symbol_id = file.identity.symbol_id(file_idx, def_idx);
		let Some(symbol) = catalog.symbols.ordinal(&symbol_id) else {
			continue;
		};
		catalog
			.locations
			.insert(symbol, (file_idx as u32, def_idx as u32));
		shard.by_location[def_idx] = Some(symbol);
		catalog
			.indexes
			.push_candidate(file.lang, symbol, &candidate(file_idx, def));
	}
}

fn unindex_shard(catalog: &mut CandidateCatalog, file_idx: usize, shard: &CandidateFileShard) {
	for (def_idx, slot) in shard.by_location.iter().enumerate() {
		let Some(symbol) = *slot else {
			continue;
		};
		catalog.locations.remove(&symbol);
		let def = shard.file.graph.def_at(def_idx);
		catalog
			.indexes
			.remove_candidate(shard.file.lang, symbol, &candidate(file_idx, def));
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
		visibility: def.visibility.as_ref(),
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
		if segment.kind == kinds::CLASS
			&& let Some(short_name) = segment.name.strip_suffix(b"Attribute")
		{
			push_key(&mut keys, short_name);
		}
	}
	if let Some(package) = python_init_package_name(candidate) {
		push_key(&mut keys, package);
	}
	keys
}

// A Python package's __init__ module answers to the package name: `import
// httpx` must find package:httpx/module:__init__ in the name index.
fn python_init_package_name<'a>(candidate: &'a LinkageCandidate<'_>) -> Option<&'a [u8]> {
	let last = candidate.last_segment?;
	if last.kind != kinds::MODULE || last.name != b"__init__" {
		return None;
	}
	let segments = candidate.moniker.as_view().segments().collect::<Vec<_>>();
	let [.., before, _] = segments.as_slice() else {
		return None;
	};
	(before.kind == kinds::PACKAGE).then_some(before.name)
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

fn is_global_candidate(candidate: &LinkageCandidate<'_>) -> bool {
	!candidate
		.last_segment
		.is_some_and(|segment| matches!(segment.kind, kinds::LOCAL | kinds::PARAM))
}

fn has_position_backed_anonymous_name(moniker: &Moniker) -> bool {
	moniker
		.as_view()
		.segments()
		.last()
		.is_some_and(|segment| segment.name.starts_with(b"__cb_"))
}
