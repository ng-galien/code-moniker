use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use roaring::RoaringBitmap;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::linkage::{ReferenceOrdinal, ReferenceSet, ReferenceSetIter};

use super::{
	CodeIndex, LinkageSnapshot, ReferenceId, SourceId, SymbolId, SymbolOrdinal, SymbolSet,
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BoundedCorridorSearch {
	pub members: Vec<SymbolId>,
	pub edges: Vec<BoundedPathEdge>,
	pub coverage: BoundedPathCoverage,
	pub forward_depth_reached: usize,
	pub reverse_depth_reached: usize,
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
	all: bool,
	sources: RoaringBitmap,
	symbols: Option<SymbolSet>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedCorridorScope(BoundedPathScope);

#[derive(Clone, Copy, Debug)]
pub struct BoundedPathRequest<'a> {
	pub from: SymbolId,
	pub to: SymbolId,
	pub relations: &'a [String],
	pub avoid: &'a [SymbolId],
	pub limits: BoundedPathLimits,
	pub scope: &'a BoundedPathScope,
}

#[derive(Clone, Copy, Debug)]
pub struct BoundedCorridorRequest<'a> {
	pub from: SymbolId,
	pub to: SymbolId,
	pub relations: &'a [String],
	pub limits: BoundedPathLimits,
	pub scope: &'a BoundedCorridorScope,
}

impl BoundedPathScope {
	pub fn all() -> Self {
		Self {
			all: true,
			sources: RoaringBitmap::new(),
			symbols: None,
		}
	}

	pub fn from_sources(sources: impl IntoIterator<Item = SourceId>) -> Self {
		Self {
			all: false,
			sources: sources
				.into_iter()
				.map(|source| source.file() as u32)
				.collect(),
			symbols: None,
		}
	}

	pub fn from_symbols(symbols: SymbolSet) -> Self {
		Self {
			all: false,
			sources: RoaringBitmap::new(),
			symbols: Some(symbols),
		}
	}

	fn contains(&self, read_index: &super::LinkageReadIndex, ordinal: SymbolOrdinal) -> bool {
		if self.all {
			return true;
		}
		self.symbols.as_ref().map_or_else(
			|| {
				read_index
					.symbol(ordinal)
					.is_some_and(|symbol| self.sources.contains(symbol.file() as u32))
			},
			|symbols| symbols.contains(ordinal),
		)
	}
}

impl BoundedCorridorScope {
	pub fn from_symbols(symbols: SymbolSet, max_symbols: usize) -> Option<Self> {
		(!symbols.is_empty() && symbols.len() <= max_symbols && max_symbols <= 100_000)
			.then(|| Self(BoundedPathScope::from_symbols(symbols)))
	}

	fn as_path_scope(&self) -> &BoundedPathScope {
		&self.0
	}
}

fn tally_reason(reasons: &mut BTreeMap<String, usize>, category: &str, reason: &str) {
	*reasons.entry(format!("{category}:{reason}")).or_default() += 1;
}

pub struct BoundedPathEngine<'a> {
	read_index: &'a super::LinkageReadIndex,
}

impl<'a> BoundedPathEngine<'a> {
	pub fn new(index: &'a CodeIndex, linkage: &'a LinkageSnapshot) -> Option<Self> {
		if linkage.index_generation != index.generation {
			return None;
		}
		Some(Self {
			read_index: linkage.read_index.get()?,
		})
	}

	pub fn search(&self, request: BoundedPathRequest<'_>) -> Option<BoundedPathSearch> {
		PathTraversal::new(
			self.read_index,
			request.relations,
			request.avoid,
			request.limits,
			request.scope,
		)
		.run(request.from, request.to)
	}

	pub fn corridor(&self, request: BoundedCorridorRequest<'_>) -> Option<BoundedCorridorSearch> {
		if request.relations.is_empty()
			|| request.limits.max_depth > 64
			|| request.limits.max_symbols == 0
			|| request.limits.max_symbols > 100_000
			|| request.limits.max_edges == 0
			|| request.limits.max_edges > 500_000
		{
			return None;
		}
		CorridorTraversal::new(
			self.read_index,
			request.relations,
			request.limits,
			request.scope.as_path_scope(),
		)
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
				avoid: &[],
				limits,
				scope,
			},
		)
	}

	pub fn bounded_corridor(
		&self,
		request: BoundedCorridorRequest<'_>,
	) -> Option<BoundedCorridorSearch> {
		bounded_corridor(&self.index, &self.linkage, request)
	}
}

