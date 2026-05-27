use code_moniker_core::core::code_graph::RefRecord;
use code_moniker_core::core::kinds::{REF_CALLS, REF_METHOD_CALL, REF_RETURNS_TYPE};
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::kinds;
use rustc_hash::FxHashMap;

use crate::linkage::decision::{
	ExternalOrigin, ReferenceLinkageDecision, ResolutionScope, UnknownReason,
};
use crate::linkage::manifest::is_builtin_external_root;
use crate::snapshot::{ReferenceId, ReferenceRecord, SymbolId};
use crate::source::CodeIndexMaterial;

pub(super) struct SemanticLinkage<'a> {
	material: &'a CodeIndexMaterial,
	return_types: FxHashMap<Moniker, Moniker>,
}

impl<'a> SemanticLinkage<'a> {
	pub(super) fn new(material: &'a CodeIndexMaterial) -> Self {
		Self {
			material,
			return_types: collect_return_types(material),
		}
	}

	pub(super) fn enhance(&self, decisions: &mut [ReferenceLinkageDecision]) {
		loop {
			let statuses = reference_statuses(self.material, decisions);
			let mut changed = false;
			for decision in decisions.iter_mut() {
				let replacement = match decision {
					ReferenceLinkageDecision::Unknown {
						reason: UnknownReason::NoCandidate,
						reference,
					} => resolve_receiver_chain(self, reference, &statuses),
					_ => None,
				};
				if let Some(replacement) = replacement {
					*decision = replacement;
					changed = true;
				}
			}
			if !changed {
				break;
			}
		}
	}

	fn resolved_method_decision(
		&self,
		owner: &Moniker,
		method_call: MethodCallReference<'_>,
	) -> Option<ReferenceLinkageDecision> {
		let target = method_target(owner, method_call.call_name(), method_call.call_arity());
		self.material.symbols_by_moniker.get(&target).map(|symbol| {
			method_call.resolved_decision(ResolutionScope::Global, vec![symbol.clone()])
		})
	}

	fn resolved_return_owner(&self, symbol: &SymbolId) -> Option<Moniker> {
		let callable = self.material.symbol_monikers.get(symbol)?;
		self.return_types.get(callable).cloned()
	}

	fn immediate_receiver_call(&self, file_idx: usize, ref_idx: usize) -> Option<ReferenceId> {
		let file = self.material.files.get(file_idx)?;
		let current = file.graph.ref_at(ref_idx);
		let current_position = current.position?;
		(0..file.graph.ref_count())
			.filter(|candidate_idx| *candidate_idx != ref_idx)
			.filter_map(|candidate_idx| {
				let candidate = file.graph.ref_at(candidate_idx);
				if !is_call_ref(candidate) || candidate.source != current.source {
					return None;
				}
				let position = candidate.position?;
				contains_position(current_position, position)
					.then_some((candidate_idx, position.1.saturating_sub(position.0)))
			})
			.max_by_key(|(_, width)| *width)
			.map(|(candidate_idx, _)| self.material.identity.reference_id(file_idx, candidate_idx))
	}

	fn is_builtin_external(&self, source_file: usize, target: &Moniker) -> bool {
		let Some(root) = external_target_root(target) else {
			return false;
		};
		self.material
			.files
			.get(source_file)
			.is_some_and(|file| is_builtin_external_root(file.lang, root))
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

fn resolve_receiver_chain(
	linkage: &SemanticLinkage<'_>,
	reference: &ReferenceRecord,
	statuses: &FxHashMap<ReferenceId, ReferenceStatus>,
) -> Option<ReferenceLinkageDecision> {
	let method_call = MethodCallReference::new(reference)?;
	let (source_file, ref_idx) = linkage
		.material
		.identity
		.reference_location(method_call.reference_id())?;
	let receiver = linkage.immediate_receiver_call(source_file, ref_idx)?;
	let owner = match statuses.get(&receiver)? {
		ReferenceStatus::Resolved(symbols) if symbols.len() == 1 => {
			linkage.resolved_return_owner(&symbols[0])?
		}
		ReferenceStatus::External(target) => external_callable_owner(target)?,
		ReferenceStatus::Resolved(_) => return None,
	};
	if external_target_shape(&owner) {
		let target = method_target(&owner, method_call.call_name(), method_call.call_arity());
		return linkage
			.is_builtin_external(source_file, &owner)
			.then(|| method_call.external_decision(target));
	}
	linkage.resolved_method_decision(&owner, method_call)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ReferenceStatus {
	Resolved(Vec<SymbolId>),
	External(Moniker),
}

fn collect_return_types(material: &CodeIndexMaterial) -> FxHashMap<Moniker, Moniker> {
	let mut out = FxHashMap::default();
	for file in &material.files {
		for reference in file
			.graph
			.refs()
			.filter(|reference| reference.kind == REF_RETURNS_TYPE)
		{
			let source = file.graph.def_at(reference.source).moniker.clone();
			out.insert(source, reference.target.clone());
		}
	}
	out
}

fn reference_statuses(
	material: &CodeIndexMaterial,
	decisions: &[ReferenceLinkageDecision],
) -> FxHashMap<ReferenceId, ReferenceStatus> {
	let mut out = FxHashMap::default();
	for decision in decisions {
		match decision {
			ReferenceLinkageDecision::Resolved {
				reference, targets, ..
			} => {
				out.insert(
					reference.id.clone(),
					ReferenceStatus::Resolved(targets.clone()),
				);
			}
			ReferenceLinkageDecision::External {
				reference, target, ..
			} => {
				if let Some(target) = target
					.as_ref()
					.or_else(|| material.reference_targets.get(&reference.id))
				{
					out.insert(
						reference.id.clone(),
						ReferenceStatus::External(target.clone()),
					);
				}
			}
			_ => {}
		}
	}
	out
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

fn external_callable_owner(target: &Moniker) -> Option<Moniker> {
	if !external_target_shape(target) {
		return None;
	}
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

fn external_target_root(target: &Moniker) -> Option<&str> {
	let head = target.as_view().segments().next()?;
	if head.kind != kinds::EXTERNAL_PKG {
		return None;
	}
	std::str::from_utf8(head.name).ok()
}
