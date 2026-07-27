mod group;
mod incremental;

use std::collections::{BTreeMap, BTreeSet};

use code_moniker_workspace::snapshot::{SourceId, SymbolInventoryIndex, SymbolSet};
use rustc_hash::FxHashMap;

use crate::check::config::{Config, ConfigError, RuleEntry};
use crate::check::eval::{CompiledRuleSpec, RuleReport, Violation};
use crate::check::expr::{self, Atom, Lhs, LhsExpr, Node, Op, Rhs};
use crate::check::path::{self, Step};

pub use group::{ScopeKey, WorkspaceGroupResult};

#[derive(Debug)]
struct CompiledWorkspaceSymbolRule {
	rule_id: String,
	raw_expr: String,
	expanded_expr: String,
	root: Node,
	severity: crate::check::config::RuleSeverity,
	message: Option<String>,
	rationale: Option<String>,
	capabilities: Vec<String>,
}

#[derive(Debug, Default)]
pub struct CompiledWorkspaceRules {
	symbol: Vec<CompiledWorkspaceSymbolRule>,
	group: Vec<group::CompiledWorkspaceGroupRule>,
}

impl CompiledWorkspaceRules {
	pub fn is_empty(&self) -> bool {
		self.symbol.is_empty() && self.group.is_empty()
	}

	pub fn specs(&self) -> Vec<CompiledRuleSpec> {
		let mut specs = self
			.symbol
			.iter()
			.map(|rule| CompiledRuleSpec {
				rule_id: rule.rule_id.to_owned(),
				severity: rule.severity,
				lang: "workspace".to_string(),
				root: "workspace".to_string(),
				subject: "symbol".to_string(),
				plan: "t1_inventory".to_string(),
				capabilities: rule.capabilities.to_vec(),
				group_by: Vec::new(),
				domain: "workspace symbols".to_string(),
				kind: None,
				expr: rule.raw_expr.to_owned(),
				expanded_expr: rule.expanded_expr.to_owned(),
				message: rule.message.to_owned(),
				rationale: rule.rationale.to_owned(),
				require_doc_comment: None,
			})
			.collect();
		group::append_group_specs(self, &mut specs);
		specs
	}
}

#[derive(Clone, Debug)]
pub struct WorkspaceSymbolViolation {
	pub source: SourceId,
	pub symbol: Option<code_moniker_workspace::snapshot::SymbolId>,
	pub source_suppression: bool,
	pub violation: Violation,
}

#[derive(Clone, Debug, Default)]
pub struct WorkspaceEvaluation {
	pub violations: Vec<WorkspaceSymbolViolation>,
	pub violation_sets: BTreeMap<String, SymbolSet>,
	pub groups: Vec<WorkspaceGroupResult>,
	pub reports: Vec<RuleReport>,
}

pub(crate) use incremental::{WorkspaceIncrementalInput, evaluate_workspace_rules_incremental};

pub fn compile_workspace_rules(
	cfg: &Config,
	scheme: &str,
) -> Result<CompiledWorkspaceRules, ConfigError> {
	let aliases = crate::check::config::resolve_aliases(&cfg.aliases)?;
	let allowed = crate::check::config::allowed_workspace_kinds();
	let mut symbol = Vec::with_capacity(cfg.workspace.symbol.rules.len());
	for (index, entry) in cfg.workspace.symbol.rules.iter().enumerate() {
		let id = entry.fallback_id(index);
		let at = format!("workspace.symbol.{id}");
		symbol.push(compile_symbol_rule(entry, at, scheme, &allowed, &aliases)?);
	}
	let group = group::compile_groups(cfg, scheme, &allowed, &aliases)?;
	Ok(CompiledWorkspaceRules { symbol, group })
}

fn compile_symbol_rule(
	entry: &RuleEntry,
	at: String,
	scheme: &str,
	allowed_kinds: &[&str],
	aliases: &std::collections::HashMap<String, String>,
) -> Result<CompiledWorkspaceSymbolRule, ConfigError> {
	let expanded = crate::check::config::substitute_aliases(&entry.expr, aliases, &at)?;
	let parsed = match expr::parse(&expanded, scheme, allowed_kinds) {
		Ok(parsed) => parsed,
		Err(error) => {
			return Err(ConfigError::InvalidExpr { at, error });
		}
	};
	let capabilities = classify_t1(&parsed.root, &at)?;
	Ok(CompiledWorkspaceSymbolRule {
		rule_id: at,
		raw_expr: entry.expr.to_owned(),
		expanded_expr: expanded,
		root: parsed.root,
		severity: entry.severity,
		message: entry.message.to_owned(),
		rationale: entry.rationale.to_owned(),
		capabilities,
	})
}

