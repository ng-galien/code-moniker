use std::collections::hash_map::Entry;
use std::path::PathBuf;

use code_moniker_core::lang::Lang;
use rustc_hash::FxHashMap;

use crate::check;
use crate::workspace::resources::identity::LocalIdentityResolver;
use crate::workspace::resources::material::LocalResourceCache;
use crate::workspace::session::{
	CodeIndex, LinkageGraph, RuleDiagnostic, RuleDiagnosticSeverity, RuleDiagnostics, SymbolId,
	WorkspaceFailure, WorkspaceResource, WorkspaceResult,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalCheckRunnerOptions {
	pub rules: PathBuf,
	pub profile: Option<String>,
	pub scheme: String,
}

impl LocalCheckRunnerOptions {
	pub fn new(rules: PathBuf, profile: Option<String>, scheme: impl Into<String>) -> Self {
		Self {
			rules,
			profile,
			scheme: scheme.into(),
		}
	}
}

pub struct LocalCheckRunner {
	options: LocalCheckRunnerOptions,
	cache: LocalResourceCache,
}

impl LocalCheckRunner {
	pub fn new(options: LocalCheckRunnerOptions, cache: LocalResourceCache) -> Self {
		Self { options, cache }
	}

	pub fn run_check(
		&mut self,
		index: &CodeIndex,
		_linkage: &LinkageGraph,
	) -> WorkspaceResult<RuleDiagnostics> {
		let material = self
			.cache
			.index_material(index.generation)
			.ok_or_else(|| diagnostic_failure("code index material is unavailable".to_string()))?;
		let generation = self.cache.next_generation();
		let diagnostics = collect_diagnostics(&material, &self.options)?;
		Ok(RuleDiagnostics::with_diagnostics(
			generation,
			index.generation,
			diagnostics,
		))
	}
}

fn collect_diagnostics(
	material: &crate::workspace::resources::material::CodeIndexMaterial,
	options: &LocalCheckRunnerOptions,
) -> WorkspaceResult<Vec<RuleDiagnostic>> {
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
					.map_err(|err| diagnostic_failure(err.to_string()))?,
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

fn load_config(options: &LocalCheckRunnerOptions) -> WorkspaceResult<check::Config> {
	let mut cfg = check::load_with_overrides(Some(&options.rules))
		.map_err(|err| diagnostic_failure(err.to_string()))?;
	if let Some(profile) = &options.profile {
		cfg.apply_profile(profile)
			.map_err(|err| diagnostic_failure(err.to_string()))?;
	}
	Ok(cfg)
}

fn diagnostic_failure(message: String) -> WorkspaceFailure {
	WorkspaceFailure::new(WorkspaceResource::RuleCheck, message)
}

fn diagnostic_from_violation(
	violation: check::Violation,
	symbol_by_identity: &std::collections::BTreeMap<String, SymbolId>,
) -> RuleDiagnostic {
	let severity = match violation.severity {
		check::RuleSeverity::Error => RuleDiagnosticSeverity::Error,
		check::RuleSeverity::Warn => RuleDiagnosticSeverity::Warn,
	};
	RuleDiagnostic::new(
		violation.rule_id,
		severity,
		symbol_by_identity.get(&violation.moniker).cloned(),
		violation.message,
	)
}
