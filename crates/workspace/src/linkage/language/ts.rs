use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::{build_manifest::Manifest, kinds};
use rustc_hash::FxHashSet;

use crate::linkage::catalog::LinkageCandidate;
use crate::linkage::catalog::LinkageQuery;
use crate::linkage::language::generic_matches;
use crate::snapshot::{RecordTable, ReferenceId, ReferenceRecord};
use crate::source::CodeIndexMaterial;

pub(super) fn matches(query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
	ts_family_matches(query, candidate) || external_package_symbol_match(query, candidate)
}

fn ts_family_matches(query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
	if generic_matches(query, candidate) {
		return true;
	}
	let query = normalize_language(query.target);
	let candidate = normalize_language(candidate.moniker);
	query.bind_match(&candidate) || candidate.bind_match(&query)
}

fn normalize_language(moniker: &Moniker) -> Moniker {
	let view = moniker.as_view();
	let mut builder = MonikerBuilder::new();
	builder.project(view.project());
	for segment in view.segments() {
		let name = if segment.kind == kinds::LANG
			&& matches!(segment.name, b"ts" | b"tsx" | b"js" | b"jsx")
		{
			b"ts".as_slice()
		} else {
			segment.name
		};
		builder.segment(segment.kind, name);
	}
	builder.build()
}

fn external_package_symbol_match(
	query: &LinkageQuery<'_>,
	candidate: &LinkageCandidate<'_>,
) -> bool {
	if query
		.target_first
		.is_none_or(|segment| segment.kind != kinds::EXTERNAL_PKG)
	{
		return false;
	}
	let Some(query_name) = query.call_name.map(str::as_bytes).or_else(|| {
		query
			.target_last
			.map(|segment| bare_callable_name(segment.name))
	}) else {
		return false;
	};
	let candidate_name = candidate.call_name.or_else(|| {
		candidate
			.last_segment
			.map(|segment| bare_callable_name(segment.name))
	});
	candidate_name == Some(query_name)
}

pub(super) fn package_prefix(target: &Moniker) -> Option<String> {
	let head = target.as_view().segments().next()?;
	if head.kind != kinds::EXTERNAL_PKG {
		return None;
	}
	std::str::from_utf8(head.name).ok().map(str::to_string)
}

pub(super) fn source_declares_external_package(
	manifest: Manifest,
	deps: &FxHashSet<String>,
	package_prefix: &str,
	_query_confidence: Option<&str>,
	_workspace_declares_package: impl Fn(&str) -> bool,
) -> bool {
	if manifest != Manifest::PackageJson {
		return false;
	}
	deps.contains(&format!("{}\0{package_prefix}", manifest.tag()))
}

pub(super) fn refine_external_reexports(
	material: &CodeIndexMaterial,
	decisions: &mut [crate::linkage::binding::ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	changed_references: Option<&FxHashSet<ReferenceId>>,
	decision_indices: &[usize],
) {
	type ExternalBinding = (crate::linkage::binding::ExternalOrigin, Moniker);
	let mut aliases = rustc_hash::FxHashMap::<(Moniker, Vec<u8>), ExternalBinding>::default();
	for &decision_idx in decision_indices {
		let decision = &decisions[decision_idx];
		let reference = &references[decision.reference_idx()];
		if reference.kind != "reexports" {
			continue;
		}
		let crate::linkage::binding::ReferenceLinkageDecision::External { origin, target, .. } =
			decision
		else {
			continue;
		};
		let Some(target) = target
			.clone()
			.or_else(|| material.reference_target(&reference.id).cloned())
		else {
			continue;
		};
		if let Some(key) = exported_binding_key(material, reference) {
			aliases.insert(key, (*origin, target));
		}
	}

	loop {
		let mut changed = false;
		for &decision_idx in decision_indices {
			let decision = &decisions[decision_idx];
			let reference = &references[decision.reference_idx()];
			if reference.kind != "reexports" {
				continue;
			}
			let Some(requested) = material.reference_target(&reference.id) else {
				continue;
			};
			let Some(target) = binding_key(requested)
				.and_then(|key| aliases.get(&key))
				.cloned()
			else {
				continue;
			};
			let Some(key) = exported_binding_key(material, reference) else {
				continue;
			};
			changed |= aliases.insert(key, target.clone()).as_ref() != Some(&target);
		}
		if !changed {
			break;
		}
	}

	for &decision_idx in decision_indices {
		let decision = &mut decisions[decision_idx];
		if changed_references.is_some_and(|changed| !changed.contains(decision.reference())) {
			continue;
		}
		let reference_idx = decision.reference_idx();
		let reference = &references[reference_idx];
		let Some(requested) = material.reference_target(&reference.id) else {
			continue;
		};
		let Some((origin, target)) = binding_key(requested).and_then(|key| aliases.get(&key))
		else {
			continue;
		};
		*decision = crate::linkage::binding::ReferenceLinkageDecision::external_target(
			*origin,
			reference_idx,
			reference.id,
			reexport_target(target, requested),
		);
	}
}

fn exported_binding_key(
	material: &CodeIndexMaterial,
	reference: &ReferenceRecord,
) -> Option<(Moniker, Vec<u8>)> {
	let owner = material.symbol_moniker(&reference.source_symbol)?.clone();
	let name = reference
		.alias
		.as_deref()
		.filter(|alias| !alias.is_empty())
		.map(|alias| alias.as_bytes().to_vec())
		.or_else(|| {
			material
				.reference_target(&reference.id)?
				.as_view()
				.segments()
				.last()
				.map(|segment| bare_callable_name(segment.name).to_vec())
		})?;
	Some((owner, name))
}

fn binding_key(target: &Moniker) -> Option<(Moniker, Vec<u8>)> {
	let segment = target.as_view().segments().last()?;
	Some((target.parent()?, bare_callable_name(segment.name).to_vec()))
}

fn reexport_target(alias: &Moniker, requested: &Moniker) -> Moniker {
	let Some(alias_last) = alias.as_view().segments().last() else {
		return alias.clone();
	};
	let Some(requested_last) = requested.as_view().segments().last() else {
		return alias.clone();
	};
	if bare_callable_name(alias_last.name) != bare_callable_name(requested_last.name) {
		return alias.clone();
	}
	let Some(owner) = alias.parent() else {
		return alias.clone();
	};
	MonikerBuilder::from_view(owner.as_view())
		.segment(requested_last.kind, requested_last.name)
		.build()
}
