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
	source_files: RoaringBitmap,
	source_roots: RoaringBitmap,
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
pub struct BoundedPathSetRequest<'a> {
	pub from: &'a SymbolSet,
	pub to: &'a SymbolSet,
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

#[derive(Clone, Copy, Debug)]
pub struct BoundedCorridorSetRequest<'a> {
	pub from: &'a SymbolSet,
	pub to: &'a SymbolSet,
	pub relations: &'a [String],
	pub limits: BoundedPathLimits,
	pub scope: &'a BoundedCorridorScope,
}

impl BoundedPathScope {
	pub fn all() -> Self {
		Self {
			all: true,
			source_files: RoaringBitmap::new(),
			source_roots: RoaringBitmap::new(),
			symbols: None,
		}
	}

	pub fn from_sources(sources: impl IntoIterator<Item = SourceId>) -> Self {
		Self {
			all: false,
			source_files: sources
				.into_iter()
				.map(|source| source.file() as u32)
				.collect(),
			source_roots: RoaringBitmap::new(),
			symbols: None,
		}
	}

	pub fn from_source_roots(source_roots: impl IntoIterator<Item = usize>) -> Self {
		Self {
			all: false,
			source_files: RoaringBitmap::new(),
			source_roots: source_roots.into_iter().map(|root| root as u32).collect(),
			symbols: None,
		}
	}

	pub fn from_symbols(symbols: SymbolSet) -> Self {
		Self {
			all: false,
			source_files: RoaringBitmap::new(),
			source_roots: RoaringBitmap::new(),
			symbols: Some(symbols),
		}
	}

	fn contains(&self, read_index: &super::LinkageReadIndex, ordinal: SymbolOrdinal) -> bool {
		if self.all {
			return true;
		}
		if let Some(symbols) = &self.symbols {
			return symbols.contains(ordinal);
		}
		let Some(symbol) = read_index.symbol(ordinal) else {
			return false;
		};
		if !self.source_roots.is_empty() {
			return read_index
				.source_roots_by_file
				.get(symbol.file())
				.is_some_and(|root| self.source_roots.contains(*root));
		}
		self.source_files.contains(symbol.file() as u32)
	}

	fn exceeds(&self, max_symbols: usize) -> bool {
		self.symbols
			.as_ref()
			.is_some_and(|symbols| symbols.len() > max_symbols)
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
		if request.scope.exceeds(request.limits.max_symbols) {
			return None;
		}
		PathTraversal::new(
			self.read_index,
			request.relations,
			request.avoid,
			request.limits,
			request.scope,
		)
		.run(request.from, request.to)
	}

	pub fn search_between(&self, request: BoundedPathSetRequest<'_>) -> Option<BoundedPathSearch> {
		if request.from.is_empty()
			|| request.to.is_empty()
			|| request.from.len() > request.limits.max_symbols
			|| request.to.len() > request.limits.max_symbols
			|| request.scope.exceeds(request.limits.max_symbols)
		{
			return None;
		}
		PathTraversal::new(
			self.read_index,
			request.relations,
			request.avoid,
			request.limits,
			request.scope,
		)
		.run_between(request.from, request.to)
	}