pub fn bounded_path(
	index: &CodeIndex,
	linkage: &LinkageSnapshot,
	request: BoundedPathRequest<'_>,
) -> Option<BoundedPathSearch> {
	BoundedPathEngine::new(index, linkage)?.search(request)
}

pub fn bounded_corridor(
	index: &CodeIndex,
	linkage: &LinkageSnapshot,
	request: BoundedCorridorRequest<'_>,
) -> Option<BoundedCorridorSearch> {
	BoundedPathEngine::new(index, linkage)?.corridor(request)
}

struct PathTraversal<'a> {
	read_index: &'a super::LinkageReadIndex,
	relations: Vec<&'a str>,
	scope: &'a BoundedPathScope,
	max_depth: usize,
	max_symbols: usize,
	max_edges: usize,
	avoided: SymbolSet,
	visited: SymbolSet,
	references: ReferenceTraversal,
	predecessors: FxHashMap<SymbolOrdinal, (SymbolOrdinal, ReferenceId)>,
	search: BoundedPathSearch,
}

impl<'a> PathTraversal<'a> {
	fn new(
		read_index: &'a super::LinkageReadIndex,
		relations: &'a [String],
		avoid: &'a [SymbolId],
		limits: BoundedPathLimits,
		scope: &'a BoundedPathScope,
	) -> Self {
		let mut seen_relations = FxHashSet::default();
		let relations = relations
			.iter()
			.map(String::as_str)
			.filter(|relation| seen_relations.insert(*relation))
			.collect::<Vec<_>>();
		let avoided = avoid
			.iter()
			.filter_map(|symbol| read_index.ordinal(symbol))
			.collect();
		Self {
			read_index,
			relations,
			scope,
			max_depth: limits.max_depth,
			max_symbols: limits.max_symbols,
			max_edges: limits.max_edges,
			avoided,
			visited: SymbolSet::new(),
			references: ReferenceTraversal::default(),
			predecessors: FxHashMap::default(),
			search: BoundedPathSearch::default(),
		}
	}

	fn run(mut self, from: SymbolId, to: SymbolId) -> Option<BoundedPathSearch> {
		let from_ordinal = self.read_index.ordinal(&from)?;
		let to_ordinal = self.read_index.ordinal(&to)?;
		if !self.scope.contains(self.read_index, from_ordinal)
			|| !self.scope.contains(self.read_index, to_ordinal)
		{
			return None;
		}
		if self.avoided.contains(from_ordinal) || self.avoided.contains(to_ordinal) {
			return None;
		}
		let mut frontier = SymbolSet::new();
		self.visited.insert(from_ordinal);
		frontier.insert(from_ordinal);
		if from_ordinal != to_ordinal {
			let reference_scope = reference_scope_terms(
				self.read_index,
				self.scope,
				TraversalDirection::Forward,
				&self.relations,
			);
			self.walk(&mut frontier, to_ordinal, &reference_scope);
		}
		self.search.coverage = std::mem::take(&mut self.references.coverage);
		self.search.explored_edges = self.references.explored_edges;
		self.search.edge_limit_reached = self.references.limit_reached;
		self.search.explored_symbols = self.visited.len();
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

	fn walk(
		&mut self,
		frontier: &mut SymbolSet,
		to_ordinal: SymbolOrdinal,
		reference_scope: &ReferenceScope<'_>,
	) {
		for depth in 0..=self.max_depth {
			self.search.depth_reached = depth;
			let postings = reference_postings_from_symbols(
				self.read_index,
				TraversalDirection::Forward,
				frontier,
				&self.relations,
			);
			if postings.is_empty() {
				break;
			}
			let batch = self.references.admit(
				self.read_index,
				postings,
				reference_scope,
				false,
				self.max_edges,
			);
			let resolved = batch
				.references
				.intersection(self.read_index.classifications.resolved());
			let (mut next, witnesses) = self.target_witnesses(&resolved);
			next.remove_all(&self.avoided);
			next.remove_all(&self.visited);
			if depth == self.max_depth {
				if !next.is_empty() {
					self.search.depth_limit_reached = true;
				}
				break;
			}
			next = self.admit_symbols(next);
			if next.is_empty() {
				break;
			}
			for target in next.iter() {
				if let Some(witness) = witnesses.get(&target).copied() {
					self.predecessors.insert(target, witness);
				}
			}
			self.visited.union_with(&next);
			if next.contains(to_ordinal) {
				self.search.depth_reached = depth + 1;
				break;
			}
			*frontier = next;
			if batch.limit_reached || self.search.symbol_limit_reached {
				break;
			}
		}
	}

	fn target_witnesses(
		&mut self,
		references: &ReferenceSet,
	) -> (
		SymbolSet,
		FxHashMap<SymbolOrdinal, (SymbolOrdinal, ReferenceId)>,
	) {
		let mut targets = SymbolSet::new();
		let mut witnesses = FxHashMap::default();
		for reference in references.iter() {
			let (Some(source), Some(target), Some(reference_id)) = (
				self.read_index.reference_source(reference),
				self.read_index.reference_target(reference),
				self.read_index.reference_id(reference),
			) else {
				self.references.tally_missing_ordinal();
				continue;
			};
			targets.insert(target);
			witnesses.entry(target).or_insert((source, reference_id));
		}
		(targets, witnesses)
	}

	fn admit_symbols(&mut self, candidates: SymbolSet) -> SymbolSet {
		let remaining = self.max_symbols.saturating_sub(self.visited.len());
		if candidates.len() > remaining {
			self.search.symbol_limit_reached = true;
			candidates.iter().take(remaining).collect()
		} else {
			candidates
		}
	}
}

struct CorridorTraversal<'a> {
	read_index: &'a super::LinkageReadIndex,
	relations: Vec<&'a str>,
	scope: &'a BoundedPathScope,
	limits: BoundedPathLimits,
	state: CorridorState,
}

