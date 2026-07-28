use std::collections::BTreeMap;

use roaring::RoaringBitmap;
use rustc_hash::{FxHashMap, FxHashSet};

use super::{
	CodeIndex, LinkageSnapshot, ReferenceId, SourceId, SymbolId, UnresolvedReason,
	WorkspaceSnapshot,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundedPathEdge {
	pub source: SymbolId,
	pub target: SymbolId,
	pub reference: ReferenceId,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BoundedPathCoverage {
	pub total: usize,
	pub decided: usize,
	pub resolved: usize,
	pub external: usize,
	pub candidate: usize,
	pub dynamic: usize,
	pub manifest_blocked: usize,
	pub unresolved: usize,
	pub gap_reasons: BTreeMap<String, usize>,
}

impl BoundedPathCoverage {
	pub fn percent(&self) -> usize {
		self.decided
			.saturating_mul(100)
			.checked_div(self.total)
			.unwrap_or(100)
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BoundedPathSearch {
	pub path: Vec<BoundedPathEdge>,
	pub coverage: BoundedPathCoverage,
	pub depth_reached: usize,
	pub explored_symbols: usize,
	pub explored_edges: usize,
	pub depth_limit_reached: bool,
	pub symbol_limit_reached: bool,
	pub edge_limit_reached: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundedPathLimits {
	pub max_depth: usize,
	pub max_symbols: usize,
	pub max_edges: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BoundedPathScope {
	sources: RoaringBitmap,
}

#[derive(Clone, Copy, Debug)]
pub struct BoundedPathRequest<'a> {
	pub from: SymbolId,
	pub to: SymbolId,
	pub relations: &'a [String],
	pub limits: BoundedPathLimits,
	pub scope: &'a BoundedPathScope,
}

impl BoundedPathScope {
	pub fn from_sources(sources: impl IntoIterator<Item = SourceId>) -> Self {
		Self {
			sources: sources
				.into_iter()
				.map(|source| source.file() as u32)
				.collect(),
		}
	}

	fn contains(&self, symbol: SymbolId) -> bool {
		self.sources.contains(symbol.file() as u32)
	}
}

#[derive(Default)]
struct ReferenceClassifications {
	external: FxHashSet<ReferenceId>,
	candidate: FxHashMap<ReferenceId, &'static str>,
	dynamic: FxHashMap<ReferenceId, &'static str>,
	manifest_blocked: FxHashSet<ReferenceId>,
	unresolved: FxHashMap<ReferenceId, UnresolvedReason>,
}

impl ReferenceClassifications {
	fn from_linkage(linkage: &LinkageSnapshot) -> Self {
		Self {
			external: Self::external(linkage),
			candidate: Self::candidate(linkage),
			dynamic: Self::dynamic(linkage),
			manifest_blocked: Self::manifest_blocked(linkage),
			unresolved: Self::unresolved(linkage),
		}
	}

	fn external(linkage: &LinkageSnapshot) -> FxHashSet<ReferenceId> {
		linkage
			.external
			.iter()
			.map(|reference| reference.reference)
			.collect()
	}

	fn candidate(linkage: &LinkageSnapshot) -> FxHashMap<ReferenceId, &'static str> {
		linkage
			.candidates
			.iter()
			.map(|reference| (reference.reference, reference.reason.as_str()))
			.collect()
	}

	fn dynamic(linkage: &LinkageSnapshot) -> FxHashMap<ReferenceId, &'static str> {
		linkage
			.dynamic
			.iter()
			.map(|reference| (reference.reference, reference.reason.as_str()))
			.collect()
	}

	fn manifest_blocked(linkage: &LinkageSnapshot) -> FxHashSet<ReferenceId> {
		linkage
			.blocked
			.iter()
			.chain(linkage.manifest_blocked.iter())
			.filter(|reference| reference.reason == UnresolvedReason::ManifestBlocked)
			.map(|reference| reference.reference)
			.collect()
	}

	fn unresolved(linkage: &LinkageSnapshot) -> FxHashMap<ReferenceId, UnresolvedReason> {
		linkage
			.unresolved
			.iter()
			.chain(
				linkage
					.blocked
					.iter()
					.filter(|reference| reference.reason != UnresolvedReason::ManifestBlocked),
			)
			.map(|reference| (reference.reference, reference.reason))
			.collect()
	}

	fn tally_gap(&self, reference: ReferenceId, coverage: &mut BoundedPathCoverage) {
		if self.external.contains(&reference) {
			coverage.external += 1;
			coverage.decided += 1;
		} else if let Some(reason) = self.candidate.get(&reference) {
			coverage.candidate += 1;
			tally_reason(&mut coverage.gap_reasons, "candidate", reason);
		} else if let Some(reason) = self.dynamic.get(&reference) {
			coverage.dynamic += 1;
			tally_reason(&mut coverage.gap_reasons, "dynamic", reason);
		} else if self.manifest_blocked.contains(&reference) {
			coverage.manifest_blocked += 1;
			*coverage
				.gap_reasons
				.entry("manifest_blocked".to_string())
				.or_default() += 1;
		} else {
			coverage.unresolved += 1;
			let reason = self
				.unresolved
				.get(&reference)
				.map_or("unclassified", UnresolvedReason::as_str);
			tally_reason(&mut coverage.gap_reasons, "unresolved", reason);
		}
	}
}

fn tally_reason(reasons: &mut BTreeMap<String, usize>, category: &str, reason: &str) {
	*reasons.entry(format!("{category}:{reason}")).or_default() += 1;
}

pub struct BoundedPathEngine<'a> {
	read_index: &'a super::LinkageReadIndex,
	classifications: ReferenceClassifications,
}

impl<'a> BoundedPathEngine<'a> {
	pub fn new(index: &CodeIndex, linkage: &'a LinkageSnapshot) -> Option<Self> {
		if linkage.index_generation != index.generation {
			return None;
		}
		Some(Self {
			read_index: linkage.read_index.get()?,
			classifications: ReferenceClassifications::from_linkage(linkage),
		})
	}

	pub fn search(&self, request: BoundedPathRequest<'_>) -> Option<BoundedPathSearch> {
		PathTraversal::new(
			self.read_index,
			&self.classifications,
			request.relations,
			request.limits,
			request.scope,
		)?
		.run(request.from, request.to)
	}
}

impl WorkspaceSnapshot {
	pub fn bounded_path(
		&self,
		from: SymbolId,
		to: SymbolId,
		relations: &[String],
		limits: BoundedPathLimits,
		scope: &BoundedPathScope,
	) -> Option<BoundedPathSearch> {
		bounded_path(
			&self.index,
			&self.linkage,
			BoundedPathRequest {
				from,
				to,
				relations,
				limits,
				scope,
			},
		)
	}
}

pub fn bounded_path(
	index: &CodeIndex,
	linkage: &LinkageSnapshot,
	request: BoundedPathRequest<'_>,
) -> Option<BoundedPathSearch> {
	BoundedPathEngine::new(index, linkage)?.search(request)
}

struct PathTraversal<'a> {
	read_index: &'a super::LinkageReadIndex,
	relations: Vec<&'a str>,
	scope: &'a BoundedPathScope,
	max_depth: usize,
	max_symbols: usize,
	max_edges: usize,
	classifications: &'a ReferenceClassifications,
	visited: RoaringBitmap,
	predecessors: FxHashMap<u32, (u32, ReferenceId)>,
	search: BoundedPathSearch,
}

impl<'a> PathTraversal<'a> {
	fn new(
		read_index: &'a super::LinkageReadIndex,
		classifications: &'a ReferenceClassifications,
		relations: &'a [String],
		limits: BoundedPathLimits,
		scope: &'a BoundedPathScope,
	) -> Option<Self> {
		let mut seen_relations = FxHashSet::default();
		Some(Self {
			read_index,
			relations: relations
				.iter()
				.map(String::as_str)
				.filter(|relation| seen_relations.insert(*relation))
				.collect(),
			scope,
			max_depth: limits.max_depth,
			max_symbols: limits.max_symbols,
			max_edges: limits.max_edges,
			classifications,
			visited: RoaringBitmap::new(),
			predecessors: FxHashMap::default(),
			search: BoundedPathSearch::default(),
		})
	}

	fn run(mut self, from: SymbolId, to: SymbolId) -> Option<BoundedPathSearch> {
		if !self.scope.contains(from) || !self.scope.contains(to) {
			return None;
		}
		let from_ordinal = self.read_index.ordinal(&from)?;
		let to_ordinal = self.read_index.ordinal(&to)?;
		let mut frontier = RoaringBitmap::new();
		self.visited.insert(from_ordinal);
		frontier.insert(from_ordinal);
		if from_ordinal != to_ordinal {
			self.walk(&mut frontier, to_ordinal);
		}
		self.search.explored_symbols = self.visited.len() as usize;
		if self.visited.contains(to_ordinal) {
			self.search.path = reconstruct_path(
				self.read_index,
				&self.predecessors,
				from_ordinal,
				to_ordinal,
			)?;
		}
		Some(self.search)
	}

	fn walk(&mut self, frontier: &mut RoaringBitmap, to_ordinal: u32) {
		'search: for depth in 0..=self.max_depth {
			self.search.depth_reached = depth;
			let mut next = RoaringBitmap::new();
			for source_ordinal in frontier.iter() {
				let relations = self.relations_for(source_ordinal);
				for relation in relations {
					let outgoing_count = self.read_index.outgoing(source_ordinal, &relation).len();
					for reference_index in 0..outgoing_count {
						if self.search.coverage.total >= self.max_edges {
							self.search.edge_limit_reached = true;
							break 'search;
						}
						let reference =
							self.read_index.outgoing(source_ordinal, &relation)[reference_index];
						if self.visit_reference(source_ordinal, reference, depth, &mut next)
							== Some(to_ordinal)
						{
							self.search.depth_reached = depth + 1;
							break 'search;
						}
					}
				}
			}
			if next.is_empty() {
				break;
			}
			*frontier = next;
		}
	}

	fn relations_for(&self, source_ordinal: u32) -> Vec<String> {
		if self.relations.is_empty() {
			self.read_index
				.outgoing_relations(source_ordinal)
				.map(str::to_string)
				.collect()
		} else {
			self.relations
				.iter()
				.map(|relation| (*relation).to_string())
				.collect()
		}
	}

	fn visit_reference(
		&mut self,
		source_ordinal: u32,
		reference: ReferenceId,
		depth: usize,
		next: &mut RoaringBitmap,
	) -> Option<u32> {
		self.search.coverage.total += 1;
		let Some(target) = self.read_index.resolved_target(&reference).copied() else {
			self.classifications
				.tally_gap(reference, &mut self.search.coverage);
			return None;
		};
		let Some(target_ordinal) = self.read_index.ordinal(&target) else {
			self.tally_missing_ordinal();
			return None;
		};
		self.search.coverage.resolved += 1;
		self.search.coverage.decided += 1;
		self.search.explored_edges += 1;
		if !self.scope.contains(target) {
			return None;
		}
		if depth >= self.max_depth {
			if !self.visited.contains(target_ordinal) {
				self.search.depth_limit_reached = true;
			}
			return None;
		}
		if self.visited.contains(target_ordinal) {
			return None;
		}
		if self.visited.len() as usize >= self.max_symbols {
			self.search.symbol_limit_reached = true;
			return None;
		}
		self.visited.insert(target_ordinal);
		self.predecessors
			.insert(target_ordinal, (source_ordinal, reference));
		next.insert(target_ordinal);
		Some(target_ordinal)
	}

	fn tally_missing_ordinal(&mut self) {
		self.search.coverage.unresolved += 1;
		*self
			.search
			.coverage
			.gap_reasons
			.entry("missing_symbol_ordinal".to_string())
			.or_default() += 1;
	}
}

