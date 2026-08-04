use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::kinds;

use crate::linkage::binding::ReferenceLinkageDecision;
use crate::linkage::catalog::{CandidateCatalog, SymbolSet};
use crate::linkage::language::{BindingTarget, PythonBindings};
use crate::linkage::resolve::{
	DecisionSelection, LinkageRefiner, MethodCallReference, ReceiverFieldTables,
	resolve_method_through_supers,
};
use crate::snapshot::{DynamicReason, RecordTable, ReferenceRecord};

pub(in crate::linkage) fn refine(
	bindings: &PythonBindings,
	linkage: &LinkageRefiner<'_>,
	tables: &ReceiverFieldTables,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	selection: DecisionSelection<'_>,
) {
	for &decision_idx in selection.indices() {
		let decision = &mut decisions[decision_idx];
		let Some(reference_idx) = decision.refinement_pending_reference_idx() else {
			continue;
		};
		if !selection.includes(decision.reference()) {
			continue;
		}
		let reference = &references[reference_idx];
		if let Some(resolved) =
			resolve_reference(bindings, linkage, tables, reference_idx, reference)
		{
			*decision = resolved;
		}
	}
}

fn resolve_reference(
	bindings: &PythonBindings,
	linkage: &LinkageRefiner<'_>,
	tables: &ReceiverFieldTables,
	reference_idx: usize,
	reference: &ReferenceRecord,
) -> Option<ReferenceLinkageDecision> {
	let (raw_owner, name) = PythonBindings::target_key(linkage.material, reference)?;
	let owner = tables
		.type_aliases
		.get(&raw_owner)
		.cloned()
		.unwrap_or_else(|| raw_owner.clone());
	let requested_target = linkage.material.reference_target(&reference.id);
	if let Some(resolved) = decision(
		bindings,
		&raw_owner,
		&name,
		reference_idx,
		reference,
		requested_target,
	) {
		return Some(resolved);
	}
	if owner != raw_owner
		&& let Some(resolved) = decision(
			bindings,
			&owner,
			&name,
			reference_idx,
			reference,
			requested_target,
		) {
		return Some(resolved);
	}
	let bound_owner = canonical_workspace_owner(bindings, &owner, linkage.candidates)?;
	if let Some(resolved) = decision(
		bindings,
		&bound_owner,
		&name,
		reference_idx,
		reference,
		requested_target,
	) {
		return Some(resolved);
	}
	let method_call = MethodCallReference::new(reference_idx, reference)?;
	resolve_method_through_supers(linkage, tables, &bound_owner, method_call)
}

fn decision(
	bindings: &PythonBindings,
	owner: &Moniker,
	name: &[u8],
	reference_idx: usize,
	reference: &ReferenceRecord,
	requested_target: Option<&Moniker>,
) -> Option<ReferenceLinkageDecision> {
	if bindings.has_dynamic_wildcards(owner) {
		let candidates = bindings
			.alias(owner, name)
			.map_or_else(SymbolSet::new, BindingTarget::workspace_candidates);
		return Some(ReferenceLinkageDecision::dynamic(
			DynamicReason::RuntimeImport,
			reference_idx,
			reference.id,
			candidates,
		));
	}
	let external = bindings.external_wildcards(owner).collect::<Vec<_>>();
	if let Some(target) = bindings.alias(owner, name) {
		return Some(binding_decision(
			target,
			!external.is_empty(),
			reference_idx,
			reference,
			requested_target,
			name,
		));
	}
	if let Some(binding) = owner_binding(bindings, owner)
		&& (!external.is_empty() || !matches!(binding, BindingTarget::Workspace { .. }))
	{
		return Some(binding_decision(
			binding,
			!external.is_empty(),
			reference_idx,
			reference,
			requested_target,
			name,
		));
	}
	match external.as_slice() {
		[(origin, target)] => Some(ReferenceLinkageDecision::external_target(
			*origin,
			reference_idx,
			reference.id,
			external_wildcard_target(target, requested_target, name),
		)),
		[] => None,
		_ => Some(ReferenceLinkageDecision::dynamic(
			DynamicReason::RuntimeImport,
			reference_idx,
			reference.id,
			SymbolSet::new(),
		)),
	}
}

fn owner_binding<'a>(bindings: &'a PythonBindings, owner: &Moniker) -> Option<&'a BindingTarget> {
	let segment = owner.as_view().segments().last()?;
	bindings.alias(&owner.parent()?, bare_callable_name(segment.name))
}

fn canonical_workspace_owner(
	bindings: &PythonBindings,
	owner: &Moniker,
	candidates: &CandidateCatalog,
) -> Option<Moniker> {
	let binding = owner_binding(bindings, owner)?;
	let BindingTarget::Workspace {
		targets,
		candidate_reason: None,
		..
	} = binding
	else {
		return None;
	};
	let symbol = targets.single()?;
	Some(candidates.candidate(symbol)?.moniker.clone())
}

fn binding_decision(
	binding: &BindingTarget,
	external_present: bool,
	reference_idx: usize,
	reference: &ReferenceRecord,
	requested_target: Option<&Moniker>,
	name: &[u8],
) -> ReferenceLinkageDecision {
	if external_present {
		return ReferenceLinkageDecision::dynamic(
			DynamicReason::RuntimeImport,
			reference_idx,
			reference.id,
			binding.workspace_candidates(),
		);
	}
	match binding {
		BindingTarget::External { origin, target } => ReferenceLinkageDecision::external_target(
			*origin,
			reference_idx,
			reference.id,
			external_wildcard_target(target, requested_target, name),
		),
		BindingTarget::Dynamic { candidates } => ReferenceLinkageDecision::dynamic(
			DynamicReason::RuntimeImport,
			reference_idx,
			reference.id,
			candidates.clone(),
		),
		BindingTarget::Workspace { .. } => {
			binding.to_decision(reference_idx, reference, requested_target)
		}
	}
}

fn external_wildcard_target(
	module: &Moniker,
	requested_target: Option<&Moniker>,
	name: &[u8],
) -> Moniker {
	let (kind, target_name) = requested_target
		.and_then(|target| target.as_view().segments().last())
		.map_or((kinds::PATH, name), |segment| (segment.kind, segment.name));
	MonikerBuilder::from_view(module.as_view())
		.segment(kind, target_name)
		.build()
}

#[cfg(test)]
mod tests {
	use super::*;

	use crate::linkage::binding::ExternalOrigin;
	use crate::snapshot::{ReferenceId, SourceId, SymbolId};

	#[test]
	fn external_wildcard_keeps_its_original_provenance() {
		let owner = MonikerBuilder::new()
			.project(b".")
			.segment(kinds::LANG, b"python")
			.segment(kinds::MODULE, b"facade")
			.build();
		let external = MonikerBuilder::new()
			.project(b".")
			.segment(kinds::EXTERNAL_PKG, b"generated")
			.build();
		let bindings = PythonBindings::with_external_wildcard(
			owner.clone(),
			external,
			ExternalOrigin::Injected,
		);
		let reference = ReferenceRecord::new(
			ReferenceId::at(0, 0),
			SourceId::at(0),
			SymbolId::at(0, 0),
			"code+moniker://./lang:python/module:facade/function:Client",
			"calls",
			None,
		);

		let decision = decision(&bindings, &owner, b"Client", 0, &reference, None)
			.expect("external wildcard decision");

		assert!(matches!(
			decision,
			ReferenceLinkageDecision::External {
				origin: ExternalOrigin::Injected,
				..
			}
		));
	}
}
