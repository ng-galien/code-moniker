use std::collections::BTreeMap;
use std::fmt;

use code_moniker_workspace::snapshot::{
	InventorySymbol, SymbolInventoryIndex, SymbolOrdinal, SymbolSet,
};
use rustc_hash::FxHashMap;

use crate::check::config::{ConfigError, WorkspaceGroupRuleEntry};
use crate::check::eval::{CompiledRuleSpec, RuleReport, Violation};
use crate::check::expr::{self, Domain, LhsExpr, Node, NumberExpr, Op, Rhs};

use super::{
	CompiledWorkspaceRules, WorkspaceEvaluation, WorkspaceSymbolViolation, classify_t1, eval_node,
	render_template,
};

const MEMBER_SAMPLE_LIMIT: usize = 5;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ScopeKey {
	pub rule_id: String,
	pub values: Vec<String>,
}

impl ScopeKey {
	pub fn canonical(&self) -> String {
		let values = self
			.values
			.iter()
			.map(|value| format!("{}:{value}", value.len()))
			.collect::<Vec<_>>()
			.join("/");
		format!("{}:{}/{}", self.rule_id.len(), self.rule_id, values)
	}

	fn label(&self) -> String {
		self.values.join(" / ")
	}
}

impl fmt::Display for ScopeKey {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str(&self.canonical())
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceGroupResult {
	pub key: ScopeKey,
	pub members: SymbolSet,
	pub passed: bool,
	pub suppressed: bool,
}

#[derive(Debug)]
pub(super) struct CompiledWorkspaceGroupRule {
	rule_id: String,
	members_expr: String,
	members: Node,
	group_by: Vec<GroupProjection>,
	group_by_exprs: Vec<String>,
	expr: String,
	predicate: GroupPredicate,
	severity: crate::check::config::RuleSeverity,
	message: Option<String>,
	rationale: Option<String>,
	suppress: Vec<Vec<String>>,
	capabilities: Vec<String>,
}

#[derive(Debug)]
enum GroupProjection {
	Language,
	Name,
	Kind,
	Shape,
	Visibility,
	SourcePath,
	SourceRoot,
	Srcset,
	Segment(String),
}

#[derive(Debug)]
struct GroupPredicate {
	op: Op,
	limit: f64,
}

pub(super) fn compile_groups(
	cfg: &crate::check::config::Config,
	scheme: &str,
	allowed_kinds: &[&str],
	aliases: &std::collections::HashMap<String, String>,
) -> Result<Vec<CompiledWorkspaceGroupRule>, ConfigError> {
	cfg.workspace
		.group
		.rules
		.iter()
		.enumerate()
		.map(|(index, entry)| {
			let id = entry.fallback_id(index);
			compile_group_rule(
				entry,
				format!("workspace.group.{id}"),
				scheme,
				allowed_kinds,
				aliases,
			)
		})
		.collect()
}

// The compiled plan outlives configuration loading and therefore owns its
// projections, messages, rationales and suppression keys.
// code-moniker: ignore[smell-clone-reflex]
fn compile_group_rule(
	entry: &WorkspaceGroupRuleEntry,
	at: String,
	scheme: &str,
	allowed_kinds: &[&str],
	aliases: &std::collections::HashMap<String, String>,
) -> Result<CompiledWorkspaceGroupRule, ConfigError> {
	if entry.group_by.is_empty() {
		return invalid(&at, "`group_by` must contain at least one projection");
	}
	let members_expr = crate::check::config::substitute_aliases(&entry.members, aliases, &at)?;
	let members = parse(&members_expr, scheme, allowed_kinds, &at)?;
	let mut capabilities = classify_t1(&members.root, &at)?;
	let group_by = entry
		.group_by
		.iter()
		.map(|projection| parse_projection(projection, &at))
		.collect::<Result<Vec<_>, _>>()?;
	for projection in &group_by {
		capabilities.push(format!("group_by.{}", projection.capability()));
	}
	let expr = crate::check::config::substitute_aliases(&entry.expr, aliases, &at)?;
	let mut group_kinds = allowed_kinds.to_vec();
	group_kinds.push("member");
	let predicate_node = parse(&expr, scheme, &group_kinds, &at)?;
	let predicate = compile_predicate(&predicate_node.root, &at)?;
	capabilities.push("group.count".to_string());
	capabilities.sort();
	capabilities.dedup();
	for suppression in &entry.suppress {
		if suppression.values.len() != group_by.len() {
			return invalid(
				&at,
				&format!(
					"suppression key has {} values, expected {} from `group_by`",
					suppression.values.len(),
					group_by.len()
				),
			);
		}
	}
	Ok(CompiledWorkspaceGroupRule {
		rule_id: at,
		members_expr,
		members: members.root,
		group_by,
		group_by_exprs: entry.group_by.clone(),
		expr,
		predicate,
		severity: entry.severity,
		message: entry.message.clone(),
		rationale: entry.rationale.clone(),
		suppress: entry
			.suppress
			.iter()
			.map(|suppression| suppression.values.clone())
			.collect(),
		capabilities,
	})
}

fn parse(
	expression: &str,
	scheme: &str,
	allowed_kinds: &[&str],
	at: &str,
) -> Result<expr::Expr, ConfigError> {
	expr::parse(expression, scheme, allowed_kinds).map_err(|error| ConfigError::InvalidExpr {
		at: at.to_string(),
		error,
	})
}

fn compile_predicate(node: &Node, at: &str) -> Result<GroupPredicate, ConfigError> {
	let Node::Atom(atom) = node else {
		return invalid(at, "`expr` must compare `count(member)` with a number");
	};
	let LhsExpr::Number(NumberExpr::Count {
		domain: Domain::Children(domain),
		filter: None,
	}) = &atom.lhs
	else {
		return invalid(at, "`expr` must use the unfiltered domain `count(member)`");
	};
	if domain != "member" {
		return invalid(at, "`expr` must use `count(member)`");
	}
	if !matches!(atom.op, Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge) {
		return invalid(at, "`count(member)` requires a numeric comparison");
	}
	let Rhs::Number(NumberExpr::Literal(limit)) = atom.rhs else {
		return invalid(
			at,
			"`count(member)` must be compared with a numeric literal",
		);
	};
	Ok(GroupPredicate { op: atom.op, limit })
}

fn invalid<T>(at: &str, message: &str) -> Result<T, ConfigError> {
	Err(ConfigError::InvalidWorkspaceGroup {
		at: at.to_string(),
		message: message.to_string(),
	})
}

fn parse_projection(raw: &str, at: &str) -> Result<GroupProjection, ConfigError> {
	let projection = match raw.trim() {
		"lang" => GroupProjection::Language,
		"name" => GroupProjection::Name,
		"kind" => GroupProjection::Kind,
		"shape" => GroupProjection::Shape,
		"visibility" => GroupProjection::Visibility,
		"source.path" => GroupProjection::SourcePath,
		"source.root" => GroupProjection::SourceRoot,
		"srcset" => GroupProjection::Srcset,
		other => {
			let Some(kind) = other
				.strip_prefix("segment(")
				.and_then(|value| value.strip_suffix(')'))
				.map(str::trim)
				.and_then(|value| {
					value
						.strip_prefix('\'')
						.and_then(|value| value.strip_suffix('\''))
						.or_else(|| {
							value
								.strip_prefix('"')
								.and_then(|value| value.strip_suffix('"'))
						})
				})
			else {
				return invalid(at, &format!("unsupported `group_by` projection `{other}`"));
			};
			if kind.is_empty() {
				return invalid(at, "segment projection requires a non-empty kind");
			}
			GroupProjection::Segment(kind.to_string())
		}
	};
	Ok(projection)
}

impl GroupProjection {
	fn capability(&self) -> String {
		match self {
			Self::Language => "lang".to_string(),
			Self::Name => "name".to_string(),
			Self::Kind => "kind".to_string(),
			Self::Shape => "shape".to_string(),
			Self::Visibility => "visibility".to_string(),
			Self::SourcePath => "source.path".to_string(),
			Self::SourceRoot => "source.root".to_string(),
			Self::Srcset => "srcset".to_string(),
			Self::Segment(kind) => format!("segment.{kind}"),
		}
	}

