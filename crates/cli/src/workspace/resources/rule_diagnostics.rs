use std::collections::hash_map::Entry;
use std::path::PathBuf;

use code_moniker_core::lang::Lang;
use rustc_hash::FxHashMap;

use crate::check;
use crate::workspace::resources::material::LocalResourceCache;
use crate::workspace::session::{
	CodeIndex, LinkageGraph, RuleDiagnostic, RuleDiagnosticSeverity, RuleDiagnostics,
	RuleDiagnosticsPort, SymbolId, WorkspaceFailure, WorkspaceResource, WorkspaceResult,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalRuleDiagnosticsOptions {
	pub rules: PathBuf,
	pub profile: Option<String>,
	pub scheme: String,
}

impl LocalRuleDiagnosticsOptions {
	pub fn new(rules: PathBuf, profile: Option<String>, scheme: impl Into<String>) -> Self {
		Self {
			rules,
			profile,
			scheme: scheme.into(),
		}
	}
}

pub struct LocalRuleDiagnostics {
	options: LocalRuleDiagnosticsOptions,
	cache: LocalResourceCache,
}

impl LocalRuleDiagnostics {
	pub fn new(options: LocalRuleDiagnosticsOptions, cache: LocalResourceCache) -> Self {
		Self { options, cache }
	}
}

impl RuleDiagnosticsPort for LocalRuleDiagnostics {
	fn collect_rule_diagnostics(
		&mut self,
		index: &CodeIndex,
		_linkage: &LinkageGraph,
	) -> WorkspaceResult<RuleDiagnostics> {
		let material = self
			.cache
			.index_material(index.generation)
			.ok_or_else(|| diagnostic_failure("code index material is unavailable".to_string()))?;
		let generation = self.cache.next_generation();
		let diagnostics = collect_diagnostics(index, &material, &self.options)?;
		Ok(RuleDiagnostics::with_diagnostics(
			generation,
			index.generation,
			diagnostics,
		))
	}
}

fn collect_diagnostics(
	index: &CodeIndex,
	material: &crate::workspace::resources::material::CodeIndexMaterial,
	options: &LocalRuleDiagnosticsOptions,
) -> WorkspaceResult<Vec<RuleDiagnostic>> {
	let cfg = load_config(options)?;
	let excludes = check::UriExclusionMatcher::new(&cfg.exclude.uris);
	let symbol_by_identity = index
		.symbols
		.iter()
		.map(|symbol| (symbol.identity.clone(), symbol.id.clone()))
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

fn load_config(options: &LocalRuleDiagnosticsOptions) -> WorkspaceResult<check::Config> {
	let mut cfg = check::load_with_overrides(Some(&options.rules))
		.map_err(|err| diagnostic_failure(err.to_string()))?;
	if let Some(profile) = &options.profile {
		cfg.apply_profile(profile)
			.map_err(|err| diagnostic_failure(err.to_string()))?;
	}
	Ok(cfg)
}

fn diagnostic_failure(message: String) -> WorkspaceFailure {
	WorkspaceFailure::new(WorkspaceResource::RuleDiagnostics, message)
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
