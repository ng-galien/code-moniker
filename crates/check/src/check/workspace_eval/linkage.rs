use code_moniker_workspace::snapshot::{
	CodeIndex, LinkageSnapshot, ReferenceId, SymbolId, SymbolInventoryIndex, SymbolSet,
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::check::eval::{RuleCoverage, RuleReport, RuleVerdict, Violation};
use crate::check::expr::{Atom, Domain, LhsExpr, Node, NumberExpr, Op, Rhs};

use super::{
	CompiledWorkspaceRules, CompiledWorkspaceSymbolRule, WorkspaceEvaluation, WorkspaceRulePlan,
	WorkspaceSymbolViolation,
};

pub(super) fn evaluate_linkage_rules(
	index: &CodeIndex,
	linkage: &LinkageSnapshot,
	universe: &SymbolSet,
	compiled: &CompiledWorkspaceRules,
	report: bool,
	evaluation: &mut WorkspaceEvaluation,
) {
	let metrics = LinkageMetrics::build(index, linkage, universe);
	let mut atom_cache = FxHashMap::default();
	for rule in compiled
		.symbol
		.iter()
		.filter(|rule| rule.plan == WorkspaceRulePlan::Linkage)
	{
		let mut coverage = CoverageCounts::default();
		let context = EvalContext {
			index,
			universe,
			metrics: &metrics,
			min_coverage: compiled.min_linkage_coverage,
		};
		let outcome = eval_node(&rule.root, &context, &mut coverage, &mut atom_cache);
		evaluation
			.violation_sets
			.insert(rule.rule_id.clone(), outcome.fail.clone());
		append_violations(evaluation, &index.inventory, rule, &outcome.fail);
		if report {
			let match_counts = rule_match_counts(&rule.root, &outcome, &context, &mut atom_cache);
			evaluation.reports.push(linkage_report(
				rule,
				universe,
				&outcome,
				match_counts,
				coverage,
				compiled.min_linkage_coverage,
			));
		}
	}
}

fn append_violations(
	evaluation: &mut WorkspaceEvaluation,
	inventory: &SymbolInventoryIndex,
	rule: &CompiledWorkspaceSymbolRule,
	violations: &SymbolSet,
) {
	for ordinal in violations.iter() {
		let Some(record) = inventory.record(ordinal) else {
			continue;
		};
		let explanation = rule.message.as_deref().map(|message| {
			super::render_template(
				message,
				&[
					("name", record.name.as_ref()),
					("kind", record.kind.as_ref()),
					("moniker", record.identity.as_ref()),
					("expr", &rule.raw_expr),
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
				message: format!(
					"{} `{}` fails workspace linkage assertion `{}`",
					record.kind, record.name, rule.raw_expr
				),
				explanation,
			},
		});
	}
}

fn linkage_report(
	rule: &CompiledWorkspaceSymbolRule,
	universe: &SymbolSet,
	outcome: &TriSet,
	match_counts: RuleMatchCounts,
	coverage: CoverageCounts,
	min_percent: usize,
) -> RuleReport {
	let verdict = if !outcome.fail.is_empty() {
		RuleVerdict::Fail
	} else if !outcome.unknown.is_empty() {
		RuleVerdict::Inconclusive
	} else {
		RuleVerdict::Pass
	};
	RuleReport {
		rule_id: rule.rule_id.clone(),
		severity: rule.severity,
		domain: "workspace linked symbols".to_string(),
		evaluated: universe.len(),
		matches: match_counts.matches,
		violations: outcome.fail.len(),
		antecedent_matches: match_counts.antecedent_matches,
		warning: (!outcome.unknown.is_empty()).then(|| {
			format!(
				"linkage coverage is below {min_percent}% for {} symbol(s)",
				outcome.unknown.len()
			)
		}),
		inconclusive: Some(outcome.unknown.len()),
		verdict: Some(verdict),
		coverage: Some(coverage.report(min_percent)),
	}
}

struct RuleMatchCounts {
	matches: usize,
	antecedent_matches: Option<usize>,
}

fn rule_match_counts(
	root: &Node,
	outcome: &TriSet,
	context: &EvalContext<'_>,
	atom_cache: &mut FxHashMap<String, SymbolSet>,
) -> RuleMatchCounts {
	let Node::Implies(antecedent, _) = root else {
		return RuleMatchCounts {
			matches: outcome.pass.len(),
			antecedent_matches: None,
		};
	};
	let mut ignored_coverage = CoverageCounts::default();
	let antecedent = eval_node(antecedent, context, &mut ignored_coverage, atom_cache);
	RuleMatchCounts {
		matches: outcome.pass.intersection(&antecedent.pass).len(),
		antecedent_matches: Some(antecedent.pass.len()),
	}
}

#[derive(Default)]
struct LinkageMetrics {
	incoming: FxHashMap<SymbolId, ReferenceMetric>,
	outgoing: FxHashMap<SymbolId, ReferenceMetric>,
}

impl LinkageMetrics {
	fn build(index: &CodeIndex, linkage: &LinkageSnapshot, universe: &SymbolSet) -> Self {
		let classifications = ReferenceClassifications::new(linkage);
		let mut metrics = Self::default();
		for reference in index.references.iter().filter(|reference| {
			index
				.inventory
				.catalog()
				.ordinal(&reference.source_symbol)
				.is_some_and(|ordinal| universe.contains(ordinal))
		}) {
			metrics.record(index, reference, classifications.classify(reference.id));
		}
		metrics
	}

	fn record(
		&mut self,
		index: &CodeIndex,
		reference: &code_moniker_workspace::snapshot::ReferenceRecord,
		classification: ReferenceClassification<'_>,
	) {
		match classification {
			ReferenceClassification::Resolved(target) => self.record_targets(
				reference.source_symbol,
				std::slice::from_ref(&target),
				ReferenceClass::Resolved,
			),
			ReferenceClassification::Candidates(targets) => {
				self.record_targets(reference.source_symbol, targets, ReferenceClass::Candidate)
			}
			ReferenceClassification::Dynamic(targets) if !targets.is_empty() => {
				self.record_targets(reference.source_symbol, targets, ReferenceClass::Dynamic)
			}
			ReferenceClassification::Dynamic(_) => {
				self.record_uncertain(index, reference, ReferenceClass::Dynamic)
			}
			ReferenceClassification::Class(ReferenceClass::External) => self
				.outgoing
				.entry(reference.source_symbol)
				.or_default()
				.record(ReferenceClass::External),
			ReferenceClassification::Class(class) => self.record_uncertain(index, reference, class),
		}
	}

	fn record_targets(&mut self, source: SymbolId, targets: &[SymbolId], class: ReferenceClass) {
		self.outgoing.entry(source).or_default().record(class);
		for target in targets {
			self.incoming.entry(*target).or_default().record(class);
		}
	}

	fn record_uncertain(
		&mut self,
		index: &CodeIndex,
		reference: &code_moniker_workspace::snapshot::ReferenceRecord,
		class: ReferenceClass,
	) {
		self.outgoing
			.entry(reference.source_symbol)
			.or_default()
			.record(class);
		attribute_uncertain_incoming(index, reference, &mut self.incoming, class);
	}

	fn metric(&self, symbol: SymbolId, domain: &Domain) -> ReferenceMetric {
		match domain {
			Domain::InRefs => self.incoming.get(&symbol),
			Domain::OutRefs => self.outgoing.get(&symbol),
			_ => None,
		}
		.cloned()
		.unwrap_or_default()
	}
}

struct ReferenceClassifications<'a> {
	resolved: FxHashMap<ReferenceId, SymbolId>,
	candidates: FxHashMap<ReferenceId, &'a [SymbolId]>,
	external: FxHashSet<ReferenceId>,
	dynamic: FxHashMap<ReferenceId, &'a [SymbolId]>,
	blocked: FxHashSet<ReferenceId>,
}

impl<'a> ReferenceClassifications<'a> {
	fn new(linkage: &'a LinkageSnapshot) -> Self {
		Self {
			resolved: linkage
				.resolved
				.iter()
				.map(|edge| (edge.reference, edge.target))
				.collect(),
			candidates: linkage
				.candidates
				.iter()
				.map(|candidate| (candidate.reference, candidate.targets.as_slice()))
				.collect(),
			external: reference_ids(linkage.external.iter().map(|item| item.reference)),
			dynamic: linkage
				.dynamic
				.iter()
				.map(|item| (item.reference, item.candidates.as_slice()))
				.collect(),
			blocked: reference_ids(linkage.blocked.iter().map(|item| item.reference)),
		}
	}

	fn classify(&self, reference: ReferenceId) -> ReferenceClassification<'_> {
		if let Some(target) = self.resolved.get(&reference) {
			ReferenceClassification::Resolved(*target)
		} else if let Some(targets) = self.candidates.get(&reference) {
			ReferenceClassification::Candidates(targets)
		} else if self.external.contains(&reference) {
			ReferenceClassification::Class(ReferenceClass::External)
		} else if let Some(targets) = self.dynamic.get(&reference) {
			ReferenceClassification::Dynamic(targets)
		} else if self.blocked.contains(&reference) {
			ReferenceClassification::Class(ReferenceClass::Blocked)
		} else {
			ReferenceClassification::Class(ReferenceClass::Unresolved)
		}
	}
}

