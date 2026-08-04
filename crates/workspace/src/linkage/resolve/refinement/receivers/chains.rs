use super::*;

pub(super) fn build_receiver_call_index(
	linkage: &LinkageRefiner<'_>,
	decisions: &[ReferenceLinkageDecision],
	pending: &[usize],
) -> ReceiverCallIndex {
	let mut pending_by_file = FxHashMap::<usize, Vec<(usize, usize)>>::default();
	for idx in pending {
		let Some(reference_idx) = decisions[*idx].refinement_pending_reference_idx() else {
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
	linkage: &LinkageRefiner<'_>,
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

pub(in crate::linkage) fn pending_receiver_chains(
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
			let reference_idx = decision.refinement_pending_reference_idx()?;
			MethodCallReference::new(reference_idx, &references[reference_idx]).map(|_| idx)
		})
		.collect()
}

pub(super) struct ChainContext<'a> {
	pub(super) statuses: &'a FxHashMap<usize, ReferenceStatus>,
	pub(super) receiver_calls: &'a ReceiverCallIndex,
	pub(super) return_types: &'a FxHashMap<Moniker, MonikerTypeSet>,
}

pub(super) fn resolve_receiver_chain(
	linkage: &LinkageRefiner<'_>,
	tables: &ReceiverFieldTables,
	context: &ChainContext<'_>,
	reference_idx: usize,
	reference: &ReferenceRecord,
) -> Option<ReferenceLinkageDecision> {
	let method_call = MethodCallReference::new(reference_idx, reference)?;
	let receiver = context.receiver_calls.get(reference_idx)?;
	match context.statuses.get(&receiver)? {
		ReferenceStatus::Resolved(symbol) => {
			let callable = linkage.candidates.candidate(*symbol)?.moniker;
			if callable
				.as_view()
				.segments()
				.last()
				.is_some_and(|segment| segment.kind == kinds::CLASS)
			{
				typed_receiver_decision(linkage, tables, callable, method_call)
			} else {
				let types = linkage.resolved_return_types(*symbol, context.return_types)?;
				typed_receiver_types_decision(linkage, tables, types, method_call)
			}
		}
		ReferenceStatus::External { origin, target } => {
			let owner = callable_owner(target)?;
			let target = method_target(&owner, method_call.call_name(), method_call.call_arity());
			Some(method_call.external_decision_with_origin(*origin, target))
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ReferenceStatus {
	Resolved(SymbolOrdinal),
	External {
		origin: ExternalOrigin,
		target: Moniker,
	},
}

pub(super) fn collect_return_types(
	material: &CodeIndexMaterial,
	candidates: &CandidateCatalog,
	decisions: &[ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
) -> FxHashMap<Moniker, MonikerTypeSet> {
	let mut out: FxHashMap<Moniker, MonikerTypeSet> = FxHashMap::default();
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
		let types = out.entry(source.clone()).or_default();
		types.insert(target);
		if reference.receiver.as_deref() == Some("python_open_type_set") {
			types.mark_open();
		}
	}
	out
}

pub(super) fn decision_reference<'a>(
	decision: &ReferenceLinkageDecision,
	references: &'a RecordTable<ReferenceRecord>,
) -> &'a ReferenceRecord {
	&references[decision.reference_idx()]
}

pub(super) fn decision_target(
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

pub(super) fn reference_statuses(
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

pub(super) fn reference_status(
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
			origin,
			target,
			..
		} => target
			.as_ref()
			.or_else(|| material.reference_target(&references[*reference_idx].id))
			.map(|target| ReferenceStatus::External {
				origin: *origin,
				target: target.clone(),
			}),
		_ => None,
	}
}

fn is_call_ref(reference: &RefRecord) -> bool {
	reference.kind == REF_CALLS
		|| reference.kind == REF_INSTANTIATES
		|| reference.kind == REF_METHOD_CALL
}

fn contains_position(outer: (u32, u32), inner: (u32, u32)) -> bool {
	outer.0 <= inner.0 && inner.1 <= outer.1 && outer != inner
}

pub(super) fn method_target(
	owner: &Moniker,
	call_name: &str,
	call_arity: Option<usize>,
) -> Moniker {
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

pub(super) fn callable_owner(target: &Moniker) -> Option<Moniker> {
	let Some(last) = target.as_view().segments().last() else {
		return Some(target.clone());
	};
	if matches!(last.kind, kinds::METHOD | kinds::CONSTRUCTOR) {
		return target.parent();
	}
	Some(target.clone())
}

pub(super) fn external_target_shape(target: &Moniker) -> bool {
	target
		.as_view()
		.segments()
		.next()
		.is_some_and(|segment| matches!(segment.kind, kinds::EXTERNAL_PKG | kinds::SDK))
}

pub(super) fn external_origin(
	linkage: &LinkageRefiner<'_>,
	tables: &ReceiverFieldTables,
	target: &Moniker,
	method_call: MethodCallReference<'_>,
) -> ExternalOrigin {
	let mut current = Some(target.clone());
	while let Some(moniker) = current {
		if let Some(origin) = tables.invariant_external_origins.get(&moniker) {
			return *origin;
		}
		current = moniker.parent();
	}
	if target
		.as_view()
		.segments()
		.next()
		.is_some_and(|segment| segment.kind == kinds::SDK)
	{
		return ExternalOrigin::Sdk;
	}
	if linkage.packages.is_foreign_moniker(target) {
		return ExternalOrigin::Dependency;
	}
	if linkage.manifest_declares_target(method_call, target) {
		return ExternalOrigin::Dependency;
	}
	ExternalOrigin::UnknownExternal
}
