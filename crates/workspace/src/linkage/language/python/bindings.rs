// code-moniker: ignore-file[smell-feature-envy-local, smell-data-clumps-param-names, smell-god-type-local-metrics]
// The binding graph deliberately joins extractor facts, linkage decisions, and candidate ordinals;
// splitting those inputs would duplicate the merge invariants that keep ambiguous bindings non-unique.
use code_moniker_core::core::kinds::REF_REEXPORTS;
use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::kinds;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::linkage::binding::{
	ExternalOrigin, ReferenceLinkageDecision, ResolutionDecision, ResolutionScope,
};
use crate::linkage::catalog::{CandidateCatalog, SymbolSet};
use crate::linkage::resolve::{
	MethodCallReference, ReceiverFieldTables, SemanticLinkage, SemanticSelection,
	resolve_method_through_supers,
};
use crate::snapshot::{
	CandidateReason, DynamicReason, RecordTable, ReferenceRecord, ResolutionEvidence,
};
use crate::source::CodeIndexMaterial;

pub(in crate::linkage) struct PythonBindingGraph {
	aliases: FxHashMap<Moniker, FxHashMap<Vec<u8>, BindingTarget>>,
	export_policies: FxHashMap<Moniker, ExportPolicy>,
	wildcard_imports: Vec<WildcardImport>,
	dynamic_wildcard_owners: FxHashSet<Moniker>,
	pending_bindings: Vec<PendingBinding>,
}

impl PythonBindingGraph {
	pub(in crate::linkage) fn build(
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog,
		decisions: &[ReferenceLinkageDecision],
		references: &RecordTable<ReferenceRecord>,
		decision_indices: &[usize],
	) -> Self {
		let mut graph = Self {
			aliases: FxHashMap::default(),
			export_policies: FxHashMap::default(),
			wildcard_imports: Vec::new(),
			dynamic_wildcard_owners: FxHashSet::default(),
			pending_bindings: Vec::new(),
		};
		graph.seed_module_definitions(material, candidates);
		for &decision_idx in decision_indices {
			let decision = &decisions[decision_idx];
			graph.record_decision(material, decision, &references[decision.reference_idx()]);
		}
		graph.propagate_bindings();
		graph
	}

	pub(in crate::linkage) fn enhance(
		&self,
		linkage: &SemanticLinkage<'_>,
		tables: &ReceiverFieldTables,
		decisions: &mut [ReferenceLinkageDecision],
		references: &RecordTable<ReferenceRecord>,
		selection: SemanticSelection<'_>,
	) {
		PythonBindingResolver { graph: self }
			.enhance(linkage, tables, decisions, references, selection);
	}

	fn record_decision(
		&mut self,
		material: &CodeIndexMaterial,
		decision: &ReferenceLinkageDecision,
		reference: &ReferenceRecord,
	) {
		let Some(raw_owner) = material.symbol_moniker(&reference.source_symbol) else {
			return;
		};
		let is_reexport = reference.kind.as_bytes() == REF_REEXPORTS;
		let python_module = is_python_module(raw_owner);
		if !is_reexport && !python_module {
			return;
		}
		let owners = if python_module {
			binding_owners(raw_owner)
		} else {
			vec![raw_owner.clone()]
		};
		if let Some(directive) = all_directive(reference) {
			self.apply_export_directive(&owners, directive);
			return;
		}
		if python_module && is_wildcard_import(reference) {
			if reference.receiver.as_deref() == Some("python_conditional_import") {
				self.dynamic_wildcard_owners.extend(owners);
				return;
			}
			self.record_wildcard(material, decision, reference, owners);
			return;
		}
		if !is_reexport
			&& !matches!(
				reference.kind.as_bytes(),
				kinds::IMPORTS_SYMBOL | kinds::IMPORTS_MODULE
			) {
			return;
		}
		let Some(name) = binding_name(material, reference) else {
			return;
		};
		if is_reexport {
			self.record_explicit_export(&owners, &name);
		}
		let fallback = material.reference_target(&reference.id).cloned();
		let Some(mut target) = BindingTarget::from_decision(decision, fallback) else {
			if let Some((target_owner, target_name)) = Self::target_key(material, reference) {
				self.pending_bindings.push(PendingBinding {
					owners,
					name,
					target_owner,
					target_name,
					conditional: reference.receiver.as_deref() == Some("python_conditional_import"),
				});
			}
			return;
		};
		if reference.receiver.as_deref() == Some("python_conditional_import") {
			target = target.into_dynamic();
		}
		self.record_aliases(owners, name, target);
	}