enum ReferenceClassification<'a> {
	Resolved(SymbolId),
	Candidates(&'a [SymbolId]),
	Dynamic(&'a [SymbolId]),
	Class(ReferenceClass),
}

fn reference_ids(ids: impl Iterator<Item = ReferenceId>) -> FxHashSet<ReferenceId> {
	ids.collect()
}

#[derive(Clone, Copy)]
enum ReferenceClass {
	Resolved,
	External,
	Candidate,
	Dynamic,
	Blocked,
	Unresolved,
}

fn attribute_uncertain_incoming(
	index: &CodeIndex,
	reference: &code_moniker_workspace::snapshot::ReferenceRecord,
	incoming: &mut FxHashMap<SymbolId, ReferenceMetric>,
	class: ReferenceClass,
) {
	let Some(target) = index
		.inventory
		.catalog()
		.ordinal_by_identity(reference.target_identity.as_ref())
		.and_then(|ordinal| index.inventory.record(ordinal))
		.map(|record| record.id)
	else {
		return;
	};
	let metric = incoming.entry(target).or_default();
	metric.record(class);
}

#[derive(Clone, Default)]
struct ReferenceMetric {
	count: usize,
	coverage: CoverageCounts,
}

impl ReferenceMetric {
	fn record(&mut self, class: ReferenceClass) {
		match class {
			ReferenceClass::Resolved => {
				self.count += 1;
				self.coverage.resolved += 1;
			}
			ReferenceClass::External => {
				self.count += 1;
				self.coverage.external += 1;
			}
			ReferenceClass::Candidate => self.coverage.candidate += 1,
			ReferenceClass::Dynamic => self.coverage.dynamic += 1,
			ReferenceClass::Blocked => self.coverage.blocked += 1,
			ReferenceClass::Unresolved => self.coverage.unresolved += 1,
		}
	}

	fn percent(&self) -> usize {
		self.coverage.percent()
	}
}

#[derive(Clone, Copy, Default)]
struct CoverageCounts {
	resolved: usize,
	external: usize,
	candidate: usize,
	dynamic: usize,
	blocked: usize,
	unresolved: usize,
}

impl CoverageCounts {
	fn add(&mut self, other: Self) {
		self.resolved += other.resolved;
		self.external += other.external;
		self.candidate += other.candidate;
		self.dynamic += other.dynamic;
		self.blocked += other.blocked;
		self.unresolved += other.unresolved;
	}

	fn report(self, min_percent: usize) -> RuleCoverage {
		RuleCoverage {
			total: self.total(),
			decided: self.decided(),
			resolved: self.resolved,
			external: self.external,
			candidate: self.candidate,
			dynamic: self.dynamic,
			blocked: self.blocked,
			unresolved: self.unresolved,
			percent: self.percent(),
			min_percent,
		}
	}

	fn percent(self) -> usize {
		self.decided()
			.saturating_mul(100)
			.checked_div(self.total())
			.unwrap_or(100)
	}

	fn total(self) -> usize {
		self.resolved
			+ self.external
			+ self.candidate
			+ self.dynamic
			+ self.blocked
			+ self.unresolved
	}

	fn decided(self) -> usize {
		self.resolved + self.external
	}
}

struct TriSet {
	pass: SymbolSet,
	fail: SymbolSet,
	unknown: SymbolSet,
}

impl TriSet {
	fn from_truth(universe: &SymbolSet, truth: SymbolSet) -> Self {
		Self {
			fail: universe.difference(&truth),
			pass: truth,
			unknown: SymbolSet::new(),
		}
	}

	fn all_pass(universe: &SymbolSet) -> Self {
		Self::from_truth(universe, universe.clone())
	}

	fn all_fail(universe: &SymbolSet) -> Self {
		Self::from_truth(universe, SymbolSet::new())
	}

	fn unknown(universe: &SymbolSet) -> Self {
		Self {
			pass: SymbolSet::new(),
			fail: SymbolSet::new(),
			unknown: universe.clone(),
		}
	}

	fn and(self, other: Self, universe: &SymbolSet) -> Self {
		Self::partition(
			universe,
			self.pass.intersection(&other.pass),
			self.fail.union(&other.fail),
		)
	}

	fn or(self, other: Self, universe: &SymbolSet) -> Self {
		Self::partition(
			universe,
			self.pass.union(&other.pass),
			self.fail.intersection(&other.fail),
		)
	}

	fn not(self) -> Self {
		Self {
			pass: self.fail,
			fail: self.pass,
			unknown: self.unknown,
		}
	}

	fn partition(universe: &SymbolSet, pass: SymbolSet, fail: SymbolSet) -> Self {
		let unknown = universe.difference(&pass.union(&fail));
		Self {
			pass,
			fail,
			unknown,
		}
	}
}

fn eval_node(
	node: &Node,
	context: &EvalContext<'_>,
	coverage: &mut CoverageCounts,
	atom_cache: &mut FxHashMap<String, SymbolSet>,
) -> TriSet {
	match node {
		Node::Atom(atom) => eval_atom(atom, context, coverage, atom_cache),
		Node::And(nodes) => nodes
			.iter()
			.map(|node| eval_node(node, context, coverage, atom_cache))
			.reduce(|left, right| left.and(right, context.universe))
			.unwrap_or_else(|| TriSet::all_pass(context.universe)),
		Node::Or(nodes) => nodes
			.iter()
			.map(|node| eval_node(node, context, coverage, atom_cache))
			.reduce(|left, right| left.or(right, context.universe))
			.unwrap_or_else(|| TriSet::all_fail(context.universe)),
		Node::Not(node) => eval_node(node, context, coverage, atom_cache).not(),
		Node::Implies(left, right) => eval_node(left, context, coverage, atom_cache).not().or(
			eval_node(right, context, coverage, atom_cache),
			context.universe,
		),
		Node::Require(_) | Node::VerticalLayout(_) | Node::Quantifier { .. } => {
			TriSet::unknown(context.universe)
		}
	}
}

struct EvalContext<'a> {
	index: &'a CodeIndex,
	universe: &'a SymbolSet,
	metrics: &'a LinkageMetrics,
	min_coverage: usize,
}