fn classify_t1(node: &Node, at: &str) -> Result<Vec<String>, ConfigError> {
	let mut capabilities = BTreeSet::new();
	collect_capabilities(node, at, &mut capabilities)?;
	Ok(capabilities.into_iter().collect())
}

fn collect_capabilities(
	node: &Node,
	at: &str,
	capabilities: &mut BTreeSet<String>,
) -> Result<(), ConfigError> {
	match node {
		Node::Atom(atom) => collect_atom_capability(atom, at, capabilities),
		Node::And(nodes) | Node::Or(nodes) => {
			for node in nodes {
				collect_capabilities(node, at, capabilities)?;
			}
			Ok(())
		}
		Node::Not(node) => collect_capabilities(node, at, capabilities),
		Node::Implies(left, right) => {
			collect_capabilities(left, at, capabilities)?;
			collect_capabilities(right, at, capabilities)
		}
		Node::Require(_) => unsupported(at, "inventory.require"),
		Node::VerticalLayout(_) => unsupported(at, "local.vertical_layout"),
		Node::Quantifier { .. } => unsupported(at, "inventory.quantifier"),
	}
}

fn collect_atom_capability(
	atom: &Atom,
	at: &str,
	capabilities: &mut BTreeSet<String>,
) -> Result<(), ConfigError> {
	let LhsExpr::Attr(lhs) = &atom.lhs else {
		return unsupported(at, "non-attribute-expression");
	};
	let facet = match lhs {
		Lhs::Name => "name",
		Lhs::Kind => "kind",
		Lhs::Shape => "shape",
		Lhs::Visibility => "visibility",
		Lhs::Moniker => "uri",
		other => return unsupported(at, &format!("projection.{}", other.as_str())),
	};
	let operation = match atom.op {
		Op::Eq | Op::Ne => "exact",
		Op::RegexMatch | Op::RegexNoMatch => "regex",
		Op::PathMatch => "path",
		other => return unsupported(at, &format!("operator.{other:?}")),
	};
	if atom.op == Op::PathMatch && *lhs != Lhs::Moniker {
		return unsupported(at, "path.non-uri");
	}
	capabilities.insert(format!("{facet}.{operation}"));
	Ok(())
}

fn unsupported<T>(at: &str, capability: &str) -> Result<T, ConfigError> {
	Err(ConfigError::UnsupportedWorkspaceExpr {
		at: at.to_string(),
		capability: capability.to_string(),
	})
}

pub fn evaluate_workspace_rules(
	inventory: &SymbolInventoryIndex,
	compiled: &CompiledWorkspaceRules,
	report: bool,
) -> WorkspaceEvaluation {
	evaluate_workspace_rules_in(inventory, inventory.all_symbols(), compiled, report)
}

pub fn evaluate_workspace_rules_in(
	inventory: &SymbolInventoryIndex,
	universe: &SymbolSet,
	compiled: &CompiledWorkspaceRules,
	report: bool,
) -> WorkspaceEvaluation {
	let mut evaluation = WorkspaceEvaluation::default();
	let mut atom_cache = FxHashMap::<String, SymbolSet>::default();
	for rule in &compiled.symbol {
		let truth = eval_node(&rule.root, inventory, universe, &mut atom_cache);
		let violations = universe.difference(&truth);
		evaluation
			.violation_sets
			.insert(rule.rule_id.clone(), violations.clone());
		if report {
			let antecedent_truth = match &rule.root {
				Node::Implies(antecedent, _) => {
					Some(eval_node(antecedent, inventory, universe, &mut atom_cache))
				}
				_ => None,
			};
			let matches = antecedent_truth.as_ref().map_or_else(
				|| truth.len(),
				|antecedent| truth.intersection(antecedent).len(),
			);
			let antecedent_matches = antecedent_truth.as_ref().map(SymbolSet::len);
			evaluation.reports.push(rule_report(
				rule,
				universe,
				matches,
				&violations,
				antecedent_matches,
			));
		}
		for ordinal in violations.iter() {
			let Some(record) = inventory.record(ordinal) else {
				continue;
			};
			let explanation = rule.message.as_deref().map(|message| {
				render_template(
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
						"{} `{}` fails workspace assertion `{}`",
						record.kind, record.name, rule.raw_expr
					),
					explanation,
				},
			});
		}
	}
	group::evaluate_groups(
		inventory,
		universe,
		compiled,
		report,
		&mut atom_cache,
		&mut evaluation,
	);
	sort_workspace_violations(&mut evaluation.violations);
	evaluation
}