#[derive(Default)]
struct CorridorState {
	seen_symbols: SymbolSet,
	references: ReferenceTraversal,
	search: BoundedCorridorSearch,
}

#[derive(Default)]
struct CorridorLayers {
	exact: Vec<SymbolSet>,
}

struct ReferenceBatch {
	references: ReferenceSet,
	limit_reached: bool,
}

#[derive(Default)]
struct ReferenceTraversal {
	seen: ReferenceSet,
	coverage: BoundedPathCoverage,
	explored_edges: usize,
	limit_reached: bool,
}

#[derive(Clone, Copy)]
enum TraversalDirection {
	Forward,
	Reverse,
}

enum ReferenceScope<'a> {
	All,
	Terms(Vec<&'a ReferenceSet>),
}

impl<'a> CorridorTraversal<'a> {
	fn new(
		read_index: &'a super::LinkageReadIndex,
		relations: &'a [String],
		limits: BoundedPathLimits,
		scope: &'a BoundedPathScope,
	) -> Self {
		let mut seen_relations = FxHashSet::default();
		let relations = relations
			.iter()
			.map(String::as_str)
			.filter(|relation| seen_relations.insert(*relation))
			.collect::<Vec<_>>();
		Self {
			read_index,
			relations,
			scope,
			limits,
			state: CorridorState::default(),
		}
	}

	fn run(mut self, from: SymbolId, to: SymbolId) -> Option<BoundedCorridorSearch> {
		let (from, to) = self.endpoints(from, to)?;
		let forward_start = self.state.admit_endpoint(from, self.limits);
		let reverse_start = if from == to {
			forward_start.clone()
		} else {
			self.state.admit_endpoint(to, self.limits)
		};
		let forward = if forward_start.is_empty() {
			CorridorLayers::default()
		} else if from == to {
			CorridorLayers::from_start(forward_start)
		} else {
			self.walk(TraversalDirection::Forward, forward_start)
		};
		let reverse = if reverse_start.is_empty() {
			CorridorLayers::default()
		} else if from == to {
			CorridorLayers::from_start(reverse_start)
		} else {
			self.walk(TraversalDirection::Reverse, reverse_start)
		};
		Some(finish_corridor(
			self.read_index,
			self.limits,
			self.state,
			forward,
			reverse,
		))
	}

	fn endpoints(&self, from: SymbolId, to: SymbolId) -> Option<(SymbolOrdinal, SymbolOrdinal)> {
		let from = self.read_index.ordinal(&from)?;
		let to = self.read_index.ordinal(&to)?;
		if !self.scope.contains(self.read_index, from) || !self.scope.contains(self.read_index, to)
		{
			return None;
		}
		Some((from, to))
	}

