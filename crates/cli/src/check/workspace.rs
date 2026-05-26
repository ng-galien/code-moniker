use std::collections::hash_map::Entry;
use std::path::PathBuf;

use code_moniker_core::lang::Lang;
use rustc_hash::FxHashMap;

use crate::check;
use code_moniker_workspace::snapshot::{CodeIndex, LinkageGraph, ResourceGeneration, SymbolId};
use code_moniker_workspace::source::{
	CodeIndexMaterial, LocalIdentityResolver, LocalResourceCache,
};

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
	cache: LocalResourceCache,
}

impl WorkspaceCheckRunner {
	pub fn new(options: WorkspaceCheckRunnerOptions, cache: LocalResourceCache) -> Self {
		Self { options, cache }
	}

	pub fn run_check(
		&mut self,
		index: &CodeIndex,
		_linkage: &LinkageGraph,
	) -> anyhow::Result<WorkspaceRuleDiagnostics> {
		let material = self
			.cache
			.index_material(index.generation)
			.ok_or_else(|| anyhow::anyhow!("code index material is unavailable"))?;
		let generation = self.cache.next_generation();
		let diagnostics = collect_diagnostics(&material, &self.options)?;
		Ok(WorkspaceRuleDiagnostics::with_diagnostics(
			generation,
			index.generation,
			diagnostics,
		))
	}
}

fn collect_diagnostics(
	material: &CodeIndexMaterial,
	options: &WorkspaceCheckRunnerOptions,
) -> anyhow::Result<Vec<WorkspaceRuleDiagnostic>> {
	let cfg = load_config(options)?;
	let excludes = check::UriExclusionMatcher::new(&cfg.exclude.uris);
	let identity = LocalIdentityResolver::new(options.scheme.clone());
	let symbol_by_identity = material
		.symbol_monikers
		.iter()
		.map(|(id, moniker)| (identity.moniker_uri(moniker), id.clone()))
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
	Ok(diagnostics)
}

fn load_config(options: &WorkspaceCheckRunnerOptions) -> anyhow::Result<check::Config> {
	let mut cfg = check::load_with_overrides(Some(&options.rules))
		.map_err(|err| anyhow::anyhow!(err.to_string()))?;
	if let Some(profile) = &options.profile {
		cfg.apply_profile(profile)
			.map_err(|err| anyhow::anyhow!(err.to_string()))?;
	}
	Ok(cfg)
}

fn diagnostic_from_violation(
	violation: check::Violation,
	symbol_by_identity: &std::collections::BTreeMap<String, SymbolId>,
) -> WorkspaceRuleDiagnostic {
	let severity = match violation.severity {
		check::RuleSeverity::Error => WorkspaceRuleDiagnosticSeverity::Error,
		check::RuleSeverity::Warn => WorkspaceRuleDiagnosticSeverity::Warn,
	};
	WorkspaceRuleDiagnostic::new(
		violation.rule_id,
		severity,
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
			.filter(|diagnostic| diagnostic.severity == WorkspaceRuleDiagnosticSeverity::Error)
			.count();
		let warnings = diagnostics
			.iter()
			.filter(|diagnostic| diagnostic.severity == WorkspaceRuleDiagnosticSeverity::Warn)
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceRuleDiagnosticSeverity {
	Error,
	Warn,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceRuleDiagnostic {
	pub rule_id: String,
	pub severity: WorkspaceRuleDiagnosticSeverity,
	pub symbol: Option<SymbolId>,
	pub message: String,
}

impl WorkspaceRuleDiagnostic {
	pub fn new(
		rule_id: impl Into<String>,
		severity: WorkspaceRuleDiagnosticSeverity,
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
