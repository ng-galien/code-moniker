use super::*;

#[derive(Clone, Copy)]
pub(in crate::linkage) struct MethodCallReference<'a> {
	pub(super) reference_idx: usize,
	pub(super) reference: &'a ReferenceRecord,
	pub(super) call_name: &'a str,
}

impl<'a> MethodCallReference<'a> {
	pub(in crate::linkage) fn new(
		reference_idx: usize,
		reference: &'a ReferenceRecord,
	) -> Option<Self> {
		if reference.kind != "method_call" && reference.kind != "calls" {
			return None;
		}
		Some(Self {
			reference_idx,
			reference,
			call_name: reference.call_name.as_deref()?,
		})
	}

	pub(super) fn call_name(&self) -> &str {
		self.call_name
	}

	pub(super) fn call_arity(&self) -> Option<usize> {
		self.reference.call_arity
	}

	pub(super) fn external_decision_with_origin(
		&self,
		origin: ExternalOrigin,
		target: Moniker,
	) -> ReferenceLinkageDecision {
		ReferenceLinkageDecision::external_target(
			origin,
			self.reference_idx,
			self.reference.id,
			target,
		)
	}

	pub(super) fn resolved_decision(
		&self,
		scope: ResolutionScope,
		targets: SymbolSet,
	) -> ReferenceLinkageDecision {
		ReferenceLinkageDecision::resolved(ResolutionDecision::new(
			scope,
			ResolutionEvidence::TypeConstraint,
			self.reference.id,
			self.reference_idx,
			targets,
		))
	}
}

#[derive(Default)]
pub(super) struct ReceiverCallIndex {
	pub(super) by_reference: FxHashMap<usize, usize>,
}

impl ReceiverCallIndex {
	pub(super) fn get(&self, reference_idx: usize) -> Option<usize> {
		self.by_reference.get(&reference_idx).copied()
	}
}

type MethodKey = (Moniker, Vec<u8>, usize);

#[derive(Default)]
pub(in crate::linkage) struct MethodTable {
	by_owner_name_arity: FxHashMap<MethodKey, Vec<SymbolOrdinal>>,
	by_owner_name: FxHashMap<(Moniker, Vec<u8>), Vec<SymbolOrdinal>>,
	owners_by_name_arity: FxHashMap<(Vec<u8>, usize), SymbolSet>,
}

impl MethodTable {
	pub(in crate::linkage) fn build(
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog,
	) -> Self {
		let mut index = Self::default();
		for file_idx in 0..material.files.len() {
			index.insert_file(material, candidates, file_idx);
		}
		index
	}

	fn insert_file(
		&mut self,
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog,
		file_idx: usize,
	) {
		let Some(file) = material.files.get(file_idx) else {
			return;
		};
		for (def_idx, def) in file.graph.defs().enumerate() {
			let Some(arity) = def.call_arity else {
				continue;
			};
			if def.call_name.is_empty() {
				continue;
			}
			let Some(parent_idx) = def.parent else {
				continue;
			};
			let owner = file.graph.def_at(parent_idx).moniker.clone();
			let Some(symbol) = candidates.symbol_at(file_idx, def_idx) else {
				continue;
			};
			let owner_symbol = candidates.indexes().symbol_by_moniker(&owner);
			let key = (owner, def.call_name.to_vec(), arity);
			insert_method_key(self, key, symbol, owner_symbol);
		}
	}

	pub(super) fn resolve_by_name(
		&self,
		owner: &Moniker,
		call_name: &str,
		call_arity: Option<usize>,
	) -> Option<SymbolSet> {
		let targets = match call_arity {
			Some(arity) => self.by_owner_name_arity.get(&(
				owner.clone(),
				call_name.as_bytes().to_vec(),
				arity,
			))?,
			None => self
				.by_owner_name
				.get(&(owner.clone(), call_name.as_bytes().to_vec()))?,
		};
		(targets.len() == 1).then(|| SymbolSet::from_symbol(targets[0]))
	}

	pub(super) fn structural_owners(
		&self,
		call_name: &str,
		call_arity: usize,
	) -> Option<&SymbolSet> {
		self.owners_by_name_arity
			.get(&(call_name.as_bytes().to_vec(), call_arity))
	}

	pub(super) fn methods_for_owners(
		&self,
		candidates: &CandidateCatalog,
		owners: &SymbolSet,
		call_name: &str,
		call_arity: usize,
	) -> SymbolSet {
		let mut methods = SymbolSet::new();
		for owner in owners.iter() {
			let Some(owner) = candidates
				.candidate(owner)
				.map(|candidate| candidate.moniker)
			else {
				continue;
			};
			if let Some(targets) = self.by_owner_name_arity.get(&(
				owner.clone(),
				call_name.as_bytes().to_vec(),
				call_arity,
			)) {
				for target in targets {
					methods.insert(*target);
				}
			}
		}
		methods
	}
}

fn insert_method_key(
	table: &mut MethodTable,
	key: MethodKey,
	symbol: SymbolOrdinal,
	owner_symbol: Option<SymbolOrdinal>,
) {
	let (owner, name, arity) = key;
	if let Some(owner) = owner_symbol {
		table
			.owners_by_name_arity
			.entry((name.clone(), arity))
			.or_default()
			.insert(owner);
	}
	table
		.by_owner_name
		.entry((owner.clone(), name.clone()))
		.or_default()
		.push(symbol);
	table
		.by_owner_name_arity
		.entry((owner, name, arity))
		.or_default()
		.push(symbol);
}
