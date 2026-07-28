use std::collections::{BTreeSet, HashMap};

use code_moniker_workspace::snapshot::{
	BoundedPathCoverage, BoundedPathEdge, BoundedPathEngine, BoundedPathLimits, BoundedPathRequest,
	BoundedPathScope, CodeIndex, LinkageSnapshot, SymbolOrdinal, SymbolSet,
};
use rustc_hash::FxHashMap;

use crate::check::config::{Config, ConfigError, WorkspacePathExpectation, WorkspacePathRuleEntry};
use crate::check::eval::{
	CompiledRuleSpec, RuleCoverage, RulePathReport, RulePathStep, RuleReport, RuleVerdict,
	Violation,
};
use crate::check::expr::{self, Node};

use super::{
	CompiledWorkspaceRules, WorkspaceEvaluation, WorkspaceSymbolViolation, classify_t1,
	render_template,
};

#[derive(Debug)]
pub(super) struct CompiledWorkspacePathRule {
	rule_id: String,
	from_raw: String,
	to_raw: String,
	from_expanded: String,
	to_expanded: String,
	from: Node,
	to: Node,
	expect: WorkspacePathExpectation,
	relation: Vec<String>,
	limits: BoundedPathLimits,
	max_pairs: usize,
	min_coverage: usize,
	severity: crate::check::config::RuleSeverity,
	message: Option<String>,
	rationale: Option<String>,
	capabilities: Vec<String>,
}

pub(super) fn compile_paths(
	cfg: &Config,
	scheme: &str,
	allowed_kinds: &[&str],
	aliases: &HashMap<String, String>,
) -> Result<Vec<CompiledWorkspacePathRule>, ConfigError> {
	cfg.workspace
		.path
		.iter()
		.enumerate()
		.map(|(index, entry)| {
			compile_path(
				entry,
				index,
				scheme,
				allowed_kinds,
				aliases,
				cfg.workspace.min_linkage_coverage.unwrap_or(100),
			)
		})
		.collect()
}

fn compile_path(
	entry: &WorkspacePathRuleEntry,
	index: usize,
	scheme: &str,
	allowed_kinds: &[&str],
	aliases: &HashMap<String, String>,
	default_coverage: usize,
) -> Result<CompiledWorkspacePathRule, ConfigError> {
	let rule_id = format!("workspace.path.{}", entry.fallback_id(index));
	let (from, from_capabilities, from_expanded) = compile_selector(
		&entry.from,
		&format!("{rule_id}.from"),
		scheme,
		allowed_kinds,
		aliases,
	)?;
	let (to, to_capabilities, to_expanded) = compile_selector(
		&entry.to,
		&format!("{rule_id}.to"),
		scheme,
		allowed_kinds,
		aliases,
	)?;
	let capabilities = from_capabilities
		.into_iter()
		.chain(to_capabilities)
		.chain(std::iter::once("graph.path".to_string()))
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect();
	Ok(CompiledWorkspacePathRule {
		rule_id,
		from_raw: entry.from.to_owned(),
		to_raw: entry.to.to_owned(),
		from_expanded,
		to_expanded,
		from,
		to,
		expect: entry.expect,
		relation: entry.relation.to_owned(),
		limits: BoundedPathLimits {
			max_depth: entry.max_depth,
			max_symbols: entry.max_symbols,
			max_edges: entry.max_edges,
		},
		max_pairs: entry.max_pairs,
		min_coverage: entry.min_coverage.unwrap_or(default_coverage),
		severity: entry.severity,
		message: entry.message.to_owned(),
		rationale: entry.rationale.to_owned(),
		capabilities,
	})
}

fn compile_selector(
	raw: &str,
	at: &str,
	scheme: &str,
	allowed_kinds: &[&str],
	aliases: &HashMap<String, String>,
) -> Result<(Node, Vec<String>, String), ConfigError> {
	let expanded = crate::check::config::substitute_aliases(raw, aliases, at)?;
	let parsed = expr::parse(&expanded, scheme, allowed_kinds).map_err(|error| {
		ConfigError::InvalidExpr {
			at: at.to_string(),
			error,
		}
	})?;
	let capabilities = classify_t1(&parsed.root, at)?;
	Ok((parsed.root, capabilities, expanded))
}

pub(super) fn append_path_specs(
	compiled: &CompiledWorkspaceRules,
	specs: &mut Vec<CompiledRuleSpec>,
) {
	specs.extend(compiled.path.iter().map(|rule| CompiledRuleSpec {
		rule_id: rule.rule_id.to_owned(),
		severity: rule.severity,
		lang: "workspace".to_string(),
		root: "workspace".to_string(),
		subject: "path".to_string(),
		plan: "t2_linkage".to_string(),
		capabilities: rule.capabilities.to_owned(),
		group_by: Vec::new(),
		domain: "workspace paths".to_string(),
		kind: None,
		expr: format!(
			"{}({} -> {})",
			expectation_name(rule.expect),
			rule.from_raw,
			rule.to_raw
		),
		expanded_expr: format!(
			"{}({} -> {})",
			expectation_name(rule.expect),
			rule.from_expanded,
			rule.to_expanded
		),
		message: rule.message.to_owned(),
		rationale: rule.rationale.to_owned(),
		require_doc_comment: None,
	}));
}

