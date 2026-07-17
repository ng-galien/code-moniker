use code_moniker_core::core::code_graph::RefRecord;
use code_moniker_core::core::kinds::{REF_CALLS, REF_METHOD_CALL, REF_READS};
use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::kinds;
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::linkage::binding::{
	ExternalOrigin, ReferenceLinkageDecision, ResolutionDecision, ResolutionScope,
};
use crate::linkage::catalog::CandidateCatalog;
use crate::linkage::catalog::ReferenceLocations;
use crate::linkage::catalog::{SymbolOrdinal, SymbolSet};
use crate::linkage::language;
use crate::linkage::resolve::WorkspacePackageIndex;
use crate::linkage::resolve::python_bindings::PythonBindingGraph;
use crate::linkage::source_groups::{LinkPermission, SourceGroupPolicy};
use crate::snapshot::{RecordTable, ReferenceId, ReferenceRecord, ResolutionEvidence};
use crate::source::CodeIndexMaterial;

pub(in crate::linkage) struct SemanticLinkage<'a> {
	material: &'a CodeIndexMaterial,
	methods: &'a MethodTable,
	candidates: &'a CandidateCatalog,
	locations: &'a ReferenceLocations,
	source_groups: &'a SourceGroupPolicy,
	packages: &'a WorkspacePackageIndex,
}

impl<'a> SemanticLinkage<'a> {
	pub(in crate::linkage) fn new(
		material: &'a CodeIndexMaterial,
		methods: &'a MethodTable,
		candidates: &'a CandidateCatalog,
		locations: &'a ReferenceLocations,
		source_groups: &'a SourceGroupPolicy,
		packages: &'a WorkspacePackageIndex,
	) -> Self {
		Self {
			material,
			methods,
			candidates,
			locations,
			source_groups,
			packages,
		}
	}

	pub(in crate::linkage) fn enhance(
		&self,
		decisions: &mut [ReferenceLinkageDecision],
		references: &RecordTable<ReferenceRecord>,
	) {
		enhance_decisions(self, decisions, references, None);
	}

	pub(in crate::linkage) fn enhance_changed(
		&self,
		decisions: &mut [ReferenceLinkageDecision],
		references: &RecordTable<ReferenceRecord>,
		changed_references: &FxHashSet<ReferenceId>,
	) {
		enhance_decisions(self, decisions, references, Some(changed_references));
	}

	fn semantic_context(&self) -> language::SemanticContext<'a> {
		language::SemanticContext {
			material: self.material,
			candidates: self.candidates,
			locations: self.locations,
			source_groups: self.source_groups,
		}
	}

	fn resolved_method_targets(
		&self,
		owner: &Moniker,
		call_name: &str,
		call_arity: Option<usize>,
	) -> Option<SymbolSet> {
		let target = method_target(owner, call_name, call_arity);
		if let Some(symbol) = self.candidates.indexes().symbol_by_moniker(&target) {
			return Some(SymbolSet::from_symbol(symbol));
		}
		self.methods.resolve_by_name(owner, call_name, call_arity)
	}

	fn resolved_return_owner(
		&self,
		symbol: SymbolOrdinal,
		return_types: &FxHashMap<Moniker, Moniker>,
	) -> Option<Moniker> {
		let callable = self.candidates.candidate(symbol)?.moniker;
		return_types.get(callable).cloned()
	}
}

fn enhance_decisions(
	linkage: &SemanticLinkage<'_>,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	changed_references: Option<&FxHashSet<ReferenceId>>,
) {
	classify_runtime_imports(linkage.material, decisions, references, changed_references);
	let bootstrap = build_receiver_field_tables(linkage, decisions, references);
	let bindings =
		PythonBindingGraph::build(linkage.material, linkage.candidates, decisions, references);
	enhance_python_bindings(
		linkage,
		&bootstrap,
		&bindings,
		decisions,
		references,
		changed_references,
	);
	let tables = build_receiver_field_tables(linkage, decisions, references);
	language::enhance_reference_semantics(
		&linkage.semantic_context(),
		&tables.extends_of,
		decisions,
		references,
		changed_references,
	);
	enhance_receiver_fields(linkage, &tables, decisions, references, changed_references);
	enhance_python_bindings(
		linkage,
		&tables,
		&bindings,
		decisions,
		references,
		changed_references,
	);
	let pending = pending_receiver_chains(decisions, references, changed_references);
	enhance_receiver_chains(linkage, &tables, decisions, references, pending);
}