	fn value(&self, record: &InventorySymbol) -> String {
		match self {
			Self::Language => record.language.to_string(),
			Self::Name => record.name.to_string(),
			Self::Kind => record.kind.to_string(),
			Self::Shape => record.shape.to_string(),
			Self::Visibility => record.visibility.to_string(),
			Self::SourcePath => record.source_path.to_string(),
			Self::SourceRoot => record.source_root.to_string(),
			Self::Srcset => record.srcset.to_string(),
			Self::Segment(kind) => record
				.segments
				.iter()
				.filter(|segment| segment.kind.as_ref() == kind)
				.map(|segment| escape_segment_component(segment.name.as_ref()))
				.collect::<Vec<_>>()
				.join("."),
		}
	}
}

fn escape_segment_component(value: &str) -> String {
	value.replace('\\', "\\\\").replace('.', "\\.")
}

impl GroupPredicate {
	fn passes(&self, member_count: usize) -> bool {
		let count = member_count as f64;
		match self.op {
			Op::Eq => count == self.limit,
			Op::Ne => count != self.limit,
			Op::Lt => count < self.limit,
			Op::Le => count <= self.limit,
			Op::Gt => count > self.limit,
			Op::Ge => count >= self.limit,
			_ => false,
		}
	}
}

// CompiledRuleSpec is the owned public explanation DTO; projection necessarily
// copies the plan strings out of the executable rule.
// code-moniker: ignore[smell-clone-reflex]
pub(super) fn append_group_specs(
	compiled: &CompiledWorkspaceRules,
	specs: &mut Vec<CompiledRuleSpec>,
) {
	specs.extend(compiled.group.iter().map(|rule| CompiledRuleSpec {
		rule_id: rule.rule_id.clone(),
		severity: rule.severity,
		lang: "workspace".to_string(),
		root: "workspace".to_string(),
		subject: "group".to_string(),
		plan: "t1_inventory".to_string(),
		capabilities: rule.capabilities.clone(),
		group_by: rule.group_by_exprs.clone(),
		domain: "workspace groups".to_string(),
		kind: None,
		expr: rule.expr.clone(),
		expanded_expr: format!("members: {}; assert: {}", rule.members_expr, rule.expr),
		message: rule.message.clone(),
		rationale: rule.rationale.clone(),
		require_doc_comment: None,
	}));
}

pub(super) fn evaluate_groups(
	inventory: &SymbolInventoryIndex,
	universe: &SymbolSet,
	compiled: &CompiledWorkspaceRules,
	report: bool,
	atom_cache: &mut FxHashMap<String, SymbolSet>,
	evaluation: &mut WorkspaceEvaluation,
) {
	for rule in &compiled.group {
		evaluate_group_rule(inventory, universe, rule, report, atom_cache, evaluation);
	}
}

fn evaluate_group_rule(
	inventory: &SymbolInventoryIndex,
	universe: &SymbolSet,
	rule: &CompiledWorkspaceGroupRule,
	report: bool,
	atom_cache: &mut FxHashMap<String, SymbolSet>,
	evaluation: &mut WorkspaceEvaluation,
) {
	let selected = eval_node(&rule.members, inventory, universe, atom_cache);
	let mut buckets = BTreeMap::<ScopeKey, SymbolSet>::new();
	for ordinal in selected.iter() {
		let Some(record) = inventory.record(ordinal) else {
			continue;
		};
		let key = ScopeKey {
			rule_id: rule.rule_id.clone(),
			values: rule
				.group_by
				.iter()
				.map(|projection| projection.value(record))
				.collect(),
		};
		buckets.entry(key).or_default().insert(ordinal);
	}
	let mut passing = 0;
	let mut violations = 0;
	for (key, members) in buckets {
		let passed = rule.predicate.passes(members.len());
		let suppressed = !passed && rule.suppress.contains(&key.values);
		passing += usize::from(passed);
		violations += usize::from(!passed && !suppressed);
		if !passed && !suppressed {
			if let Some(violation) = group_violation(inventory, rule, &key, &members) {
				evaluation.violations.push(violation);
			}
		}
		evaluation.groups.push(WorkspaceGroupResult {
			key,
			members,
			passed,
			suppressed,
		});
	}
	if report {
		evaluation.reports.push(RuleReport {
			rule_id: rule.rule_id.clone(),
			severity: rule.severity,
			domain: "workspace groups".to_string(),
			evaluated: evaluation
				.groups
				.iter()
				.filter(|group| group.key.rule_id == rule.rule_id)
				.count(),
			matches: passing,
			violations,
			antecedent_matches: None,
			warning: None,
			inconclusive: None,
			verdict: None,
			coverage: None,
			path: None,
		});
	}
}

fn group_violation(
	inventory: &SymbolInventoryIndex,
	rule: &CompiledWorkspaceGroupRule,
	key: &ScopeKey,
	members: &SymbolSet,
) -> Option<WorkspaceSymbolViolation> {
	let primary = primary_member(inventory, members)?;
	let member_summary = member_summary(inventory, members);
	let group = key.label();
	let explanation = rule.message.as_deref().map(|message| {
		render_template(
			message,
			&[
				("group", group.as_str()),
				("members", member_summary.as_str()),
				("expr", rule.expr.as_str()),
			],
		)
	});
	Some(WorkspaceSymbolViolation {
		source: primary.1.source,
		symbol: Some(primary.1.id),
		source_suppression: false,
		violation: Violation {
			rule_id: rule.rule_id.clone(),
			severity: rule.severity,
			moniker: key.canonical(),
			kind: "group".to_string(),
			lines: primary.1.line_range.unwrap_or((0, 0)),
			message: format!(
				"group `{group}` has {member_summary} and fails `{}`",
				rule.expr
			),
			explanation,
		},
	})
}

fn primary_member<'a>(
	inventory: &'a SymbolInventoryIndex,
	members: &SymbolSet,
) -> Option<(SymbolOrdinal, &'a InventorySymbol)> {
	members
		.iter()
		.filter_map(|ordinal| inventory.record(ordinal).map(|record| (ordinal, record)))
		.min_by(|left, right| {
			left.1
				.source_path
				.cmp(&right.1.source_path)
				.then_with(|| left.1.identity.cmp(&right.1.identity))
				.then_with(|| left.0.cmp(&right.0))
		})
}

