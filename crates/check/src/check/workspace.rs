use std::collections::hash_map::Entry;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
	state: Option<WorkspaceEvaluationState>,
}

impl WorkspaceCheckRunner {
	pub fn new(options: WorkspaceCheckRunnerOptions, cache: ResourceCache) -> Self {
		Self {
			options,
			cache,
			state: None,
		}
	}

	pub fn run_check(
		&mut self,
		index: &CodeIndex,
		_linkage: &LinkageSnapshot,
	) -> anyhow::Result<WorkspaceRuleDiagnostics> {
		let material = environment::cached_index_material(&self.cache, index.generation)
			.ok_or_else(|| anyhow::anyhow!("code index material is unavailable"))?;
		let generation = environment::next_resource_generation(&self.cache);
		let collected = collect_diagnostics(
			&material,
			index,
			&self.options,
			&self.cache,
			self.state.as_ref(),
		)?;
		self.state = Some(collected.state);
		let mut diagnostics = WorkspaceRuleDiagnostics::with_diagnostics(
			generation,
			index.generation,
			collected.diagnostics,
		);
		diagnostics.evaluation = collected.metrics;
		Ok(diagnostics)
	}
}

struct WorkspaceEvaluationState {
	index_generation: ResourceGeneration,
	fingerprint: String,
	inventory: Arc<code_moniker_workspace::snapshot::SymbolInventoryIndex>,
	universe: SymbolSet,
	evaluation: crate::check::workspace_eval::WorkspaceEvaluation,
}

struct CollectedDiagnostics {
	diagnostics: Vec<WorkspaceRuleDiagnostic>,
	metrics: WorkspaceEvaluationMetrics,
	state: WorkspaceEvaluationState,
}

fn collect_diagnostics(
	material: &IndexedSourceMaterial,
	index: &CodeIndex,
	options: &WorkspaceCheckRunnerOptions,
	cache: &ResourceCache,
	previous: Option<&WorkspaceEvaluationState>,
) -> anyhow::Result<CollectedDiagnostics> {
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
				.map(|violation| diagnostic_from_violation(violation, None, &symbol_by_identity)),
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
	let fingerprint = workspace_fingerprint(&workspace_rules, &cfg);
	let (workspace_evaluation, metrics) = evaluate_workspace_snapshot(
		index,
		&included_symbols,
		&workspace_rules,
		&fingerprint,
		cache,
		previous,
	);
	let mut workspace_by_source = std::collections::BTreeMap::<
		usize,
		Vec<crate::check::workspace_eval::WorkspaceSymbolViolation>,
	>::new();
	for violation in workspace_evaluation.violations.iter().cloned() {
		workspace_by_source
			.entry(violation.source.file())
			.or_default()
			.push(violation);
	}
	for (source, workspace_violations) in workspace_by_source {
		let Some(file) = material.files.get(source) else {
			continue;
		};
		let primary_symbols = workspace_violations
			.iter()
			.filter_map(|workspace_violation| {
				Some((
					(
						workspace_violation.violation.rule_id.clone(),
						workspace_violation.violation.moniker.clone(),
					),
					workspace_violation.symbol?,
				))
			})
			.collect::<std::collections::BTreeMap<_, _>>();
		let (suppressible, violations): (Vec<_>, Vec<_>) = workspace_violations
			.into_iter()
			.partition(|violation| violation.source_suppression);
		let mut violations = violations
			.into_iter()
			.map(|violation| violation.violation)
			.collect::<Vec<_>>();
		violations.extend(check::apply_suppressions(
			&file.graph,
			&file.source,
			suppressible
				.into_iter()
				.map(|violation| violation.violation)
				.collect(),
		));
		diagnostics.extend(violations.into_iter().map(|violation| {
			let primary = primary_symbols
				.get(&(violation.rule_id.clone(), violation.moniker.clone()))
				.copied();
			diagnostic_from_violation(violation, primary, &symbol_by_identity)
		}));
	}
	Ok(CollectedDiagnostics {
		diagnostics,
		metrics,
		state: WorkspaceEvaluationState {
			index_generation: index.generation,
			fingerprint,
			inventory: Arc::clone(&index.inventory),
			universe: included_symbols,
			evaluation: workspace_evaluation,
		},
	})
}

