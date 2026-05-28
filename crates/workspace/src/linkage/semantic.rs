use code_moniker_core::core::code_graph::RefRecord;
use code_moniker_core::core::kinds::{REF_CALLS, REF_METHOD_CALL};
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::kinds;
use rayon::prelude::*;
use rustc_hash::FxHashMap;

use crate::linkage::decision::{
	ExternalOrigin, ReferenceLinkageDecision, ResolutionScope, UnknownReason,
};
use crate::snapshot::{ReferenceId, ReferenceRecord, SymbolId};
use crate::source::CodeIndexMaterial;

pub(super) struct SemanticLinkage<'a> {
	material: &'a CodeIndexMaterial,
	callables: CallableIndex,
}

impl<'a> SemanticLinkage<'a> {
	pub(super) fn new(material: &'a CodeIndexMaterial) -> Self {
		Self {
			material,
			callables: CallableIndex::build(material),
		}
	}

	pub(super) fn enhance(&self, decisions: &mut [ReferenceLinkageDecision]) {
		let mut statuses = reference_statuses(self.material, decisions);
		let return_types = collect_return_types(self.material, decisions);
		let mut pending = pending_receiver_chains(decisions);
		let receiver_calls = build_receiver_call_index(self.material, decisions, &pending);
		loop {
			let replacements = pending
				.par_iter()
				.filter_map(|idx| match &decisions[*idx] {
					ReferenceLinkageDecision::Unknown {
						reason: UnknownReason::NoCandidate,
						reference,
					} => resolve_receiver_chain(
						self,
						reference,
						&statuses,
						&receiver_calls,
						&return_types,
					)
					.map(|replacement| (*idx, replacement)),
					_ => None,
				})
				.collect::<Vec<_>>();
			if replacements.is_empty() {
				break;
			}
			for (idx, replacement) in replacements {
				if let Some((reference, status)) = reference_status(self.material, &replacement) {
					statuses.insert(reference, status);
				}
				decisions[idx] = replacement;
			}
			pending.retain(|idx| {
				matches!(
					decisions[*idx],
					ReferenceLinkageDecision::Unknown {
						reason: UnknownReason::NoCandidate,
						..
					}
				)
			});
		}
	}

	fn resolved_method_decision(
		&self,
		owner: &Moniker,
		method_call: MethodCallReference<'_>,
	) -> Option<ReferenceLinkageDecision> {
		let target = method_target(owner, method_call.call_name(), method_call.call_arity());
		if let Some(symbol) = self.material.symbols_by_moniker.get(&target) {
			return Some(
				method_call.resolved_decision(ResolutionScope::Global, vec![symbol.clone()]),
			);
		}
		let targets = self.callables.resolve(owner, &method_call)?;
		Some(method_call.resolved_decision(ResolutionScope::Global, targets))
	}

	fn resolved_return_owner(
		&self,
		symbol: &SymbolId,
		return_types: &FxHashMap<Moniker, Moniker>,
	) -> Option<Moniker> {
		let callable = self.material.symbol_monikers.get(symbol)?;
		return_types.get(callable).cloned()
	}
}

struct MethodCallReference<'a> {
	reference: &'a ReferenceRecord,
	call_name: &'a str,
}

impl<'a> MethodCallReference<'a> {
	fn new(reference: &'a ReferenceRecord) -> Option<Self> {
		if reference.kind != "method_call" {
			return None;
		}
		Some(Self {
			reference,
			call_name: reference.call_name.as_deref()?,
		})
	}

	fn reference_id(&self) -> &ReferenceId {
		&self.reference.id
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
			self.reference,
			target,
		)
	}

	fn resolved_decision(
		&self,
		scope: ResolutionScope,
		targets: Vec<SymbolId>,
	) -> ReferenceLinkageDecision {
		ReferenceLinkageDecision::resolved(scope, self.reference, targets)
	}
}

#[derive(Default)]
struct ReceiverCallIndex {
	by_reference: FxHashMap<(usize, usize), ReferenceId>,
}

impl ReceiverCallIndex {
	fn get(&self, file_idx: usize, ref_idx: usize) -> Option<&ReferenceId> {
		self.by_reference.get(&(file_idx, ref_idx))
	}
}

struct CallableIndex {
	by_owner_name_arity: FxHashMap<(Moniker, Vec<u8>, usize), Vec<SymbolId>>,
}

impl CallableIndex {
	fn build(material: &CodeIndexMaterial) -> Self {
		let mut by_owner_name_arity =
			FxHashMap::<(Moniker, Vec<u8>, usize), Vec<SymbolId>>::default();
		for (file_idx, file) in material.files.iter().enumerate() {
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
				let symbol = file.identity.symbol_id(file_idx, def_idx);
				by_owner_name_arity
					.entry((owner, def.call_name.clone(), arity))
					.or_default()
					.push(symbol);
			}
		}
		Self {
			by_owner_name_arity,
		}
	}

	fn resolve(
		&self,
		owner: &Moniker,
		method_call: &MethodCallReference<'_>,
	) -> Option<Vec<SymbolId>> {
		let arity = method_call.call_arity()?;
		let key = (
			owner.clone(),
			method_call.call_name().as_bytes().to_vec(),
			arity,
		);
		let targets = self.by_owner_name_arity.get(&key)?;
		(targets.len() == 1).then(|| targets.clone())
	}
}