	fn walk(&mut self, direction: TraversalDirection, start: SymbolSet) -> CorridorLayers {
		let reference_scope =
			reference_scope_terms(self.read_index, self.scope, direction, &self.relations);
		let mut frontier = start.clone();
		let mut reached = start;
		let mut exact = vec![frontier.clone()];
		for depth in 0..=self.limits.max_depth {
			self.record_depth(direction, depth);
			let postings = reference_postings_from_symbols(
				self.read_index,
				direction,
				&frontier,
				&self.relations,
			);
			if postings.is_empty() {
				break;
			}
			let batch = self.state.admit_references(
				self.read_index,
				postings,
				&reference_scope,
				matches!(direction, TraversalDirection::Reverse),
				self.limits,
			);
			let resolved = batch
				.references
				.intersection(self.read_index.classifications.resolved());
			let mut next = self.symbols_from_references(direction, &resolved);
			next.remove_all(&reached);
			if depth == self.limits.max_depth {
				if !next.is_empty() {
					self.state.search.depth_limit_reached = true;
				}
				break;
			}
			if next.is_empty() {
				break;
			}
			next = self.state.admit_symbols(next, self.limits);
			if next.is_empty() {
				break;
			}
			reached.union_with(&next);
			exact.push(next.clone());
			frontier = next;
			if self.state.search.symbol_limit_reached {
				break;
			}
		}
		CorridorLayers { exact }
	}

	fn record_depth(&mut self, direction: TraversalDirection, depth: usize) {
		match direction {
			TraversalDirection::Forward => self.state.search.forward_depth_reached = depth,
			TraversalDirection::Reverse => self.state.search.reverse_depth_reached = depth,
		}
	}

	fn symbols_from_references(
		&mut self,
		direction: TraversalDirection,
		references: &ReferenceSet,
	) -> SymbolSet {
		let mut symbols = SymbolSet::new();
		for reference in references.iter() {
			let symbol = match direction {
				TraversalDirection::Forward => self.read_index.reference_target(reference),
				TraversalDirection::Reverse => self.read_index.reference_source(reference),
			};
			if let Some(symbol) = symbol {
				symbols.insert(symbol);
			} else {
				self.state.tally_missing_ordinal();
			}
		}
		symbols
	}
}

impl CorridorLayers {
	fn from_start(start: SymbolSet) -> Self {
		Self { exact: vec![start] }
	}
}

impl CorridorState {
	fn admit_endpoint(&mut self, endpoint: SymbolOrdinal, limits: BoundedPathLimits) -> SymbolSet {
		if self.seen_symbols.contains(endpoint) {
			return SymbolSet::from_symbol(endpoint);
		}
		if self.seen_symbols.len() >= limits.max_symbols {
			self.search.symbol_limit_reached = true;
			return SymbolSet::new();
		}
		self.seen_symbols.insert(endpoint);
		SymbolSet::from_symbol(endpoint)
	}

	fn admit_symbols(&mut self, candidates: SymbolSet, limits: BoundedPathLimits) -> SymbolSet {
		let already_seen = candidates.intersection(&self.seen_symbols);
		let unseen = candidates.difference(&self.seen_symbols);
		let remaining = limits.max_symbols.saturating_sub(self.seen_symbols.len());
		let admitted_unseen = if unseen.len() > remaining {
			self.search.symbol_limit_reached = true;
			unseen.iter().take(remaining).collect()
		} else {
			unseen
		};
		self.seen_symbols.union_with(&admitted_unseen);
		already_seen.union(&admitted_unseen)
	}

	fn admit_references(
		&mut self,
		read_index: &super::LinkageReadIndex,
		postings: Vec<&ReferenceSet>,
		reference_scope: &ReferenceScope<'_>,
		reuse_seen: bool,
		limits: BoundedPathLimits,
	) -> ReferenceBatch {
		self.references.admit(
			read_index,
			postings,
			reference_scope,
			reuse_seen,
			limits.max_edges,
		)
	}

	fn tally_missing_ordinal(&mut self) {
		self.references.tally_missing_ordinal();
	}
}

fn reference_postings_from_symbols<'a>(
	read_index: &'a super::LinkageReadIndex,
	direction: TraversalDirection,
	symbols: &SymbolSet,
	relations: &[&str],
) -> Vec<&'a ReferenceSet> {
	symbols
		.iter()
		.flat_map(|symbol| match direction {
			TraversalDirection::Forward => read_index.outgoing_postings(symbol, relations),
			TraversalDirection::Reverse => read_index.incoming_postings(symbol, relations),
		})
		.collect()
}

