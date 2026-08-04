use std::path::PathBuf;

use crate::source::CodeIndexMaterial;

use crate::code::CodeIndexGraphDiff;
use crate::snapshot::{ReferenceId, SourceId, SymbolId};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinkageGraphDelta {
	references: ReferenceDelta,
	symbols: SymbolDelta,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinkageRefreshImpact {
	changed_sources: Vec<SourceId>,
	changed_paths: Vec<PathBuf>,
	references: ReferenceDelta,
	symbols: SymbolDelta,
	precise: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::linkage) enum ReferenceDelta {
	#[default]
	Unchanged,
	Changed {
		changed: Vec<ReferenceId>,
		removed: Vec<ReferenceId>,
		removed_binding: bool,
		removed_semantic_fact: bool,
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
	},
}

impl LinkageRefreshImpact {
	pub fn new(changed_sources: Vec<SourceId>, changed_paths: Vec<PathBuf>) -> Self {
		Self {
			changed_sources,
			changed_paths,
			references: ReferenceDelta::Unchanged,
			symbols: SymbolDelta::Unchanged,
			precise: false,
		}
	}

	pub fn with_graph_delta(
		changed_sources: Vec<SourceId>,
		changed_paths: Vec<PathBuf>,
		graph_delta: LinkageGraphDelta,
	) -> Self {
		Self {
			changed_sources,
			changed_paths,
			references: graph_delta.references,
			symbols: graph_delta.symbols,
			precise: true,
		}
	}

	pub fn is_empty(&self) -> bool {
		self.changed_sources.is_empty()
			&& self.changed_paths.is_empty()
			&& self.references.is_empty()
			&& matches!(self.symbols, SymbolDelta::Unchanged)
	}

	pub(in crate::linkage) fn changed_sources(&self) -> &[SourceId] {
		&self.changed_sources
	}

	pub(in crate::linkage) fn changed_paths(&self) -> &[PathBuf] {
		&self.changed_paths
	}

	pub(in crate::linkage) fn has_precise_graph_diff(&self) -> bool {
		self.precise
	}

	pub(in crate::linkage) fn references(&self) -> &ReferenceDelta {
		&self.references
	}

	pub(in crate::linkage) fn definitions(&self) -> &SymbolDelta {
		&self.symbols
	}
}

pub(in crate::linkage) fn changes_c_include_topology(
	impact: &LinkageRefreshImpact,
	material: &CodeIndexMaterial,
) -> bool {
	let changed_c_path = impact
		.changed_paths
		.iter()
		.any(|path| is_c_family_path(path));
	let ReferenceDelta::Changed {
		changed,
		removed_binding,
		..
	} = &impact.references
	else {
		return false;
	};
	if *removed_binding && changed_c_path {
		return true;
	}
	changed.iter().any(|reference| {
		let Some((source_file, local_reference)) = material.identity.reference_location(reference)
		else {
			return false;
		};
		material.files.get(source_file).is_some_and(|file| {
			file.lang == code_moniker_core::lang::Lang::C
				&& file.graph.ref_at(local_reference).kind
					== code_moniker_core::lang::kinds::IMPORTS_MODULE
		})
	})
}

fn is_c_family_path(path: &std::path::Path) -> bool {
	path.extension()
		.and_then(|extension| extension.to_str())
		.is_some_and(|extension| {
			matches!(
				extension.to_ascii_lowercase().as_str(),
				"c" | "h" | "cc" | "cpp" | "cxx" | "c++"
			)
		})
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
			removed: graph_diff.removed_references.clone(),
			removed_binding: graph_diff.removed_reference_kinds.iter().any(|kind| {
				matches!(
					kind.as_bytes(),
					code_moniker_core::lang::kinds::IMPORTS_MODULE
						| code_moniker_core::lang::kinds::IMPORTS_SYMBOL
						| code_moniker_core::core::kinds::REF_REEXPORTS
				)
			}),
			removed_semantic_fact: graph_diff.removed_reference_kinds.iter().any(|kind| {
				matches!(
					kind.as_bytes(),
					code_moniker_core::lang::kinds::TYPED_AS
						| code_moniker_core::lang::kinds::RETURNS_TYPE
				)
			}),
			remapped: graph_diff.reference_id_remaps.clone(),
		}
	}

	pub(in crate::linkage) fn is_empty(&self) -> bool {
		matches!(self, Self::Unchanged)
	}

	pub(in crate::linkage) fn changed_ids(&self) -> &[ReferenceId] {
		match self {
			Self::Unchanged => &[],
			Self::Changed { changed, .. } => changed,
		}
	}

	pub(in crate::linkage) fn id_remaps(&self) -> &[(ReferenceId, ReferenceId)] {
		match self {
			Self::Unchanged => &[],
			Self::Changed { remapped, .. } => remapped,
		}
	}

	pub(in crate::linkage) fn removed_ids(&self) -> &[ReferenceId] {
		match self {
			Self::Unchanged => &[],
			Self::Changed { removed, .. } => removed,
		}
	}

	pub(in crate::linkage) fn removed_binding(&self) -> bool {
		matches!(
			self,
			Self::Changed {
				removed_binding: true,
				..
			}
		)
	}

	pub(in crate::linkage) fn removed_semantic_fact(&self) -> bool {
		matches!(
			self,
			Self::Changed {
				removed_semantic_fact: true,
				..
			}
		)
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
		}
	}

	pub(in crate::linkage) fn candidate_ids(&self) -> &[SymbolId] {
		match self {
			Self::AdditiveOnly { added } => added,
			Self::Mixed {
				candidate_changed, ..
			} => candidate_changed,
			Self::Unchanged | Self::RemovedOnly { .. } => &[],
		}
	}

	pub(in crate::linkage) fn changed_ids(&self) -> &[SymbolId] {
		match self {
			Self::AdditiveOnly { added } => added,
			Self::Mixed { changed, .. } => changed,
			Self::Unchanged | Self::RemovedOnly { .. } => &[],
		}
	}

	pub(in crate::linkage) fn retargeted_identities(&self) -> &[String] {
		match self {
			Self::RemovedOnly {
				retargeted_identities,
				..
			}
			| Self::Mixed {
				retargeted_identities,
				..
			} => retargeted_identities,
			Self::Unchanged | Self::AdditiveOnly { .. } => &[],
		}
	}
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