fn sort_workspace_violations(violations: &mut [WorkspaceSymbolViolation]) {
	violations.sort_by(|left, right| {
		left.violation
			.rule_id
			.cmp(&right.violation.rule_id)
			.then_with(|| left.violation.moniker.cmp(&right.violation.moniker))
	});
}

fn rule_report(
	rule: &CompiledWorkspaceSymbolRule,
	universe: &SymbolSet,
	matches: usize,
	violations: &SymbolSet,
	antecedent_matches: Option<usize>,
) -> RuleReport {
	RuleReport {
		rule_id: rule.rule_id.clone(),
		severity: rule.severity,
		domain: "workspace symbols".to_string(),
		evaluated: universe.len(),
		matches,
		violations: violations.len(),
		antecedent_matches,
		warning: None,
	}
}

fn eval_node(
	node: &Node,
	inventory: &SymbolInventoryIndex,
	universe: &SymbolSet,
	atom_cache: &mut FxHashMap<String, SymbolSet>,
) -> SymbolSet {
	match node {
		Node::Atom(atom) => {
			if let Some(cached) = atom_cache.get(&atom.raw) {
				return cached.to_owned();
			}
			let result = eval_atom(atom, inventory, universe);
			atom_cache.insert(atom.raw.to_owned(), result.to_owned());
			result
		}
		Node::And(nodes) => nodes
			.iter()
			.map(|node| eval_node(node, inventory, universe, atom_cache))
			.reduce(|left, right| left.intersection(&right))
			.unwrap_or_else(|| universe.clone()),
		Node::Or(nodes) => nodes
			.iter()
			.map(|node| eval_node(node, inventory, universe, atom_cache))
			.reduce(|left, right| left.union(&right))
			.unwrap_or_default(),
		Node::Not(node) => universe.difference(&eval_node(node, inventory, universe, atom_cache)),
		Node::Implies(left, right) => universe
			.difference(&eval_node(left, inventory, universe, atom_cache))
			.union(&eval_node(right, inventory, universe, atom_cache)),
		Node::Require(_) | Node::VerticalLayout(_) | Node::Quantifier { .. } => SymbolSet::new(),
	}
}

fn eval_atom(atom: &Atom, inventory: &SymbolInventoryIndex, universe: &SymbolSet) -> SymbolSet {
	let LhsExpr::Attr(lhs) = &atom.lhs else {
		return SymbolSet::new();
	};
	match atom.op {
		Op::Eq | Op::Ne => eval_exact(*lhs, &atom.rhs, atom.op, inventory, universe),
		Op::RegexMatch | Op::RegexNoMatch => eval_regex(*lhs, atom, inventory, universe),
		Op::PathMatch => eval_path(*lhs, &atom.rhs, inventory, universe),
		_ => SymbolSet::new(),
	}
}

fn eval_exact(
	lhs: Lhs,
	rhs: &Rhs,
	op: Op,
	inventory: &SymbolInventoryIndex,
	universe: &SymbolSet,
) -> SymbolSet {
	let Rhs::Str(value) = rhs else {
		return SymbolSet::new();
	};
	let matched = match lhs {
		Lhs::Name => inventory.facets().symbols_by_name(value).cloned(),
		Lhs::Kind => inventory.facets().symbols_by_kind(value).cloned(),
		Lhs::Shape => inventory.facets().symbols_by_shape(value).cloned(),
		Lhs::Visibility => inventory.facets().symbols_by_visibility(value).cloned(),
		Lhs::Moniker => inventory
			.catalog()
			.ordinal_by_identity(value)
			.map(SymbolSet::from_symbol),
		_ => None,
	}
	.unwrap_or_default();
	if op == Op::Ne {
		universe.difference(&matched)
	} else {
		matched.intersection(universe)
	}
}

