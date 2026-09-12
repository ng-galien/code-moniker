use super::{CompiledWorkspaceRules, WorkspaceEvaluation, WorkspaceRulePlan};
use crate::check::eval::indexed::IndexedPredicates;
use crate::check::eval::{RuleReport, RuleVerdict};
use crate::check::expr::Node;
use code_moniker_workspace::snapshot::{CodeIndex, LinkageSnapshot, SymbolSet};

pub(super) fn evaluate(
	index: &CodeIndex,
	linkage: &LinkageSnapshot,
	universe: &SymbolSet,
	compiled: &CompiledWorkspaceRules,
	report: bool,
	evaluation: &mut WorkspaceEvaluation,
) {
	let rules: Vec<_> = compiled
		.symbol
		.iter()
		.filter(|r| r.plan == WorkspaceRulePlan::Structure)
		.collect();
	if rules.is_empty() {
		return;
	}
	// Build against the whole snapshot. `universe` selects subjects, not their dependencies.
	let context = IndexedPredicates::new(index, linkage);
	for rule in rules {
		let mut failures = SymbolSet::new();
		let mut unknown = 0;
		let mut matches = 0;
		let mut antecedent_matches = 0;
		for ordinal in universe.iter() {
			let Some(symbol) = index.inventory.record(ordinal) else {
				continue;
			};
			let premise = if let Node::Implies(premise, _) = &rule.root {
				context.evaluate(premise, symbol.id)
			} else {
				Some(true)
			};
			if premise == Some(true) {
				antecedent_matches += 1;
			}
			match context.evaluate(&rule.root, symbol.id) {
				Some(true) if premise == Some(true) => matches += 1,
				Some(false) => {
					failures.insert(ordinal);
				}
				None => unknown += 1,
				_ => {}
			}
		}
		super::linkage::append_violations(evaluation, &index.inventory, rule, &failures);
		if report {
			evaluation.reports.push(RuleReport {
				rule_id:rule.rule_id.clone(),severity:rule.severity,domain:"workspace structural symbols".into(),
				evaluated:universe.len(),matches,violations:failures.len(),
				antecedent_matches: matches!(&rule.root,Node::Implies(..)).then_some(antecedent_matches),
				warning:(unknown>0).then(|| "Structural traversal requires resolved relations; some targets or incoming sets are uncertain.".into()),
				inconclusive:Some(unknown),verdict:Some(if !failures.is_empty() {RuleVerdict::Fail} else if unknown>0 {RuleVerdict::Inconclusive} else {RuleVerdict::Pass}),
				coverage:None,path:None,
			});
		}
		evaluation
			.violation_sets
			.insert(rule.rule_id.clone(), failures);
	}
}
