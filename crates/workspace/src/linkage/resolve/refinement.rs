use code_moniker_core::core::code_graph::RefRecord;
use code_moniker_core::core::kinds::{REF_CALLS, REF_INSTANTIATES, REF_METHOD_CALL, REF_READS};
use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::kinds;
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::linkage::binding::{
	ExternalOrigin, ReferenceLinkageDecision, ResolutionDecision, ResolutionScope,
};
use crate::linkage::catalog::CandidateCatalog;
use crate::linkage::catalog::LinkageQuery;
use crate::linkage::catalog::ReferenceLocations;
use crate::linkage::catalog::{SymbolOrdinal, SymbolSet};
use crate::linkage::language;
use crate::linkage::language::{
	CIncludeVisibility, PythonBindings, classify_c_preprocessor_tokens,
	classify_c_unindexed_external_dependencies, refine_c_include_visibility,
};
use crate::linkage::resolve::ManifestPolicy;
use crate::linkage::resolve::WorkspacePackageIndex;
use crate::linkage::resolve::refine_python_bindings;
use crate::linkage::source_groups::{LinkPermission, SourceGroupPolicy};
use crate::snapshot::{RecordTable, ReferenceId, ReferenceRecord, ResolutionEvidence};
use crate::source::CodeIndexMaterial;

mod receivers;

pub(in crate::linkage) use receivers::{
	MethodCallReference, MethodTable, ReceiverFieldTables, resolve_method_through_supers,
};
use receivers::{
	build_receiver_field_tables, pending_receiver_chains, refine_receiver_chains,
	refine_receiver_fields, refine_structural_receivers,
};

pub(in crate::linkage) struct LinkageRefiner<'a> {
	pub(in crate::linkage) material: &'a CodeIndexMaterial,
	methods: &'a MethodTable,
	pub(in crate::linkage) candidates: &'a CandidateCatalog,
	pub(in crate::linkage) locations: &'a ReferenceLocations,
	source_groups: &'a SourceGroupPolicy,
	packages: &'a WorkspacePackageIndex,
	manifests: &'a ManifestPolicy,
}

pub(in crate::linkage) struct RefinementPolicies<'a> {
	source_groups: &'a SourceGroupPolicy,
	packages: &'a WorkspacePackageIndex,
	manifests: &'a ManifestPolicy,
}

#[derive(Clone, Copy)]
pub(in crate::linkage) struct DecisionSelection<'a> {
	decision_indices: &'a [usize],
	changed_references: Option<&'a FxHashSet<ReferenceId>>,
}

impl<'a> DecisionSelection<'a> {
	fn new(
		decision_indices: &'a [usize],
		changed_references: Option<&'a FxHashSet<ReferenceId>>,
	) -> Self {
		Self {
			decision_indices,
			changed_references,
		}
	}

	pub(in crate::linkage) fn indices(self) -> &'a [usize] {
		self.decision_indices
	}

	pub(in crate::linkage) fn includes(self, reference: &ReferenceId) -> bool {
		self.changed_references
			.is_none_or(|changed| changed.contains(reference))
	}
}

impl<'a> RefinementPolicies<'a> {
	pub(in crate::linkage) fn new(
		source_groups: &'a SourceGroupPolicy,
		packages: &'a WorkspacePackageIndex,
		manifests: &'a ManifestPolicy,
	) -> Self {
		Self {
			source_groups,
			packages,
			manifests,
		}
	}
}

impl<'a> LinkageRefiner<'a> {
	pub(in crate::linkage) fn new(
		material: &'a CodeIndexMaterial,
		methods: &'a MethodTable,
		candidates: &'a CandidateCatalog,
		locations: &'a ReferenceLocations,
		policies: RefinementPolicies<'a>,
	) -> Self {
		Self {
			material,
			methods,
			candidates,
			locations,
			source_groups: policies.source_groups,
			packages: policies.packages,
			manifests: policies.manifests,
		}
	}

	pub(in crate::linkage) fn refine(
		&self,
		decisions: &mut [ReferenceLinkageDecision],
		references: &RecordTable<ReferenceRecord>,
	) {
		refine_decisions(self, decisions, references, None);
	}

	pub(in crate::linkage) fn refine_changed(
		&self,
		decisions: &mut [ReferenceLinkageDecision],
		references: &RecordTable<ReferenceRecord>,
		changed_references: &FxHashSet<ReferenceId>,
	) {
		refine_decisions(self, decisions, references, Some(changed_references));
	}
}

fn refine_decisions(
	linkage: &LinkageRefiner<'_>,
	decisions: &mut [ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	changed_references: Option<&FxHashSet<ReferenceId>>,
) {
	let (c_decisions, python_decisions, typescript_decisions) =
		partition_language_decisions(linkage, decisions);
	let c_selection = DecisionSelection::new(&c_decisions, changed_references);
	let python_selection = DecisionSelection::new(&python_decisions, changed_references);
	language::classify_runtime_imports(
		linkage.material,
		decisions,
		references,
		changed_references,
		&python_decisions,
	);
	language::refine_external_reexports(
		linkage.material,
		decisions,
		references,
		changed_references,
		&typescript_decisions,
	);
	let c_includes = (!c_decisions.is_empty()).then(|| CIncludeVisibility::build(linkage.material));
	if let Some(c_includes) = &c_includes {
		refine_c_include_visibility(linkage, c_includes, decisions, references, c_selection);
		classify_c_preprocessor_tokens(linkage, c_includes, decisions, references, c_selection);
	}
	let bindings = if python_decisions.is_empty() {
		None
	} else {
		let bootstrap = build_receiver_field_tables(linkage, decisions, references);
		let bindings = PythonBindings::build(
			linkage.material,
			linkage.candidates,
			decisions,
			references,
			&python_decisions,
		);
		refine_python_bindings(
			&bindings,
			linkage,
			&bootstrap,
			decisions,
			references,
			python_selection,
		);
		Some(bindings)
	};
	let tables = build_receiver_field_tables(linkage, decisions, references);
	refine_receiver_fields(linkage, &tables, decisions, references, changed_references);
	if let Some(bindings) = &bindings {
		refine_python_bindings(
			bindings,
			linkage,
			&tables,
			decisions,
			references,
			python_selection,
		);
	}
	let pending = pending_receiver_chains(decisions, references, changed_references);
	refine_receiver_chains(linkage, &tables, decisions, references, pending);
	refine_structural_receivers(linkage, decisions, references, changed_references);
	language::classify_open_references(linkage.material, decisions, references, changed_references);
	if let Some(c_includes) = &c_includes {
		classify_c_unindexed_external_dependencies(
			linkage,
			c_includes,
			decisions,
			references,
			c_selection,
		);
	}
}

fn partition_language_decisions(
	linkage: &LinkageRefiner<'_>,
	decisions: &[ReferenceLinkageDecision],
) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
	let mut c = Vec::new();
	let mut python = Vec::new();
	let mut typescript = Vec::new();
	for (decision_idx, decision) in decisions.iter().enumerate() {
		let Some(location) = linkage.locations.get(decision.reference_idx()) else {
			continue;
		};
		let Some(file) = linkage.material.files.get(location.source_file) else {
			continue;
		};
		match file.lang {
			code_moniker_core::lang::Lang::C => c.push(decision_idx),
			code_moniker_core::lang::Lang::Python => python.push(decision_idx),
			code_moniker_core::lang::Lang::Ts => typescript.push(decision_idx),
			_ => {}
		}
	}
	(c, python, typescript)
}