fn classify_runtime_imports(
	material: &CodeIndexMaterial,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	changed_references: Option<&FxHashSet<ReferenceId>>,
) {
	let mut local_import_candidates = FxHashMap::default();
	let mut module_import_candidates = FxHashMap::default();
	for decision in decisions.iter() {
		let reference = &references[decision.reference_idx()];
		if reference.receiver.as_deref() != Some("python_conditional_import")
			|| !matches!(
				reference.kind.as_bytes(),
				kinds::IMPORTS_MODULE | kinds::IMPORTS_SYMBOL
			) {
			continue;
		}
		let Some(name) = runtime_binding_name(reference) else {
			continue;
		};
		let source_is_module = material
			.symbol_moniker(&reference.source_symbol)
			.and_then(|moniker| moniker.as_view().segments().last())
			.is_some_and(|segment| segment.kind == kinds::MODULE);
		let candidates = if source_is_module {
			module_import_candidates
				.entry((reference.source, name.to_owned()))
				.or_insert_with(SymbolSet::new)
		} else {
			local_import_candidates
				.entry((reference.source_symbol, name.to_owned()))
				.or_insert_with(SymbolSet::new)
		};
		if let Some(targets) = decision.linkage_targets() {
			for target in targets.iter() {
				candidates.insert(target);
			}
		}
	}
	for decision in decisions {
		if changed_references.is_some_and(|changed| !changed.contains(decision.reference())) {
			continue;
		}
		let reference_idx = decision.reference_idx();
		let reference = &references[reference_idx];
		if reference.receiver.as_deref() != Some("python_conditional_import") {
			continue;
		}
		let mut candidates = decision
			.linkage_targets()
			.cloned()
			.unwrap_or_else(SymbolSet::new);
		if let Some(name) = runtime_binding_name(reference) {
			if let Some(imports) =
				local_import_candidates.get(&(reference.source_symbol, name.to_owned()))
			{
				for target in imports.iter() {
					candidates.insert(target);
				}
			}
			if let Some(imports) =
				module_import_candidates.get(&(reference.source, name.to_owned()))
			{
				for target in imports.iter() {
					candidates.insert(target);
				}
			}
		}
		*decision = ReferenceLinkageDecision::dynamic(
			crate::snapshot::DynamicReason::RuntimeImport,
			reference_idx,
			reference.id,
			candidates,
		);
	}
}

fn runtime_binding_name(reference: &ReferenceRecord) -> Option<&str> {
	reference
		.call_name
		.as_deref()
		.or_else(|| reference.alias.as_deref().filter(|alias| !alias.is_empty()))
		.or_else(|| reference.target_identity.rsplit(':').next())
}

fn enhance_receiver_chains(
	linkage: &SemanticLinkage<'_>,
	tables: &ReceiverFieldTables,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	mut pending: Vec<usize>,
) {
	if pending.is_empty() {
		return;
	}
	let receiver_calls = build_receiver_call_index(linkage, decisions, &pending);
	let wanted = receiver_calls
		.by_reference
		.values()
		.copied()
		.collect::<FxHashSet<_>>();
	let mut statuses = reference_statuses(linkage.material, decisions, references, &wanted);
	let return_types =
		collect_return_types(linkage.material, linkage.candidates, decisions, references);
	loop {
		let context = ChainContext {
			statuses: &statuses,
			receiver_calls: &receiver_calls,
			return_types: &return_types,
		};
		let replacements = pending
			.par_iter()
			.filter_map(|idx| {
				let reference_idx = decisions[*idx].semantic_pending_reference_idx()?;
				resolve_receiver_chain(
					linkage,
					tables,
					&context,
					reference_idx,
					&references[reference_idx],
				)
				.map(|replacement| (*idx, replacement))
			})
			.collect::<Vec<_>>();
		if replacements.is_empty() {
			break;
		}
		for (idx, replacement) in replacements {
			if let Some(status) = reference_status(linkage.material, &replacement, references) {
				statuses.insert(replacement.reference_idx(), status);
			}
			decisions[idx] = replacement;
		}
		pending.retain(|idx| decisions[*idx].semantic_pending_reference_idx().is_some());
	}
}

struct ReceiverFieldTables {
	field_types: FxHashMap<Moniker, FxHashMap<Vec<u8>, Moniker>>,
	extends_of: FxHashMap<Moniker, Moniker>,
	supers: FxHashMap<Moniker, Vec<Moniker>>,
	type_aliases: FxHashMap<Moniker, Moniker>,
	value_types: FxHashMap<Moniker, Moniker>,
}

