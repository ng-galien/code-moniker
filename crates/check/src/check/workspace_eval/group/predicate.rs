use code_moniker_workspace::snapshot::{SymbolInventoryIndex, SymbolSet};

use crate::check::config::ConfigError;
use crate::check::expr::{AggregateKind, Domain, Lhs, LhsExpr, Node, NumberExpr, Op, Rhs};

#[derive(Debug)]
pub(super) enum GroupPredicate {
	Compare {
		expr: GroupNumberExpr,
		op: Op,
		limit: f64,
	},
	And(Vec<GroupPredicate>),
	Or(Vec<GroupPredicate>),
	Not(Box<GroupPredicate>),
	Implies(Box<GroupPredicate>, Box<GroupPredicate>),
}

#[derive(Debug)]
pub(super) enum GroupNumberExpr {
	MemberCount,
	LineAggregate {
		kind: AggregateKind,
		percentile: Option<f64>,
	},
}

#[derive(Debug)]
pub(super) struct GroupPredicateOutcome {
	pub passed: bool,
	pub observations: Vec<String>,
	pub error: Option<String>,
}

pub(super) fn compile(node: &Node, at: &str) -> Result<GroupPredicate, ConfigError> {
	match node {
		Node::Atom(atom) => {
			if !matches!(atom.op, Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge) {
				return invalid(
					at,
					"`workspace.group.expr` statistics require a numeric comparison",
				);
			}
			let LhsExpr::Number(number) = &atom.lhs else {
				return invalid(
					at,
					"`workspace.group.expr` must compare a supported group number with a literal",
				);
			};
			let Rhs::Number(NumberExpr::Literal(limit)) = atom.rhs else {
				return invalid(
					at,
					"`workspace.group.expr` must compare a supported group number with a numeric literal",
				);
			};
			Ok(GroupPredicate::Compare {
				expr: compile_number_expr(number, at)?,
				op: atom.op,
				limit,
			})
		}
		Node::And(nodes) => Ok(GroupPredicate::And(
			nodes
				.iter()
				.map(|node| compile(node, at))
				.collect::<Result<_, _>>()?,
		)),
		Node::Or(nodes) => Ok(GroupPredicate::Or(
			nodes
				.iter()
				.map(|node| compile(node, at))
				.collect::<Result<_, _>>()?,
		)),
		Node::Not(node) => Ok(GroupPredicate::Not(Box::new(compile(node, at)?))),
		Node::Implies(antecedent, consequent) => Ok(GroupPredicate::Implies(
			Box::new(compile(antecedent, at)?),
			Box::new(compile(consequent, at)?),
		)),
		_ => invalid(
			at,
			"`workspace.group.expr` only supports boolean combinations of group numeric comparisons",
		),
	}
}

fn compile_number_expr(expr: &NumberExpr, at: &str) -> Result<GroupNumberExpr, ConfigError> {
	match expr {
		NumberExpr::Count {
			domain: Domain::Children(domain),
			filter: None,
		} if domain == "member" => Ok(GroupNumberExpr::MemberCount),
		NumberExpr::Aggregate {
			kind,
			domain: Domain::Children(domain),
			expr,
			percentile,
		} if domain == "member" && matches!(expr.as_ref(), NumberExpr::Projection(Lhs::Lines)) => {
			if *kind == AggregateKind::Percentile
				&& percentile.is_none_or(|value| !(0.0..=100.0).contains(&value))
			{
				return invalid(at, "`percentile(member, lines, P)` requires P in 0..=100");
			}
			Ok(GroupNumberExpr::LineAggregate {
				kind: *kind,
				percentile: *percentile,
			})
		}
		_ => invalid(
			at,
			"`workspace.group.expr` supports `count(member)` and descriptive aggregates over `(member, lines)`",
		),
	}
}

fn invalid<T>(at: &str, message: &str) -> Result<T, ConfigError> {
	Err(ConfigError::InvalidWorkspaceGroup {
		at: at.to_string(),
		message: message.to_string(),
	})
}

impl GroupPredicate {
	pub fn append_capabilities(&self, capabilities: &mut Vec<String>) {
		match self {
			Self::Compare { expr, .. } => capabilities.push(expr.capability()),
			Self::And(predicates) | Self::Or(predicates) => {
				for predicate in predicates {
					predicate.append_capabilities(capabilities);
				}
			}
			Self::Not(predicate) => predicate.append_capabilities(capabilities),
			Self::Implies(antecedent, consequent) => {
				antecedent.append_capabilities(capabilities);
				consequent.append_capabilities(capabilities);
			}
		}
	}

	pub fn evaluate(
		&self,
		inventory: &SymbolInventoryIndex,
		members: &SymbolSet,
	) -> GroupPredicateOutcome {
		match self {
			Self::Compare { expr, op, limit } => expr.compare(*op, *limit, inventory, members),
			Self::And(predicates) => Self::evaluate_sequence(predicates, inventory, members, false),
			Self::Or(predicates) => Self::evaluate_sequence(predicates, inventory, members, true),
			Self::Not(predicate) => Self::evaluate_not(predicate, inventory, members),
			Self::Implies(antecedent, consequent) => {
				Self::evaluate_implies(antecedent, consequent, inventory, members)
			}
		}
	}