pub(super) fn evaluate_path_rules(
	index: &CodeIndex,
	linkage: &LinkageSnapshot,
	universe: &SymbolSet,
	compiled: &CompiledWorkspaceRules,
	report: bool,
	evaluation: &mut WorkspaceEvaluation,
) {
	let Some(engine) = prepare_path_engine(!compiled.path.is_empty(), || {
		BoundedPathEngine::new(index, linkage)
	}) else {
		return;
	};
	let scope = BoundedPathScope::from_sources(
		universe
			.iter()
			.filter_map(|ordinal| index.inventory.record(ordinal).map(|record| record.source)),
	);
	let mut atom_cache = FxHashMap::default();
	for rule in &compiled.path {
		let from = super::eval_node(&rule.from, &index.inventory, universe, &mut atom_cache);
		let to = super::eval_node(&rule.to, &index.inventory, universe, &mut atom_cache);
		let outcome = evaluate_path_rule(index, engine.as_ref(), &scope, rule, &from, &to);
		let anchor = path_anchor(index, &outcome, &from);
		let witness = witness_steps(index, &outcome.witness);
		let mut violation_set = SymbolSet::new();
		if outcome.verdict == Some(RuleVerdict::Fail)
			&& let Some(anchor) = anchor
		{
			violation_set.insert(anchor);
			append_violation(evaluation, index, rule, anchor, &witness);
		}
		evaluation
			.violation_sets
			.insert(rule.rule_id.clone(), violation_set);
		if report {
			evaluation
				.reports
				.push(path_report(rule, from.len(), to.len(), outcome, witness));
		}
	}
}

fn prepare_path_engine<T>(has_path_rules: bool, prepare: impl FnOnce() -> T) -> Option<T> {
	has_path_rules.then(prepare)
}

fn evaluate_path_rule(
	index: &CodeIndex,
	engine: Option<&BoundedPathEngine<'_>>,
	scope: &BoundedPathScope,
	rule: &CompiledWorkspacePathRule,
	from: &SymbolSet,
	to: &SymbolSet,
) -> PathOutcome {
	if from.is_empty() || to.is_empty() {
		let reason = if from.is_empty() {
			"empty_source_selector"
		} else {
			"empty_target_selector"
		};
		return PathOutcome::inconclusive(reason);
	}
	let Some(engine) = engine else {
		return PathOutcome::inconclusive("path_index_unavailable");
	};
	let mut outcome = PathOutcome::default();
	let total_pairs = from.len().saturating_mul(to.len());
	for from_ordinal in from.iter() {
		for to_ordinal in to.iter() {
			if outcome.evaluated_pairs >= rule.max_pairs {
				outcome.pair_limit_reached = true;
				outcome.reasons.insert("pair_limit".to_string());
				return outcome.finish(rule.expect, total_pairs);
			}
			if outcome.explored_symbols >= rule.limits.max_symbols {
				outcome.symbol_limit_reached = true;
				outcome.reasons.insert("symbol_limit".to_string());
				return outcome.finish(rule.expect, total_pairs);
			}
			if outcome.coverage.total >= rule.limits.max_edges {
				outcome.edge_limit_reached = true;
				outcome.reasons.insert("edge_limit".to_string());
				return outcome.finish(rule.expect, total_pairs);
			}
			let Some(from_record) = index.inventory.record(from_ordinal) else {
				outcome.reasons.insert("source_symbol_missing".to_string());
				outcome.incomplete = true;
				continue;
			};
			let Some(to_record) = index.inventory.record(to_ordinal) else {
				outcome.reasons.insert("target_symbol_missing".to_string());
				outcome.incomplete = true;
				continue;
			};
			outcome.evaluated_pairs += 1;
			let limits = BoundedPathLimits {
				max_depth: rule.limits.max_depth,
				max_symbols: rule.limits.max_symbols - outcome.explored_symbols,
				max_edges: rule.limits.max_edges - outcome.coverage.total,
			};
			let Some(search) = engine.search(BoundedPathRequest {
				from: from_record.id,
				to: to_record.id,
				relations: &rule.relation,
				limits,
				scope,
			}) else {
				outcome.reasons.insert("path_index_unavailable".to_string());
				outcome.incomplete = true;
				continue;
			};
			let found = from_record.id == to_record.id || !search.path.is_empty();
			let complete = search_complete(&search, rule.min_coverage);
			if search.coverage.percent() < rule.min_coverage {
				outcome
					.reasons
					.insert("coverage_below_threshold".to_string());
			}
			outcome.absorb(search);
			if found {
				outcome.found = true;
				return outcome.finish(rule.expect, total_pairs);
			}
			if !complete {
				outcome.incomplete = true;
			}
		}
	}
	outcome.finish(rule.expect, total_pairs)
}

