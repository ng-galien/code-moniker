//! Evaluate structural predicates over the published index and resolved linkage.
//! No source concatenation and no change to the scope of local predicates.
use super::value::{apply_op, apply_op_values};
use super::{Atom, AtomOutcome, Domain, Failure, Node, NodeOutcome, QuantKind, Value, walk_node};
use crate::check::expr::{CollectionExpr, CollectionOp, Lhs, LhsExpr, NumberExpr, Op, Rhs};
use code_moniker_workspace::snapshot::{
	CodeIndex, LinkageSnapshot, ReferenceId, ReferenceRecord, SymbolId, SymbolRecord,
};
use rustc_hash::{FxHashMap, FxHashSet};

mod validate;
pub(in crate::check) use validate::validate;

#[derive(Clone, Copy)]
enum Item<'a> {
	Symbol(SymbolId),
	Reference(&'a ReferenceRecord),
}

pub(in crate::check) struct IndexedPredicates<'a> {
	index: &'a CodeIndex,
	symbols: FxHashMap<SymbolId, &'a SymbolRecord>,
	children: FxHashMap<SymbolId, Vec<SymbolId>>,
	outgoing: FxHashMap<SymbolId, Vec<&'a ReferenceRecord>>,
	incoming: FxHashMap<SymbolId, Vec<&'a ReferenceRecord>>,
	targets: FxHashMap<ReferenceId, SymbolId>,
	uncertain_incoming: FxHashSet<SymbolId>,
	uncertain_children: FxHashSet<SymbolId>,
}

impl<'a> IndexedPredicates<'a> {
	pub(in crate::check) fn new(index: &'a CodeIndex, linkage: &'a LinkageSnapshot) -> Self {
		let symbols: FxHashMap<_, _> = index.symbols.iter().map(|s| (s.id, s)).collect();
		let targets: FxHashMap<_, _> = linkage
			.resolved
			.iter()
			.map(|r| (r.reference, r.target))
			.collect();
		let mut children: FxHashMap<_, Vec<_>> = FxHashMap::default();
		let mut outgoing: FxHashMap<_, Vec<_>> = FxHashMap::default();
		let mut incoming: FxHashMap<_, Vec<_>> = FxHashMap::default();
		let memberships: FxHashMap<_, _> = index
			.references
			.iter()
			.filter(|r| r.kind == "member_of")
			.map(|r| (r.id, r.source_symbol))
			.collect();
		let members: FxHashSet<_> = memberships.values().copied().collect();
		for symbol in index.symbols.iter() {
			if members.contains(&symbol.id) {
				continue;
			}
			if let Some(parent) = symbol.parent {
				children.entry(parent).or_default().push(symbol.id);
			}
		}
		for (reference, member) in &memberships {
			if let Some(owner) = targets.get(reference) {
				children.entry(*owner).or_default().push(*member);
			}
		}
		let mut uncertain_children: FxHashSet<_> = linkage
			.candidates
			.iter()
			.filter(|r| memberships.contains_key(&r.reference))
			.flat_map(|r| r.targets.iter().copied())
			.chain(
				linkage
					.dynamic
					.iter()
					.filter(|r| memberships.contains_key(&r.reference))
					.flat_map(|r| r.candidates.iter().copied()),
			)
			.collect();
		for items in children.values_mut() {
			items.sort_by_key(|id| (symbols[id].byte_range, *id));
			items.dedup();
		}
		for reference in index.references.iter() {
			outgoing
				.entry(reference.source_symbol)
				.or_default()
				.push(reference);
			if let Some(target) = targets.get(&reference.id) {
				incoming.entry(*target).or_default().push(reference);
			}
		}
		let mut uncertain_incoming: FxHashSet<_> = linkage
			.candidates
			.iter()
			.flat_map(|r| r.targets.iter().copied())
			.chain(
				linkage
					.dynamic
					.iter()
					.flat_map(|r| r.candidates.iter().copied()),
			)
			.collect();
		// Do not treat an unresolved exact target as proof that its incoming set is empty.
		for reference in index
			.references
			.iter()
			.filter(|r| !targets.contains_key(&r.id))
		{
			if let Some(items) = index
				.inventory
				.facets()
				.symbols_by_identity(&reference.target_identity)
			{
				for ordinal in items.iter() {
					if let Some(record) = index.inventory.record(ordinal) {
						uncertain_incoming.insert(record.id);
						if memberships.contains_key(&reference.id) {
							uncertain_children.insert(record.id);
						}
					}
				}
			}
		}
		Self {
			index,
			symbols,
			children,
			outgoing,
			incoming,
			targets,
			uncertain_incoming,
			uncertain_children,
		}
	}

