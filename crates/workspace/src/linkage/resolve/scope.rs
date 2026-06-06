use std::collections::BTreeSet;

use rustc_hash::FxHashSet;

use crate::linkage::catalog::SymbolSet;
use crate::linkage::catalog::{CandidateCatalog, LinkageQuery, SymbolOrdinal, query_keys};
use crate::linkage::language;

pub(in crate::linkage) struct LocalScopeResolver;

impl LocalScopeResolver {
	pub(in crate::linkage) fn resolve(
		&self,
		query: &LinkageQuery<'_>,
		candidates: &CandidateCatalog<'_>,
	) -> SymbolSet {
		local_symbols(candidates, query)
	}
}

pub(in crate::linkage) fn matches_any_source(
	catalog: &CandidateCatalog<'_>,
	query: &LinkageQuery<'_>,
	source_files: &BTreeSet<usize>,
) -> bool {
	CandidateSourceMatcher::new(catalog, query, source_files).matches()
}

pub(in crate::linkage) fn matches_any_symbol(
	catalog: &CandidateCatalog<'_>,
	query: &LinkageQuery<'_>,
	symbols: &SymbolSet,
) -> bool {
	symbols.iter().any(|symbol| {
		catalog
			.candidate(symbol)
			.is_some_and(|candidate| language::matches_candidate(query, candidate))
	})
}

fn local_symbols(catalog: &CandidateCatalog<'_>, query: &LinkageQuery<'_>) -> SymbolSet {
	matching_symbols(catalog, local_indexes(catalog, query), query)
}

fn global_symbols(catalog: &CandidateCatalog<'_>, query: &LinkageQuery<'_>) -> SymbolSet {
	matching_symbols(catalog, global_indexes(catalog, query), query)
}

fn local_indexes(catalog: &CandidateCatalog<'_>, query: &LinkageQuery<'_>) -> SymbolSet {
	let mut symbols = SymbolSet::new();
	for key in query_keys(query) {
		if let Some(candidate_indexes) = catalog
			.indexes()
			.symbols_by_source_key(query.source_file, &key)
		{
			for symbol in candidate_indexes.iter() {
				symbols.insert(symbol);
			}
		}
	}
	symbols
}

fn global_indexes(catalog: &CandidateCatalog<'_>, query: &LinkageQuery<'_>) -> SymbolSet {
	let mut symbols = SymbolSet::new();
	for key in query_keys(query) {
		if let Some(candidate_indexes) = catalog.indexes().symbols_by_key(&key) {
			for symbol in candidate_indexes.iter() {
				let Some(candidate) = catalog.candidate(symbol) else {
					continue;
				};
				if candidate.source_file == query.source_file {
					continue;
				}
				symbols.insert(symbol);
			}
		}
	}
	symbols
}

fn matching_symbols<'a>(
	catalog: &CandidateCatalog<'a>,
	indexes: SymbolSet,
	query: &LinkageQuery<'_>,
) -> SymbolSet {
	indexes
		.iter()
		.filter_map(|symbol| {
			catalog
				.candidate(symbol)
				.map(|candidate| (symbol, candidate))
		})
		.filter(|(_, candidate)| language::matches_candidate(query, candidate))
		.map(|(symbol, _)| symbol)
		.collect()
}

struct CandidateSourceMatcher<'a, 'q> {
	catalog: &'a CandidateCatalog<'a>,
	query: &'q LinkageQuery<'q>,
	source_files: &'q BTreeSet<usize>,
	seen: FxHashSet<SymbolOrdinal>,
	found: bool,
}

impl<'a, 'q> CandidateSourceMatcher<'a, 'q> {
	fn new(
		catalog: &'a CandidateCatalog<'a>,
		query: &'q LinkageQuery<'q>,
		source_files: &'q BTreeSet<usize>,
	) -> Self {
		Self {
			catalog,
			query,
			source_files,
			seen: FxHashSet::default(),
			found: false,
		}
	}

	fn matches(mut self) -> bool {
		if self.source_files.is_empty() {
			return false;
		}
		for key in query_keys(self.query) {
			self.visit_key(&key);
			if self.found {
				return true;
			}
		}
		self.found
	}

	fn visit_key(&mut self, key: &[u8]) {
		for source_file in self.source_files {
			self.visit_source_key(*source_file, key);
			if self.found {
				return;
			}
		}
	}

	fn visit_source_key(&mut self, source_file: usize, key: &[u8]) {
		let Some(indexes) = self
			.catalog
			.indexes()
			.symbols_by_source_key(source_file, key)
		else {
			return;
		};
		for symbol in indexes.iter() {
			if self.matches_symbol(symbol) {
				self.found = true;
				return;
			}
		}
	}

	fn matches_symbol(&mut self, symbol: SymbolOrdinal) -> bool {
		if !self.seen.insert(symbol) {
			return false;
		}
		self.catalog
			.candidate(symbol)
			.is_some_and(|candidate| language::matches_candidate(self.query, candidate))
	}
}

pub(in crate::linkage) struct GlobalScopeResolver;

impl GlobalScopeResolver {
	pub(in crate::linkage) fn resolve(
		&self,
		query: &LinkageQuery<'_>,
		candidates: &CandidateCatalog<'_>,
	) -> SymbolSet {
		global_symbols(candidates, query)
	}
}