fn eval_regex(
	lhs: Lhs,
	atom: &Atom,
	inventory: &SymbolInventoryIndex,
	universe: &SymbolSet,
) -> SymbolSet {
	let Some(regex) = atom.regex.as_ref() else {
		return SymbolSet::new();
	};
	let matched = match lhs {
		Lhs::Name => union_postings(inventory.facets().name_postings(), |value| {
			regex.is_match(value)
		}),
		Lhs::Kind => union_postings(inventory.facets().kind_postings(), |value| {
			regex.is_match(value)
		}),
		Lhs::Shape => union_postings(inventory.facets().shape_postings(), |value| {
			regex.is_match(value)
		}),
		Lhs::Visibility => union_postings(inventory.facets().visibility_postings(), |value| {
			regex.is_match(value)
		}),
		_ => SymbolSet::new(),
	};
	if atom.op == Op::RegexNoMatch {
		universe.difference(&matched)
	} else {
		matched.intersection(universe)
	}
}

fn union_postings<'a>(
	postings: impl Iterator<Item = (&'a str, &'a SymbolSet)>,
	matches: impl Fn(&str) -> bool,
) -> SymbolSet {
	let mut result = SymbolSet::new();
	for (value, symbols) in postings {
		if matches(value) {
			result.union_with(symbols);
		}
	}
	result
}

fn eval_path(
	lhs: Lhs,
	rhs: &Rhs,
	inventory: &SymbolInventoryIndex,
	universe: &SymbolSet,
) -> SymbolSet {
	if lhs != Lhs::Moniker {
		return SymbolSet::new();
	}
	let Rhs::PathPattern(pattern) = rhs else {
		return SymbolSet::new();
	};
	let candidates = exact_segment_candidates(pattern, inventory)
		.map(|candidates| candidates.intersection(universe))
		.unwrap_or_else(|| universe.clone());
	candidates
		.iter()
		.filter(|ordinal| {
			let Some(record) = inventory.record(*ordinal) else {
				return false;
			};
			let segments = record
				.segments
				.iter()
				.map(|segment| (segment.kind.as_ref(), segment.name.as_ref()))
				.collect::<Vec<_>>();
			path::matches_text_segments(pattern, &segments)
		})
		.collect()
}

fn exact_segment_candidates(
	pattern: &path::Pattern,
	inventory: &SymbolInventoryIndex,
) -> Option<SymbolSet> {
	pattern
		.steps
		.iter()
		.filter_map(|step| match step {
			Step::Literal { kind, name } => Some((
				std::str::from_utf8(kind).ok()?,
				std::str::from_utf8(name).ok()?,
			)),
			_ => None,
		})
		.filter_map(|(kind, name)| inventory.facets().symbols_by_segment(kind, name).cloned())
		.reduce(|left, right| left.intersection(&right))
}

