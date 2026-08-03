use super::*;

mod chains;
mod fields;
mod methods;

pub(in crate::linkage) use chains::pending_receiver_chains;
use chains::{
	ChainContext, build_receiver_call_index, callable_owner, collect_return_types,
	decision_reference, decision_target, external_origin, external_target_shape, method_target,
	reference_status, reference_statuses, resolve_receiver_chain,
};
use fields::{MonikerTypeSet, typed_receiver_decision, typed_receiver_types_decision};
pub(in crate::linkage) use fields::{ReceiverFieldTables, resolve_method_through_supers};
pub(in crate::linkage) use fields::{build_receiver_field_tables, refine_receiver_fields};
use methods::ReceiverCallIndex;
pub(in crate::linkage) use methods::{MethodCallReference, MethodTable};

impl LinkageRefiner<'_> {
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

	fn resolved_return_types<'b>(
		&self,
		symbol: SymbolOrdinal,
		return_types: &'b FxHashMap<Moniker, MonikerTypeSet>,
	) -> Option<&'b MonikerTypeSet> {
		let callable = self.candidates.candidate(symbol)?.moniker;
		return_types.get(callable)
	}

	fn manifest_declares_target(
		&self,
		method_call: MethodCallReference<'_>,
		target: &Moniker,
	) -> bool {
		let Some(location) = self.locations.get(method_call.reference_idx) else {
			return false;
		};
		let Some(query) = LinkageQuery::at(method_call.reference, self.material, location) else {
			return false;
		};
		self.manifests
			.declares_external_target(&query.with_target(target))
	}
}

pub(super) fn refine_structural_receivers(
	linkage: &LinkageRefiner<'_>,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	changed_references: Option<&FxHashSet<ReferenceId>>,
) {
	let mut owners_by_binding: FxHashMap<(crate::snapshot::SymbolId, String), SymbolSet> =
		FxHashMap::default();
	if let Some(changed) = changed_references {
		let mut affected = FxHashMap::default();
		for reference_id in changed {
			let Some((source_file, local_reference)) =
				linkage.material.identity.reference_location(reference_id)
			else {
				continue;
			};
			let Some(reference_idx) = linkage
				.locations
				.reference_idx(source_file, local_reference)
			else {
				continue;
			};
			let reference = &references[reference_idx];
			let Some(receiver) = structural_receiver_name(reference) else {
				continue;
			};
			affected.insert((reference.source_symbol, receiver.to_owned()), source_file);
		}
		for (binding, source_file) in affected {
			let Some(file) = linkage.material.files.get(source_file) else {
				continue;
			};
			for local_reference in 0..file.graph.ref_count() {
				let Some(reference_idx) = linkage
					.locations
					.reference_idx(source_file, local_reference)
				else {
					continue;
				};
				let reference = &references[reference_idx];
				if reference.source_symbol != binding.0
					|| structural_receiver_name(reference) != Some(binding.1.as_str())
				{
					continue;
				}
				accumulate_structural_owner(linkage, &mut owners_by_binding, reference);
			}
		}
	} else {
		for reference in references.iter() {
			accumulate_structural_owner(linkage, &mut owners_by_binding, reference);
		}
	}

	const MAX_STRUCTURAL_OWNERS: usize = 32;
	for decision in decisions {
		let Some(reference_idx) = decision.refinement_pending_reference_idx() else {
			continue;
		};
		if changed_references.is_some_and(|changed| !changed.contains(decision.reference())) {
			continue;
		}
		let reference = &references[reference_idx];
		let Some(receiver) = structural_receiver_name(reference) else {
			continue;
		};
		let Some(owners) = owners_by_binding.get(&(reference.source_symbol, receiver.to_owned()))
		else {
			continue;
		};
		if owners.is_empty() || owners.len() > MAX_STRUCTURAL_OWNERS {
			continue;
		}
		let Some(call_name) = reference.call_name.as_deref() else {
			continue;
		};
		let Some(call_arity) = reference.call_arity else {
			continue;
		};
		let targets =
			linkage
				.methods
				.methods_for_owners(linkage.candidates, owners, call_name, call_arity);
		if targets.is_empty() {
			continue;
		}
		*decision = ReferenceLinkageDecision::dynamic(
			crate::snapshot::DynamicReason::DuckTypedCandidateSet,
			reference_idx,
			reference.id,
			targets,
		);
	}
}

fn accumulate_structural_owner(
	linkage: &LinkageRefiner<'_>,
	owners_by_binding: &mut FxHashMap<(crate::snapshot::SymbolId, String), SymbolSet>,
	reference: &ReferenceRecord,
) {
	let Some(receiver) = structural_receiver_name(reference) else {
		return;
	};
	let Some(call_name) = reference.call_name.as_deref() else {
		return;
	};
	let Some(call_arity) = reference.call_arity else {
		return;
	};
	let owners = linkage
		.methods
		.structural_owners(call_name, call_arity)
		.cloned()
		.unwrap_or_else(SymbolSet::new);
	match owners_by_binding.entry((reference.source_symbol, receiver.to_owned())) {
		std::collections::hash_map::Entry::Vacant(entry) => {
			entry.insert(owners);
		}
		std::collections::hash_map::Entry::Occupied(mut entry) => {
			entry.get_mut().intersect_with(&owners);
		}
	}
}

fn structural_receiver_name(reference: &ReferenceRecord) -> Option<&str> {
	if reference.kind != "method_call" {
		return None;
	}
	let receiver = reference.receiver.as_deref()?;
	if receiver.is_empty()
		|| matches!(
			receiver,
			"self" | "cls" | "call" | "member" | "subscript" | "python_conditional_import"
		) || !receiver
		.bytes()
		.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
	{
		return None;
	}
	Some(receiver)
}

pub(super) fn refine_receiver_chains(
	linkage: &LinkageRefiner<'_>,
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
				let reference_idx = decisions[*idx].refinement_pending_reference_idx()?;
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
		pending.retain(|idx| decisions[*idx].refinement_pending_reference_idx().is_some());
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn return_type_sets_preserve_distinct_candidates() {
		let first = MonikerBuilder::new()
			.project(b"app")
			.segment(b"class", b"First")
			.build();
		let second = MonikerBuilder::new()
			.project(b"app")
			.segment(b"class", b"Second")
			.build();
		let mut types = MonikerTypeSet::default();
		types.insert(first.clone());
		types.insert(second.clone());
		types.insert(first.clone());

		assert_eq!(
			types.iter().cloned().collect::<Vec<_>>(),
			vec![first, second]
		);
	}
}