fn build_receiver_field_tables(
	linkage: &SemanticLinkage<'_>,
	decisions: &[ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
) -> ReceiverFieldTables {
	let mut tables = ReceiverFieldTables {
		field_types: FxHashMap::default(),
		extends_of: FxHashMap::default(),
		supers: FxHashMap::default(),
		type_aliases: FxHashMap::default(),
		value_types: FxHashMap::default(),
	};
	for decision in decisions {
		let reference = decision_reference(decision, references);
		let table_kind = reference.kind.as_bytes();
		if !is_type_level_kind(table_kind) {
			continue;
		}
		let Some(target) =
			decision_target(linkage.material, linkage.candidates, decision, references)
				.or_else(|| linkage.material.reference_target(&reference.id).cloned())
		else {
			continue;
		};
		if let Some(raw) = linkage.material.reference_target(&reference.id)
			&& raw != &target
		{
			tables.type_aliases.insert(raw.clone(), target.clone());
		}
		let Some(source) = linkage.material.symbol_moniker(&reference.source_symbol) else {
			continue;
		};
		insert_type_fact(&mut tables, table_kind, source.clone(), target);
	}
	tables
}

fn insert_type_fact(
	tables: &mut ReceiverFieldTables,
	kind: &[u8],
	source: Moniker,
	target: Moniker,
) {
	match kind {
		kinds::EXTENDS => {
			tables.extends_of.insert(source.clone(), target.clone());
			tables.supers.entry(source).or_default().push(target);
		}
		kinds::IMPLEMENTS => {
			tables.supers.entry(source).or_default().push(target);
		}
		kinds::TYPED_AS => {
			if let Some((owner, name)) = field_owner_and_name(&source) {
				tables
					.field_types
					.entry(owner)
					.or_default()
					.insert(name, target);
			} else if source
				.as_view()
				.segments()
				.last()
				.is_some_and(|segment| segment.kind == kinds::PATH)
			{
				tables.value_types.insert(source, target);
			}
		}
		_ => {}
	}
}

fn is_type_level_kind(kind: &[u8]) -> bool {
	matches!(
		kind,
		kinds::TYPED_AS
			| kinds::EXTENDS
			| kinds::IMPLEMENTS
			| kinds::USES_TYPE
			| kinds::IMPORTS_SYMBOL
			| kinds::IMPORTS_MODULE
			| kinds::INSTANTIATES
	)
}

fn field_owner_and_name(field: &Moniker) -> Option<(Moniker, Vec<u8>)> {
	let last = field.as_view().segments().last()?;
	if last.kind != kinds::FIELD {
		return None;
	}
	Some((field.parent()?, last.name.to_vec()))
}

fn enhance_receiver_fields(
	linkage: &SemanticLinkage<'_>,
	tables: &ReceiverFieldTables,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	changed_references: Option<&FxHashSet<ReferenceId>>,
) {
	let replacements = decisions
		.par_iter()
		.enumerate()
		.filter_map(|(idx, decision)| {
			if changed_references.is_some_and(|changed| !changed.contains(decision.reference())) {
				return None;
			}
			let reference_idx = decision.semantic_pending_reference_idx()?;
			let reference = &references[reference_idx];
			resolve_receiver_field_call(linkage, tables, reference_idx, reference)
				.or_else(|| resolve_imported_method_call(linkage, tables, reference_idx, reference))
				.or_else(|| resolve_self_method_call(linkage, tables, reference_idx, reference))
				.or_else(|| resolve_typed_value_call(linkage, tables, reference_idx, reference))
				.or_else(|| {
					resolve_typed_value_annotation(linkage, tables, reference_idx, reference)
				})
				.map(|replacement| (idx, replacement))
		})
		.collect::<Vec<_>>();
	for (idx, replacement) in replacements {
		decisions[idx] = replacement;
	}
}

fn resolve_receiver_field_call(
	linkage: &SemanticLinkage<'_>,
	tables: &ReceiverFieldTables,
	reference_idx: usize,
	reference: &ReferenceRecord,
) -> Option<ReferenceLinkageDecision> {
	let method_call = MethodCallReference::new(reference_idx, reference)?;
	let receiver = reference
		.receiver
		.as_deref()
		.filter(|name| !name.is_empty())?;
	let source = linkage.material.symbol_moniker(&reference.source_symbol)?;
	let mut owner = Some(source.clone());
	while let Some(current) = owner {
		if let Some(ty) = field_type_through_extends(tables, &current, receiver.as_bytes()) {
			return typed_receiver_decision(linkage, tables, ty, method_call);
		}
		if let Some(ty) = receiver_value_type(tables, &current, receiver.as_bytes()) {
			return typed_receiver_decision(linkage, tables, ty, method_call);
		}
		owner = current.parent();
	}
	None
}

fn receiver_value_type<'a>(
	tables: &'a ReceiverFieldTables,
	owner: &Moniker,
	name: &[u8],
) -> Option<&'a Moniker> {
	let value = MonikerBuilder::from_view(owner.as_view())
		.segment(kinds::PATH, name)
		.build();
	tables.value_types.get(&value)
}