impl ReferenceTraversal {
	fn admit(
		&mut self,
		read_index: &super::LinkageReadIndex,
		postings: Vec<&ReferenceSet>,
		reference_scope: &ReferenceScope<'_>,
		reuse_seen: bool,
		max_edges: usize,
	) -> ReferenceBatch {
		let remaining = max_edges.saturating_sub(self.seen.len() as usize);
		let (already_seen, admitted_unseen, limit_reached) =
			bounded_posting_union(postings, reference_scope, &self.seen, reuse_seen, remaining);
		if limit_reached {
			self.limit_reached = true;
		}
		self.observe(read_index, &admitted_unseen);
		self.seen.union_with(&admitted_unseen);
		ReferenceBatch {
			references: already_seen.union(&admitted_unseen),
			limit_reached,
		}
	}

	fn observe(&mut self, read_index: &super::LinkageReadIndex, references: &ReferenceSet) {
		let classifications = &read_index.classifications;
		let resolved = references.intersection(classifications.resolved());
		let external = references.intersection(classifications.external());
		let candidate = references.intersection(classifications.candidate());
		let dynamic = references.intersection(classifications.dynamic());
		let manifest_blocked = references.intersection(classifications.manifest_blocked());
		let unresolved = references.intersection(classifications.unresolved());
		let mut classified = resolved.union(&external);
		classified.union_with(&candidate);
		classified.union_with(&dynamic);
		classified.union_with(&manifest_blocked);
		classified.union_with(&unresolved);
		let unclassified = references.difference(&classified);

		self.coverage.total += references.len() as usize;
		self.coverage.resolved += resolved.len() as usize;
		self.coverage.external += external.len() as usize;
		self.coverage.candidate += candidate.len() as usize;
		self.coverage.dynamic += dynamic.len() as usize;
		self.coverage.manifest_blocked += manifest_blocked.len() as usize;
		self.coverage.unresolved += (unresolved.len() + unclassified.len()) as usize;
		self.coverage.decided += (resolved.len() + external.len()) as usize;
		self.explored_edges += resolved.len() as usize;

		for reference in candidate.iter() {
			tally_reason(
				&mut self.coverage.gap_reasons,
				"candidate",
				classifications.candidate_reason(reference),
			);
		}
		for reference in dynamic.iter() {
			tally_reason(
				&mut self.coverage.gap_reasons,
				"dynamic",
				classifications.dynamic_reason(reference),
			);
		}
		if !manifest_blocked.is_empty() {
			*self
				.coverage
				.gap_reasons
				.entry("manifest_blocked".to_string())
				.or_default() += manifest_blocked.len() as usize;
		}
		for reference in unresolved.iter() {
			tally_reason(
				&mut self.coverage.gap_reasons,
				"unresolved",
				classifications.unresolved_reason(reference),
			);
		}
		if !unclassified.is_empty() {
			*self
				.coverage
				.gap_reasons
				.entry("unresolved:unclassified".to_string())
				.or_default() += unclassified.len() as usize;
		}
	}

	fn tally_missing_ordinal(&mut self) {
		*self
			.coverage
			.gap_reasons
			.entry("missing_symbol_ordinal".to_string())
			.or_default() += 1;
	}
}

fn bounded_posting_union(
	postings: Vec<&ReferenceSet>,
	reference_scope: &ReferenceScope<'_>,
	seen: &ReferenceSet,
	reuse_seen: bool,
	limit: usize,
) -> (ReferenceSet, ReferenceSet, bool) {
	let already_seen = if reuse_seen {
		reusable_references(&postings, reference_scope, seen)
	} else {
		ReferenceSet::new()
	};
	let candidates = PostingUnion::new(postings);
	let mut references = match reference_scope {
		ReferenceScope::All => ScopedPostingUnion::All(candidates),
		ReferenceScope::Terms(terms) => ScopedPostingUnion::Intersection {
			candidates,
			scope: PostingUnion::new(terms.clone()),
			candidate: None,
			allowed: None,
		},
	};
	let mut admitted = ReferenceSet::new();
	for reference in &mut references {
		if seen.contains(reference) {
			continue;
		}
		if admitted.len() as usize == limit {
			return (already_seen, admitted, true);
		}
		admitted.insert(reference);
	}
	(already_seen, admitted, false)
}

fn reusable_references(
	postings: &[&ReferenceSet],
	reference_scope: &ReferenceScope<'_>,
	seen: &ReferenceSet,
) -> ReferenceSet {
	if seen.is_empty() {
		return ReferenceSet::new();
	}
	let reusable = collect_posting_intersection(postings.to_vec(), vec![seen]);
	if reusable.is_empty() {
		return reusable;
	}
	match reference_scope {
		ReferenceScope::All => reusable,
		ReferenceScope::Terms(terms) => {
			collect_posting_intersection(vec![&reusable], terms.clone())
		}
	}
}