	fn evaluate_sequence(
		predicates: &[GroupPredicate],
		inventory: &SymbolInventoryIndex,
		members: &SymbolSet,
		any: bool,
	) -> GroupPredicateOutcome {
		let mut observations = Vec::new();
		let mut pending_error = None;
		for predicate in predicates {
			let outcome = predicate.evaluate(inventory, members);
			observations.extend(outcome.observations);
			if let Some(error) = outcome.error {
				pending_error.get_or_insert(error);
				continue;
			}
			if outcome.passed == any {
				return GroupPredicateOutcome {
					passed: any,
					observations,
					error: None,
				};
			}
		}
		let passed = pending_error.is_none() && !any;
		GroupPredicateOutcome {
			passed,
			observations,
			error: pending_error,
		}
	}

	fn evaluate_not(
		predicate: &GroupPredicate,
		inventory: &SymbolInventoryIndex,
		members: &SymbolSet,
	) -> GroupPredicateOutcome {
		let mut outcome = predicate.evaluate(inventory, members);
		if outcome.error.is_none() {
			outcome.passed = !outcome.passed;
		}
		outcome
	}

	fn evaluate_implies(
		antecedent: &GroupPredicate,
		consequent: &GroupPredicate,
		inventory: &SymbolInventoryIndex,
		members: &SymbolSet,
	) -> GroupPredicateOutcome {
		let mut antecedent = antecedent.evaluate(inventory, members);
		if antecedent.error.is_none() {
			if !antecedent.passed {
				antecedent.passed = true;
				return antecedent;
			}
			let consequent = consequent.evaluate(inventory, members);
			antecedent.observations.extend(consequent.observations);
			antecedent.passed = consequent.passed;
			antecedent.error = consequent.error;
			return antecedent;
		}

		let consequent = consequent.evaluate(inventory, members);
		antecedent.observations.extend(consequent.observations);
		if consequent.error.is_none() && consequent.passed {
			antecedent.passed = true;
			antecedent.error = None;
		}
		antecedent
	}
}

struct GroupNumberEvaluation {
	value: Option<f64>,
	observation: String,
	error: Option<String>,
}

impl GroupNumberExpr {
	fn capability(&self) -> String {
		match self {
			Self::MemberCount => "group.count".to_string(),
			Self::LineAggregate { kind, .. } => format!("group.{}.lines", kind.as_str()),
		}
	}

	fn compare(
		&self,
		op: Op,
		limit: f64,
		inventory: &SymbolInventoryIndex,
		members: &SymbolSet,
	) -> GroupPredicateOutcome {
		let number = self.evaluate(inventory, members);
		let passed = number
			.value
			.is_some_and(|value| compare_numbers(value, op, limit));
		GroupPredicateOutcome {
			passed,
			observations: vec![number.observation],
			error: number.error,
		}
	}

	fn evaluate(
		&self,
		inventory: &SymbolInventoryIndex,
		members: &SymbolSet,
	) -> GroupNumberEvaluation {
		let label = self.label();
		match self {
			Self::MemberCount => {
				let value = members.len() as f64;
				GroupNumberEvaluation {
					value: Some(value),
					observation: format!("{label}={}", render_number(value)),
					error: None,
				}
			}
			Self::LineAggregate { kind, percentile } => {
				let values = members
					.iter()
					.filter_map(|ordinal| inventory.record(ordinal))
					.filter_map(|record| record.line_range)
					.filter_map(|(start, end)| {
						(end >= start).then_some((u64::from(end) - u64::from(start) + 1) as f64)
					})
					.collect::<Vec<_>>();
				let available = values.len();
				let total = members.len();
				if available != total {
					let observation =
						format!("{label}=unavailable ({available}/{total} line ranges)");
					return GroupNumberEvaluation {
						value: None,
						error: Some(observation.clone()),
						observation,
					};
				}
				let value = crate::check::eval::stats::aggregate(*kind, values, *percentile);
				let Some(value) = value else {
					let observation =
						format!("{label}=unavailable ({available}/{total} line ranges)");
					return GroupNumberEvaluation {
						value: None,
						error: Some(format!(
							"{observation}; the statistic is undefined for these values"
						)),
						observation,
					};
				};
				GroupNumberEvaluation {
					value: Some(value),
					observation: format!(
						"{label}={} ({available}/{total} line ranges)",
						render_number(value)
					),
					error: None,
				}
			}
		}
	}

	fn label(&self) -> String {
		match self {
			Self::MemberCount => "count(member)".to_string(),
			Self::LineAggregate {
				kind: AggregateKind::Percentile,
				percentile: Some(percentile),
			} => format!("percentile(member, lines, {})", render_number(*percentile)),
			Self::LineAggregate { kind, .. } => {
				format!("{}(member, lines)", kind.as_str())
			}
		}
	}
}

fn compare_numbers(value: f64, op: Op, limit: f64) -> bool {
	match op {
		Op::Eq => value == limit,
		Op::Ne => value != limit,
		Op::Lt => value < limit,
		Op::Le => value <= limit,
		Op::Gt => value > limit,
		Op::Ge => value >= limit,
		_ => false,
	}
}

fn render_number(value: f64) -> String {
	format!("{value:.6}")
		.trim_end_matches('0')
		.trim_end_matches('.')
		.to_string()
}