fn eval_atom(
	atom: &Atom,
	context: &EvalContext<'_>,
	coverage: &mut CoverageCounts,
	atom_cache: &mut FxHashMap<String, SymbolSet>,
) -> TriSet {
	let LhsExpr::Number(NumberExpr::Count { domain, filter }) = &atom.lhs else {
		return TriSet::from_truth(
			context.universe,
			super::eval_node(
				&Node::Atom(atom.clone()),
				&context.index.inventory,
				context.universe,
				atom_cache,
			),
		);
	};
	let Rhs::Number(NumberExpr::Literal(limit)) = &atom.rhs else {
		return TriSet::unknown(context.universe);
	};
	if filter.is_some() || !matches!(domain, Domain::InRefs | Domain::OutRefs) {
		return TriSet::unknown(context.universe);
	}
	let mut pass = SymbolSet::new();
	let mut fail = SymbolSet::new();
	for ordinal in context.universe.iter() {
		let Some(record) = context.index.inventory.record(ordinal) else {
			continue;
		};
		let metric = context.metrics.metric(record.id, domain);
		coverage.add(metric.coverage);
		if metric.percent() < context.min_coverage {
			continue;
		}
		if compare(metric.count as f64, atom.op, *limit) {
			pass.insert(ordinal);
		} else {
			fail.insert(ordinal);
		}
	}
	TriSet::partition(context.universe, pass, fail)
}

