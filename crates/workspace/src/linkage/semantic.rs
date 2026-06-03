use code_moniker_core::core::code_graph::RefRecord;
use code_moniker_core::core::kinds::{REF_CALLS, REF_METHOD_CALL, REF_READS, REF_REEXPORTS};
use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::kinds;
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::linkage::decision::{
	ExternalOrigin, ReferenceLinkageDecision, ResolutionScope, UnknownReason,
};
use crate::linkage::language;
use crate::snapshot::{ReferenceId, ReferenceRecord, SymbolId};
use crate::source::CodeIndexMaterial;

pub(super) struct SemanticLinkage<'a> {
	material: &'a CodeIndexMaterial,
	methods: &'a MethodTable,
}

impl<'a> SemanticLinkage<'a> {
	pub(super) fn new(material: &'a CodeIndexMaterial, methods: &'a MethodTable) -> Self {
		Self { material, methods }
	}

	pub(super) fn enhance(
		&self,
		decisions: &mut [ReferenceLinkageDecision],
		references: &[ReferenceRecord],
	) {
		language::enhance_reference_semantics(self.material, decisions, references, None);
		enhance_reexport_aliases(self, decisions, references, None);
		let pending = pending_receiver_chains(decisions, references, None);
		enhance_receiver_chains(self, decisions, references, pending);
	}

	pub(super) fn enhance_changed(
		&self,
		decisions: &mut [ReferenceLinkageDecision],
		references: &[ReferenceRecord],
		changed_references: &FxHashSet<ReferenceId>,
	) {
		language::enhance_reference_semantics(
			self.material,
			decisions,
			references,
			Some(changed_references),
		);
		enhance_reexport_aliases(self, decisions, references, Some(changed_references));
		let pending = pending_receiver_chains(decisions, references, Some(changed_references));
		enhance_receiver_chains(self, decisions, references, pending);
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
		let targets = self.methods.resolve(owner, &method_call)?;
		Some(method_call.resolved_decision(ResolutionScope::Global, targets))
	}

	fn resolved_return_owner(
		&self,
		symbol: &SymbolId,
		return_types: &FxHashMap<Moniker, Moniker>,
	) -> Option<Moniker> {
		let callable = self.material.symbol_moniker(symbol)?;
		return_types.get(callable).cloned()
	}
}

