use std::collections::hash_map::Entry;
use std::path::{Path, PathBuf};

use code_moniker_core::lang::Lang;
use rustc_hash::FxHashMap;

use crate::check;
use code_moniker_workspace::environment::{
	self, IdentityResolver, IndexedSourceMaterial, ResourceCache,
};
use code_moniker_workspace::snapshot::{
	CodeIndex, LinkageSnapshot, ResourceGeneration, SymbolId, SymbolSet,
};

use crate::{RuleSetRequest, RuleSeverity};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceCheckRunnerOptions {
	pub rules: PathBuf,
	pub profile: Option<String>,
	pub scheme: String,
}

impl WorkspaceCheckRunnerOptions {
	pub fn new(rules: PathBuf, profile: Option<String>, scheme: impl Into<String>) -> Self {
		Self {
			rules,
			profile,
			scheme: scheme.into(),
		}
	}
}

pub struct WorkspaceCheckRunner {
	options: WorkspaceCheckRunnerOptions,
	cache: ResourceCache,
}

impl WorkspaceCheckRunner {
	pub fn new(options: WorkspaceCheckRunnerOptions, cache: ResourceCache) -> Self {
		Self { options, cache }
	}

	pub fn run_check(
		&mut self,
		index: &CodeIndex,
		_linkage: &LinkageSnapshot,
	) -> anyhow::Result<WorkspaceRuleDiagnostics> {
		let material = environment::cached_index_material(&self.cache, index.generation)
			.ok_or_else(|| anyhow::anyhow!("code index material is unavailable"))?;
		let generation = environment::next_resource_generation(&self.cache);
		let diagnostics = collect_diagnostics(&material, index, &self.options)?;
		Ok(WorkspaceRuleDiagnostics::with_diagnostics(
			generation,
			index.generation,
			diagnostics,
		))
	}
}

fn collect_diagnostics(
	material: &IndexedSourceMaterial,
	index: &CodeIndex,
	options: &WorkspaceCheckRunnerOptions,
) -> anyhow::Result<Vec<WorkspaceRuleDiagnostic>> {
	let cfg = load_config(options)?;
	let excludes = check::UriExclusionMatcher::new(&cfg.exclude.uris);
	let identity = IdentityResolver::new(options.scheme.clone());
	let symbol_by_identity = material
		.symbols()
		.map(|(id, moniker)| (identity.moniker_uri(moniker), id))
		.collect::<std::collections::BTreeMap<_, _>>();
	let mut compiled: FxHashMap<Lang, check::CompiledRules> = FxHashMap::default();
	let mut diagnostics = Vec::new();
	for file in material
		.files
		.iter()
		.filter(|file| !excludes.matches_path(&file.path))
	{
		let rules = match compiled.entry(file.lang) {
			Entry::Occupied(entry) => entry.into_mut(),
			Entry::Vacant(entry) => entry.insert(
				check::compile_rules(&cfg, file.lang, &options.scheme)
					.map_err(|err| anyhow::anyhow!(err.to_string()))?,
			),
		};
		let raw =
			check::evaluate_compiled(&file.graph, &file.source, file.lang, &options.scheme, rules);
		let violations = check::apply_suppressions(&file.graph, &file.source, raw);
		diagnostics.extend(
			violations
				.into_iter()
				.map(|violation| diagnostic_from_violation(violation, &symbol_by_identity)),
		);
	}
	let workspace_rules =
		crate::check::workspace_eval::compile_workspace_rules(&cfg, &options.scheme)?;
	let included_symbols = index
		.inventory
		.all_symbols()
		.iter()
		.filter(|ordinal| {
			index.inventory.record(*ordinal).is_some_and(|record| {
				!excludes.matches_path(Path::new(record.source_path.as_ref()))
			})
		})
		.collect::<SymbolSet>();
	let workspace_evaluation = crate::check::workspace_eval::evaluate_workspace_rules_in(
		&index.inventory,
		&included_symbols,
		&workspace_rules,
		false,
	);
	let mut workspace_by_source = std::collections::BTreeMap::<usize, Vec<check::Violation>>::new();
	for violation in workspace_evaluation.violations {
		workspace_by_source
			.entry(violation.source.file())
			.or_default()
			.push(violation.violation);
	}
	for (source, violations) in workspace_by_source {
		let Some(file) = material.files.get(source) else {
			continue;
		};
		let violations = check::apply_suppressions(&file.graph, &file.source, violations);
		diagnostics.extend(
			violations
				.into_iter()
				.map(|violation| diagnostic_from_violation(violation, &symbol_by_identity)),
		);
	}
	Ok(diagnostics)
}

fn load_config(options: &WorkspaceCheckRunnerOptions) -> anyhow::Result<check::Config> {
	RuleSetRequest::with_rules(options.rules.clone(), options.scheme.clone())
		.with_profile(options.profile.clone())
		.load_config()
}

fn diagnostic_from_violation(
	violation: check::Violation,
	symbol_by_identity: &std::collections::BTreeMap<String, SymbolId>,
) -> WorkspaceRuleDiagnostic {
	WorkspaceRuleDiagnostic::new(
		violation.rule_id,
		violation.severity,
		symbol_by_identity.get(&violation.moniker).cloned(),
		violation.message,
	)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceRuleDiagnostics {
	pub generation: ResourceGeneration,
	pub index_generation: ResourceGeneration,
	pub errors: usize,
	pub warnings: usize,
	pub diagnostics: Vec<WorkspaceRuleDiagnostic>,
}

impl WorkspaceRuleDiagnostics {
	pub fn with_diagnostics(
		generation: ResourceGeneration,
		index_generation: ResourceGeneration,
		diagnostics: Vec<WorkspaceRuleDiagnostic>,
	) -> Self {
		let errors = diagnostics
			.iter()
			.filter(|diagnostic| diagnostic.severity.is_error())
			.count();
		let warnings = diagnostics
			.iter()
			.filter(|diagnostic| diagnostic.severity.is_warn())
			.count();
		Self {
			generation,
			index_generation,
			errors,
			warnings,
			diagnostics,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceRuleDiagnostic {
	pub rule_id: String,
	pub severity: RuleSeverity,
	pub symbol: Option<SymbolId>,
	pub message: String,
}

impl WorkspaceRuleDiagnostic {
	pub fn new(
		rule_id: impl Into<String>,
		severity: RuleSeverity,
		symbol: Option<SymbolId>,
		message: impl Into<String>,
	) -> Self {
		Self {
			rule_id: rule_id.into(),
			severity,
			symbol,
			message: message.into(),
		}
	}
}