fn collect_posting_intersection(
	left: Vec<&ReferenceSet>,
	right: Vec<&ReferenceSet>,
) -> ReferenceSet {
	ScopedPostingUnion::Intersection {
		candidates: PostingUnion::new(left),
		scope: PostingUnion::new(right),
		candidate: None,
		allowed: None,
	}
	.collect()
}

struct PostingUnion<'a> {
	iterators: Vec<ReferenceSetIter<'a>>,
	heap: BinaryHeap<Reverse<(ReferenceOrdinal, usize)>>,
}

impl<'a> PostingUnion<'a> {
	fn new(postings: Vec<&'a ReferenceSet>) -> Self {
		let mut iterators = postings
			.into_iter()
			.map(ReferenceSet::ordered_iter)
			.collect::<Vec<_>>();
		let mut heap = BinaryHeap::new();
		for (index, iterator) in iterators.iter_mut().enumerate() {
			if let Some(reference) = iterator.next() {
				heap.push(Reverse((reference, index)));
			}
		}
		Self { iterators, heap }
	}

	fn advance_to(&mut self, target: ReferenceOrdinal) {
		while self
			.heap
			.peek()
			.is_some_and(|Reverse((reference, _))| *reference < target)
		{
			let Reverse((_, index)) = self.heap.pop().expect("peeked posting");
			let iterator = &mut self.iterators[index];
			iterator.advance_to(target);
			if let Some(reference) = iterator.next() {
				self.heap.push(Reverse((reference, index)));
			}
		}
	}
}

impl Iterator for PostingUnion<'_> {
	type Item = ReferenceOrdinal;

	fn next(&mut self) -> Option<Self::Item> {
		let Reverse((reference, index)) = self.heap.pop()?;
		if let Some(next) = self.iterators[index].next() {
			self.heap.push(Reverse((next, index)));
		}
		while self
			.heap
			.peek()
			.is_some_and(|Reverse((duplicate, _))| *duplicate == reference)
		{
			let Reverse((_, index)) = self.heap.pop().expect("peeked duplicate");
			if let Some(next) = self.iterators[index].next() {
				self.heap.push(Reverse((next, index)));
			}
		}
		Some(reference)
	}
}

enum ScopedPostingUnion<'a> {
	All(PostingUnion<'a>),
	Intersection {
		candidates: PostingUnion<'a>,
		scope: PostingUnion<'a>,
		candidate: Option<ReferenceOrdinal>,
		allowed: Option<ReferenceOrdinal>,
	},
}

impl Iterator for ScopedPostingUnion<'_> {
	type Item = ReferenceOrdinal;

	fn next(&mut self) -> Option<Self::Item> {
		match self {
			Self::All(candidates) => candidates.next(),
			Self::Intersection {
				candidates,
				scope,
				candidate,
				allowed,
			} => loop {
				if candidate.is_none() {
					*candidate = candidates.next();
				}
				if allowed.is_none() {
					*allowed = scope.next();
				}
				let left = (*candidate)?;
				let right = (*allowed)?;
				match left.cmp(&right) {
					std::cmp::Ordering::Less => {
						candidates.advance_to(right);
						*candidate = candidates.next();
					}
					std::cmp::Ordering::Greater => {
						scope.advance_to(left);
						*allowed = scope.next();
					}
					std::cmp::Ordering::Equal => {
						*candidate = candidates.next();
						*allowed = scope.next();
						return Some(left);
					}
				}
			},
		}
	}
}