fn member_summary(inventory: &SymbolInventoryIndex, members: &SymbolSet) -> String {
	let mut names = members
		.iter()
		.filter_map(|ordinal| inventory.record(ordinal))
		.map(|record| format!("{} ({})", record.name, record.source_path))
		.collect::<Vec<_>>();
	names.sort();
	names.truncate(MEMBER_SAMPLE_LIMIT);
	let suffix =
		(members.len() > names.len()).then(|| format!(", +{} more", members.len() - names.len()));
	format!(
		"{} members: {}{}",
		members.len(),
		names.join(", "),
		suffix.as_deref().unwrap_or_default()
	)
}

pub(super) struct GroupIncrementalInput<'a> {
	pub previous_inventory: &'a SymbolInventoryIndex,
	pub current_inventory: &'a SymbolInventoryIndex,
	pub previous_universe: &'a SymbolSet,
	pub current_universe: &'a SymbolSet,
	pub previous_dirty: &'a SymbolSet,
	pub current_dirty: &'a SymbolSet,
	pub compiled: &'a CompiledWorkspaceRules,
	pub previous: &'a WorkspaceEvaluation,
}

pub(super) fn evaluate_groups_incremental(
	input: GroupIncrementalInput<'_>,
) -> (
	Vec<WorkspaceGroupResult>,
	Vec<WorkspaceSymbolViolation>,
	usize,
) {
	let GroupIncrementalInput {
		previous_inventory,
		current_inventory,
		previous_universe,
		current_universe,
		previous_dirty,
		current_dirty,
		compiled,
		previous,
	} = input;
	let mut next_by_key = index_group_results(previous);
	let mut affected = std::collections::BTreeSet::new();
	let mut previous_cache = FxHashMap::default();
	let mut current_cache = FxHashMap::default();
	for rule in &compiled.group {
		let previous_selected = eval_node(
			&rule.members,
			previous_inventory,
			&previous_dirty.intersection(previous_universe),
			&mut previous_cache,
		);
		let current_selected = eval_node(
			&rule.members,
			current_inventory,
			&current_dirty.intersection(current_universe),
			&mut current_cache,
		);
		let previous_changed = bucket_members(previous_inventory, rule, &previous_selected);
		let current_changed = bucket_members(current_inventory, rule, &current_selected);
		let keys = previous_changed
			.keys()
			.chain(current_changed.keys())
			.cloned()
			.collect::<std::collections::BTreeSet<_>>();
		for key in keys {
			affected.insert(key.canonical());
			let mut members = next_by_key
				.get(&key)
				.map(|group| group.members.clone())
				.unwrap_or_default();
			members.remove_all(previous_dirty);
			members.intersect_with(current_universe);
			if let Some(changed) = current_changed.get(&key) {
				members.union_with(changed);
			}
			if members.is_empty() {
				next_by_key.remove(&key);
				continue;
			}
			let passed = rule.predicate.passes(members.len());
			let suppressed = !passed && rule.suppress.contains(&key.values);
			next_by_key.insert(
				key.clone(),
				WorkspaceGroupResult {
					key,
					members,
					passed,
					suppressed,
				},
			);
		}
	}
	let groups = next_by_key.into_values().collect::<Vec<_>>();
	let violations = group_diagnostics(current_inventory, compiled, &groups);
	(groups, violations, affected.len())
}