	pub fn corridor(&self, request: BoundedCorridorRequest<'_>) -> Option<BoundedCorridorSearch> {
		if request.relations.is_empty()
			|| request
				.scope
				.as_path_scope()
				.exceeds(request.limits.max_symbols)
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

	pub fn corridor_between(
		&self,
		request: BoundedCorridorSetRequest<'_>,
	) -> Option<BoundedCorridorSearch> {
		if request.from.is_empty()
			|| request.to.is_empty()
			|| request.from.len() > request.limits.max_symbols
			|| request.to.len() > request.limits.max_symbols
			|| request.from.union_len(request.to) > request.limits.max_symbols
			|| request
				.scope
				.as_path_scope()
				.exceeds(request.limits.max_symbols)
			|| request.relations.is_empty()
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
		.run_between(request.from, request.to)
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

	pub fn bounded_path_between(
		&self,
		request: BoundedPathSetRequest<'_>,
	) -> Option<BoundedPathSearch> {
		BoundedPathEngine::new(&self.index, &self.linkage)?.search_between(request)
	}

	pub fn bounded_corridor(
		&self,
		request: BoundedCorridorRequest<'_>,
	) -> Option<BoundedCorridorSearch> {
		bounded_corridor(&self.index, &self.linkage, request)
	}

	pub fn bounded_corridor_between(
		&self,
		request: BoundedCorridorSetRequest<'_>,
	) -> Option<BoundedCorridorSearch> {
		BoundedPathEngine::new(&self.index, &self.linkage)?.corridor_between(request)
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

	fn run(self, from: SymbolId, to: SymbolId) -> Option<BoundedPathSearch> {
		let from = SymbolSet::from_symbol(self.read_index.ordinal(&from)?);
		let to = SymbolSet::from_symbol(self.read_index.ordinal(&to)?);
		self.run_between(&from, &to)
	}

	fn run_between(mut self, from: &SymbolSet, to: &SymbolSet) -> Option<BoundedPathSearch> {
		let mut from_ordinals = self.endpoint_ordinals(from);
		let to_ordinals = self.endpoint_ordinals(to);
		if from_ordinals.is_empty() || to_ordinals.is_empty() {
			return None;
		}
		if from_ordinals.len() > self.max_symbols {
			self.search.symbol_limit_reached = true;
			from_ordinals = from_ordinals.iter().take(self.max_symbols).collect();
		}
		let mut frontier = from_ordinals.clone();
		self.visited = from_ordinals.clone();
		if !from_ordinals.intersects(&to_ordinals) {
			self.walk(&mut frontier, &to_ordinals);
		}
		self.search.coverage = std::mem::take(&mut self.references.coverage);
		self.search.explored_edges = self.references.explored_edges;
		self.search.edge_limit_reached = self.references.limit_reached;
		self.search.explored_symbols = self.visited.len();
		if let Some(to_ordinal) = self
			.visited
			.iter()
			.find(|ordinal| to_ordinals.contains(*ordinal))
		{
			self.search.path = reconstruct_path_from_any(
				self.read_index,
				&self.predecessors,
				&from_ordinals,
				to_ordinal,
			)?;
		}
		Some(self.search)
	}

	fn endpoint_ordinals(&self, symbols: &SymbolSet) -> SymbolSet {
		symbols
			.iter()
			.filter(|ordinal| {
				self.scope.contains(self.read_index, *ordinal) && !self.avoided.contains(*ordinal)
			})
			.collect()
	}

	fn walk(&mut self, frontier: &mut SymbolSet, to_ordinals: &SymbolSet) {
		for depth in 0..=self.max_depth {
			self.search.depth_reached = depth;
			let selection = ReferenceSelection {
				direction: TraversalDirection::Forward,
				frontier,
				scope: self.scope,
				reuse_seen: false,
			};
			let batch =
				self.references
					.admit(self.read_index, &self.relations, selection, self.max_edges);
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
			if next.intersects(to_ordinals) {
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

struct ReferencePostingUnion<'a> {
	iterators: Vec<ReferenceSetIter<'a>>,
	heap: BinaryHeap<Reverse<(ReferenceOrdinal, usize)>>,
}

struct ReferencePostingIntersection<'a> {
	left: ReferencePostingUnion<'a>,
	right: ReferencePostingUnion<'a>,
	left_current: Option<ReferenceOrdinal>,
	right_current: Option<ReferenceOrdinal>,
}

impl<'a> ReferencePostingUnion<'a> {
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

impl Iterator for ReferencePostingUnion<'_> {
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

impl<'a> ReferencePostingIntersection<'a> {
	fn new(left: Vec<&'a ReferenceSet>, right: Vec<&'a ReferenceSet>) -> Self {
		Self {
			left: ReferencePostingUnion::new(left),
			right: ReferencePostingUnion::new(right),
			left_current: None,
			right_current: None,
		}
	}
}

impl Iterator for ReferencePostingIntersection<'_> {
	type Item = ReferenceOrdinal;

	fn next(&mut self) -> Option<Self::Item> {
		loop {
			if self.left_current.is_none() {
				self.left_current = self.left.next();
			}
			if self.right_current.is_none() {
				self.right_current = self.right.next();
			}
			let left = self.left_current?;
			let right = self.right_current?;
			match left.cmp(&right) {
				std::cmp::Ordering::Less => {
					self.left.advance_to(right);
					self.left_current = None;
				}
				std::cmp::Ordering::Greater => {
					self.right.advance_to(left);
					self.right_current = None;
				}
				std::cmp::Ordering::Equal => {
					self.left_current = None;
					self.right_current = None;
					return Some(left);
				}
			}
		}
	}
}

#[derive(Clone, Copy)]
struct ReferenceSelection<'a> {
	direction: TraversalDirection,
	frontier: &'a SymbolSet,
	scope: &'a BoundedPathScope,
	reuse_seen: bool,
}

enum ReferenceScope<'a> {
	All,
	Terms(Vec<&'a ReferenceSet>),
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

	fn run(self, from: SymbolId, to: SymbolId) -> Option<BoundedCorridorSearch> {
		let from = SymbolSet::from_symbol(self.read_index.ordinal(&from)?);
		let to = SymbolSet::from_symbol(self.read_index.ordinal(&to)?);
		self.run_between(&from, &to)
	}

	fn run_between(mut self, from: &SymbolSet, to: &SymbolSet) -> Option<BoundedCorridorSearch> {
		let (from, to) = self.endpoints_between(from, to)?;
		let same = from == to;
		let forward_start = self.state.admit_symbols(from, self.limits);
		let reverse_start = if same {
			forward_start.clone()
		} else {
			self.state.admit_symbols(to, self.limits)
		};
		let forward = if forward_start.is_empty() {
			CorridorLayers::default()
		} else if same {
			CorridorLayers::from_start(forward_start)
		} else {
			self.walk(TraversalDirection::Forward, forward_start)
		};
		let reverse = if reverse_start.is_empty() {
			CorridorLayers::default()
		} else if same {
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

	fn endpoints_between(
		&self,
		from: &SymbolSet,
		to: &SymbolSet,
	) -> Option<(SymbolSet, SymbolSet)> {
		let collect = |symbols: &SymbolSet| {
			symbols
				.iter()
				.filter(|ordinal| self.scope.contains(self.read_index, *ordinal))
				.collect::<SymbolSet>()
		};
		let from = collect(from);
		let to = collect(to);
		(!from.is_empty() && !to.is_empty()).then_some((from, to))
	}

	fn walk(&mut self, direction: TraversalDirection, start: SymbolSet) -> CorridorLayers {
		let mut frontier = start.clone();
		let mut reached = start;
		let mut exact = vec![frontier.clone()];
		for depth in 0..=self.limits.max_depth {
			self.record_depth(direction, depth);
			let selection = ReferenceSelection {
				direction,
				frontier: &frontier,
				scope: self.scope,
				reuse_seen: matches!(direction, TraversalDirection::Reverse),
			};
			let batch = self.state.admit_references(
				self.read_index,
				&self.relations,
				selection,
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
		relations: &[&str],
		selection: ReferenceSelection<'_>,
		limits: BoundedPathLimits,
	) -> ReferenceBatch {
		self.references
			.admit(read_index, relations, selection, limits.max_edges)
	}

	fn tally_missing_ordinal(&mut self) {
		self.references.tally_missing_ordinal();
	}
}

impl ReferenceTraversal {
	fn admit(
		&mut self,
		read_index: &super::LinkageReadIndex,
		relations: &[&str],
		selection: ReferenceSelection<'_>,
		max_edges: usize,
	) -> ReferenceBatch {
		let remaining = max_edges.saturating_sub(self.seen.len() as usize);
		let (already_seen, admitted_unseen, limit_reached) =
			bounded_frontier_references(read_index, relations, selection, &self.seen, remaining);
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

fn bounded_frontier_references(
	read_index: &super::LinkageReadIndex,
	relations: &[&str],
	selection: ReferenceSelection<'_>,
	seen: &ReferenceSet,
	limit: usize,
) -> (ReferenceSet, ReferenceSet, bool) {
	let already_seen = if selection.reuse_seen {
		seen.iter()
			.filter(|reference| {
				reference_matches_selection(read_index, relations, selection, *reference)
			})
			.collect()
	} else {
		ReferenceSet::new()
	};
	let mut admission = ReferenceAdmission::new(seen, limit, already_seen);
	let frontier_postings = reference_postings_from_symbols(
		read_index,
		selection.direction,
		selection.frontier,
		relations,
	);
	match reference_scope_terms(read_index, selection.scope, selection.direction, relations) {
		ReferenceScope::All => admission.admit(ReferencePostingUnion::new(frontier_postings)),
		ReferenceScope::Terms(scope_postings) => admission.admit(
			ReferencePostingIntersection::new(frontier_postings, scope_postings),
		),
	}
	admission.finish()
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

struct ReferenceAdmission<'a> {
	seen: &'a ReferenceSet,
	limit: usize,
	already_seen: ReferenceSet,
	admitted: ReferenceSet,
	limit_reached: bool,
}

impl<'a> ReferenceAdmission<'a> {
	fn new(seen: &'a ReferenceSet, limit: usize, already_seen: ReferenceSet) -> Self {
		Self {
			seen,
			limit,
			already_seen,
			admitted: ReferenceSet::new(),
			limit_reached: false,
		}
	}

	fn admit(&mut self, references: impl Iterator<Item = ReferenceOrdinal>) {
		if self.limit_reached {
			return;
		}
		for reference in references {
			if self.seen.contains(reference) {
				continue;
			}
			if self.admitted.len() as usize == self.limit {
				self.limit_reached = true;
				return;
			}
			self.admitted.insert(reference);
		}
	}

	fn finish(self) -> (ReferenceSet, ReferenceSet, bool) {
		(self.already_seen, self.admitted, self.limit_reached)
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
	let (selected_boundaries, postings_by_boundary) = if !scope.source_roots.is_empty() {
		(
			&scope.source_roots,
			match direction {
				TraversalDirection::Forward => &read_index.references_by_target_root,
				TraversalDirection::Reverse => &read_index.references_by_source_root,
			},
		)
	} else {
		(
			&scope.source_files,
			match direction {
				TraversalDirection::Forward => &read_index.references_by_target_file,
				TraversalDirection::Reverse => &read_index.references_by_source_file,
			},
		)
	};
	let mut terms = vec![match direction {
		TraversalDirection::Forward => &read_index.references_without_target,
		TraversalDirection::Reverse => &read_index.references_without_source,
	}];
	if selected_boundaries.len() as usize <= postings_by_boundary.len() {
		terms.extend(
			selected_boundaries
				.iter()
				.filter_map(|boundary| postings_by_boundary.get(&boundary)),
		);
	} else {
		terms.extend(
			postings_by_boundary
				.iter()
				.filter(|(boundary, _)| selected_boundaries.contains(**boundary))
				.map(|(_, posting)| posting),
		);
	}
	ReferenceScope::Terms(terms)
}

fn reference_matches_selection(
	read_index: &super::LinkageReadIndex,
	relations: &[&str],
	selection: ReferenceSelection<'_>,
	reference: ReferenceOrdinal,
) -> bool {
	let (frontier, opposite) = match selection.direction {
		TraversalDirection::Forward => (
			read_index.reference_source(reference),
			read_index.reference_target(reference),
		),
		TraversalDirection::Reverse => (
			read_index.reference_target(reference),
			read_index.reference_source(reference),
		),
	};
	let Some(frontier) = frontier else {
		return false;
	};
	if !selection.frontier.contains(frontier)
		|| opposite.is_some_and(|opposite| !selection.scope.contains(read_index, opposite))
	{
		return false;
	}
	let postings = match selection.direction {
		TraversalDirection::Forward => read_index.outgoing_postings(frontier, relations),
		TraversalDirection::Reverse => read_index.incoming_postings(frontier, relations),
	};
	postings
		.into_iter()
		.any(|posting| posting.contains(reference))
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

fn reconstruct_path_from_any(
	read_index: &super::LinkageReadIndex,
	predecessors: &FxHashMap<SymbolOrdinal, (SymbolOrdinal, ReferenceId)>,
	from: &SymbolSet,
	to: SymbolOrdinal,
) -> Option<Vec<BoundedPathEdge>> {
	let mut path = Vec::new();
	let mut current = to;
	while !from.contains(current) {
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
	use std::sync::Arc;

	use super::*;
	use crate::snapshot::{
		CandidateReason, CandidateReference, CandidateScope, ChangeOverlay, CodeIndex,
		DynamicReason, DynamicReference, ExternalReference, ExternalReferenceOrigin, LinkageEdge,
		LinkageReadIndexHandle, LinkageSnapshot, ReferenceRecord, ResolutionEvidence,
		ResourceGeneration, SourceCatalog, SourceId, SymbolRecord, UnresolvedReason,
		UnresolvedReference, WorkspaceTimings,
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
		let selected = BoundedPathScope::from_source_roots([first_source.file()]);
		let scoped = snapshot
			.bounded_path(from, to, &["calls".to_string()], limits, &selected)
			.expect("scoped path search");
		assert!(scoped.path.is_empty(), "{scoped:?}");
		assert_eq!(scoped.coverage.resolved, 0, "{scoped:?}");
		assert_eq!(scoped.coverage.decided, 0, "{scoped:?}");

		let all_sources =
			BoundedPathScope::from_source_roots([first_source.file(), other_source.file()]);
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
	fn selected_root_scope_uses_one_posting_across_many_files() {
		let from_source = SourceId::at(0);
		let to_source = SourceId::at(1);
		let from = SymbolId::at(0, 0);
		let to = SymbolId::at(1, 0);
		let reference = ReferenceId::at(0, 0);
		let mut snapshot = path_snapshot(
			vec![
				SymbolRecord::new(from, from_source, "from", "fn"),
				SymbolRecord::new(to, to_source, "to", "fn"),
			],
			vec![ReferenceRecord::new(
				reference,
				from_source,
				from,
				to.to_string(),
				"calls",
				None,
			)],
			vec![LinkageEdge::new(reference, to)],
			vec![(0, from), (1, to)],
		);
		snapshot.linkage.read_index = LinkageReadIndexHandle::from_snapshot_with_catalog(
			&snapshot.linkage,
			&snapshot.index.references,
			Arc::clone(snapshot.index.inventory.catalog()),
			vec![0, 0],
		);
		let scope = BoundedPathScope::from_source_roots([0]);
		let search = snapshot
			.bounded_path(
				from,
				to,
				&["calls".to_string()],
				BoundedPathLimits {
					max_depth: 1,
					max_symbols: 2,
					max_edges: 1,
				},
				&scope,
			)
			.expect("same-root cross-file path");
		assert_eq!(search.path.len(), 1, "{search:?}");
		let read_index = snapshot.linkage.read_index.get().expect("read index");
		assert_eq!(read_index.references_by_target_root.len(), 1);
		assert_eq!(
			read_index
				.references_by_target_root
				.get(&0)
				.expect("root posting")
				.len(),
			1
		);
	}

	#[test]
	fn owner_endpoint_sets_find_member_paths_with_bitmap_traversal() {
		let source = SourceId::at(0);
		let owner = SymbolId::at(0, 0);
		let member = SymbolId::at(0, 1);
		let target = SymbolId::at(0, 2);
		let reference = ReferenceId::at(0, 0);
		let snapshot = path_snapshot(
			vec![
				SymbolRecord::new(owner, source, "Owner", "struct"),
				SymbolRecord::new(member, source, "field", "field"),
				SymbolRecord::new(target, source, "Target", "struct"),
			],
			vec![ReferenceRecord::new(
				reference,
				source,
				member,
				target.to_string(),
				"uses_type",
				None,
			)],
			vec![LinkageEdge::new(reference, target)],
			vec![(1, owner), (2, member), (3, target)],
		);
		let relations = vec!["uses_type".to_string()];
		let limits = BoundedPathLimits {
			max_depth: 2,
			max_symbols: 3,
			max_edges: 3,
		};
		let read_index = snapshot.linkage.read_index.get().expect("path index");
		let endpoint_set = |symbols: &[SymbolId]| {
			symbols
				.iter()
				.filter_map(|symbol| read_index.ordinal(symbol))
				.collect::<SymbolSet>()
		};
		let from_endpoints = endpoint_set(&[owner, member]);
		let to_endpoints = endpoint_set(&[target]);
		let path_scope = BoundedPathScope::from_source_roots([source.file()]);
		let path = snapshot
			.bounded_path_between(BoundedPathSetRequest {
				from: &from_endpoints,
				to: &to_endpoints,
				relations: &relations,
				avoid: &[],
				limits,
				scope: &path_scope,
			})
			.expect("owner-expanded path search");
		assert_eq!(path.path.len(), 1, "{path:?}");
		assert_eq!(path.path[0].source, member, "{path:?}");
		assert_eq!(path.path[0].target, target, "{path:?}");

		let corridor_symbols = [owner, member, target]
			.into_iter()
			.filter_map(|symbol| read_index.ordinal(&symbol))
			.collect();
		let corridor_scope =
			BoundedCorridorScope::from_symbols(corridor_symbols, 3).expect("bounded scope");
		let corridor = snapshot
			.bounded_corridor_between(BoundedCorridorSetRequest {
				from: &from_endpoints,
				to: &to_endpoints,
				relations: &relations,
				limits,
				scope: &corridor_scope,
			})
			.expect("owner-expanded corridor search");
		assert_eq!(corridor.members, vec![member, target], "{corridor:?}");
		assert_eq!(corridor.edges.len(), 1, "{corridor:?}");
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
				&BoundedPathScope::from_source_roots([source.file()]),
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
				&BoundedPathScope::from_source_roots([source.file()]),
			)
			.expect("limited path search");
		assert_eq!(search.coverage.total, 3, "{search:?}");
		assert_eq!(search.explored_edges, 3, "{search:?}");
		assert!(search.edge_limit_reached, "{search:?}");
	}

	#[test]
	fn edge_budget_stops_before_materializing_frontier_relation_product() {
		let source = SourceId::at(0);
		let sink = SymbolId::at(0, 512);
		let mut symbols = (0..512)
			.map(|index| {
				let id = SymbolId::at(0, index);
				SymbolRecord::new(id, source, format!("from_{index}"), "fn")
			})
			.collect::<Vec<_>>();
		symbols.push(SymbolRecord::new(sink, source, "sink", "fn"));
		let relations = (0..16).map(|index| format!("r{index}")).collect::<Vec<_>>();
		let mut references = Vec::with_capacity(512 * relations.len());
		let mut edges = Vec::with_capacity(512 * relations.len());
		for symbol in 0..512 {
			for relation in &relations {
				let id = ReferenceId::at(0, references.len());
				references.push(ReferenceRecord::new(
					id,
					source,
					SymbolId::at(0, symbol),
					sink.to_string(),
					relation,
					None,
				));
				edges.push(LinkageEdge::new(id, sink));
			}
		}
		let ordinals = (0..=512)
			.map(|index| ((index + 1) as u32, SymbolId::at(0, index)))
			.collect::<Vec<_>>();
		let snapshot = path_snapshot(symbols, references, edges, ordinals);
		let read_index = snapshot.linkage.read_index.get().expect("path index");
		let from = (0..512)
			.filter_map(|index| read_index.ordinal(&SymbolId::at(0, index)))
			.collect();
		let to = SymbolSet::from_symbol(read_index.ordinal(&sink).expect("sink ordinal"));
		let search = snapshot
			.bounded_path_between(BoundedPathSetRequest {
				from: &from,
				to: &to,
				relations: &relations,
				avoid: &[],
				limits: BoundedPathLimits {
					max_depth: 1,
					max_symbols: 1_024,
					max_edges: 1,
				},
				scope: &BoundedPathScope::all(),
			})
			.expect("bounded frontier path");
		assert_eq!(search.coverage.total, 1, "{search:?}");
		assert_eq!(search.explored_edges, 1, "{search:?}");
		assert!(search.edge_limit_reached, "{search:?}");
	}

	#[test]
	fn relation_or_order_does_not_change_a_budgeted_path() {
		let source = SourceId::at(0);
		let from = SymbolId::at(0, 0);
		let dead_end = SymbolId::at(0, 1);
		let to = SymbolId::at(0, 2);
		let dead = ReferenceId::at(0, 0);
		let live = ReferenceId::at(0, 1);
		let snapshot = path_snapshot(
			vec![
				SymbolRecord::new(from, source, "from", "fn"),
				SymbolRecord::new(dead_end, source, "dead_end", "fn"),
				SymbolRecord::new(to, source, "to", "fn"),
			],
			vec![
				ReferenceRecord::new(dead, source, from, dead_end.to_string(), "dead", None),
				ReferenceRecord::new(live, source, from, to.to_string(), "live", None),
			],
			vec![LinkageEdge::new(dead, dead_end), LinkageEdge::new(live, to)],
			vec![(1, from), (2, dead_end), (3, to)],
		);
		let limits = BoundedPathLimits {
			max_depth: 1,
			max_symbols: 3,
			max_edges: 1,
		};
		let first = snapshot
			.bounded_path(
				from,
				to,
				&["dead".to_string(), "live".to_string()],
				limits,
				&BoundedPathScope::all(),
			)
			.expect("first relation order");
		let reversed = snapshot
			.bounded_path(
				from,
				to,
				&["live".to_string(), "dead".to_string()],
				limits,
				&BoundedPathScope::all(),
			)
			.expect("reversed relation order");
		assert_eq!(first, reversed);
		assert!(first.path.is_empty(), "{first:?}");
		assert!(first.edge_limit_reached, "{first:?}");
	}

	#[test]
	fn selective_scope_postings_anchor_before_a_large_frontier_adjacency() {
		let source = SourceId::at(0);
		let from = SymbolId::at(0, 0);
		let to = SymbolId::at(0, 1_025);
		let mut symbols = vec![SymbolRecord::new(from, source, "from", "fn")];
		let mut references = Vec::new();
		let mut edges = Vec::new();
		for index in 1..=1_024 {
			let target = SymbolId::at(0, index);
			symbols.push(SymbolRecord::new(
				target,
				source,
				format!("outside_{index}"),
				"fn",
			));
			let reference = ReferenceId::at(0, references.len());
			references.push(ReferenceRecord::new(
				reference,
				source,
				from,
				target.to_string(),
				"calls",
				None,
			));
			edges.push(LinkageEdge::new(reference, target));
		}
		symbols.push(SymbolRecord::new(to, source, "to", "fn"));
		let live = ReferenceId::at(0, references.len());
		references.push(ReferenceRecord::new(
			live,
			source,
			from,
			to.to_string(),
			"calls",
			None,
		));
		edges.push(LinkageEdge::new(live, to));
		let ordinals = (0..=1_025)
			.map(|index| ((index + 1) as u32, SymbolId::at(0, index)))
			.collect::<Vec<_>>();
		let snapshot = path_snapshot(symbols, references, edges, ordinals);
		let read_index = snapshot.linkage.read_index.get().expect("path index");
		let scope_symbols = [from, to]
			.into_iter()
			.filter_map(|symbol| read_index.ordinal(&symbol))
			.collect();
		let search = snapshot
			.bounded_path(
				from,
				to,
				&["calls".to_string()],
				BoundedPathLimits {
					max_depth: 1,
					max_symbols: 2,
					max_edges: 1,
				},
				&BoundedPathScope::from_symbols(scope_symbols),
			)
			.expect("scope-anchored path");
		assert_eq!(search.path.len(), 1, "{search:?}");
		assert!(!search.edge_limit_reached, "{search:?}");
	}

	#[test]
	fn scope_intersection_precedes_edge_lookahead() {
		let source = SourceId::at(0);
		let from = SymbolId::at(0, 0);
		let first_outsider = SymbolId::at(0, 1);
		let second_outsider = SymbolId::at(0, 2);
		let to = SymbolId::at(0, 3);
		let first = ReferenceId::at(0, 0);
		let second = ReferenceId::at(0, 1);
		let live = ReferenceId::at(0, 2);
		let snapshot = path_snapshot(
			vec![
				SymbolRecord::new(from, source, "from", "fn"),
				SymbolRecord::new(first_outsider, source, "first_outsider", "fn"),
				SymbolRecord::new(second_outsider, source, "second_outsider", "fn"),
				SymbolRecord::new(to, source, "to", "fn"),
			],
			vec![
				ReferenceRecord::new(first, source, first_outsider, to.to_string(), "calls", None),
				ReferenceRecord::new(
					second,
					source,
					second_outsider,
					to.to_string(),
					"calls",
					None,
				),
				ReferenceRecord::new(live, source, from, to.to_string(), "calls", None),
			],
			vec![
				LinkageEdge::new(first, to),
				LinkageEdge::new(second, to),
				LinkageEdge::new(live, to),
			],
			vec![
				(1, from),
				(2, first_outsider),
				(3, second_outsider),
				(4, to),
			],
		);
		let read_index = snapshot.linkage.read_index.get().expect("path index");
		let scope = [from, to]
			.into_iter()
			.filter_map(|symbol| read_index.ordinal(&symbol))
			.collect();
		let search = snapshot
			.bounded_path(
				from,
				to,
				&["calls".to_string()],
				BoundedPathLimits {
					max_depth: 1,
					max_symbols: 2,
					max_edges: 1,
				},
				&BoundedPathScope::from_symbols(scope),
			)
			.expect("scope-anchored path");

		assert_eq!(search.path.len(), 1, "{search:?}");
		assert_eq!(search.path[0].reference, live, "{search:?}");
		assert!(!search.edge_limit_reached, "{search:?}");
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
		let scope = BoundedPathScope::from_source_roots([source.file()]);
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

	#[test]
	fn set_entrypoints_reject_oversized_endpoint_bitmaps_before_traversal() {
		let source = SourceId::at(0);
		let first = SymbolId::at(0, 0);
		let second = SymbolId::at(0, 1);
		let snapshot = path_snapshot(
			vec![
				SymbolRecord::new(first, source, "first", "fn"),
				SymbolRecord::new(second, source, "second", "fn"),
			],
			Vec::new(),
			Vec::new(),
			vec![(1, first), (2, second)],
		);
		let engine =
			BoundedPathEngine::new(&snapshot.index, &snapshot.linkage).expect("path engine");
		let catalog = snapshot.index.inventory.catalog();
		let first = catalog.ordinal(&first).expect("first ordinal");
		let second = catalog.ordinal(&second).expect("second ordinal");
		let both = [first, second].into_iter().collect::<SymbolSet>();
		let first_only = SymbolSet::from_symbol(first);
		let second_only = SymbolSet::from_symbol(second);
		let relations = vec!["calls".to_string()];
		let limits = BoundedPathLimits {
			max_depth: 1,
			max_symbols: 1,
			max_edges: 1,
		};
		let path_scope = BoundedPathScope::from_source_roots([source.file()]);
		assert!(
			engine
				.search_between(BoundedPathSetRequest {
					from: &both,
					to: &second_only,
					relations: &relations,
					avoid: &[],
					limits,
					scope: &path_scope,
				})
				.is_none()
		);
		let oversized_scope = BoundedPathScope::from_symbols(both.clone());
		assert!(
			engine
				.search_between(BoundedPathSetRequest {
					from: &first_only,
					to: &second_only,
					relations: &relations,
					avoid: &[],
					limits,
					scope: &oversized_scope,
				})
				.is_none()
		);
		let corridor_scope =
			BoundedCorridorScope::from_symbols(both.clone(), 2).expect("corridor scope");
		assert!(
			engine
				.corridor_between(BoundedCorridorSetRequest {
					from: &first_only,
					to: &first_only,
					relations: &relations,
					limits,
					scope: &corridor_scope,
				})
				.is_none(),
			"the request endpoints fit max_symbols, but the corridor scope does not"
		);
		assert!(
			engine
				.corridor_between(BoundedCorridorSetRequest {
					from: &first_only,
					to: &second_only,
					relations: &relations,
					limits,
					scope: &corridor_scope,
				})
				.is_none()
		);
	}
}