	pub(in crate::check) fn evaluate(&self, node: &Node, id: SymbolId) -> Option<bool> {
		truth(self.node(node, Item::Symbol(id), Item::Symbol(id)))
	}

	fn node(&self, node: &Node, item: Item<'a>, current: Item<'a>) -> NodeOutcome {
		walk_node(
			node,
			&|a| self.atom(a, item, current),
			&|kind, domain, filter| self.quantifier(kind, domain, filter, item),
			&|_| NodeOutcome::NotApplicable,
			&|_| NodeOutcome::NotApplicable,
		)
	}

	fn quantifier(
		&self,
		kind: QuantKind,
		domain: &Domain,
		filter: &Node,
		item: Item<'a>,
	) -> NodeOutcome {
		let (items, complete) = self.domain(domain, item);
		let mut unknown = !complete;
		for nested in items {
			match (kind, truth(self.node(filter, nested, item))) {
				(QuantKind::Any, Some(true)) => return NodeOutcome::Pass,
				(QuantKind::All, Some(false)) | (QuantKind::None, Some(true)) => return failed(),
				(_, None) => unknown = true,
				_ => {}
			}
		}
		if unknown {
			NodeOutcome::NotApplicable
		} else if kind == QuantKind::Any {
			failed()
		} else {
			NodeOutcome::Pass
		}
	}

	fn source(&self, item: Item<'a>) -> SymbolId {
		match item {
			Item::Symbol(id) => id,
			Item::Reference(r) => r.source_symbol,
		}
	}

	fn domain(&self, domain: &Domain, item: Item<'a>) -> (Vec<Item<'a>>, bool) {
		let source = self.source(item);
		match domain {
			Domain::Children(kind) => (
				self.children
					.get(&source)
					.into_iter()
					.flatten()
					.filter(|id| self.symbols[id].kind == *kind)
					.map(|id| Item::Symbol(*id))
					.collect(),
				!self.uncertain_children.contains(&source),
			),
			Domain::ChildrenByShape(shape) => (
				self.children
					.get(&source)
					.into_iter()
					.flatten()
					.filter(|id| {
						code_moniker_core::core::shape::Shape::for_kind(
							self.symbols[id].kind.as_bytes(),
						)
						.as_str() == shape
					})
					.map(|id| Item::Symbol(*id))
					.collect(),
				!self.uncertain_children.contains(&source),
			),
			Domain::OutRefs | Domain::SourceOutRefs => (
				self.outgoing
					.get(&source)
					.into_iter()
					.flatten()
					.map(|r| Item::Reference(r))
					.collect(),
				true,
			),
			Domain::InRefs | Domain::SourceInRefs => self.incoming(source),
			Domain::TargetInRefs | Domain::TargetOutRefs => {
				let Item::Reference(r) = item else {
					return (Vec::new(), false);
				};
				let Some(target) = self.targets.get(&r.id) else {
					return (Vec::new(), false);
				};
				self.domain(
					if matches!(domain, Domain::TargetInRefs) {
						&Domain::InRefs
					} else {
						&Domain::OutRefs
					},
					Item::Symbol(*target),
				)
			}
			_ => (Vec::new(), false),
		}
	}