fn render_template(template: &str, values: &[(&str, &str)]) -> String {
	let mut rendered = template.to_string();
	for (name, value) in values {
		rendered = rendered.replace(&format!("{{{name}}}"), value);
	}
	rendered
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;

	use code_moniker_workspace::snapshot::{
		RecordTable, ResourceGeneration, SourceFileRecord, SymbolId, SymbolRecord,
	};

	use super::*;

	fn fixture() -> SymbolInventoryIndex {
		let sources = vec![
			SourceFileRecord {
				id: SourceId::at(0),
				uri: "good".to_string(),
				source_root: 0,
				path: "src/main/java/acme/infra/GoodRepository.java".to_string(),
				rel_path: "src/main/java/acme/infra/GoodRepository.java".to_string(),
				anchor: "src/main/java/acme/infra/GoodRepository.java".to_string(),
				language: "java".to_string(),
				text: String::new(),
			},
			SourceFileRecord {
				id: SourceId::at(1),
				uri: "bad".to_string(),
				source_root: 0,
				path: "src/main/java/acme/domain/BadRepository.java".to_string(),
				rel_path: "src/main/java/acme/domain/BadRepository.java".to_string(),
				anchor: "src/main/java/acme/domain/BadRepository.java".to_string(),
				language: "java".to_string(),
				text: String::new(),
			},
			SourceFileRecord {
				id: SourceId::at(2),
				uri: "other".to_string(),
				source_root: 0,
				path: "src/main/java/acme/domain/Helper.java".to_string(),
				rel_path: "src/main/java/acme/domain/Helper.java".to_string(),
				anchor: "src/main/java/acme/domain/Helper.java".to_string(),
				language: "java".to_string(),
				text: String::new(),
			},
		];
		let symbol = |file, name: &str, dir: &str| {
			let mut symbol =
				SymbolRecord::new(SymbolId::at(file, 0), SourceId::at(file), name, "class");
			symbol.identity = Arc::from(format!(
				"code+moniker://./lang:java/srcset:main/package:acme/dir:{dir}/class:{name}"
			));
			symbol.line_range = Some((3, 3));
			symbol
		};
		let symbols = RecordTable::from_shards(vec![
			Arc::from(vec![symbol(0, "GoodRepository", "infra")]),
			Arc::from(vec![symbol(1, "BadRepository", "domain")]),
			Arc::from(vec![symbol(2, "Helper", "domain")]),
		]);
		SymbolInventoryIndex::build(ResourceGeneration::new(1), &sources, &symbols)
	}

	#[test]
	fn implication_uses_active_universe_and_path_posting() {
		let cfg = crate::check::config::load_from_str(
			r#"
			[[workspace.symbol.where]]
			id = "repositories-under-infra"
			expr = "name =~ Repository$ => uri ~ '**/dir:infra/**'"
			"#,
			"<test>",
			Some(false),
		)
		.expect("config");
		let compiled = compile_workspace_rules(&cfg, "code+moniker://").expect("workspace compile");
		let result = evaluate_workspace_rules(&fixture(), &compiled, true);
		assert_eq!(result.violations.len(), 1);
		assert!(
			result.violations[0]
				.violation
				.moniker
				.ends_with("class:BadRepository")
		);
		assert_eq!(result.reports[0].antecedent_matches, Some(2));
		assert_eq!(result.reports[0].matches, 1);
	}

	#[test]
	fn java_inventory_keeps_package_segments_for_path_rules() {
		let source = "package com.acme.infra;\n\npublic class GoodRepository {}\n";
		let graph = code_moniker_workspace::environment::extract_source_with(
			code_moniker_core::lang::Lang::Java,
			source,
			std::path::Path::new("src/main/java/com/acme/infra/GoodRepository.java"),
			&code_moniker_workspace::environment::ExtractContext::default(),
		);
		let symbols = code_moniker_workspace::environment::symbol_records_for_graph(
			0,
			SourceId::at(0),
			&graph,
			source,
			code_moniker_core::lang::Lang::Java,
			"code+moniker://",
		);
		let source_record = SourceFileRecord {
			id: SourceId::at(0),
			uri: "good".to_string(),
			source_root: 0,
			path: "src/main/java/com/acme/infra/GoodRepository.java".to_string(),
			rel_path: "src/main/java/com/acme/infra/GoodRepository.java".to_string(),
			anchor: "src/main/java/com/acme/infra/GoodRepository.java".to_string(),
			language: "java".to_string(),
			text: String::new(),
		};
		let inventory = SymbolInventoryIndex::build(
			ResourceGeneration::new(1),
			&[source_record],
			&RecordTable::from_shards(vec![Arc::from(symbols)]),
		);
		let repository = inventory
			.all_symbols()
			.iter()
			.filter_map(|ordinal| inventory.record(ordinal))
			.find(|record| record.kind.as_ref() == "class")
			.expect("repository class");
		assert!(
			repository
				.segments
				.iter()
				.any(|segment| segment.kind.as_ref() == "package"
					&& segment.name.as_ref().ends_with("infra")),
			"{repository:#?}"
		);
		let pattern = path::parse("**/package:infra/**").expect("package path pattern");
		let segments = repository
			.segments
			.iter()
			.map(|segment| (segment.kind.as_ref(), segment.name.as_ref()))
			.collect::<Vec<_>>();
		assert!(
			path::matches_text_segments(&pattern, &segments),
			"{segments:#?}"
		);
	}
}