fn index_group_results(
	evaluation: &WorkspaceEvaluation,
) -> BTreeMap<ScopeKey, WorkspaceGroupResult> {
	evaluation
		.groups
		.iter()
		.cloned()
		.map(|group| (group.key.clone(), group))
		.collect()
}

fn bucket_members(
	inventory: &SymbolInventoryIndex,
	rule: &CompiledWorkspaceGroupRule,
	selected: &SymbolSet,
) -> BTreeMap<ScopeKey, SymbolSet> {
	let mut buckets = BTreeMap::new();
	for ordinal in selected.iter() {
		let Some(record) = inventory.record(ordinal) else {
			continue;
		};
		let key = ScopeKey {
			rule_id: rule.rule_id.clone(),
			values: rule
				.group_by
				.iter()
				.map(|projection| projection.value(record))
				.collect(),
		};
		buckets
			.entry(key)
			.or_insert_with(SymbolSet::new)
			.insert(ordinal);
	}
	buckets
}

fn group_diagnostics(
	inventory: &SymbolInventoryIndex,
	compiled: &CompiledWorkspaceRules,
	groups: &[WorkspaceGroupResult],
) -> Vec<WorkspaceSymbolViolation> {
	let rules = compiled
		.group
		.iter()
		.map(|rule| (rule.rule_id.as_str(), rule))
		.collect::<BTreeMap<_, _>>();
	groups
		.iter()
		.filter(|group| !group.passed && !group.suppressed)
		.filter_map(|group| {
			group_violation(
				inventory,
				rules.get(group.key.rule_id.as_str()).copied()?,
				&group.key,
				&group.members,
			)
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use std::collections::BTreeSet;
	use std::sync::Arc;

	use code_moniker_workspace::snapshot::{
		RecordTable, ResourceGeneration, SourceFileRecord, SourceId, SymbolId, SymbolRecord,
	};

	use super::*;

	fn source(file: usize, package: &str) -> SourceFileRecord {
		let path = format!(
			"src/main/java/{}/Type{file}.java",
			package.replace('.', "/")
		);
		SourceFileRecord {
			id: SourceId::at(file),
			uri: path.clone(),
			source_root: 0,
			path: path.clone(),
			rel_path: path.clone(),
			anchor: path,
			language: "java".to_string(),
			text: String::new(),
		}
	}

	fn invoice(file: usize, package: &str, container: &str) -> SymbolRecord {
		let packages = package
			.split('.')
			.map(|name| format!("package:{name}"))
			.collect::<Vec<_>>()
			.join("/");
		let mut symbol = SymbolRecord::new(
			SymbolId::at(file, 0),
			SourceId::at(file),
			"Invoice",
			"class",
		);
		symbol.identity = Arc::from(format!(
			"code+moniker://./lang:java/srcset:main/{packages}/class:{container}/class:Invoice"
		));
		symbol.line_range = Some((4, 4));
		symbol
	}

	fn group_config() -> crate::check::config::Config {
		crate::check::config::load_from_str(
			r#"
			[[workspace.group.where]]
			id = "unique"
			members = "name = 'Invoice'"
			group_by = ["lang", "segment('package')", "name"]
			expr = "count(member) <= 1"
			"#,
			"<test>",
			Some(false),
		)
		.expect("group config")
	}

	#[test]
	fn moving_one_member_rebuilds_the_old_and_new_scope_keys() {
		let sources = vec![source(0, "com.acme.sales"), source(1, "com.acme.sales")];
		let before_symbols = RecordTable::from_shards(vec![
			Arc::from(vec![invoice(0, "com.acme.sales", "SalesA")]),
			Arc::from(vec![invoice(1, "com.acme.sales", "SalesB")]),
		]);
		let before =
			SymbolInventoryIndex::build(ResourceGeneration::new(1), &sources, &before_symbols);
		let compiled = super::super::compile_workspace_rules(&group_config(), "code+moniker://")
			.expect("plan");
		let before_result = super::super::evaluate_workspace_rules(&before, &compiled, false);
		assert_eq!(before_result.groups.len(), 1);
		assert!(!before_result.groups[0].passed);
		assert_eq!(before_result.groups[0].members.len(), 2);

		let after_sources = vec![source(0, "com.acme.sales"), source(1, "com.acme.orders")];
		let after_symbols = RecordTable::from_shards(vec![
			Arc::from(vec![invoice(0, "com.acme.sales", "SalesA")]),
			Arc::from(vec![invoice(1, "com.acme.orders", "SalesB")]),
		]);
		let after = before.refresh(
			ResourceGeneration::new(2),
			&after_sources,
			&after_symbols,
			&BTreeSet::from([1]),
		);
		let after_result = super::super::evaluate_workspace_rules(&after, &compiled, false);
		assert_eq!(after_result.groups.len(), 2);
		assert!(after_result.groups.iter().all(|group| group.passed));
		let packages = after_result
			.groups
			.iter()
			.map(|group| group.key.values[1].as_str())
			.collect::<BTreeSet<_>>();
		assert_eq!(
			packages,
			BTreeSet::from(["com.acme.orders", "com.acme.sales"])
		);
	}

	#[test]
	fn segment_projection_preserves_component_boundaries() {
		let sources = vec![source(0, "acme"), source(1, "acme")];
		let symbol = |file, identity: &str| {
			let mut symbol = SymbolRecord::new(
				SymbolId::at(file, 0),
				SourceId::at(file),
				"Invoice",
				"class",
			);
			symbol.identity = Arc::from(identity);
			symbol
		};
		let symbols = RecordTable::from_shards(vec![
			Arc::from(vec![symbol(
				0,
				"code+moniker://./lang:java/dir:a.b/dir:c/class:Invoice",
			)]),
			Arc::from(vec![symbol(
				1,
				"code+moniker://./lang:java/dir:a/dir:b.c/class:Invoice",
			)]),
		]);
		let inventory = SymbolInventoryIndex::build(ResourceGeneration::new(1), &sources, &symbols);
		let cfg = crate::check::config::load_from_str(
			r#"
			[[workspace.group.where]]
			id = "distinct-segment-sequences"
			members = "name = 'Invoice'"
			group_by = ["segment('dir')", "name"]
			expr = "count(member) <= 1"
			"#,
			"<test>",
			Some(false),
		)
		.expect("group config");
		let compiled =
			super::super::compile_workspace_rules(&cfg, "code+moniker://").expect("group plan");
		let result = super::super::evaluate_workspace_rules(&inventory, &compiled, false);
		assert_eq!(result.groups.len(), 2);
		assert!(result.groups.iter().all(|group| group.passed));
	}

	#[test]
	fn canonical_scope_key_prefixes_rule_id_and_values() {
		let left = ScopeKey {
			rule_id: "workspace.group.a".to_string(),
			values: vec!["x".to_string(), "y".to_string()],
		};
		let right = ScopeKey {
			rule_id: "workspace.group.a/1:x".to_string(),
			values: vec!["y".to_string()],
		};
		assert_ne!(left.canonical(), right.canonical());
	}
}