fn workspace_fingerprint(
	compiled: &crate::check::workspace_eval::CompiledWorkspaceRules,
	cfg: &check::Config,
) -> String {
	format!(
		"{:?}|workspace={:?}|exclude={:?}",
		compiled.specs(),
		cfg.workspace,
		cfg.exclude.uris
	)
}

fn evaluate_workspace_snapshot(
	index: &CodeIndex,
	universe: &SymbolSet,
	compiled: &crate::check::workspace_eval::CompiledWorkspaceRules,
	fingerprint: &str,
	cache: &ResourceCache,
	previous: Option<&WorkspaceEvaluationState>,
) -> (
	crate::check::workspace_eval::WorkspaceEvaluation,
	WorkspaceEvaluationMetrics,
) {
	let Some(previous) = previous.filter(|state| state.fingerprint == fingerprint) else {
		return evaluate_workspace_full(index, universe, compiled);
	};
	if previous.index_generation == index.generation {
		return (
			previous.evaluation.clone(),
			WorkspaceEvaluationMetrics {
				mode: WorkspaceEvaluationMode::Incremental,
				dirty_symbols: 0,
				evaluated_symbols: 0,
				affected_groups: 0,
			},
		);
	}
	let Some((diff_base, diff)) = environment::cached_index_diff(cache, index.generation) else {
		return evaluate_workspace_full(index, universe, compiled);
	};
	if diff_base != previous.index_generation {
		return evaluate_workspace_full(index, universe, compiled);
	}
	let incremental = crate::check::workspace_eval::evaluate_workspace_rules_incremental(
		crate::check::workspace_eval::WorkspaceIncrementalInput {
			previous_inventory: &previous.inventory,
			current_inventory: &index.inventory,
			previous_universe: &previous.universe,
			current_universe: universe,
			diff: &diff,
			compiled,
			previous: &previous.evaluation,
		},
	);
	(
		incremental.evaluation,
		WorkspaceEvaluationMetrics {
			mode: WorkspaceEvaluationMode::Incremental,
			dirty_symbols: incremental.dirty_symbols,
			evaluated_symbols: incremental.evaluated_symbols,
			affected_groups: incremental.affected_groups,
		},
	)
}

fn evaluate_workspace_full(
	index: &CodeIndex,
	universe: &SymbolSet,
	compiled: &crate::check::workspace_eval::CompiledWorkspaceRules,
) -> (
	crate::check::workspace_eval::WorkspaceEvaluation,
	WorkspaceEvaluationMetrics,
) {
	let evaluation = crate::check::workspace_eval::evaluate_workspace_rules_in(
		&index.inventory,
		universe,
		compiled,
		false,
	);
	let metrics = WorkspaceEvaluationMetrics {
		mode: WorkspaceEvaluationMode::Full,
		dirty_symbols: universe.len(),
		evaluated_symbols: universe.len(),
		affected_groups: evaluation.groups.len(),
	};
	(evaluation, metrics)
}

fn load_config(options: &WorkspaceCheckRunnerOptions) -> anyhow::Result<check::Config> {
	RuleSetRequest::with_rules(options.rules.clone(), options.scheme.clone())
		.with_profile(options.profile.clone())
		.load_config()
}

fn diagnostic_from_violation(
	violation: check::Violation,
	primary: Option<SymbolId>,
	symbol_by_identity: &std::collections::BTreeMap<String, SymbolId>,
) -> WorkspaceRuleDiagnostic {
	WorkspaceRuleDiagnostic::new(
		violation.rule_id,
		violation.severity,
		primary.or_else(|| symbol_by_identity.get(&violation.moniker).copied()),
		violation.message,
	)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceRuleDiagnostics {
	pub generation: ResourceGeneration,
	pub index_generation: ResourceGeneration,
	pub evaluation: WorkspaceEvaluationMetrics,
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
			evaluation: WorkspaceEvaluationMetrics::full(),
			errors,
			warnings,
			diagnostics,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceEvaluationMode {
	Full,
	Incremental,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceEvaluationMetrics {
	pub mode: WorkspaceEvaluationMode,
	pub dirty_symbols: usize,
	pub evaluated_symbols: usize,
	pub affected_groups: usize,
}

impl WorkspaceEvaluationMetrics {
	fn full() -> Self {
		Self {
			mode: WorkspaceEvaluationMode::Full,
			dirty_symbols: 0,
			evaluated_symbols: 0,
			affected_groups: 0,
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
