use std::path::PathBuf;

use code_moniker_core::lang::build_manifest::Manifest;

use crate::code::CodeIndexGraphDiff;
use crate::snapshot::{ReferenceId, SourceId, SymbolId};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinkageGraphDelta {
	references: ReferenceDelta,
	symbols: SymbolDelta,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinkageRefreshImpact {
	scope: RefreshScope,
	references: ReferenceDelta,
	symbols: SymbolDelta,
	precision: LinkageDiffPrecision,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum LinkageDiffPrecision {
	#[default]
	SourceLevel,
	Precise,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::linkage) struct RefreshScope {
	changed_sources: Vec<SourceId>,
	changed_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::linkage) enum ReferenceDelta {
	#[default]
	Unchanged,
	Changed {
		changed: Vec<ReferenceId>,
		remapped: Vec<(ReferenceId, ReferenceId)>,
	},
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::linkage) enum SymbolDelta {
	#[default]
	Unchanged,
	AdditiveOnly {
		added: Vec<SymbolId>,
	},
	RemovedOnly {
		removed: Vec<SymbolId>,
		retargeted_identities: Vec<String>,
	},
	Mixed {
		candidate_changed: Vec<SymbolId>,
		changed: Vec<SymbolId>,
		retargeted_identities: Vec<String>,
		remapped: Vec<(SymbolId, SymbolId)>,
	},
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::linkage) enum LinkageRefreshShape<'a> {
	Empty,
	SourceLevel,
	ManifestPolicy,
	AdditiveSymbolsOnly(&'a [SymbolId]),
	RemovedSymbolsOnly(&'a [SymbolId]),
	LinkageRelevant,
}

impl LinkageRefreshImpact {
	pub fn new(changed_sources: Vec<SourceId>, changed_paths: Vec<PathBuf>) -> Self {
		Self {
			scope: RefreshScope::new(changed_sources, changed_paths),
			references: ReferenceDelta::Unchanged,
			symbols: SymbolDelta::Unchanged,
			precision: LinkageDiffPrecision::SourceLevel,
		}
	}

	pub fn with_graph_delta(
		changed_sources: Vec<SourceId>,
		changed_paths: Vec<PathBuf>,
		graph_delta: LinkageGraphDelta,
	) -> Self {
		Self {
			scope: RefreshScope::new(changed_sources, changed_paths),
			references: graph_delta.references,
			symbols: graph_delta.symbols,
			precision: LinkageDiffPrecision::Precise,
		}
	}

	pub fn is_empty(&self) -> bool {
		self.scope.is_empty()
			&& self.references.is_empty()
			&& symbol_delta_is_unchanged(&self.symbols)
	}

	pub(in crate::linkage) fn shape(&self) -> LinkageRefreshShape<'_> {
		classify_refresh_shape(self)
	}

	pub(in crate::linkage) fn changed_sources(&self) -> &[SourceId] {
		self.scope.changed_sources()
	}

	pub(in crate::linkage) fn changed_paths(&self) -> &[PathBuf] {
		self.scope.changed_paths()
	}

	pub(in crate::linkage) fn has_precise_graph_diff(&self) -> bool {
		self.precision == LinkageDiffPrecision::Precise
	}
}

impl LinkageGraphDelta {
	pub fn from_code_index(graph_diff: CodeIndexGraphDiff) -> Self {
		Self {
			references: ReferenceDelta::from_code_index(&graph_diff),
			symbols: SymbolDelta::from_code_index(graph_diff),
		}
	}
}

impl From<CodeIndexGraphDiff> for LinkageGraphDelta {
	fn from(graph_diff: CodeIndexGraphDiff) -> Self {
		Self::from_code_index(graph_diff)
	}
}

fn classify_refresh_shape(impact: &LinkageRefreshImpact) -> LinkageRefreshShape<'_> {
	if impact.is_empty() {
		return LinkageRefreshShape::Empty;
	}
	if !impact.has_precise_graph_diff() {
		return LinkageRefreshShape::SourceLevel;
	}
	if impact.scope.has_manifest_path_change() {
		return LinkageRefreshShape::ManifestPolicy;
	}
	if !impact.references.is_empty() {
		return LinkageRefreshShape::LinkageRelevant;
	}
	match &impact.symbols {
		SymbolDelta::AdditiveOnly { added } => LinkageRefreshShape::AdditiveSymbolsOnly(added),
		SymbolDelta::RemovedOnly { removed, .. } => {
			LinkageRefreshShape::RemovedSymbolsOnly(removed)
		}
		SymbolDelta::Unchanged | SymbolDelta::Mixed { .. } => LinkageRefreshShape::LinkageRelevant,
	}
}

impl RefreshScope {
	fn new(changed_sources: Vec<SourceId>, changed_paths: Vec<PathBuf>) -> Self {
		Self {
			changed_sources,
			changed_paths,
		}
	}

	fn is_empty(&self) -> bool {
		self.changed_sources.is_empty() && self.changed_paths.is_empty()
	}

	fn changed_sources(&self) -> &[SourceId] {
		&self.changed_sources
	}

	fn changed_paths(&self) -> &[PathBuf] {
		&self.changed_paths
	}

	fn has_manifest_path_change(&self) -> bool {
		self.changed_paths
			.iter()
			.any(|path| Manifest::for_filename(path).is_some())
	}
}

impl ReferenceDelta {
	fn from_code_index(graph_diff: &CodeIndexGraphDiff) -> Self {
		if graph_diff.changed_references.is_empty()
			&& graph_diff.removed_references.is_empty()
			&& graph_diff.reference_id_remaps.is_empty()
		{
			return Self::Unchanged;
		}
		Self::Changed {
			changed: graph_diff.changed_references.clone(),
			remapped: graph_diff.reference_id_remaps.clone(),
		}
	}

	pub(in crate::linkage) fn is_empty(&self) -> bool {
		matches!(self, Self::Unchanged)
	}

	pub(in crate::linkage) fn changed(&self) -> &[ReferenceId] {
		match self {
			Self::Unchanged => &[],
			Self::Changed { changed, .. } => changed,
		}
	}

	pub(in crate::linkage) fn remaps(&self) -> &[(ReferenceId, ReferenceId)] {
		match self {
			Self::Unchanged => &[],
			Self::Changed { remapped, .. } => remapped,
		}
	}
}

impl SymbolDelta {
	fn from_code_index(graph_diff: CodeIndexGraphDiff) -> Self {
		if symbol_delta_is_empty(&graph_diff) {
			return Self::Unchanged;
		}
		if is_additive_symbol_delta(&graph_diff) {
			return Self::AdditiveOnly {
				added: graph_diff.added_symbols,
			};
		}
		if is_removed_symbol_delta(&graph_diff) {
			return Self::RemovedOnly {
				removed: graph_diff.removed_symbols,
				retargeted_identities: graph_diff.removed_symbol_identities,
			};
		}
		let retargeted_identities = retargeted_symbol_identities_from_diff(&graph_diff);
		Self::Mixed {
			candidate_changed: candidate_changed_symbols(&graph_diff),
			changed: graph_diff.changed_symbols,
			retargeted_identities,
			remapped: graph_diff.symbol_id_remaps,
		}
	}
}

pub(in crate::linkage) fn changed_reference_ids(impact: &LinkageRefreshImpact) -> &[ReferenceId] {
	impact.references.changed()
}

pub(in crate::linkage) fn reference_id_remaps(
	impact: &LinkageRefreshImpact,
) -> &[(ReferenceId, ReferenceId)] {
	impact.references.remaps()
}

pub(in crate::linkage) fn primary_changed_symbol_ids(impact: &LinkageRefreshImpact) -> &[SymbolId] {
	match &impact.symbols {
		SymbolDelta::AdditiveOnly { added } => added,
		SymbolDelta::Mixed {
			candidate_changed, ..
		} => candidate_changed,
		SymbolDelta::Unchanged | SymbolDelta::RemovedOnly { .. } => &[],
	}
}

pub(in crate::linkage) fn changed_symbol_ids(impact: &LinkageRefreshImpact) -> &[SymbolId] {
	match &impact.symbols {
		SymbolDelta::AdditiveOnly { added } => added,
		SymbolDelta::Mixed { changed, .. } => changed,
		SymbolDelta::Unchanged | SymbolDelta::RemovedOnly { .. } => &[],
	}
}

pub(in crate::linkage) fn retargeted_symbol_identities(impact: &LinkageRefreshImpact) -> &[String] {
	match &impact.symbols {
		SymbolDelta::RemovedOnly {
			retargeted_identities,
			..
		}
		| SymbolDelta::Mixed {
			retargeted_identities,
			..
		} => retargeted_identities,
		SymbolDelta::Unchanged | SymbolDelta::AdditiveOnly { .. } => &[],
	}
}

pub(in crate::linkage) fn symbol_id_remaps(
	impact: &LinkageRefreshImpact,
) -> &[(SymbolId, SymbolId)] {
	match &impact.symbols {
		SymbolDelta::Mixed { remapped, .. } => remapped,
		SymbolDelta::Unchanged
		| SymbolDelta::AdditiveOnly { .. }
		| SymbolDelta::RemovedOnly { .. } => &[],
	}
}

fn symbol_delta_is_unchanged(symbols: &SymbolDelta) -> bool {
	matches!(symbols, SymbolDelta::Unchanged)
}

fn symbol_delta_is_empty(graph_diff: &CodeIndexGraphDiff) -> bool {
	graph_diff.added_symbols.is_empty()
		&& graph_diff.modified_symbols.is_empty()
		&& graph_diff.changed_symbols.is_empty()
		&& graph_diff.removed_symbols.is_empty()
		&& graph_diff.modified_symbol_identities.is_empty()
		&& graph_diff.removed_symbol_identities.is_empty()
		&& graph_diff.symbol_id_remaps.is_empty()
}

fn is_additive_symbol_delta(graph_diff: &CodeIndexGraphDiff) -> bool {
	!graph_diff.added_symbols.is_empty()
		&& graph_diff.modified_symbols.is_empty()
		&& graph_diff.removed_symbols.is_empty()
		&& graph_diff.symbol_id_remaps.is_empty()
		&& graph_diff
			.changed_symbols
			.iter()
			.all(|symbol| graph_diff.added_symbols.contains(symbol))
}

fn is_removed_symbol_delta(graph_diff: &CodeIndexGraphDiff) -> bool {
	!graph_diff.removed_symbols.is_empty()
		&& graph_diff.added_symbols.is_empty()
		&& graph_diff.modified_symbols.is_empty()
		&& graph_diff.changed_symbols.is_empty()
		&& graph_diff.symbol_id_remaps.is_empty()
}

fn candidate_changed_symbols(graph_diff: &CodeIndexGraphDiff) -> Vec<SymbolId> {
	graph_diff
		.added_symbols
		.iter()
		.chain(graph_diff.modified_symbols.iter())
		.cloned()
		.collect()
}

fn retargeted_symbol_identities_from_diff(graph_diff: &CodeIndexGraphDiff) -> Vec<String> {
	graph_diff
		.modified_symbol_identities
		.iter()
		.chain(graph_diff.removed_symbol_identities.iter())
		.cloned()
		.collect()
}