fn search_complete(
	search: &code_moniker_workspace::snapshot::BoundedPathSearch,
	min_coverage: usize,
) -> bool {
	!search.depth_limit_reached
		&& !search.symbol_limit_reached
		&& !search.edge_limit_reached
		&& search.coverage.percent() >= min_coverage
}

#[derive(Default)]
struct PathOutcome {
	verdict: Option<RuleVerdict>,
	found: bool,
	incomplete: bool,
	evaluated_pairs: usize,
	coverage: BoundedPathCoverage,
	explored_symbols: usize,
	explored_edges: usize,
	depth_limit_reached: bool,
	symbol_limit_reached: bool,
	edge_limit_reached: bool,
	pair_limit_reached: bool,
	reasons: BTreeSet<String>,
	witness: Vec<BoundedPathEdge>,
}

impl PathOutcome {
	fn inconclusive(reason: &str) -> Self {
		Self {
			verdict: Some(RuleVerdict::Inconclusive),
			incomplete: true,
			reasons: std::iter::once(reason.to_string()).collect(),
			..Self::default()
		}
	}

	fn absorb(&mut self, search: code_moniker_workspace::snapshot::BoundedPathSearch) {
		self.explored_symbols += search.explored_symbols;
		self.explored_edges += search.explored_edges;
		self.depth_limit_reached |= search.depth_limit_reached;
		self.symbol_limit_reached |= search.symbol_limit_reached;
		self.edge_limit_reached |= search.edge_limit_reached;
		if search.depth_limit_reached {
			self.reasons.insert("depth_limit".to_string());
		}
		if search.symbol_limit_reached {
			self.reasons.insert("symbol_limit".to_string());
		}
		if search.edge_limit_reached {
			self.reasons.insert("edge_limit".to_string());
		}
		for (reason, count) in &search.coverage.gap_reasons {
			self.reasons.insert(format!("{reason}:{count}"));
		}
		if self.witness.is_empty() && !search.path.is_empty() {
			self.witness = search.path;
		}
		add_coverage(&mut self.coverage, search.coverage);
	}

	fn finish(mut self, expect: WorkspacePathExpectation, total_pairs: usize) -> Self {
		if self.evaluated_pairs < total_pairs && !self.found {
			self.incomplete = true;
		}
		self.verdict = Some(if self.found {
			match expect {
				WorkspacePathExpectation::Reachable => RuleVerdict::Pass,
				WorkspacePathExpectation::NoPath => RuleVerdict::Fail,
			}
		} else if self.incomplete {
			RuleVerdict::Inconclusive
		} else {
			match expect {
				WorkspacePathExpectation::Reachable => RuleVerdict::Fail,
				WorkspacePathExpectation::NoPath => RuleVerdict::Pass,
			}
		});
		self
	}
}

fn add_coverage(total: &mut BoundedPathCoverage, next: BoundedPathCoverage) {
	total.total += next.total;
	total.decided += next.decided;
	total.resolved += next.resolved;
	total.external += next.external;
	total.candidate += next.candidate;
	total.dynamic += next.dynamic;
	total.manifest_blocked += next.manifest_blocked;
	total.unresolved += next.unresolved;
	for (reason, count) in next.gap_reasons {
		*total.gap_reasons.entry(reason).or_default() += count;
	}
}

fn path_anchor(
	index: &CodeIndex,
	outcome: &PathOutcome,
	from: &SymbolSet,
) -> Option<SymbolOrdinal> {
	outcome
		.witness
		.first()
		.and_then(|edge| index.inventory.catalog().ordinal(&edge.source))
		.or_else(|| from.iter().next())
}

fn append_violation(
	evaluation: &mut WorkspaceEvaluation,
	index: &CodeIndex,
	rule: &CompiledWorkspacePathRule,
	anchor: SymbolOrdinal,
	witness_steps: &[RulePathStep],
) {
	let Some(record) = index.inventory.record(anchor) else {
		return;
	};
	let witness = witness_label(witness_steps);
	let expectation = expectation_name(rule.expect);
	let explanation = rule.message.as_deref().map(|message| {
		render_template(
			message,
			&[
				("from", record.identity.as_ref()),
				("to", &rule.to_raw),
				("expect", expectation),
				("path", &witness),
			],
		)
	});
	evaluation.violations.push(WorkspaceSymbolViolation {
		source: record.source,
		symbol: Some(record.id),
		source_suppression: true,
		violation: Violation {
			rule_id: rule.rule_id.clone(),
			severity: rule.severity,
			moniker: record.identity.to_string(),
			kind: record.kind.to_string(),
			lines: record.line_range.unwrap_or((0, 0)),
			message: format!("workspace path expectation `{expectation}` failed: {witness}"),
			explanation,
		},
	});
}