	fn apply_export_directive(&mut self, owners: &[Moniker], directive: ExportDirective) {
		for owner in owners {
			match directive {
				ExportDirective::Replace => {
					self.export_policies
						.insert(owner.clone(), ExportPolicy::Static(FxHashSet::default()));
				}
				ExportDirective::Extend => {
					self.export_policies
						.entry(owner.clone())
						.or_insert(ExportPolicy::Dynamic);
				}
				ExportDirective::Dynamic => {
					self.export_policies
						.insert(owner.clone(), ExportPolicy::Dynamic);
				}
			}
		}
	}

	fn record_explicit_export(&mut self, owners: &[Moniker], name: &[u8]) {
		for owner in owners {
			let policy = self
				.export_policies
				.entry(owner.clone())
				.or_insert_with(|| ExportPolicy::Static(FxHashSet::default()));
			if let ExportPolicy::Static(names) = policy {
				names.insert(name.to_vec());
			}
		}
	}

	fn record_aliases(&mut self, owners: Vec<Moniker>, name: Vec<u8>, target: BindingTarget) {
		for owner in owners {
			self.merge_alias((owner, name.clone()), target.clone());
		}
	}

	fn seed_module_definitions(
		&mut self,
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog,
	) {
		for (file_idx, file) in material.files.iter().enumerate() {
			if file.lang != code_moniker_core::lang::Lang::Python {
				continue;
			}
			for (def_idx, def) in file.graph.defs().enumerate() {
				let Some(owner) = def.moniker.parent().filter(is_python_module) else {
					continue;
				};
				let Some(segment) = def.moniker.as_view().segments().last() else {
					continue;
				};
				let Some(symbol) = candidates.symbol_at(file_idx, def_idx) else {
					continue;
				};
				let target = BindingTarget::Workspace {
					scope: ResolutionScope::Global,
					evidence: ResolutionEvidence::ExactBinding,
					targets: SymbolSet::from_symbol(symbol),
					candidate_reason: None,
				};
				for owner in binding_owners(&owner) {
					self.merge_alias(
						(owner, bare_callable_name(segment.name).to_vec()),
						target.clone(),
					);
				}
			}
		}
	}

	pub(in crate::linkage) fn target_key(
		material: &CodeIndexMaterial,
		reference: &ReferenceRecord,
	) -> Option<(Moniker, Vec<u8>)> {
		let target = material.reference_target(&reference.id)?;
		let name = target
			.as_view()
			.segments()
			.last()
			.map(|segment| bare_callable_name(segment.name).to_vec())
			.or_else(|| {
				reference
					.call_name
					.as_deref()
					.map(|name| name.as_bytes().to_vec())
			})?;
		Some((target.parent()?, name))
	}

	fn alias(&self, owner: &Moniker, name: &[u8]) -> Option<&BindingTarget> {
		self.aliases.get(owner)?.get(name)
	}

	fn record_wildcard(
		&mut self,
		material: &CodeIndexMaterial,
		decision: &ReferenceLinkageDecision,
		reference: &ReferenceRecord,
		owners: Vec<Moniker>,
	) {
		let Some(target) = material.reference_target(&reference.id).cloned() else {
			return;
		};
		let external = match decision {
			ReferenceLinkageDecision::External { origin, .. } => Some(ExternalWildcard {
				origin: *origin,
				target: target.clone(),
			}),
			_ => None,
		};
		for owner in owners {
			self.wildcard_imports.push(WildcardImport {
				owner,
				target: target.clone(),
				external: external.clone(),
			});
		}
	}

	fn propagate_wildcards(&mut self) {
		loop {
			let mut changed = false;
			for wildcard in self.wildcard_imports.clone() {
				if wildcard.external.is_some() {
					continue;
				}
				if matches!(
					self.export_policies.get(&wildcard.target),
					Some(ExportPolicy::Dynamic)
				) || self.dynamic_wildcard_owners.contains(&wildcard.target)
				{
					changed |= self.dynamic_wildcard_owners.insert(wildcard.owner.clone());
					continue;
				}
				let exports = self.exported_aliases(&wildcard.target);
				for (name, target) in exports {
					changed |= self.merge_alias((wildcard.owner.clone(), name), target);
				}
			}
			if !changed {
				break;
			}
		}
	}