fn field_type_through_extends<'a>(
	tables: &'a ReceiverFieldTables,
	class: &Moniker,
	name: &[u8],
) -> Option<&'a Moniker> {
	let mut current = class;
	let mut seen = FxHashSet::default();
	for _ in 0..16 {
		if let Some(ty) = tables
			.field_types
			.get(current)
			.and_then(|fields| fields.get(name))
		{
			return Some(ty);
		}
		let next = tables.extends_of.get(current)?;
		if !seen.insert(next) {
			return None;
		}
		current = next;
	}
	None
}

fn typed_receiver_decision(
	linkage: &SemanticLinkage<'_>,
	tables: &ReceiverFieldTables,
	ty: &Moniker,
	method_call: MethodCallReference<'_>,
) -> Option<ReferenceLinkageDecision> {
	let owner = callable_owner(ty)?;
	resolve_method_through_supers(linkage, tables, &owner, method_call)
}

fn resolve_imported_method_call(
	linkage: &SemanticLinkage<'_>,
	tables: &ReceiverFieldTables,
	reference_idx: usize,
	reference: &ReferenceRecord,
) -> Option<ReferenceLinkageDecision> {
	let method_call = MethodCallReference::new(reference_idx, reference)?;
	let raw_target = linkage.material.reference_target(&reference.id)?;
	let last = raw_target.as_view().segments().last()?;
	if !matches!(last.kind, kinds::METHOD | kinds::CONSTRUCTOR) {
		return None;
	}
	let owner_raw = raw_target.parent()?;
	let owner = canonical_type_owner(tables, &owner_raw);
	resolve_method_through_supers(linkage, tables, &owner, method_call)
}

fn resolve_typed_value_call(
	linkage: &SemanticLinkage<'_>,
	tables: &ReceiverFieldTables,
	reference_idx: usize,
	reference: &ReferenceRecord,
) -> Option<ReferenceLinkageDecision> {
	let method_call = MethodCallReference::new(reference_idx, reference)?;
	let raw_target = linkage.material.reference_target(&reference.id)?;
	let last = raw_target.as_view().segments().last()?;
	if !matches!(last.kind, kinds::FUNCTION | kinds::METHOD) {
		return None;
	}
	let value = raw_target.parent()?;
	if value.as_view().segments().last()?.kind != kinds::PATH {
		return None;
	}
	let value = tables.type_aliases.get(&value).cloned().unwrap_or(value);
	let ty = tables.value_types.get(&value)?;
	typed_receiver_decision(linkage, tables, ty, method_call)
}

fn resolve_typed_value_annotation(
	linkage: &SemanticLinkage<'_>,
	tables: &ReceiverFieldTables,
	reference_idx: usize,
	reference: &ReferenceRecord,
) -> Option<ReferenceLinkageDecision> {
	if reference.kind != "annotates" {
		return None;
	}
	let raw_target = linkage.material.reference_target(&reference.id)?;
	let segments = raw_target.as_view().segments().collect::<Vec<_>>();
	let [.., value_segment, last] = segments.as_slice() else {
		return None;
	};
	if !matches!(last.kind, kinds::FUNCTION | kinds::METHOD) || value_segment.kind != kinds::PATH {
		return None;
	}
	let call_name = std::str::from_utf8(bare_callable_name(last.name)).ok()?;
	let value = raw_target.parent()?;
	let value = tables.type_aliases.get(&value).cloned().unwrap_or(value);
	let ty = tables.value_types.get(&value)?;
	let owner = callable_owner(ty)?;
	let method_call = MethodCallReference {
		reference_idx,
		reference,
		call_name,
	};
	resolve_method_through_supers(linkage, tables, &owner, method_call)
}

fn resolve_self_method_call(
	linkage: &SemanticLinkage<'_>,
	tables: &ReceiverFieldTables,
	reference_idx: usize,
	reference: &ReferenceRecord,
) -> Option<ReferenceLinkageDecision> {
	let method_call = MethodCallReference::new(reference_idx, reference)?;
	if !matches!(reference.receiver.as_deref(), Some("self") | Some("cls")) {
		return None;
	}
	let source = linkage.material.symbol_moniker(&reference.source_symbol)?;
	let owner = enclosing_class(source)?;
	resolve_method_through_supers(linkage, tables, &owner, method_call)
}