fn reconstruct_path(
	read_index: &super::LinkageReadIndex,
	predecessors: &FxHashMap<u32, (u32, ReferenceId)>,
	from: u32,
	to: u32,
) -> Option<Vec<BoundedPathEdge>> {
	let mut path = Vec::new();
	let mut current = to;
	while current != from {
		let (previous, reference) = predecessors.get(&current).copied()?;
		path.push(BoundedPathEdge {
			source: read_index.symbol(previous)?,
			target: read_index.symbol(current)?,
			reference,
		});
		current = previous;
	}
	path.reverse();
	Some(path)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::snapshot::{
		CandidateReason, CandidateReference, CandidateScope, ChangeOverlay, CodeIndex,
		DynamicReason, DynamicReference, ExternalReference, ExternalReferenceOrigin, LinkageEdge,
		LinkageReadIndexHandle, LinkageSnapshot, ReferenceRecord, ResolutionEvidence,
		ResourceGeneration, SourceCatalog, SymbolRecord, UnresolvedReference, WorkspaceTimings,
	};

	fn path_snapshot(
		symbols: Vec<SymbolRecord>,
		references: Vec<ReferenceRecord>,
		edges: Vec<LinkageEdge>,
		ordinals: Vec<(u32, SymbolId)>,
	) -> WorkspaceSnapshot {
		let generation = ResourceGeneration::new(1);
		let index = CodeIndex::with_references(generation, generation, symbols, references);
		let read_index =
			LinkageReadIndexHandle::from_edges_with_ordinals(&edges, &index.references, ordinals);
		let mut linkage = LinkageSnapshot::new(generation, generation, edges.len(), 0);
		linkage.resolved = edges;
		linkage.read_index = read_index;
		WorkspaceSnapshot {
			generation,
			catalog: SourceCatalog::new(generation, Vec::new()),
			index,
			linkage,
			changes: ChangeOverlay::new(generation, generation, generation, Vec::new()),
			timings: WorkspaceTimings::default(),
		}
	}

	#[test]
	fn selected_source_scope_blocks_cross_root_detours_with_sparse_ordinals() {
		let first_source = SourceId::at(0);
		let other_source = SourceId::at(1);
		let from = SymbolId::at(0, 0);
		let to = SymbolId::at(0, 1);
		let bridge = SymbolId::at(1, 0);
		let first = ReferenceId::at(0, 0);
		let second = ReferenceId::at(1, 0);
		let snapshot = path_snapshot(
			vec![
				SymbolRecord::new(from, first_source, "from", "fn"),
				SymbolRecord::new(to, first_source, "to", "fn"),
				SymbolRecord::new(bridge, other_source, "bridge", "fn"),
			],
			vec![
				ReferenceRecord::new(first, first_source, from, bridge.to_string(), "calls", None),
				ReferenceRecord::new(second, other_source, bridge, to.to_string(), "calls", None),
			],
			vec![
				LinkageEdge::new(first, bridge),
				LinkageEdge::new(second, to),
			],
			vec![(7, from), (2_000_000, bridge), (u32::MAX - 1, to)],
		);
		let limits = BoundedPathLimits {
			max_depth: 4,
			max_symbols: 10,
			max_edges: 10,
		};
		let selected = BoundedPathScope::from_sources([first_source]);
		let scoped = snapshot
			.bounded_path(from, to, &["calls".to_string()], limits, &selected)
			.expect("scoped path search");
		assert!(scoped.path.is_empty(), "{scoped:?}");
		assert_eq!(scoped.coverage.resolved, 1, "{scoped:?}");
		assert_eq!(scoped.coverage.decided, 1, "{scoped:?}");

		let all_sources = BoundedPathScope::from_sources([first_source, other_source]);
		let unscoped = snapshot
			.bounded_path(from, to, &["calls".to_string()], limits, &all_sources)
			.expect("all-roots path search");
		assert_eq!(unscoped.path.len(), 2, "{unscoped:?}");
		assert_eq!(
			snapshot
				.linkage
				.read_index
				.get()
				.expect("path read index")
				.active_symbol_slots(),
			3,
			"sparse stable ordinals must not allocate historical holes"
		);
	}

	#[test]
	fn coverage_counts_every_unlinked_reference_category() {
		let source = SourceId::at(0);
		let from = SymbolId::at(0, 0);
		let to = SymbolId::at(0, 1);
		let ids = (0..5)
			.map(|index| ReferenceId::at(0, index))
			.collect::<Vec<_>>();
		let references = ids
			.iter()
			.enumerate()
			.map(|(index, id)| {
				ReferenceRecord::new(*id, source, from, format!("missing:{index}"), "calls", None)
			})
			.collect();
		let mut snapshot = path_snapshot(
			vec![
				SymbolRecord::new(from, source, "from", "fn"),
				SymbolRecord::new(to, source, "to", "fn"),
			],
			references,
			Vec::new(),
			vec![(11, from), (97, to)],
		);
		snapshot.linkage.external.push(ExternalReference::new(
			ids[0],
			"external",
			ExternalReferenceOrigin::Dependency,
		));
		snapshot.linkage.candidates.push(CandidateReference::new(
			ids[1],
			vec![to],
			CandidateReason::MultipleTargets,
			CandidateScope::Local,
			ResolutionEvidence::NameMatch,
		));
		snapshot.linkage.dynamic.push(DynamicReference::new(
			ids[2],
			"dynamic",
			DynamicReason::RuntimeMutation,
			Vec::new(),
		));
		snapshot.linkage.blocked.push(UnresolvedReference::new(
			ids[3],
			"blocked",
			UnresolvedReason::ManifestBlocked,
		));
		snapshot.linkage.unresolved.push(UnresolvedReference::new(
			ids[4],
			"missing",
			UnresolvedReason::NoCandidate,
		));

		let search = snapshot
			.bounded_path(
				from,
				to,
				&["calls".to_string()],
				BoundedPathLimits {
					max_depth: 4,
					max_symbols: 10,
					max_edges: 10,
				},
				&BoundedPathScope::from_sources([source]),
			)
			.expect("coverage path search");
		assert_eq!(search.coverage.total, 5, "{search:?}");
		assert_eq!(search.coverage.decided, 1, "{search:?}");
		assert_eq!(search.coverage.external, 1, "{search:?}");
		assert_eq!(search.coverage.candidate, 1, "{search:?}");
		assert_eq!(search.coverage.dynamic, 1, "{search:?}");
		assert_eq!(search.coverage.manifest_blocked, 1, "{search:?}");
		assert_eq!(search.coverage.unresolved, 1, "{search:?}");
	}

	#[test]
	fn edge_budget_stops_inside_a_large_indexed_adjacency() {
		let source = SourceId::at(0);
		let from = SymbolId::at(0, 0);
		let sink = SymbolId::at(0, 1);
		let to = SymbolId::at(0, 2);
		let references = (0..2_048)
			.map(|index| {
				let id = ReferenceId::at(0, index);
				ReferenceRecord::new(id, source, from, sink.to_string(), "calls", None)
			})
			.collect::<Vec<_>>();
		let edges = references
			.iter()
			.map(|reference| LinkageEdge::new(reference.id, sink))
			.collect();
		let snapshot = path_snapshot(
			vec![
				SymbolRecord::new(from, source, "from", "fn"),
				SymbolRecord::new(sink, source, "sink", "fn"),
				SymbolRecord::new(to, source, "to", "fn"),
			],
			references,
			edges,
			vec![(1, from), (2, sink), (3, to)],
		);

		let search = snapshot
			.bounded_path(
				from,
				to,
				&["calls".to_string()],
				BoundedPathLimits {
					max_depth: 4,
					max_symbols: 10,
					max_edges: 3,
				},
				&BoundedPathScope::from_sources([source]),
			)
			.expect("budgeted path search");
		assert_eq!(search.coverage.total, 3, "{search:?}");
		assert_eq!(search.explored_edges, 3, "{search:?}");
		assert!(search.edge_limit_reached, "{search:?}");
	}

	#[test]
	fn prepared_engine_reuses_linkage_classifications_across_searches() {
		let source = SourceId::at(0);
		let from = SymbolId::at(0, 0);
		let middle = SymbolId::at(0, 1);
		let to = SymbolId::at(0, 2);
		let first = ReferenceId::at(0, 0);
		let second = ReferenceId::at(0, 1);
		let snapshot = path_snapshot(
			vec![
				SymbolRecord::new(from, source, "from", "fn"),
				SymbolRecord::new(middle, source, "middle", "fn"),
				SymbolRecord::new(to, source, "to", "fn"),
			],
			vec![
				ReferenceRecord::new(first, source, from, middle.to_string(), "calls", None),
				ReferenceRecord::new(second, source, middle, to.to_string(), "calls", None),
			],
			vec![
				LinkageEdge::new(first, middle),
				LinkageEdge::new(second, to),
			],
			vec![(1, from), (2, middle), (3, to)],
		);
		let scope = BoundedPathScope::from_sources([source]);
		let relations = vec!["calls".to_string()];
		let limits = BoundedPathLimits {
			max_depth: 4,
			max_symbols: 10,
			max_edges: 10,
		};
		let engine =
			BoundedPathEngine::new(&snapshot.index, &snapshot.linkage).expect("path engine");

		let first_search = engine
			.search(BoundedPathRequest {
				from,
				to: middle,
				relations: &relations,
				limits,
				scope: &scope,
			})
			.expect("first search");
		let second_search = engine
			.search(BoundedPathRequest {
				from,
				to,
				relations: &relations,
				limits,
				scope: &scope,
			})
			.expect("second search");

		assert_eq!(first_search.path.len(), 1);
		assert_eq!(second_search.path.len(), 2);
	}
}