	fn propagate_bindings(&mut self) {
		let pending_bindings = std::mem::take(&mut self.pending_bindings);
		loop {
			self.propagate_wildcards();
			let mut changed = false;
			for pending in &pending_bindings {
				let Some(mut target) = self
					.alias(&pending.target_owner, &pending.target_name)
					.cloned()
				else {
					continue;
				};
				if pending.conditional {
					target = target.into_dynamic();
				}
				for owner in &pending.owners {
					changed |=
						self.merge_alias((owner.clone(), pending.name.clone()), target.clone());
				}
			}
			if !changed {
				break;
			}
		}
		self.pending_bindings = pending_bindings;
		self.propagate_wildcards();
	}

	fn exported_aliases(&self, owner: &Moniker) -> Vec<(Vec<u8>, BindingTarget)> {
		let policy = self.export_policies.get(owner);
		self.aliases
			.get(owner)
			.into_iter()
			.flat_map(|aliases| aliases.iter())
			.filter(|(name, _)| match policy {
				None => !name.starts_with(b"_"),
				Some(ExportPolicy::Static(names)) => names.contains(*name),
				Some(ExportPolicy::Dynamic) => false,
			})
			.map(|(name, target)| (name.clone(), target.clone()))
			.collect()
	}

	fn merge_alias(&mut self, key: (Moniker, Vec<u8>), target: BindingTarget) -> bool {
		let (owner, name) = key;
		let aliases = self.aliases.entry(owner).or_default();
		let Some(existing) = aliases.remove(&name) else {
			aliases.insert(name, target);
			return true;
		};
		let merged = existing.clone().merge(target);
		let changed = !merged.equivalent(&existing);
		aliases.insert(name, merged);
		changed
	}
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

struct PythonBindingResolver<'a> {
	graph: &'a PythonBindingGraph,
}

