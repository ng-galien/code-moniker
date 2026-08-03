use super::*;

pub(in crate::linkage) struct ReceiverFieldTables {
	field_types: FxHashMap<Moniker, FxHashMap<Vec<u8>, Moniker>>,
	extends_of: FxHashMap<Moniker, Moniker>,
	supers: FxHashMap<Moniker, Vec<Moniker>>,
	pub(in crate::linkage) type_aliases: FxHashMap<Moniker, Moniker>,
	value_types: FxHashMap<Moniker, MonikerTypeSet>,
	pub(super) invariant_external_origins: FxHashMap<Moniker, ExternalOrigin>,
}

impl ReceiverFieldTables {
	fn record_alias(&mut self, raw: &Moniker, target: &Moniker, origin: Option<ExternalOrigin>) {
		self.type_aliases.insert(raw.clone(), target.clone());
		self.record_external_origin(raw, origin);
	}

	fn record_external_origin(&mut self, target: &Moniker, origin: Option<ExternalOrigin>) {
		if let Some(origin @ (ExternalOrigin::Sdk | ExternalOrigin::Injected)) = origin {
			self.invariant_external_origins
				.insert(target.clone(), origin);
		}
	}
}

#[derive(Default)]
pub(super) struct MonikerTypeSet {
	types: Vec<Moniker>,
	open: bool,
}

impl MonikerTypeSet {
	pub(super) fn insert(&mut self, target: Moniker) {
		if !self.types.contains(&target) {
			self.types.push(target);
		}
	}

	pub(super) fn iter(&self) -> impl Iterator<Item = &Moniker> {
		self.types.iter()
	}

	pub(super) fn mark_open(&mut self) {
		self.open = true;
	}
}

pub(in crate::linkage) fn build_receiver_field_tables(
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
		invariant_external_origins: FxHashMap::default(),
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
		let external_origin = match decision {
			ReferenceLinkageDecision::External { origin, .. } => Some(*origin),
			_ => None,
		};
		tables.record_external_origin(&target, external_origin);
		if let Some(raw) = linkage.material.reference_target(&reference.id)
			&& raw != &target
		{
			tables.record_alias(raw, &target, external_origin);
		}
		let Some(source) = linkage.material.symbol_moniker(&reference.source_symbol) else {
			continue;
		};
		insert_type_fact(&mut tables, reference, source.clone(), target);
	}
	tables
}

fn insert_type_fact(
	tables: &mut ReceiverFieldTables,
	reference: &ReferenceRecord,
	source: Moniker,
	target: Moniker,
) {
	match reference.kind.as_bytes() {
		kinds::EXTENDS => {
			tables.extends_of.insert(source.clone(), target.clone());
			tables.supers.entry(source).or_default().push(target);
		}
		kinds::IMPLEMENTS => {
			tables.supers.entry(source).or_default().push(target);
		}
		kinds::TYPED_AS => {
			if let Some(name) = reference.alias.as_deref().filter(|name| !name.is_empty()) {
				let value = MonikerBuilder::from_view(source.as_view())
					.segment(kinds::PATH, name.as_bytes())
					.build();
				let types = tables.value_types.entry(value).or_default();
				types.insert(target);
				if reference.receiver.as_deref() == Some("python_open_type_set") {
					types.mark_open();
				}
			} else if let Some((owner, name)) = field_owner_and_name(&source) {
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
				let types = tables.value_types.entry(source).or_default();
				types.insert(target);
				if reference.receiver.as_deref() == Some("python_open_type_set") {
					types.mark_open();
				}
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

pub(in crate::linkage) fn enhance_receiver_fields(
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
			let reference_idx = decision.semantic_type_refinable_reference_idx()?;
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
		if let Some(types) = receiver_value_type(tables, &current, receiver.as_bytes()) {
			return typed_receiver_types_decision(linkage, tables, types, method_call);
		}
		owner = current.parent();
	}
	None
}

fn receiver_value_type<'a>(
	tables: &'a ReceiverFieldTables,
	owner: &Moniker,
	name: &[u8],
) -> Option<&'a MonikerTypeSet> {
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

pub(super) fn typed_receiver_decision(
	linkage: &SemanticLinkage<'_>,
	tables: &ReceiverFieldTables,
	ty: &Moniker,
	method_call: MethodCallReference<'_>,
) -> Option<ReferenceLinkageDecision> {
	let ty = tables.type_aliases.get(ty).unwrap_or(ty);
	let owner = callable_owner(ty)?;
	resolve_method_through_supers(linkage, tables, &owner, method_call)
}

pub(super) fn typed_receiver_types_decision(
	linkage: &SemanticLinkage<'_>,
	tables: &ReceiverFieldTables,
	types: &MonikerTypeSet,
	method_call: MethodCallReference<'_>,
) -> Option<ReferenceLinkageDecision> {
	let mut targets = SymbolSet::new();
	let mut external_target = None;
	let mut open = types.open;
	for ty in types.iter() {
		let Some(decision) = typed_receiver_decision(linkage, tables, ty, method_call) else {
			open = true;
			continue;
		};
		match decision {
			ReferenceLinkageDecision::Unique { resolution }
			| ReferenceLinkageDecision::Candidate { resolution, .. } => {
				for target in resolution.targets.iter() {
					targets.insert(target);
				}
			}
			ReferenceLinkageDecision::External { origin, target, .. } => {
				let external = target.map(|target| (origin, target));
				if external_target.is_some() && external_target != external {
					open = true;
				}
				external_target = external;
			}
			ReferenceLinkageDecision::Dynamic { candidates, .. } => {
				for target in candidates.iter() {
					targets.insert(target);
				}
				open = true;
			}
			ReferenceLinkageDecision::Blocked { .. } | ReferenceLinkageDecision::Unknown { .. } => {
				open = true
			}
		}
	}
	if open || (!targets.is_empty() && external_target.is_some()) {
		return Some(ReferenceLinkageDecision::dynamic(
			crate::snapshot::DynamicReason::DuckTypedCandidateSet,
			method_call.reference_idx,
			method_call.reference.id,
			targets,
		));
	}
	if !targets.is_empty() {
		let resolution = ResolutionDecision::new(
			ResolutionScope::Global,
			ResolutionEvidence::TypeConstraint,
			method_call.reference.id,
			method_call.reference_idx,
			targets,
		);
		return Some(if resolution.targets.len() == 1 {
			ReferenceLinkageDecision::resolved(resolution)
		} else {
			ReferenceLinkageDecision::candidate(
				crate::snapshot::CandidateReason::MultipleTargets,
				resolution,
			)
		});
	}
	external_target
		.map(|(origin, target)| method_call.external_decision_with_origin(origin, target))
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
	let types = tables.value_types.get(&value)?;
	typed_receiver_types_decision(linkage, tables, types, method_call)
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
	let method_call = MethodCallReference {
		reference_idx,
		reference,
		call_name,
	};
	typed_receiver_types_decision(
		linkage,
		tables,
		tables.value_types.get(&value)?,
		method_call,
	)
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

pub(in crate::linkage) fn resolve_method_through_supers(
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
			let origin = external_origin(linkage, tables, &current, method_call);
			let target = method_target(&current, method_call.call_name(), method_call.call_arity());
			return Some(method_call.external_decision_with_origin(origin, target));
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