fn reference_scope_terms<'a>(
	read_index: &'a super::LinkageReadIndex,
	scope: &'a BoundedPathScope,
	direction: TraversalDirection,
	relations: &[&str],
) -> ReferenceScope<'a> {
	if scope.all {
		return ReferenceScope::All;
	}
	if let Some(symbols) = &scope.symbols {
		let mut terms = vec![match direction {
			TraversalDirection::Forward => &read_index.references_without_target,
			TraversalDirection::Reverse => &read_index.references_without_source,
		}];
		for symbol in symbols.iter() {
			terms.extend(match direction {
				TraversalDirection::Forward => read_index.incoming_postings(symbol, relations),
				TraversalDirection::Reverse => read_index.outgoing_postings(symbol, relations),
			});
		}
		return ReferenceScope::Terms(terms);
	}
	let postings_by_file = match direction {
		TraversalDirection::Forward => &read_index.references_by_target_file,
		TraversalDirection::Reverse => &read_index.references_by_source_file,
	};
	if postings_by_file
		.keys()
		.all(|file| scope.sources.contains(*file))
	{
		return ReferenceScope::All;
	}
	let mut terms = vec![match direction {
		TraversalDirection::Forward => &read_index.references_without_target,
		TraversalDirection::Reverse => &read_index.references_without_source,
	}];
	if scope.sources.len() as usize <= postings_by_file.len() {
		terms.extend(
			scope
				.sources
				.iter()
				.filter_map(|file| postings_by_file.get(&file)),
		);
	} else {
		terms.extend(
			postings_by_file
				.iter()
				.filter(|(file, _)| scope.sources.contains(**file))
				.map(|(_, posting)| posting),
		);
	}
	ReferenceScope::Terms(terms)
}

fn finish_corridor(
	read_index: &super::LinkageReadIndex,
	limits: BoundedPathLimits,
	mut state: CorridorState,
	forward: CorridorLayers,
	reverse: CorridorLayers,
) -> BoundedCorridorSearch {
	let reverse_cumulative = cumulative_symbol_layers(&reverse.exact);
	let mut members = SymbolSet::new();
	for (forward_depth, layer) in forward.exact.iter().enumerate() {
		let remaining = limits.max_depth.saturating_sub(forward_depth);
		let Some(reverse_within) =
			reverse_cumulative.get(remaining.min(reverse_cumulative.len().saturating_sub(1)))
		else {
			continue;
		};
		members.union_with(&layer.intersection(reverse_within));
	}
	state.search.members = members
		.iter()
		.filter_map(|ordinal| read_index.symbol(ordinal))
		.collect();
	let edge_references = corridor_edge_references(
		read_index,
		limits.max_depth,
		&state.references.seen,
		&forward.exact,
		&reverse.exact,
	);
	state.search.edges = edge_references
		.iter()
		.filter_map(|reference| {
			let source = read_index.reference_source(reference)?;
			let target = read_index.reference_target(reference)?;
			Some(BoundedPathEdge {
				source: read_index.symbol(source)?,
				target: read_index.symbol(target)?,
				reference: read_index.reference_id(reference)?,
			})
		})
		.collect();
	state
		.search
		.edges
		.sort_by_key(|edge| (edge.source, edge.target, edge.reference));
	state.search.coverage = std::mem::take(&mut state.references.coverage);
	state.search.explored_edges = state.references.explored_edges;
	state.search.edge_limit_reached = state.references.limit_reached;
	state.search.explored_symbols = state.seen_symbols.len();
	state.search
}

fn cumulative_symbol_layers(layers: &[SymbolSet]) -> Vec<SymbolSet> {
	let mut cumulative = Vec::with_capacity(layers.len());
	let mut reached = SymbolSet::new();
	for layer in layers {
		reached.union_with(layer);
		cumulative.push(reached.clone());
	}
	cumulative
}

fn corridor_edge_references(
	read_index: &super::LinkageReadIndex,
	max_depth: usize,
	seen_references: &ReferenceSet,
	forward_layers: &[SymbolSet],
	reverse_layers: &[SymbolSet],
) -> ReferenceSet {
	let mut corridor = ReferenceSet::new();
	let resolved = seen_references.intersection(read_index.classifications.resolved());
	for reference in resolved.iter() {
		let (Some(source), Some(target)) = (
			read_index.reference_source(reference),
			read_index.reference_target(reference),
		) else {
			continue;
		};
		let (Some(forward_depth), Some(reverse_depth)) = (
			layer_depth(forward_layers, source),
			layer_depth(reverse_layers, target),
		) else {
			continue;
		};
		if forward_depth
			.saturating_add(1)
			.saturating_add(reverse_depth)
			<= max_depth
		{
			corridor.insert(reference);
		}
	}
	corridor
}

fn layer_depth(layers: &[SymbolSet], symbol: SymbolOrdinal) -> Option<usize> {
	layers.iter().position(|layer| layer.contains(symbol))
}