impl PythonBindingResolver<'_> {
	fn enhance(
		&self,
		linkage: &SemanticLinkage<'_>,
		tables: &ReceiverFieldTables,
		decisions: &mut [ReferenceLinkageDecision],
		references: &RecordTable<ReferenceRecord>,
		selection: SemanticSelection<'_>,
	) {
		for &decision_idx in selection.indices() {
			let decision = &mut decisions[decision_idx];
			let Some(reference_idx) = decision.semantic_pending_reference_idx() else {
				continue;
			};
			if !selection.includes(decision.reference()) {
				continue;
			}
			let reference = &references[reference_idx];
			if let Some(resolved) =
				self.resolve_reference(linkage, tables, reference_idx, reference)
			{
				*decision = resolved;
			}
		}
	}

	fn resolve_reference(
		&self,
		linkage: &SemanticLinkage<'_>,
		tables: &ReceiverFieldTables,
		reference_idx: usize,
		reference: &ReferenceRecord,
	) -> Option<ReferenceLinkageDecision> {
		let (raw_owner, name) = PythonBindingGraph::target_key(linkage.material, reference)?;
		let owner = tables
			.type_aliases
			.get(&raw_owner)
			.cloned()
			.unwrap_or_else(|| raw_owner.clone());
		let requested_target = linkage.material.reference_target(&reference.id);
		if let Some(resolved) = self.decision(
			&raw_owner,
			&name,
			reference_idx,
			reference,
			requested_target,
		) {
			return Some(resolved);
		}
		if owner != raw_owner
			&& let Some(resolved) =
				self.decision(&owner, &name, reference_idx, reference, requested_target)
		{
			return Some(resolved);
		}
		let bound_owner = self.canonical_workspace_owner(&owner, linkage.candidates)?;
		if let Some(resolved) = self.decision(
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
		&self,
		owner: &Moniker,
		name: &[u8],
		reference_idx: usize,
		reference: &ReferenceRecord,
		requested_target: Option<&Moniker>,
	) -> Option<ReferenceLinkageDecision> {
		if self.graph.dynamic_wildcard_owners.contains(owner) {
			let candidates = self
				.graph
				.alias(owner, name)
				.map_or_else(SymbolSet::new, BindingTarget::workspace_candidates);
			return Some(ReferenceLinkageDecision::dynamic(
				DynamicReason::RuntimeImport,
				reference_idx,
				reference.id,
				candidates,
			));
		}
		let external = self
			.graph
			.wildcard_imports
			.iter()
			.filter(|wildcard| &wildcard.owner == owner)
			.filter_map(|wildcard| wildcard.external.as_ref())
			.collect::<Vec<_>>();
		if let Some(target) = self.graph.alias(owner, name) {
			return Some(binding_decision(
				target,
				!external.is_empty(),
				reference_idx,
				reference,
				requested_target,
				name,
			));
		}
		if let Some(binding) = self.owner_binding(owner)
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
			[target] => Some(ReferenceLinkageDecision::external_target(
				target.origin,
				reference_idx,
				reference.id,
				external_wildcard_target(&target.target, requested_target, name),
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

	fn owner_binding(&self, owner: &Moniker) -> Option<&BindingTarget> {
		let segment = owner.as_view().segments().last()?;
		self.graph
			.alias(&owner.parent()?, bare_callable_name(segment.name))
	}

	fn canonical_workspace_owner(
		&self,
		owner: &Moniker,
		candidates: &CandidateCatalog,
	) -> Option<Moniker> {
		let binding = self.owner_binding(owner)?;
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
}

enum ExportPolicy {
	Static(FxHashSet<Vec<u8>>),
	Dynamic,
}

#[derive(Clone, Copy)]
enum ExportDirective {
	Replace,
	Extend,
	Dynamic,
}

fn all_directive(reference: &ReferenceRecord) -> Option<ExportDirective> {
	if reference.kind.as_bytes() != REF_REEXPORTS
		|| reference.alias.as_deref().is_some_and(|a| !a.is_empty())
	{
		return None;
	}
	match reference.receiver.as_deref()? {
		"python_all_replace" => Some(ExportDirective::Replace),
		"python_all_extend" => Some(ExportDirective::Extend),
		"python_all_dynamic" => Some(ExportDirective::Dynamic),
		_ => None,
	}
}

#[derive(Clone)]
struct WildcardImport {
	owner: Moniker,
	target: Moniker,
	external: Option<ExternalWildcard>,
}

#[derive(Clone)]
struct PendingBinding {
	owners: Vec<Moniker>,
	name: Vec<u8>,
	target_owner: Moniker,
	target_name: Vec<u8>,
	conditional: bool,
}

#[derive(Clone)]
struct ExternalWildcard {
	origin: ExternalOrigin,
	target: Moniker,
}

#[derive(Clone)]
enum BindingTarget {
	Workspace {
		scope: ResolutionScope,
		evidence: ResolutionEvidence,
		targets: SymbolSet,
		candidate_reason: Option<CandidateReason>,
	},
	External {
		origin: ExternalOrigin,
		target: Moniker,
	},
	Dynamic {
		candidates: SymbolSet,
	},
}

impl BindingTarget {
	fn into_dynamic(self) -> Self {
		match self {
			Self::Workspace { targets, .. }
			| Self::Dynamic {
				candidates: targets,
			} => Self::Dynamic {
				candidates: targets,
			},
			Self::External { .. } => Self::Dynamic {
				candidates: SymbolSet::new(),
			},
		}
	}

	fn workspace_candidates(&self) -> SymbolSet {
		match self {
			Self::Workspace { targets, .. } => targets.clone(),
			Self::Dynamic { candidates } => candidates.clone(),
			Self::External { .. } => SymbolSet::new(),
		}
	}

	fn equivalent(&self, other: &Self) -> bool {
		match (self, other) {
			(
				Self::Workspace {
					scope,
					evidence,
					targets,
					candidate_reason,
				},
				Self::Workspace {
					scope: other_scope,
					evidence: other_evidence,
					targets: other_targets,
					candidate_reason: other_reason,
				},
			) => {
				scope == other_scope
					&& evidence == other_evidence
					&& candidate_reason == other_reason
					&& targets == other_targets
			}
			(
				Self::External { origin, target },
				Self::External {
					origin: other_origin,
					target: other_target,
				},
			) => origin == other_origin && target == other_target,
			(
				Self::Dynamic { candidates },
				Self::Dynamic {
					candidates: other_candidates,
				},
			) => candidates == other_candidates,
			_ => false,
		}
	}

	fn from_decision(
		decision: &ReferenceLinkageDecision,
		fallback_external_target: Option<Moniker>,
	) -> Option<Self> {
		match decision {
			ReferenceLinkageDecision::Unique { resolution } => {
				Some(Self::from_workspace_resolution(resolution, None))
			}
			ReferenceLinkageDecision::Candidate { reason, resolution } => {
				Some(Self::from_workspace_resolution(resolution, Some(*reason)))
			}
			ReferenceLinkageDecision::External { origin, target, .. } => {
				Self::from_external_resolution(*origin, target, fallback_external_target)
			}
			ReferenceLinkageDecision::Dynamic { candidates, .. } => Some(Self::Dynamic {
				candidates: candidates.clone(),
			}),
			_ => None,
		}
	}

	fn from_workspace_resolution(
		resolution: &ResolutionDecision,
		candidate_reason: Option<CandidateReason>,
	) -> Self {
		Self::Workspace {
			scope: resolution.scope,
			evidence: resolution.evidence,
			targets: resolution.targets.clone(),
			candidate_reason,
		}
	}

	fn from_external_resolution(
		origin: ExternalOrigin,
		target: &Option<Moniker>,
		fallback: Option<Moniker>,
	) -> Option<Self> {
		Some(Self::External {
			origin,
			target: target.clone().or(fallback)?,
		})
	}

	fn merge(self, other: Self) -> Self {
		match (self, other) {
			(
				Self::Workspace {
					scope,
					evidence,
					targets,
					candidate_reason,
				},
				Self::Workspace {
					scope: other_scope,
					evidence: other_evidence,
					targets: other_targets,
					candidate_reason: other_reason,
				},
			) => {
				let mut merged = targets;
				for target in other_targets.iter() {
					merged.insert(target);
				}
				let candidate_reason = candidate_reason
					.or(other_reason)
					.or_else(|| (merged.len() > 1).then_some(CandidateReason::MultipleTargets));
				Self::Workspace {
					scope: if scope == other_scope {
						scope
					} else {
						ResolutionScope::Global
					},
					evidence: if evidence == other_evidence {
						evidence
					} else {
						ResolutionEvidence::GlobalBinding
					},
					targets: merged,
					candidate_reason,
				}
			}
			(Self::External { origin, target }, Self::External { target: other, .. })
				if target == other =>
			{
				Self::External { origin, target }
			}
			(Self::Dynamic { candidates }, Self::Workspace { targets, .. }) => Self::Dynamic {
				candidates: merged_targets(&candidates, &targets),
			},
			(Self::Workspace { targets, .. }, Self::Dynamic { candidates }) => Self::Dynamic {
				candidates: merged_targets(&targets, &candidates),
			},
			(Self::Dynamic { candidates }, Self::Dynamic { candidates: other }) => Self::Dynamic {
				candidates: merged_targets(&candidates, &other),
			},
			(Self::Workspace { targets, .. }, Self::External { .. }) => Self::Dynamic {
				candidates: targets,
			},
			(Self::External { .. }, Self::Workspace { targets, .. }) => Self::Dynamic {
				candidates: targets,
			},
			(Self::Dynamic { candidates }, Self::External { .. }) => Self::Dynamic { candidates },
			(Self::External { .. }, Self::Dynamic { candidates }) => Self::Dynamic { candidates },
			(Self::External { .. }, Self::External { .. }) => Self::Dynamic {
				candidates: SymbolSet::new(),
			},
		}
	}

	fn to_decision(
		&self,
		reference_idx: usize,
		reference: &ReferenceRecord,
		requested_target: Option<&Moniker>,
	) -> ReferenceLinkageDecision {
		match self {
			Self::Workspace {
				scope,
				evidence,
				targets,
				candidate_reason,
			} => {
				let resolution = ResolutionDecision::new(
					*scope,
					*evidence,
					reference.id,
					reference_idx,
					targets.clone(),
				);
				if targets.len() == 1 && candidate_reason.is_none() {
					ReferenceLinkageDecision::resolved(resolution)
				} else {
					ReferenceLinkageDecision::candidate(
						candidate_reason.unwrap_or(CandidateReason::MultipleTargets),
						resolution,
					)
				}
			}
			Self::External { origin, target } => ReferenceLinkageDecision::external_target(
				*origin,
				reference_idx,
				reference.id,
				reexport_external_target(target, requested_target),
			),
			Self::Dynamic { candidates } => ReferenceLinkageDecision::dynamic(
				DynamicReason::RuntimeImport,
				reference_idx,
				reference.id,
				candidates.clone(),
			),
		}
	}
}

fn merged_targets(left: &SymbolSet, right: &SymbolSet) -> SymbolSet {
	let mut merged = left.clone();
	for target in right.iter() {
		merged.insert(target);
	}
	merged
}

fn is_python_module(owner: &Moniker) -> bool {
	let segments = owner.as_view().segments().collect::<Vec<_>>();
	segments
		.first()
		.is_some_and(|segment| segment.kind == kinds::LANG && segment.name == b"python")
		&& segments
			.last()
			.is_some_and(|segment| segment.kind == kinds::MODULE)
}

fn binding_owners(owner: &Moniker) -> Vec<Moniker> {
	let mut owners = vec![owner.clone()];
	if let Some(collapsed) = collapsed_init_owner(owner) {
		if !owners.contains(&collapsed) {
			owners.push(collapsed);
		}
	}
	if let Some(folded) = folded_init_import_owner(owner) {
		if !owners.contains(&folded) {
			owners.push(folded);
		}
	}
	owners
}

fn collapsed_init_owner(owner: &Moniker) -> Option<Moniker> {
	let segments = owner.as_view().segments().collect::<Vec<_>>();
	let [first, .., package, module] = segments.as_slice() else {
		return None;
	};
	if first.kind != kinds::LANG
		|| first.name != b"python"
		|| module.kind != kinds::MODULE
		|| module.name != b"__init__"
		|| package.kind != kinds::PACKAGE
	{
		return None;
	}
	let package_name = package.name.to_vec();
	let prefix = owner.parent()?.parent()?;
	Some(
		MonikerBuilder::from_view(prefix.as_view())
			.segment(kinds::MODULE, &package_name)
			.build(),
	)
}

fn folded_init_import_owner(owner: &Moniker) -> Option<Moniker> {
	let view = owner.as_view();
	let segments = view.segments().collect::<Vec<_>>();
	let [lang, packages @ .., module] = segments.as_slice() else {
		return None;
	};
	if lang.kind != kinds::LANG
		|| lang.name != b"python"
		|| packages.is_empty()
		|| packages
			.iter()
			.any(|segment| segment.kind != kinds::PACKAGE)
		|| module.kind != kinds::MODULE
		|| module.name != b"__init__"
	{
		return None;
	}
	let mut builder = MonikerBuilder::new();
	builder
		.project(view.project())
		.segment(lang.kind, lang.name);
	if packages.len() == 1 {
		builder.segment(kinds::MODULE, packages[0].name);
	} else {
		builder
			.segment(kinds::PACKAGE, packages[0].name)
			.segment(kinds::MODULE, packages[1].name);
		for package in &packages[2..] {
			builder.segment(kinds::PATH, package.name);
		}
	}
	Some(builder.build())
}

fn is_wildcard_import(reference: &ReferenceRecord) -> bool {
	reference.kind.as_bytes() == kinds::IMPORTS_MODULE && reference.alias.as_deref() == Some("*")
}

fn binding_name(material: &CodeIndexMaterial, reference: &ReferenceRecord) -> Option<Vec<u8>> {
	if let Some(alias) = reference.alias.as_deref().filter(|alias| !alias.is_empty()) {
		return Some(alias.as_bytes().to_vec());
	}
	let target = material.reference_target(&reference.id)?;
	if reference.kind.as_bytes() == kinds::IMPORTS_MODULE {
		let mut segments = target.as_view().segments();
		let first = segments.next()?;
		let binding = if matches!(first.kind, kinds::LANG | kinds::SDK) {
			segments.next()?
		} else {
			first
		};
		return Some(bare_callable_name(binding.name).to_vec());
	}
	let last = target.as_view().segments().last()?;
	(last.kind == kinds::PATH).then(|| bare_callable_name(last.name).to_vec())
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
		let graph = PythonBindingGraph {
			aliases: FxHashMap::default(),
			export_policies: FxHashMap::default(),
			wildcard_imports: vec![WildcardImport {
				owner: owner.clone(),
				target: external.clone(),
				external: Some(ExternalWildcard {
					origin: ExternalOrigin::Injected,
					target: external,
				}),
			}],
			dynamic_wildcard_owners: FxHashSet::default(),
			pending_bindings: Vec::new(),
		};
		let reference = ReferenceRecord::new(
			ReferenceId::at(0, 0),
			SourceId::at(0),
			SymbolId::at(0, 0),
			"code+moniker://./lang:python/module:facade/function:Client",
			"calls",
			None,
		);

		let decision = PythonBindingResolver { graph: &graph }
			.decision(&owner, b"Client", 0, &reference, None)
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