fn enclosing_class(source: &Moniker) -> Option<Moniker> {
	let mut current = source.parent();
	while let Some(owner) = current {
		if owner.as_view().segments().last()?.kind == kinds::CLASS {
			return Some(owner);
		}
		current = owner.parent();
	}
	None
}

fn canonical_type_owner(tables: &ReceiverFieldTables, owner: &Moniker) -> Moniker {
	if let Some(alias) = tables.type_aliases.get(owner) {
		return alias.clone();
	}
	let Some(stripped) = strip_self_path_echo(owner) else {
		return owner.clone();
	};
	tables
		.type_aliases
		.get(&stripped)
		.cloned()
		.unwrap_or(stripped)
}

fn strip_self_path_echo(owner: &Moniker) -> Option<Moniker> {
	let segments = owner.as_view().segments().collect::<Vec<_>>();
	let [.., before, last] = segments.as_slice() else {
		return None;
	};
	(last.kind == kinds::PATH && last.name == before.name).then(|| owner.parent())?
}

fn resolve_method_through_supers(
	linkage: &SemanticLinkage<'_>,
	tables: &ReceiverFieldTables,
	owner: &Moniker,
	method_call: MethodCallReference<'_>,
) -> Option<ReferenceLinkageDecision> {
	let mut stack = vec![owner.clone()];
	let mut seen = FxHashSet::default();
	while let Some(current) = stack.pop() {
		if seen.len() > 32 || !seen.insert(current.clone()) {
			continue;
		}
		if let Some(targets) = linkage.resolved_method_targets(
			&current,
			method_call.call_name(),
			method_call.call_arity(),
		) {
			let decision = method_call.resolved_decision(ResolutionScope::Global, targets);
			return declared_groups_permit_decision(linkage, &decision).then_some(decision);
		}
		if external_target_shape(&current) || linkage.packages.is_foreign_moniker(&current) {
			let target = method_target(&current, method_call.call_name(), method_call.call_arity());
			return Some(method_call.external_decision(target));
		}
		if let Some(parents) = tables.supers.get(&current) {
			for parent in parents {
				let parent = tables.type_aliases.get(parent).unwrap_or(parent);
				stack.push(parent.clone());
			}
		}
	}
	None
}

fn declared_groups_permit_decision(
	linkage: &SemanticLinkage<'_>,
	decision: &ReferenceLinkageDecision,
) -> bool {
	let Some(targets) = decision.linkage_targets() else {
		return true;
	};
	let Some(location) = linkage.locations.get(decision.reference_idx()) else {
		return true;
	};
	targets.iter().all(|symbol| {
		linkage
			.candidates
			.candidate(symbol)
			.is_none_or(|candidate| {
				linkage.source_groups.link_permission(
					linkage.material,
					location.source_file,
					candidate.source_file,
				) != Some(LinkPermission::Blocked)
			})
	})
}

struct MethodCallReference<'a> {
	reference_idx: usize,
	reference: &'a ReferenceRecord,
	call_name: &'a str,
}

impl<'a> MethodCallReference<'a> {
	fn new(reference_idx: usize, reference: &'a ReferenceRecord) -> Option<Self> {
		if reference.kind != "method_call" && reference.kind != "calls" {
			return None;
		}
		Some(Self {
			reference_idx,
			reference,
			call_name: reference.call_name.as_deref()?,
		})
	}

	fn call_name(&self) -> &str {
		self.call_name
	}

	fn call_arity(&self) -> Option<usize> {
		self.reference.call_arity
	}

	fn external_decision(&self, target: Moniker) -> ReferenceLinkageDecision {
		ReferenceLinkageDecision::external_target(
			ExternalOrigin::Dependency,
			self.reference_idx,
			self.reference.id,
			target,
		)
	}

	fn resolved_decision(
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
struct ReceiverCallIndex {
	by_reference: FxHashMap<usize, usize>,
}

impl ReceiverCallIndex {
	fn get(&self, reference_idx: usize) -> Option<usize> {
		self.by_reference.get(&reference_idx).copied()
	}
}

type MethodKey = (Moniker, Vec<u8>, usize);

#[derive(Default)]
pub(in crate::linkage) struct MethodTable {
	by_owner_name_arity: FxHashMap<MethodKey, Vec<SymbolOrdinal>>,
	by_owner_name: FxHashMap<(Moniker, Vec<u8>), Vec<SymbolOrdinal>>,
	keys_by_file: FxHashMap<usize, Vec<MethodKey>>,
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
			let key = (owner, def.call_name.to_vec(), arity);
			insert_method_key(self, file_idx, key, symbol);
		}
	}

