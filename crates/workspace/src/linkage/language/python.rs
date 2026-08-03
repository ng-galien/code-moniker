use code_moniker_core::core::moniker::Segment;
use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::lang::kinds;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::linkage::binding::ReferenceLinkageDecision;
use crate::linkage::catalog::LinkageCandidate;
use crate::linkage::catalog::LinkageQuery;
use crate::linkage::catalog::SymbolSet;
use crate::linkage::language::generic_matches;
use crate::snapshot::{DynamicReason, RecordTable, ReferenceId, ReferenceRecord};
use crate::source::CodeIndexMaterial;

mod bindings;

pub(in crate::linkage) use bindings::PythonBindingGraph;

pub(super) fn matches(query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
	generic_matches(query, candidate) || python_path_target_matches_def(query, candidate)
}

fn python_path_target_matches_def(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	let target_segments = query.target_segments().collect::<Vec<_>>();
	let candidate_segments =
		normalized_python_segments(candidate.moniker.as_view().segments().collect::<Vec<_>>());
	if target_segments.len() != candidate_segments.len() || target_segments.is_empty() {
		return false;
	}
	if is_non_shadowable_python_sdk_target(&target_segments) {
		return false;
	}
	target_segments
		.iter()
		.zip(candidate_segments.iter())
		.all(|(target, candidate_segment)| {
			python_segment_matches(query, candidate, *target, *candidate_segment)
		})
}