	fn incoming(&self, symbol: SymbolId) -> (Vec<Item<'a>>, bool) {
		(
			self.incoming
				.get(&symbol)
				.into_iter()
				.flatten()
				.map(|r| Item::Reference(r))
				.collect(),
			!self.uncertain_incoming.contains(&symbol),
		)
	}

	fn scalar(&self, lhs: Lhs, item: Item<'a>) -> Option<Value> {
		let source = self.source(item);
		let (id, attr) = match lhs {
			Lhs::SourceName => (source, Lhs::Name),
			Lhs::SourceKind => (source, Lhs::Kind),
			Lhs::SourceSignature => (source, Lhs::Signature),
			Lhs::SourceShape => (source, Lhs::Shape),
			Lhs::SourceMoniker => (source, Lhs::Moniker),
			Lhs::SourceSrcset => (source, Lhs::Srcset),
			Lhs::SourceVisibility => (source, Lhs::Visibility),
			Lhs::TargetName
			| Lhs::TargetKind
			| Lhs::TargetSignature
			| Lhs::TargetShape
			| Lhs::TargetMoniker
			| Lhs::TargetSrcset
			| Lhs::TargetVisibility => {
				let Item::Reference(r) = item else {
					return None;
				};
				let attr = match lhs {
					Lhs::TargetName => Lhs::Name,
					Lhs::TargetKind => Lhs::Kind,
					Lhs::TargetSignature => Lhs::Signature,
					Lhs::TargetShape => Lhs::Shape,
					Lhs::TargetSrcset => Lhs::Srcset,
					Lhs::TargetVisibility => Lhs::Visibility,
					_ => Lhs::Moniker,
				};
				(*self.targets.get(&r.id)?, attr)
			}
			Lhs::Kind if matches!(item, Item::Reference(_)) => {
				let Item::Reference(r) = item else {
					unreachable!()
				};
				return Some(Value::Str(r.kind.clone()));
			}
			_ => (source, lhs),
		};
		let symbol = self.symbols.get(&id)?;
		Some(match attr {
			Lhs::Srcset => Value::Str(self.index.inventory.record_by_id(&id)?.srcset.to_string()),
			Lhs::Name => Value::Str(symbol.name.clone()),
			Lhs::Kind => Value::Str(symbol.kind.clone()),
			Lhs::Signature => Value::Str(symbol.signature.clone()),
			Lhs::Visibility => Value::Str(symbol.visibility.clone()),
			Lhs::Shape => Value::Str(
				code_moniker_core::core::shape::Shape::for_kind(symbol.kind.as_bytes())
					.as_str()
					.into(),
			),
			Lhs::Moniker => Value::Moniker(
				code_moniker_core::core::uri::from_uri(
					&symbol.identity,
					&code_moniker_core::core::uri::UriConfig {
						scheme: &self.index.identity_scheme,
					},
				)
				.ok()?,
			),
			_ => return None,
		})
	}

	fn number(&self, expr: &NumberExpr, item: Item<'a>, current: Item<'a>) -> Option<f64> {
		match expr {
			NumberExpr::Literal(n) => Some(*n),
			NumberExpr::Size(collection) => {
				Some(self.collection(collection, item, current)?.len() as f64)
			}
			NumberExpr::Count { domain, filter } => {
				let (items, complete) = self.domain(domain, item);
				if !complete {
					return None;
				}
				let mut count = 0;
				for nested in items {
					if match filter {
						Some(filter) => truth(self.node(filter, nested, item))?,
						None => true,
					} {
						count += 1;
					}
				}
				Some(count as f64)
			}
			_ => None,
		}
	}