fn build_receiver_call_index(
	material: &CodeIndexMaterial,
	decisions: &[ReferenceLinkageDecision],
	pending: &[usize],
) -> ReceiverCallIndex {
	let mut pending_by_file = FxHashMap::<usize, Vec<usize>>::default();
	for idx in pending {
		let ReferenceLinkageDecision::Unknown { reference, .. } = &decisions[*idx] else {
			continue;
		};
		let Some((file_idx, ref_idx)) = material.identity.reference_location(&reference.id) else {
			continue;
		};
		pending_by_file
			.entry(file_idx)
			.or_insert_with(Vec::new)
			.push(ref_idx);
	}

	let mut index = ReceiverCallIndex::default();
	for (file_idx, pending_refs) in pending_by_file {
		index_file_receiver_calls(material, file_idx, &pending_refs, &mut index);
	}
	index
}

fn index_file_receiver_calls(
	material: &CodeIndexMaterial,
	file_idx: usize,
	pending_refs: &[usize],
	index: &mut ReceiverCallIndex,
) {
	let Some(file) = material.files.get(file_idx) else {
		return;
	};
	let calls_by_source = sorted_call_spans_by_source(file);
	for ref_idx in pending_refs {
		let current = file.graph.ref_at(*ref_idx);
		let Some(calls) = calls_by_source.get(current.source) else {
			continue;
		};
		let Some(receiver_idx) = immediate_receiver_call_idx(file, *ref_idx, calls) else {
			continue;
		};
		index.by_reference.insert(
			(file_idx, *ref_idx),
			material.identity.reference_id(file_idx, receiver_idx),
		);
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

fn pending_receiver_chains(decisions: &[ReferenceLinkageDecision]) -> Vec<usize> {
	decisions
		.iter()
		.enumerate()
		.filter_map(|(idx, decision)| {
			let ReferenceLinkageDecision::Unknown {
				reason: UnknownReason::NoCandidate,
				reference,
			} = decision
			else {
				return None;
			};
			MethodCallReference::new(reference).map(|_| idx)
		})
		.collect()
}

fn resolve_receiver_chain(
	linkage: &SemanticLinkage<'_>,
	reference: &ReferenceRecord,
	statuses: &FxHashMap<ReferenceId, ReferenceStatus>,
	receiver_calls: &ReceiverCallIndex,
	return_types: &FxHashMap<Moniker, Moniker>,
) -> Option<ReferenceLinkageDecision> {
	let method_call = MethodCallReference::new(reference)?;
	let (source_file, ref_idx) = linkage
		.material
		.identity
		.reference_location(method_call.reference_id())?;
	let receiver = receiver_calls.get(source_file, ref_idx)?;
	let owner = match statuses.get(receiver)? {
		ReferenceStatus::Resolved(symbols) if symbols.len() == 1 => {
			linkage.resolved_return_owner(&symbols[0], return_types)?
		}
		ReferenceStatus::External(target) => {
			let owner = callable_owner(target)?;
			let target = method_target(&owner, method_call.call_name(), method_call.call_arity());
			return Some(method_call.external_decision(target));
		}
		ReferenceStatus::Resolved(_) => return None,
	};
	if external_target_shape(&owner) {
		let target = method_target(&owner, method_call.call_name(), method_call.call_arity());
		return Some(method_call.external_decision(target));
	}
	linkage.resolved_method_decision(&owner, method_call)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ReferenceStatus {
	Resolved(Vec<SymbolId>),
	External(Moniker),
}

fn collect_return_types(
	material: &CodeIndexMaterial,
	decisions: &[ReferenceLinkageDecision],
) -> FxHashMap<Moniker, Moniker> {
	let mut out = FxHashMap::default();
	for decision in decisions {
		let reference = decision_reference(decision);
		if reference.kind != "returns_type" {
			continue;
		}
		let Some(source) = material.symbol_monikers.get(&reference.source_symbol) else {
			continue;
		};
		let Some(target) = decision_target(material, decision) else {
			continue;
		};
		out.insert(source.clone(), target);
	}
	out
}

fn decision_reference(decision: &ReferenceLinkageDecision) -> &ReferenceRecord {
	match decision {
		ReferenceLinkageDecision::Resolved { reference, .. }
		| ReferenceLinkageDecision::External { reference, .. }
		| ReferenceLinkageDecision::Blocked { reference, .. }
		| ReferenceLinkageDecision::Unknown { reference, .. } => reference,
	}
}

fn decision_target(
	material: &CodeIndexMaterial,
	decision: &ReferenceLinkageDecision,
) -> Option<Moniker> {
	match decision {
		ReferenceLinkageDecision::Resolved { targets, .. } if targets.len() == 1 => {
			material.symbol_monikers.get(&targets[0]).cloned()
		}
		ReferenceLinkageDecision::External {
			reference, target, ..
		} => target
			.clone()
			.or_else(|| material.reference_targets.get(&reference.id).cloned()),
		_ => None,
	}
}

fn reference_statuses(
	material: &CodeIndexMaterial,
	decisions: &[ReferenceLinkageDecision],
) -> FxHashMap<ReferenceId, ReferenceStatus> {
	let mut out = FxHashMap::default();
	for decision in decisions {
		if let Some((reference, status)) = reference_status(material, decision) {
			out.insert(reference, status);
		}
	}
	out
}

fn reference_status(
	material: &CodeIndexMaterial,
	decision: &ReferenceLinkageDecision,
) -> Option<(ReferenceId, ReferenceStatus)> {
	match decision {
		ReferenceLinkageDecision::Resolved {
			reference, targets, ..
		} => Some((
			reference.id.clone(),
			ReferenceStatus::Resolved(targets.clone()),
		)),
		ReferenceLinkageDecision::External {
			reference, target, ..
		} => target
			.as_ref()
			.or_else(|| material.reference_targets.get(&reference.id))
			.map(|target| {
				(
					reference.id.clone(),
					ReferenceStatus::External(target.clone()),
				)
			}),
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