fn is_non_shadowable_python_sdk_target(segments: &[Segment<'_>]) -> bool {
	if !segments
		.first()
		.is_some_and(|segment| segment.kind == kinds::SDK && segment.name == b"python")
	{
		return false;
	}
	let mut path = segments
		.iter()
		.skip(1)
		.filter(|segment| segment.kind == kinds::PATH);
	let Some(module) = path.next() else {
		return false;
	};
	match module.name {
		b"sys"
		| b"builtins"
		| b"_frozen_importlib"
		| b"_frozen_importlib_external"
		| b"zipimport" => true,
		b"importlib" => path
			.next()
			.is_some_and(|segment| matches!(segment.name, b"_bootstrap" | b"_bootstrap_external")),
		_ => false,
	}
}

// A Python package is imported by its bare name, but its definitions live in
// package:X/module:__init__ — collapse that pair to module:X so `import
// httpx` and `httpx.get(...)` line up with the __init__ reexports.
fn normalized_python_segments(segments: Vec<Segment<'_>>) -> Vec<Segment<'_>> {
	let mut normalized: Vec<Segment<'_>> = Vec::with_capacity(segments.len());
	let mut idx = 0;
	while idx < segments.len() {
		if segments[idx].kind == kinds::PACKAGE
			&& idx + 1 < segments.len()
			&& segments[idx + 1].kind == kinds::MODULE
			&& segments[idx + 1].name == b"__init__"
		{
			normalized.push(Segment {
				kind: kinds::MODULE,
				name: segments[idx].name,
			});
			idx += 2;
			continue;
		}
		normalized.push(segments[idx]);
		idx += 1;
	}
	normalized
}

fn python_segment_matches(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
	target: Segment<'_>,
	candidate_segment: Segment<'_>,
) -> bool {
	if target.kind == kinds::SDK
		&& target.name == b"python"
		&& candidate_segment.kind == kinds::LANG
		&& candidate_segment.name == b"python"
	{
		return true;
	}
	if target.kind == candidate_segment.kind {
		return python_segment_name_matches(query, candidate, target, candidate_segment);
	}
	if target.kind == kinds::PATH
		&& is_python_path_target_kind(candidate_segment.kind)
		&& target.name == candidate_segment.name
	{
		return true;
	}
	if target.kind == kinds::MODULE
		&& candidate_segment.kind == kinds::PACKAGE
		&& target.name == candidate_segment.name
	{
		return true;
	}
	if is_python_callable_kind(target.kind)
		&& matches!(candidate_segment.kind, kinds::PATH | kinds::CLASS)
		&& bare_callable_name(target.name) == candidate_segment.name
	{
		return true;
	}
	target.kind == kinds::LOCAL
		&& candidate_segment.kind == kinds::PARAM
		&& target.name == candidate_segment.name
}

fn python_segment_name_matches(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
	target: Segment<'_>,
	candidate_segment: Segment<'_>,
) -> bool {
	if is_python_callable_kind(target.kind) && is_python_callable_kind(candidate_segment.kind) {
		if query.call_name.is_none() {
			return bare_callable_name(target.name) == bare_callable_name(candidate_segment.name);
		}
		return query
			.call_name
			.is_some_and(|name| Some(name.as_bytes()) == candidate.call_name)
			&& python_call_arity_matches(query.call_arity, candidate.call_arity);
	}
	bare_callable_name(target.name) == bare_callable_name(candidate_segment.name)
}

// Python defaults and keyword arguments let a call site pass fewer
// arguments than the definition declares.
fn python_call_arity_matches(call: Option<usize>, def: Option<usize>) -> bool {
	match (call, def) {
		(Some(call), Some(def)) => call <= def,
		_ => call == def,
	}
}

fn is_python_path_target_kind(kind: &[u8]) -> bool {
	matches!(kind, kinds::CLASS | kinds::TYPE | kinds::MODULE) || is_python_callable_kind(kind)
}

fn is_python_callable_kind(kind: &[u8]) -> bool {
	matches!(
		kind,
		kinds::FUNCTION | kinds::ASYNC_FUNCTION | kinds::METHOD
	)
}

pub(super) fn classify_open_reference(
	material: &CodeIndexMaterial,
	decision: &mut crate::linkage::binding::ReferenceLinkageDecision,
	reference_idx: usize,
	reference: &ReferenceRecord,
) {
	let imported_external = reference.confidence.as_deref() == Some("imported")
		&& material
			.reference_target(&reference.id)
			.is_some_and(super::external_target_shape);
	let reason = if imported_external {
		Some(DynamicReason::ExternalDependencyUnindexed)
	} else {
		match reference.kind.as_str() {
			"method_call" => Some(match reference.receiver.as_deref() {
				Some("self" | "cls") if explicit_mixin_source(material, reference) => {
					DynamicReason::MixinContract
				}
				Some("member" | "subscript") => DynamicReason::DynamicAttribute,
				_ => DynamicReason::InsufficientLocalFacts,
			}),
			"reads" if reference.confidence.as_deref() == Some("unresolved") => {
				Some(DynamicReason::InsufficientLocalFacts)
			}
			"annotates" | "uses_type" if reference.confidence.as_deref() == Some("name_match") => {
				Some(DynamicReason::InsufficientLocalFacts)
			}
			_ => None,
		}
	};
	let Some(reason) = reason else { return };
	let candidates = decision
		.linkage_targets()
		.cloned()
		.unwrap_or_else(crate::linkage::catalog::SymbolSet::new);
	*decision = crate::linkage::binding::ReferenceLinkageDecision::dynamic(
		reason,
		reference_idx,
		reference.id,
		candidates,
	);
}

pub(super) fn classify_runtime_imports(
	material: &CodeIndexMaterial,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	changed_references: Option<&FxHashSet<ReferenceId>>,
	decision_indices: &[usize],
) {
	let mut local_import_candidates = FxHashMap::default();
	let mut module_import_candidates = FxHashMap::default();
	for &decision_idx in decision_indices {
		let decision = &decisions[decision_idx];
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
	for &decision_idx in decision_indices {
		let decision = &mut decisions[decision_idx];
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
			DynamicReason::RuntimeImport,
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

fn explicit_mixin_source(material: &CodeIndexMaterial, reference: &ReferenceRecord) -> bool {
	material
		.symbol_moniker(&reference.source_symbol)
		.and_then(enclosing_class)
		.and_then(|class| {
			class
				.as_view()
				.segments()
				.last()
				.map(|segment| segment.name.to_vec())
		})
		.is_some_and(|name| name.ends_with(b"Mixin"))
}

fn enclosing_class(
	source: &code_moniker_core::core::moniker::Moniker,
) -> Option<code_moniker_core::core::moniker::Moniker> {
	let mut current = Some(source.clone());
	while let Some(moniker) = current {
		if moniker
			.as_view()
			.segments()
			.last()
			.is_some_and(|segment| segment.kind == kinds::CLASS)
		{
			return Some(moniker);
		}
		current = moniker.parent();
	}
	None
}