fn reconstruct_path(
	read_index: &super::LinkageReadIndex,
	predecessors: &FxHashMap<SymbolOrdinal, (SymbolOrdinal, ReferenceId)>,
	from: SymbolOrdinal,
	to: SymbolOrdinal,
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
		ResourceGeneration, SourceCatalog, SymbolRecord, UnresolvedReason, UnresolvedReference,
		WorkspaceTimings,
	};

	fn path_snapshot(
		symbols: Vec<SymbolRecord>,
		references: Vec<ReferenceRecord>,
		edges: Vec<LinkageEdge>,
		ordinals: Vec<(u32, SymbolId)>,
	) -> WorkspaceSnapshot {
		let generation = ResourceGeneration::new(1);
		let index = CodeIndex::with_references(generation, generation, symbols, references);
		let mut linkage = LinkageSnapshot::new(generation, generation, edges.len(), 0);
		linkage.resolved = edges;
		linkage.read_index = LinkageReadIndexHandle::from_snapshot_with_ordinals(
			&linkage,
			&index.references,
			ordinals,
		);
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
		assert_eq!(scoped.coverage.resolved, 0, "{scoped:?}");
		assert_eq!(scoped.coverage.decided, 0, "{scoped:?}");

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
		snapshot.linkage.read_index = LinkageReadIndexHandle::from_snapshot_with_ordinals(
			&snapshot.linkage,
			&snapshot.index.references,
			[(11, from), (97, to)],
		);

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
	fn edge_limit_stops_inside_a_large_indexed_adjacency() {
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
			.expect("limited path search");
		assert_eq!(search.coverage.total, 3, "{search:?}");
		assert_eq!(search.explored_edges, 3, "{search:?}");
		assert!(search.edge_limit_reached, "{search:?}");
	}

	#[test]
	fn corridor_reuses_seen_edges_after_a_lower_unseen_ordinal_reaches_the_limit() {
		let source = SourceId::at(0);
		let outsider = SymbolId::at(0, 0);
		let from = SymbolId::at(0, 1);
		let middle = SymbolId::at(0, 2);
		let to = SymbolId::at(0, 3);
		let lower_unseen = ReferenceId::at(0, 0);
		let first_seen = ReferenceId::at(0, 1);
		let second_seen = ReferenceId::at(0, 2);
		let snapshot = path_snapshot(
			vec![
				SymbolRecord::new(outsider, source, "outsider", "fn"),
				SymbolRecord::new(from, source, "from", "fn"),
				SymbolRecord::new(middle, source, "middle", "fn"),
				SymbolRecord::new(to, source, "to", "fn"),
			],
			vec![
				ReferenceRecord::new(
					lower_unseen,
					source,
					outsider,
					to.to_string(),
					"calls",
					None,
				),
				ReferenceRecord::new(first_seen, source, from, middle.to_string(), "calls", None),
				ReferenceRecord::new(second_seen, source, middle, to.to_string(), "calls", None),
			],
			vec![
				LinkageEdge::new(lower_unseen, to),
				LinkageEdge::new(first_seen, middle),
				LinkageEdge::new(second_seen, to),
			],
			vec![(1, outsider), (2, from), (3, middle), (4, to)],
		);
		let read_index = snapshot.linkage.read_index.get().expect("corridor index");
		let symbols = [outsider, from, middle, to]
			.into_iter()
			.filter_map(|symbol| read_index.ordinal(&symbol))
			.collect();
		let scope = BoundedCorridorScope::from_symbols(symbols, 4).expect("bounded scope");
		let search = snapshot
			.bounded_corridor(BoundedCorridorRequest {
				from,
				to,
				relations: &["calls".to_string()],
				limits: BoundedPathLimits {
					max_depth: 4,
					max_symbols: 4,
					max_edges: 2,
				},
				scope: &scope,
			})
			.expect("corridor search");

		assert_eq!(search.members, vec![from, middle, to], "{search:?}");
		assert_eq!(search.edges.len(), 2, "{search:?}");
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
				avoid: &[],
				limits,
				scope: &scope,
			})
			.expect("first search");
		let second_search = engine
			.search(BoundedPathRequest {
				from,
				to,
				relations: &relations,
				avoid: &[],
				limits,
				scope: &scope,
			})
			.expect("second search");
		let avoided_search = engine
			.search(BoundedPathRequest {
				from,
				to,
				relations: &relations,
				avoid: &[middle],
				limits,
				scope: &scope,
			})
			.expect("search avoiding the mandatory boundary");

		assert_eq!(first_search.path.len(), 1);
		assert_eq!(second_search.path.len(), 2);
		assert!(avoided_search.path.is_empty(), "{avoided_search:?}");
	}
}
