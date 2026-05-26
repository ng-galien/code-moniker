use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::core::moniker::query::bare_callable_name;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::linkage::query::LinkageQuery;
use crate::snapshot::SymbolId;
use crate::source::CodeIndexMaterial;

#[derive(Clone, Copy)]
pub(super) struct LinkageCandidate<'a> {
	pub(super) symbol: &'a SymbolId,
	pub(super) moniker: &'a Moniker,
	pub(super) call_name: Option<&'a [u8]>,
	pub(super) call_arity: Option<usize>,
	pub(super) source_file: usize,
}

pub(super) struct CandidateCatalog<'a> {
	candidates: Vec<LinkageCandidate<'a>>,
	by_name: FxHashMap<Vec<u8>, Vec<usize>>,
	by_source_name: FxHashMap<(usize, Vec<u8>), Vec<usize>>,
}

impl<'a> CandidateCatalog<'a> {
	pub(super) fn new(material: &'a CodeIndexMaterial) -> Self {
		let mut catalog = Self {
			candidates: Vec::new(),
			by_name: FxHashMap::default(),
			by_source_name: FxHashMap::default(),
		};
		for (symbol, moniker) in &material.symbol_monikers {
			let Some(candidate) = candidate(material, symbol, moniker) else {
				continue;
			};
			let idx = catalog.candidates.len();
			for key in candidate_keys(candidate.moniker, candidate.call_name) {
				catalog.by_name.entry(key.clone()).or_default().push(idx);
				catalog
					.by_source_name
					.entry((candidate.source_file, key))
					.or_default()
					.push(idx);
			}
			catalog.candidates.push(candidate);
		}
		catalog
	}

	pub(super) fn local_matches(&self, query: &LinkageQuery<'_>) -> Vec<LinkageCandidate<'a>> {
		self.lookup_local(query)
			.into_iter()
			.filter(|candidate| query.matches(candidate))
			.collect()
	}

	pub(super) fn global_matches(&self, query: &LinkageQuery<'_>) -> Vec<LinkageCandidate<'a>> {
		self.lookup_global(query)
			.into_iter()
			.filter(|candidate| query.matches(candidate))
			.collect()
	}

	fn lookup_local(&self, query: &LinkageQuery<'_>) -> Vec<LinkageCandidate<'a>> {
		let mut seen = FxHashSet::default();
		let mut matches = Vec::new();
		for key in query_keys(query) {
			let Some(indexes) = self.by_source_name.get(&(query.source_file, key)) else {
				continue;
			};
			self.push_candidates(indexes, &mut seen, &mut matches);
		}
		matches
	}

	fn lookup_global(&self, query: &LinkageQuery<'_>) -> Vec<LinkageCandidate<'a>> {
		let mut seen = FxHashSet::default();
		let mut matches = Vec::new();
		for key in query_keys(query) {
			let Some(indexes) = self.by_name.get(&key) else {
				continue;
			};
			for idx in indexes {
				let candidate = self.candidates[*idx];
				if candidate.source_file == query.source_file || !seen.insert(*idx) {
					continue;
				}
				matches.push(candidate);
			}
		}
		matches
	}

	fn push_candidates(
		&self,
		indexes: &[usize],
		seen: &mut FxHashSet<usize>,
		matches: &mut Vec<LinkageCandidate<'a>>,
	) {
		for idx in indexes {
			if seen.insert(*idx) {
				matches.push(self.candidates[*idx]);
			}
		}
	}
}

pub(super) fn candidate_symbols(candidates: Vec<LinkageCandidate<'_>>) -> Vec<SymbolId> {
	candidates
		.into_iter()
		.map(|candidate| candidate.symbol.clone())
		.collect()
}

fn candidate<'a>(
	material: &'a CodeIndexMaterial,
	symbol: &'a SymbolId,
	moniker: &'a Moniker,
) -> Option<LinkageCandidate<'a>> {
	let (source_file, def_idx) = material.identity.symbol_location(symbol)?;
	let def = material.files.get(source_file)?.graph.defs().nth(def_idx)?;
	Some(LinkageCandidate {
		symbol,
		moniker,
		call_name: (!def.call_name.is_empty()).then_some(def.call_name.as_slice()),
		call_arity: def.call_arity,
		source_file,
	})
}

fn query_keys(query: &LinkageQuery<'_>) -> Vec<Vec<u8>> {
	let mut keys = Vec::new();
	if let Some(name) = query.call_name {
		push_key(&mut keys, name.as_bytes());
	}
	if let Some(name) = last_segment_name(query.target) {
		push_key(&mut keys, name);
	}
	keys
}

fn candidate_keys(moniker: &Moniker, call_name: Option<&[u8]>) -> Vec<Vec<u8>> {
	let mut keys = Vec::new();
	if let Some(name) = call_name {
		push_key(&mut keys, name);
	}
	if let Some(name) = last_segment_name(moniker) {
		push_key(&mut keys, name);
	}
	if let Some(name) = rust_mod_rs_module_name(moniker) {
		push_key(&mut keys, name);
	}
	keys
}

fn last_segment_name(moniker: &Moniker) -> Option<&[u8]> {
	moniker
		.as_view()
		.segments()
		.last()
		.map(|segment| bare_callable_name(segment.name))
}

fn rust_mod_rs_module_name(moniker: &Moniker) -> Option<&[u8]> {
	let segments = moniker.as_view().segments().collect::<Vec<_>>();
	let [.., parent, leaf] = segments.as_slice() else {
		return None;
	};
	(parent.kind == b"dir" && leaf.kind == b"module" && leaf.name == b"mod").then_some(parent.name)
}

fn push_key(keys: &mut Vec<Vec<u8>>, key: &[u8]) {
	if key.is_empty() || keys.iter().any(|existing| existing.as_slice() == key) {
		return;
	}
	keys.push(key.to_vec());
}