fn enhance_receiver_chains(
	linkage: &SemanticLinkage<'_>,
	decisions: &mut [ReferenceLinkageDecision],
	references: &[ReferenceRecord],
	mut pending: Vec<usize>,
) {
	if pending.is_empty() {
		return;
	}
	let receiver_calls =
		build_receiver_call_index(linkage.material, decisions, references, &pending);
	let wanted = receiver_calls
		.by_reference
		.values()
		.collect::<FxHashSet<_>>();
	let mut statuses = reference_statuses(linkage.material, decisions, references, &wanted);
	let return_types = collect_return_types(linkage.material, decisions, references);
	loop {
		let replacements = pending
			.par_iter()
			.filter_map(|idx| match &decisions[*idx] {
				ReferenceLinkageDecision::Unknown {
					reason: UnknownReason::NoCandidate,
					reference_idx,
					..
				} => resolve_receiver_chain(
					linkage,
					*reference_idx,
					&references[*reference_idx],
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
			if let Some((reference, status)) =
				reference_status(linkage.material, &replacement, references)
			{
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

struct MethodCallReference<'a> {
	reference_idx: usize,
	reference: &'a ReferenceRecord,
	call_name: &'a str,
}

impl<'a> MethodCallReference<'a> {
	fn new(reference_idx: usize, reference: &'a ReferenceRecord) -> Option<Self> {
		if reference.kind != "method_call" {
			return None;
		}
		Some(Self {
			reference_idx,
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
			self.reference_idx,
			self.reference.id.clone(),
			target,
		)
	}

	fn resolved_decision(
		&self,
		scope: ResolutionScope,
		targets: Vec<SymbolId>,
	) -> ReferenceLinkageDecision {
		ReferenceLinkageDecision::resolved(
			scope,
			self.reference_idx,
			self.reference.id.clone(),
			targets,
		)
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

type MethodKey = (Moniker, Vec<u8>, usize);

#[derive(Default)]
pub(super) struct MethodTable {
	by_owner_name_arity: FxHashMap<MethodKey, Vec<SymbolId>>,
	keys_by_file: FxHashMap<usize, Vec<MethodKey>>,
}

impl MethodTable {
	pub(super) fn build(material: &CodeIndexMaterial) -> Self {
		let mut index = Self::default();
		for file_idx in 0..material.files.len() {
			index.insert_file(material, file_idx);
		}
		index
	}

	pub(super) fn refresh_files(
		&mut self,
		material: &CodeIndexMaterial,
		file_indexes: &std::collections::BTreeSet<usize>,
	) {
		for &file_idx in file_indexes {
			self.remove_file(material, file_idx);
		}
		for &file_idx in file_indexes {
			self.insert_file(material, file_idx);
		}
	}

	fn remove_file(&mut self, material: &CodeIndexMaterial, file_idx: usize) {
		let Some(keys) = self.keys_by_file.remove(&file_idx) else {
			return;
		};
		for key in keys {
			let Some(symbols) = self.by_owner_name_arity.get_mut(&key) else {
				continue;
			};
			symbols.retain(|symbol| {
				material
					.identity
					.symbol_location(symbol)
					.map(|(file, _)| file)
					!= Some(file_idx)
			});
			if symbols.is_empty() {
				self.by_owner_name_arity.remove(&key);
			}
		}
	}

	fn insert_file(&mut self, material: &CodeIndexMaterial, file_idx: usize) {
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
			let symbol = file.identity.symbol_id(file_idx, def_idx);
			let key = (owner, def.call_name.to_vec(), arity);
			self.by_owner_name_arity
				.entry(key.clone())
				.or_default()
				.push(symbol);
			self.keys_by_file.entry(file_idx).or_default().push(key);
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

fn enhance_reexport_aliases(
	linkage: &SemanticLinkage<'_>,
	decisions: &mut [ReferenceLinkageDecision],
	references: &[ReferenceRecord],
	changed_references: Option<&FxHashSet<ReferenceId>>,
) {
	let aliases = build_reexport_aliases(linkage.material, decisions, references);
	if aliases.is_empty() {
		return;
	}
	for decision in decisions.iter_mut() {
		let reference_idx = match decision {
			ReferenceLinkageDecision::Unknown {
				reason: UnknownReason::NoCandidate,
				reference_idx,
				..
			} => *reference_idx,
			_ => continue,
		};
		if changed_references.is_some_and(|changed| !changed.contains(decision.reference())) {
			continue;
		}
		let Some((owner, name)) =
			reference_target_alias_key(linkage.material, &references[reference_idx])
		else {
			continue;
		};
		let Some(alias) = aliases.get(&(owner, name)) else {
			continue;
		};
		let reference = &references[reference_idx];
		let requested_target = linkage.material.reference_target(&reference.id);
		*decision = alias.to_decision(reference_idx, reference, requested_target);
	}
}

#[derive(Clone)]
enum ReexportAliasTarget {
	Resolved {
		scope: ResolutionScope,
		targets: Vec<SymbolId>,
	},
	External {
		origin: ExternalOrigin,
		target: Moniker,
	},
}

impl ReexportAliasTarget {
	fn from_decision(
		decision: &ReferenceLinkageDecision,
		fallback_external_target: Option<Moniker>,
	) -> Option<Self> {
		match decision {
			ReferenceLinkageDecision::Resolved { scope, targets, .. } if targets.len() == 1 => {
				Some(Self::Resolved {
					scope: *scope,
					targets: targets.clone(),
				})
			}
			ReferenceLinkageDecision::External { origin, target, .. } => Some(Self::External {
				origin: *origin,
				target: target.clone().or(fallback_external_target)?,
			}),
			_ => None,
		}
	}

	fn to_decision(
		&self,
		reference_idx: usize,
		reference: &ReferenceRecord,
		requested_target: Option<&Moniker>,
	) -> ReferenceLinkageDecision {
		match self {
			Self::Resolved { scope, targets } => ReferenceLinkageDecision::resolved(
				*scope,
				reference_idx,
				reference.id.clone(),
				targets.clone(),
			),
			Self::External { origin, target } => ReferenceLinkageDecision::external_target(
				*origin,
				reference_idx,
				reference.id.clone(),
				reexport_external_target(target, requested_target),
			),
		}
	}
}

fn reexport_external_target(alias_target: &Moniker, requested_target: Option<&Moniker>) -> Moniker {
	let Some(requested_target) = requested_target else {
		return alias_target.clone();
	};
	let Some(alias_last) = alias_target.as_view().segments().last() else {
		return alias_target.clone();
	};
	let Some(requested_last) = requested_target.as_view().segments().last() else {
		return alias_target.clone();
	};
	if bare_callable_name(alias_last.name) != bare_callable_name(requested_last.name) {
		return alias_target.clone();
	}
	let Some(owner) = alias_target.parent() else {
		return alias_target.clone();
	};
	MonikerBuilder::from_view(owner.as_view())
		.segment(requested_last.kind, requested_last.name)
		.build()
}

fn build_reexport_aliases(
	material: &CodeIndexMaterial,
	decisions: &[ReferenceLinkageDecision],
	references: &[ReferenceRecord],
) -> FxHashMap<(Moniker, Vec<u8>), ReexportAliasTarget> {
	let mut aliases = FxHashMap::default();
	for decision in decisions {
		let reference = decision_reference(decision, references);
		if reference.kind.as_bytes() != REF_REEXPORTS {
			continue;
		}
		let Some(owner) = material.symbol_moniker(&reference.source_symbol) else {
			continue;
		};
		let Some(name) = reexport_alias_name(material, reference) else {
			continue;
		};
		let fallback_external_target = material.reference_target(&reference.id).cloned();
		let Some(target) = ReexportAliasTarget::from_decision(decision, fallback_external_target)
		else {
			continue;
		};
		aliases.insert((owner.clone(), name), target);
	}
	aliases
}

fn reexport_alias_name(
	material: &CodeIndexMaterial,
	reference: &ReferenceRecord,
) -> Option<Vec<u8>> {
	if let Some(alias) = reference.alias.as_deref().filter(|alias| !alias.is_empty()) {
		return Some(alias.as_bytes().to_vec());
	}
	let target = material.reference_target(&reference.id)?;
	let last = target.as_view().segments().last()?;
	if last.kind != kinds::PATH {
		return None;
	}
	Some(bare_callable_name(last.name).to_vec())
}

fn reference_target_alias_key(
	material: &CodeIndexMaterial,
	reference: &ReferenceRecord,
) -> Option<(Moniker, Vec<u8>)> {
	let target = material.reference_target(&reference.id)?;
	let name = reference
		.call_name
		.as_deref()
		.map(|name| name.as_bytes().to_vec())
		.or_else(|| {
			target
				.as_view()
				.segments()
				.last()
				.map(|segment| bare_callable_name(segment.name).to_vec())
		})?;
	let owner = target.parent()?;
	Some((owner, name))
}

fn build_receiver_call_index(
	material: &CodeIndexMaterial,
	decisions: &[ReferenceLinkageDecision],
	references: &[ReferenceRecord],
	pending: &[usize],
) -> ReceiverCallIndex {
	let mut pending_by_file = FxHashMap::<usize, Vec<usize>>::default();
	for idx in pending {
		let ReferenceLinkageDecision::Unknown { reference_idx, .. } = &decisions[*idx] else {
			continue;
		};
		let reference = &references[*reference_idx];
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
		let Some(receiver_idx) = immediate_receiver_call_idx(file, *ref_idx, calls)
			.or_else(|| immediate_receiver_read_idx(file, *ref_idx))
		else {
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
	references: &[ReferenceRecord],
	changed_references: Option<&FxHashSet<ReferenceId>>,
) -> Vec<usize> {
	decisions
		.iter()
		.enumerate()
		.filter_map(|(idx, decision)| {
			if changed_references.is_some_and(|changed| !changed.contains(decision.reference())) {
				return None;
			}
			let ReferenceLinkageDecision::Unknown {
				reason: UnknownReason::NoCandidate,
				reference_idx,
				..
			} = decision
			else {
				return None;
			};
			MethodCallReference::new(*reference_idx, &references[*reference_idx]).map(|_| idx)
		})
		.collect()
}

fn resolve_receiver_chain(
	linkage: &SemanticLinkage<'_>,
	reference_idx: usize,
	reference: &ReferenceRecord,
	statuses: &FxHashMap<ReferenceId, ReferenceStatus>,
	receiver_calls: &ReceiverCallIndex,
	return_types: &FxHashMap<Moniker, Moniker>,
) -> Option<ReferenceLinkageDecision> {
	let method_call = MethodCallReference::new(reference_idx, reference)?;
	let (source_file, ref_idx) = linkage
		.material
		.identity
		.reference_location(method_call.reference_id())?;
	let receiver = receiver_calls.get(source_file, ref_idx)?;
	let owner = match statuses.get(receiver)? {
		ReferenceStatus::Resolved(symbol) => linkage.resolved_return_owner(symbol, return_types)?,
		ReferenceStatus::External(target) => {
			let owner = callable_owner(target)?;
			let target = method_target(&owner, method_call.call_name(), method_call.call_arity());
			return Some(method_call.external_decision(target));
		}
	};
	if external_target_shape(&owner) {
		let target = method_target(&owner, method_call.call_name(), method_call.call_arity());
		return Some(method_call.external_decision(target));
	}
	linkage.resolved_method_decision(&owner, method_call)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ReferenceStatus {
	Resolved(SymbolId),
	External(Moniker),
}

fn collect_return_types(
	material: &CodeIndexMaterial,
	decisions: &[ReferenceLinkageDecision],
	references: &[ReferenceRecord],
) -> FxHashMap<Moniker, Moniker> {
	let mut out = FxHashMap::default();
	for decision in decisions {
		let reference = decision_reference(decision, references);
		if reference.kind != "returns_type" {
			continue;
		}
		let Some(source) = material.symbol_moniker(&reference.source_symbol) else {
			continue;
		};
		let Some(target) = decision_target(material, decision, references) else {
			continue;
		};
		out.insert(source.clone(), target);
	}
	out
}

fn decision_reference<'a>(
	decision: &ReferenceLinkageDecision,
	references: &'a [ReferenceRecord],
) -> &'a ReferenceRecord {
	&references[decision.reference_idx()]
}

fn decision_target(
	material: &CodeIndexMaterial,
	decision: &ReferenceLinkageDecision,
	references: &[ReferenceRecord],
) -> Option<Moniker> {
	match decision {
		ReferenceLinkageDecision::Resolved { targets, .. } if targets.len() == 1 => {
			material.symbol_moniker(&targets[0]).cloned()
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
	references: &[ReferenceRecord],
	wanted: &FxHashSet<&ReferenceId>,
) -> FxHashMap<ReferenceId, ReferenceStatus> {
	let mut out = FxHashMap::default();
	for decision in decisions {
		if !wanted.contains(decision.reference()) {
			continue;
		}
		if let Some((reference, status)) = reference_status(material, decision, references) {
			out.insert(reference, status);
		}
	}
	out
}

fn reference_status(
	material: &CodeIndexMaterial,
	decision: &ReferenceLinkageDecision,
	references: &[ReferenceRecord],
) -> Option<(ReferenceId, ReferenceStatus)> {
	match decision {
		ReferenceLinkageDecision::Resolved {
			reference_idx,
			targets,
			..
		} => {
			let [target] = targets.as_slice() else {
				return None;
			};
			Some((
				references[*reference_idx].id.clone(),
				ReferenceStatus::Resolved(target.clone()),
			))
		}
		ReferenceLinkageDecision::External {
			reference_idx,
			target,
			..
		} => target
			.as_ref()
			.or_else(|| material.reference_target(&references[*reference_idx].id))
			.map(|target| {
				(
					references[*reference_idx].id.clone(),
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