fn path_report(
	rule: &CompiledWorkspacePathRule,
	from_count: usize,
	to_count: usize,
	outcome: PathOutcome,
	witness: Vec<RulePathStep>,
) -> RuleReport {
	let verdict = outcome.verdict.unwrap_or(RuleVerdict::Inconclusive);
	let coverage = RuleCoverage {
		total: outcome.coverage.total,
		decided: outcome.coverage.decided,
		resolved: outcome.coverage.resolved,
		external: outcome.coverage.external,
		candidate: outcome.coverage.candidate,
		dynamic: outcome.coverage.dynamic,
		blocked: outcome.coverage.manifest_blocked,
		unresolved: outcome.coverage.unresolved,
		percent: outcome.coverage.percent(),
		min_percent: rule.min_coverage,
	};
	let reasons = outcome.reasons.into_iter().collect::<Vec<_>>();
	RuleReport {
		rule_id: rule.rule_id.clone(),
		severity: rule.severity,
		domain: "workspace paths".to_string(),
		evaluated: outcome.evaluated_pairs,
		matches: usize::from(verdict == RuleVerdict::Pass),
		violations: usize::from(verdict == RuleVerdict::Fail),
		antecedent_matches: None,
		warning: (verdict == RuleVerdict::Inconclusive).then(|| {
			format!(
				"workspace path result is inconclusive: {}",
				reasons.join(", ")
			)
		}),
		inconclusive: Some(usize::from(verdict == RuleVerdict::Inconclusive)),
		verdict: Some(verdict),
		coverage: Some(coverage),
		path: Some(RulePathReport {
			expectation: expectation_name(rule.expect).to_string(),
			relation: rule.relation.clone(),
			max_depth: rule.limits.max_depth,
			max_symbols: rule.limits.max_symbols,
			max_edges: rule.limits.max_edges,
			max_pairs: rule.max_pairs,
			min_coverage: rule.min_coverage,
			source_symbols: from_count,
			target_symbols: to_count,
			evaluated_pairs: outcome.evaluated_pairs,
			explored_symbols: outcome.explored_symbols,
			explored_edges: outcome.explored_edges,
			depth_limit_reached: outcome.depth_limit_reached,
			symbol_limit_reached: outcome.symbol_limit_reached,
			edge_limit_reached: outcome.edge_limit_reached,
			pair_limit_reached: outcome.pair_limit_reached,
			reasons,
			witness,
		}),
	}
}

fn witness_steps(index: &CodeIndex, witness: &[BoundedPathEdge]) -> Vec<RulePathStep> {
	witness
		.iter()
		.filter_map(|edge| {
			let source_ordinal = index.inventory.catalog().ordinal(&edge.source)?;
			let target_ordinal = index.inventory.catalog().ordinal(&edge.target)?;
			let source = index.inventory.record(source_ordinal)?;
			let target = index.inventory.record(target_ordinal)?;
			let reference = index
				.references
				.file_records(edge.reference.file())
				.get(edge.reference.reference())
				.filter(|reference| reference.id == edge.reference)?;
			Some(RulePathStep {
				source: source.identity.to_string(),
				target: target.identity.to_string(),
				relation: reference.kind.to_string(),
				reference: reference.id.to_string(),
				file: source.source_path.to_string(),
				line_range: reference.line_range,
			})
		})
		.collect()
}

fn witness_label(steps: &[RulePathStep]) -> String {
	if steps.is_empty() {
		return "no witness".to_string();
	}
	let mut labels = Vec::with_capacity(steps.len() + 1);
	labels.push(steps[0].source.clone());
	labels.extend(steps.iter().map(|step| step.target.clone()));
	labels.join(" -> ")
}

fn expectation_name(expect: WorkspacePathExpectation) -> &'static str {
	match expect {
		WorkspacePathExpectation::Reachable => "reachable",
		WorkspacePathExpectation::NoPath => "no_path",
	}
}

#[cfg(test)]
mod tests {
	use std::cell::Cell;

	use super::prepare_path_engine;

	#[test]
	fn empty_path_plan_does_not_prepare_the_linkage_engine() {
		let preparations = Cell::new(0);
		let engine = prepare_path_engine(false, || {
			preparations.set(preparations.get() + 1);
			Some(())
		});

		assert!(engine.is_none());
		assert_eq!(preparations.get(), 0);
	}
}