fn compare(left: f64, op: Op, right: f64) -> bool {
	match op {
		Op::Eq => left == right,
		Op::Ne => left != right,
		Op::Lt => left < right,
		Op::Le => left <= right,
		Op::Gt => left > right,
		Op::Ge => left >= right,
		_ => false,
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;

	use code_moniker_workspace::snapshot::{
		DynamicReason, DynamicReference, LinkageEdge, ReferenceRecord, ResourceGeneration,
		SourceId, SymbolRecord, UnresolvedReason, UnresolvedReference,
	};

	use super::*;

	const TARGET_IDENTITY: &str =
		"code+moniker://./lang:java/srcset:main/package:com/package:acme/class:Target";

	fn symbol(def: usize, name: &str) -> SymbolRecord {
		let mut symbol = SymbolRecord::new(SymbolId::at(0, def), SourceId::at(0), name, "class");
		symbol.identity = Arc::from(format!(
			"code+moniker://./lang:java/srcset:main/package:com/package:acme/class:{name}"
		));
		symbol
	}

	fn fixture(
		resolved: usize,
		unresolved: usize,
	) -> (CodeIndex, LinkageSnapshot, CompiledWorkspaceRules) {
		let references = (0..resolved + unresolved)
			.map(|reference| {
				ReferenceRecord::new(
					ReferenceId::at(0, reference),
					SourceId::at(0),
					SymbolId::at(0, 0),
					TARGET_IDENTITY,
					"calls",
					Some((3, 3)),
				)
			})
			.collect::<Vec<_>>();
		let generation = ResourceGeneration::new(1);
		let index = CodeIndex::with_references(
			generation,
			generation,
			vec![symbol(0, "Caller"), symbol(1, "Target")],
			references,
		);
		let edges = (0..resolved)
			.map(|reference| LinkageEdge::new(ReferenceId::at(0, reference), SymbolId::at(0, 1)))
			.collect();
		let gaps = (resolved..resolved + unresolved)
			.map(|reference| {
				UnresolvedReference::new(
					ReferenceId::at(0, reference),
					TARGET_IDENTITY,
					UnresolvedReason::NoCandidate,
				)
			})
			.collect();
		let linkage =
			LinkageSnapshot::with_refs(ResourceGeneration::new(2), generation, edges, gaps);
		(index, linkage, rules(None))
	}

	fn rules(min_coverage: Option<usize>) -> CompiledWorkspaceRules {
		let threshold = min_coverage
			.map(|value| format!("[workspace]\nmin_linkage_coverage = {value}\n"))
			.unwrap_or_default();
		let cfg = crate::check::config::load_from_str(
			&format!(
				r#"
			{threshold}
			[[workspace.symbol.where]]
			id = "target-has-callers"
			expr = "name = 'Target' => count(in_refs) >= 1"

			[[workspace.symbol.where]]
			id = "caller-has-targets"
			expr = "name = 'Caller' => count(out_refs) >= 1"
			"#
			),
			"<test>",
			Some(false),
		)
		.expect("linkage rule config");
		super::super::compile_workspace_rules(&cfg, "code+moniker://").expect("linkage plans")
	}

	#[test]
	fn direct_linkage_rules_report_pass_fail_and_inconclusive() {
		let (resolved_index, resolved_linkage, resolved_rules) = fixture(1, 0);
		let resolved = super::super::evaluate_workspace_rules_linked(
			&resolved_index,
			&resolved_linkage,
			&resolved_rules,
			true,
		)
		.expect("matching linkage generation");
		assert!(resolved.violations.is_empty());
		assert_eq!(resolved.reports.len(), 2);
		assert!(
			resolved
				.reports
				.iter()
				.all(|report| report.verdict == Some(RuleVerdict::Pass))
		);
		assert!(
			resolved
				.reports
				.iter()
				.all(|report| report.antecedent_matches == Some(1) && report.matches == 1)
		);
		assert!(resolved.reports.iter().all(|report| {
			report
				.coverage
				.as_ref()
				.is_some_and(|coverage| coverage.percent == 100)
		}));

		let (partial_index, partial_linkage, partial_rules) = fixture(1, 1);
		let partial = super::super::evaluate_workspace_rules_linked(
			&partial_index,
			&partial_linkage,
			&partial_rules,
			true,
		)
		.expect("matching linkage generation");
		assert!(partial.violations.is_empty());
		assert!(
			partial
				.reports
				.iter()
				.all(|report| report.verdict == Some(RuleVerdict::Inconclusive))
		);
		assert!(partial.reports.iter().all(|report| {
			report
				.coverage
				.as_ref()
				.is_some_and(|coverage| coverage.percent == 50 && coverage.unresolved == 1)
		}));
		let accepted_partial = super::super::evaluate_workspace_rules_linked(
			&partial_index,
			&partial_linkage,
			&rules(Some(50)),
			true,
		)
		.expect("matching linkage generation");
		assert!(
			accepted_partial
				.reports
				.iter()
				.all(|report| report.verdict == Some(RuleVerdict::Pass))
		);

		let (failed_index, failed_linkage, failed_rules) = fixture(0, 0);
		let failed = super::super::evaluate_workspace_rules_linked(
			&failed_index,
			&failed_linkage,
			&failed_rules,
			true,
		)
		.expect("matching linkage generation");
		assert_eq!(failed.violations.len(), 2);
		assert!(
			failed
				.reports
				.iter()
				.all(|report| report.verdict == Some(RuleVerdict::Fail))
		);
	}

	#[test]
	fn dynamic_candidates_make_incoming_counts_inconclusive() {
		let generation = ResourceGeneration::new(1);
		let reference = ReferenceRecord::new(
			ReferenceId::at(0, 0),
			SourceId::at(0),
			SymbolId::at(0, 0),
			"dynamic-target",
			"calls",
			Some((3, 3)),
		);
		let index = CodeIndex::with_references(
			generation,
			generation,
			vec![symbol(0, "Caller"), symbol(1, "A"), symbol(2, "B")],
			vec![reference],
		);
		let mut linkage =
			LinkageSnapshot::with_refs(ResourceGeneration::new(2), generation, vec![], vec![]);
		linkage.dynamic = vec![DynamicReference::new(
			ReferenceId::at(0, 0),
			"dynamic-target",
			DynamicReason::DuckTypedCandidateSet,
			vec![SymbolId::at(0, 1), SymbolId::at(0, 2)],
		)];
		linkage.dynamic_refs = 1;
		let cfg = crate::check::config::load_from_str(
			r#"
			[[workspace.symbol.where]]
			id = "dynamic-targets-are-unused"
			expr = "(name = 'A' OR name = 'B') => count(in_refs) = 0"
			"#,
			"<test>",
			Some(false),
		)
		.expect("dynamic linkage config");
		let compiled = super::super::compile_workspace_rules(&cfg, "code+moniker://")
			.expect("dynamic linkage plan");

		let evaluation =
			super::super::evaluate_workspace_rules_linked(&index, &linkage, &compiled, true)
				.expect("matching linkage generation");
		let report = evaluation.reports.first().expect("dynamic report");
		assert_eq!(report.verdict, Some(RuleVerdict::Inconclusive));
		assert_eq!(report.inconclusive, Some(2));
		assert_eq!(
			report.coverage.as_ref().map(|coverage| coverage.dynamic),
			Some(2)
		);
	}

	#[test]
	fn linked_evaluator_rejects_a_stale_linkage_generation() {
		let (index, mut linkage, compiled) = fixture(1, 0);
		linkage.index_generation = ResourceGeneration::new(index.generation.value() + 1);

		let error =
			super::super::evaluate_workspace_rules_linked(&index, &linkage, &compiled, true)
				.expect_err("stale linkage must be rejected");
		assert_eq!(error.index_generation(), index.generation);
		assert_eq!(error.linkage_index_generation(), linkage.index_generation);
	}
}