	fn collection(
		&self,
		expr: &CollectionExpr,
		item: Item<'a>,
		current: Item<'a>,
	) -> Option<Vec<Value>> {
		match expr {
			CollectionExpr::Projection(p) => {
				let (items, complete) =
					self.domain(&p.domain, if p.current { current } else { item });
				if !complete {
					return None;
				}
				let lhs = Lhs::from_projection_name(&p.path.join("."))?;
				items
					.into_iter()
					.map(|item| self.scalar(lhs, item))
					.collect()
			}
			CollectionExpr::Unique(inner) => Some(super::collection::unique(
				self.collection(inner, item, current)?,
			)),
			CollectionExpr::Binary { op, left, right } => {
				let left = self.collection(left, item, current)?;
				let right = self.collection(right, item, current)?;
				Some(match op {
					CollectionOp::Intersect => super::collection::intersect(&left, &right),
					CollectionOp::Union => super::collection::union(&left, &right),
					CollectionOp::Difference => super::collection::difference(&left, &right),
				})
			}
			_ => None,
		}
	}

	// Independent declarations have no intrinsic order across source documents.
	fn comparison_collection(
		&self,
		expr: &CollectionExpr,
		item: Item<'a>,
		current: Item<'a>,
		ordered: bool,
	) -> Option<Vec<Value>> {
		if ordered {
			let CollectionExpr::Projection(p) = expr else {
				return None;
			};
			let (items, complete) = self.domain(&p.domain, if p.current { current } else { item });
			if !complete {
				return None;
			}
			let mut source = None;
			for item in &items {
				let Item::Symbol(id) = item else { return None };
				let symbol = self.symbols[id];
				if items.len() > 1 && symbol.byte_range.is_none() {
					return None;
				}
				if source.is_some_and(|s| s != symbol.source) {
					return None;
				}
				source = Some(symbol.source);
			}
		}
		self.collection(expr, item, current)
	}

	fn atom(&self, atom: &Atom, item: Item<'a>, current: Item<'a>) -> AtomOutcome {
		let result =
			if let (LhsExpr::Collection(left), Rhs::Collection(right)) = (&atom.lhs, &atom.rhs) {
				self.comparison_collection(left, item, current, atom.op == Op::Prefix)
					.zip(self.comparison_collection(right, item, current, atom.op == Op::Prefix))
					.map(|(left, right)| {
						if atom.op == Op::Prefix {
							super::collection::is_prefix(&left, &right)
						} else {
							super::collection::is_subset(&left, &right)
						}
					})
			} else {
				let value = match &atom.lhs {
					LhsExpr::Attr(lhs) => self.scalar(*lhs, item),
					LhsExpr::Number(n) => self.number(n, item, current).map(Value::Number),
					_ => None,
				};
				let Some(value) = value else {
					return AtomOutcome::NotApplicable;
				};
				let rhs = match &atom.rhs {
					Rhs::Projection(lhs) => self.scalar(*lhs, item),
					Rhs::CurrentProjection(lhs) => self.scalar(*lhs, current),
					Rhs::Number(n) => self.number(n, item, current).map(Value::Number),
					_ => return apply_op(&value, atom),
				};
				return rhs.map_or(AtomOutcome::NotApplicable, |rhs| {
					apply_op_values(&value, atom.op, &rhs)
				});
			};
		match result {
			Some(true) => AtomOutcome::Pass,
			Some(false) => AtomOutcome::Fail {
				actual: "collection mismatch".into(),
				expected: format!("{:?}", atom.op),
				position: None,
			},
			None => AtomOutcome::NotApplicable,
		}
	}
}

fn truth(outcome: NodeOutcome) -> Option<bool> {
	match outcome {
		NodeOutcome::Pass => Some(true),
		NodeOutcome::Fail(_) => Some(false),
		NodeOutcome::NotApplicable => None,
	}
}
fn failed() -> NodeOutcome {
	NodeOutcome::Fail(Failure {
		atom_raw: String::new(),
		lhs_label: String::new(),
		actual: String::new(),
		expected: String::new(),
		def_idx: None,
		position: None,
		details: None,
	})
}