	fn resolve_by_name(
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
}

fn insert_method_key(
	table: &mut MethodTable,
	file_idx: usize,
	key: MethodKey,
	symbol: SymbolOrdinal,
) {
	table
		.by_owner_name
		.entry((key.0.clone(), key.1.clone()))
		.or_default()
		.push(symbol);
	table
		.by_owner_name_arity
		.entry(key.clone())
		.or_default()
		.push(symbol);
	table.keys_by_file.entry(file_idx).or_default().push(key);
}

fn enhance_python_bindings(
	linkage: &SemanticLinkage<'_>,
	tables: &ReceiverFieldTables,
	bindings: &PythonBindingGraph,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	changed_references: Option<&FxHashSet<ReferenceId>>,
) {
	for decision in decisions.iter_mut() {
		let Some(reference_idx) = decision.semantic_pending_reference_idx() else {
			continue;
		};
		if changed_references.is_some_and(|changed| !changed.contains(decision.reference())) {
			continue;
		}
		let Some((owner, name)) =
			PythonBindingGraph::target_key(linkage.material, &references[reference_idx])
		else {
			continue;
		};
		let raw_owner = owner;
		let owner = tables
			.type_aliases
			.get(&raw_owner)
			.cloned()
			.unwrap_or_else(|| raw_owner.clone());
		let reference = &references[reference_idx];
		let requested_target = linkage.material.reference_target(&reference.id);
		if let Some(resolved) = bindings.decision(
			&raw_owner,
			&name,
			reference_idx,
			reference,
			requested_target,
		) {
			*decision = resolved;
			continue;
		}
		if owner != raw_owner
			&& let Some(resolved) =
				bindings.decision(&owner, &name, reference_idx, reference, requested_target)
		{
			*decision = resolved;
			continue;
		}
		let Some(bound_owner) = bindings.canonical_workspace_owner(&owner, linkage.candidates)
		else {
			continue;
		};
		if let Some(resolved) = bindings.decision(
			&bound_owner,
			&name,
			reference_idx,
			reference,
			requested_target,
		) {
			*decision = resolved;
			continue;
		}
		let Some(method_call) = MethodCallReference::new(reference_idx, reference) else {
			continue;
		};
		let Some(resolved) =
			resolve_method_through_supers(linkage, tables, &bound_owner, method_call)
		else {
			continue;
		};
		*decision = resolved;
	}
}

fn build_receiver_call_index(
	linkage: &SemanticLinkage<'_>,
	decisions: &[ReferenceLinkageDecision],
	pending: &[usize],
) -> ReceiverCallIndex {
	let mut pending_by_file = FxHashMap::<usize, Vec<(usize, usize)>>::default();
	for idx in pending {
		let Some(reference_idx) = decisions[*idx].semantic_pending_reference_idx() else {
			continue;
		};
		let Some(location) = linkage.locations.get(reference_idx) else {
			continue;
		};
		pending_by_file
			.entry(location.source_file)
			.or_insert_with(Vec::new)
			.push((reference_idx, location.reference));
	}

	let mut index = ReceiverCallIndex::default();
	for (file_idx, pending_refs) in pending_by_file {
		index_file_receiver_calls(linkage, file_idx, &pending_refs, &mut index);
	}
	index
}

fn index_file_receiver_calls(
	linkage: &SemanticLinkage<'_>,
	file_idx: usize,
	pending_refs: &[(usize, usize)],
	index: &mut ReceiverCallIndex,
) {
	let Some(file) = linkage.material.files.get(file_idx) else {
		return;
	};
	let calls_by_source = sorted_call_spans_by_source(file);
	for (reference_idx, ref_idx) in pending_refs {
		let current = file.graph.ref_at(*ref_idx);
		let Some(calls) = calls_by_source.get(current.source) else {
			continue;
		};
		let Some(receiver_idx) = immediate_receiver_call_idx(file, *ref_idx, calls)
			.or_else(|| immediate_receiver_read_idx(file, *ref_idx))
		else {
			continue;
		};
		let Some(receiver_reference_idx) = linkage.locations.reference_idx(file_idx, receiver_idx)
		else {
			continue;
		};
		index
			.by_reference
			.insert(*reference_idx, receiver_reference_idx);
	}
}

#[derive(Clone, Copy)]
struct CallSpan {
	ref_idx: usize,
	start: u32,
	end: u32,
	width: u32,
}

fn sorted_call_spans_by_source(file: &crate::source::IndexedSourceFile) -> Vec<Vec<CallSpan>> {
	let mut by_source = vec![Vec::new(); file.graph.def_count()];
	for ref_idx in 0..file.graph.ref_count() {
		let reference = file.graph.ref_at(ref_idx);
		if !is_call_ref(reference) {
			continue;
		}
		let Some((start, end)) = reference.position else {
			continue;
		};
		let Some(source_calls) = by_source.get_mut(reference.source) else {
			continue;
		};
		source_calls.push(CallSpan {
			ref_idx,
			start,
			end,
			width: end.saturating_sub(start),
		});
	}
	for source_calls in &mut by_source {
		source_calls.sort_by_key(|call| std::cmp::Reverse(call.width));
	}
	by_source
}

fn immediate_receiver_call_idx(
	file: &crate::source::IndexedSourceFile,
	ref_idx: usize,
	calls: &[CallSpan],
) -> Option<usize> {
	let current = file.graph.ref_at(ref_idx);
	let current_position = current.position?;
	calls
		.iter()
		.find(|candidate| {
			candidate.ref_idx != ref_idx
				&& contains_position(current_position, (candidate.start, candidate.end))
		})
		.map(|candidate| candidate.ref_idx)
}

fn immediate_receiver_read_idx(
	file: &crate::source::IndexedSourceFile,
	ref_idx: usize,
) -> Option<usize> {
	let current = file.graph.ref_at(ref_idx);
	let current_position = current.position?;
	let receiver_hint = current.receiver_hint.as_ref();
	if receiver_hint.is_empty() {
		return None;
	}
	(0..file.graph.ref_count())
		.filter(|&idx| idx != ref_idx)
		.find(|&idx| {
			let candidate = file.graph.ref_at(idx);
			candidate.source == current.source
				&& candidate.kind.as_ref() == REF_READS
				&& candidate
					.position
					.is_some_and(|pos| contains_position(current_position, pos))
				&& candidate
					.target
					.as_view()
					.segments()
					.last()
					.is_some_and(|seg| seg.name == receiver_hint)
		})
}

fn pending_receiver_chains(
	decisions: &[ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	changed_references: Option<&FxHashSet<ReferenceId>>,
) -> Vec<usize> {
	decisions
		.iter()
		.enumerate()
		.filter_map(|(idx, decision)| {
			if changed_references.is_some_and(|changed| !changed.contains(decision.reference())) {
				return None;
			}
			let reference_idx = decision.semantic_pending_reference_idx()?;
			MethodCallReference::new(reference_idx, &references[reference_idx]).map(|_| idx)
		})
		.collect()
}

struct ChainContext<'a> {
	statuses: &'a FxHashMap<usize, ReferenceStatus>,
	receiver_calls: &'a ReceiverCallIndex,
	return_types: &'a FxHashMap<Moniker, Moniker>,
}

fn resolve_receiver_chain(
	linkage: &SemanticLinkage<'_>,
	tables: &ReceiverFieldTables,
	context: &ChainContext<'_>,
	reference_idx: usize,
	reference: &ReferenceRecord,
) -> Option<ReferenceLinkageDecision> {
	let method_call = MethodCallReference::new(reference_idx, reference)?;
	let receiver = context.receiver_calls.get(reference_idx)?;
	let owner = match context.statuses.get(&receiver)? {
		ReferenceStatus::Resolved(symbol) => {
			linkage.resolved_return_owner(*symbol, context.return_types)?
		}
		ReferenceStatus::External(target) => {
			let owner = callable_owner(target)?;
			let target = method_target(&owner, method_call.call_name(), method_call.call_arity());
			return Some(method_call.external_decision(target));
		}
	};
	let owner = tables.type_aliases.get(&owner).cloned().unwrap_or(owner);
	resolve_method_through_supers(linkage, tables, &owner, method_call)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ReferenceStatus {
	Resolved(SymbolOrdinal),
	External(Moniker),
}

fn collect_return_types(
	material: &CodeIndexMaterial,
	candidates: &CandidateCatalog,
	decisions: &[ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
) -> FxHashMap<Moniker, Moniker> {
	let mut out = FxHashMap::default();
	let mut ambiguous = FxHashSet::default();
	for decision in decisions {
		let reference = decision_reference(decision, references);
		if reference.kind != "returns_type" {
			continue;
		}
		let Some(source) = material.symbol_moniker(&reference.source_symbol) else {
			continue;
		};
		let Some(target) = decision_target(material, candidates, decision, references) else {
			continue;
		};
		record_return_type(&mut out, &mut ambiguous, source, target);
	}
	out
}

fn record_return_type(
	out: &mut FxHashMap<Moniker, Moniker>,
	ambiguous: &mut FxHashSet<Moniker>,
	source: &Moniker,
	target: Moniker,
) {
	if ambiguous.contains(source) {
		return;
	}
	if out.get(source).is_some_and(|known| known != &target) {
		out.remove(source);
		ambiguous.insert(source.clone());
		return;
	}
	out.insert(source.clone(), target);
}

fn decision_reference<'a>(
	decision: &ReferenceLinkageDecision,
	references: &'a RecordTable<ReferenceRecord>,
) -> &'a ReferenceRecord {
	&references[decision.reference_idx()]
}

fn decision_target(
	material: &CodeIndexMaterial,
	candidates: &CandidateCatalog,
	decision: &ReferenceLinkageDecision,
	references: &RecordTable<ReferenceRecord>,
) -> Option<Moniker> {
	match decision {
		ReferenceLinkageDecision::Unique { resolution } if resolution.targets.len() == 1 => {
			candidates
				.candidate(resolution.targets.single()?)
				.map(|candidate| candidate.moniker.clone())
		}
		ReferenceLinkageDecision::External {
			reference_idx,
			target,
			..
		} => target.clone().or_else(|| {
			material
				.reference_target(&references[*reference_idx].id)
				.cloned()
		}),
		_ => None,
	}
}

fn reference_statuses(
	material: &CodeIndexMaterial,
	decisions: &[ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	wanted: &FxHashSet<usize>,
) -> FxHashMap<usize, ReferenceStatus> {
	let mut out = FxHashMap::default();
	for decision in decisions {
		let reference_idx = decision.reference_idx();
		if !wanted.contains(&reference_idx) {
			continue;
		}
		if let Some(status) = reference_status(material, decision, references) {
			out.insert(reference_idx, status);
		}
	}
	out
}

fn reference_status(
	material: &CodeIndexMaterial,
	decision: &ReferenceLinkageDecision,
	references: &RecordTable<ReferenceRecord>,
) -> Option<ReferenceStatus> {
	match decision {
		ReferenceLinkageDecision::Unique { resolution } => {
			resolution.targets.single().map(ReferenceStatus::Resolved)
		}
		ReferenceLinkageDecision::External {
			reference_idx,
			target,
			..
		} => target
			.as_ref()
			.or_else(|| material.reference_target(&references[*reference_idx].id))
			.map(|target| ReferenceStatus::External(target.clone())),
		_ => None,
	}
}

fn is_call_ref(reference: &RefRecord) -> bool {
	reference.kind == REF_CALLS || reference.kind == REF_METHOD_CALL
}

fn contains_position(outer: (u32, u32), inner: (u32, u32)) -> bool {
	outer.0 <= inner.0 && inner.1 <= outer.1 && outer != inner
}

fn method_target(owner: &Moniker, call_name: &str, call_arity: Option<usize>) -> Moniker {
	let arity = call_arity.unwrap_or_default();
	let mut segment = Vec::with_capacity(call_name.len() + 2 + arity.saturating_mul(2));
	segment.extend_from_slice(call_name.as_bytes());
	segment.push(b'(');
	for idx in 0..arity {
		if idx > 0 {
			segment.push(b',');
		}
		segment.push(b'_');
	}
	segment.push(b')');
	MonikerBuilder::from_view(owner.as_view())
		.segment(kinds::METHOD, &segment)
		.build()
}

fn callable_owner(target: &Moniker) -> Option<Moniker> {
	let Some(last) = target.as_view().segments().last() else {
		return Some(target.clone());
	};
	if matches!(last.kind, kinds::METHOD | kinds::CONSTRUCTOR) {
		return target.parent();
	}
	Some(target.clone())
}

fn external_target_shape(target: &Moniker) -> bool {
	target
		.as_view()
		.segments()
		.any(|segment| segment.kind == kinds::EXTERNAL_PKG)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn conflicting_return_types_remain_ambiguous() {
		let source = MonikerBuilder::new()
			.project(b"app")
			.segment(b"function", b"make()")
			.build();
		let first = MonikerBuilder::new()
			.project(b"app")
			.segment(b"class", b"First")
			.build();
		let second = MonikerBuilder::new()
			.project(b"app")
			.segment(b"class", b"Second")
			.build();
		let mut out = FxHashMap::default();
		let mut ambiguous = FxHashSet::default();

		record_return_type(&mut out, &mut ambiguous, &source, first);
		record_return_type(&mut out, &mut ambiguous, &source, second);

		assert!(!out.contains_key(&source));
		assert!(ambiguous.contains(&source));
	}
}
