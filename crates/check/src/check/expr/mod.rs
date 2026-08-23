//! Rule DSL for `code-moniker check`. Full reference: docs/cli/check-dsl.md.

mod ast;
mod atom;
mod collection;
mod cursor;
mod domain;
mod error;
mod metrics;
mod number;
mod pairs;
mod parse;
mod value;

pub(in crate::check) use ast::*;
pub use error::ParseError;
pub(in crate::check) use parse::parse;

pub(in crate::check) fn contains_inline_project_selector(input: &str) -> bool {
	let input = replace_aliases_with_neutral_atoms(input);
	let mut allowed_kinds = super::config::allowed_workspace_kinds();
	allowed_kinds.push("member");
	parse(&input, "code+moniker://", &allowed_kinds)
		.is_ok_and(|expr| node_contains_inline_project_selector(&expr.root))
}

fn replace_aliases_with_neutral_atoms(input: &str) -> String {
	let bytes = input.as_bytes();
	let mut output = String::with_capacity(input.len());
	let mut index = 0;
	let mut copied_until = 0;
	while index < bytes.len() {
		if bytes[index] != b'$' {
			index += 1;
			continue;
		}
		let start = index + 1;
		let mut end = start;
		while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
			end += 1;
		}
		if end == start {
			index += 1;
			continue;
		}
		output.push_str(&input[copied_until..index]);
		output.push_str("(name = '__code_moniker_alias__')");
		copied_until = end;
		index = end;
	}
	output.push_str(&input[copied_until..]);
	output
}

fn node_contains_inline_project_selector(node: &Node) -> bool {
	match node {
		Node::Atom(atom) => {
			atom_is_inline_project_selector(atom) || lhs_contains_inline_project_selector(&atom.lhs)
		}
		Node::And(nodes) | Node::Or(nodes) => {
			nodes.iter().any(node_contains_inline_project_selector)
		}
		Node::Not(node) => node_contains_inline_project_selector(node),
		Node::Implies(left, right) => {
			node_contains_inline_project_selector(left)
				|| node_contains_inline_project_selector(right)
		}
		Node::Quantifier { filter, .. } => node_contains_inline_project_selector(filter),
		Node::Require(_) | Node::VerticalLayout(_) => false,
	}
}

fn atom_is_inline_project_selector(atom: &Atom) -> bool {
	matches!(
		atom.lhs,
		LhsExpr::Attr(Lhs::Moniker | Lhs::SourceMoniker | Lhs::TargetMoniker)
	) && matches!(atom.rhs, Rhs::PathPattern(_) | Rhs::Moniker(_))
}

fn lhs_contains_inline_project_selector(lhs: &LhsExpr) -> bool {
	match lhs {
		LhsExpr::Number(number) => number_contains_inline_project_selector(number),
		LhsExpr::Mode(value) => value
			.filter
			.as_deref()
			.is_some_and(node_contains_inline_project_selector),
		LhsExpr::Attr(_)
		| LhsExpr::Collection(_)
		| LhsExpr::PairProjection(_)
		| LhsExpr::SegmentOf { .. } => false,
	}
}

fn number_contains_inline_project_selector(number: &NumberExpr) -> bool {
	match number {
		NumberExpr::Count { filter, .. } => filter
			.as_deref()
			.is_some_and(node_contains_inline_project_selector),
		NumberExpr::Aggregate { expr, .. } => number_contains_inline_project_selector(expr),
		NumberExpr::Entropy(value) => value
			.filter
			.as_deref()
			.is_some_and(node_contains_inline_project_selector),
		NumberExpr::Literal(_)
		| NumberExpr::Projection(_)
		| NumberExpr::Metric { .. }
		| NumberExpr::Size(_) => false,
	}
}

#[cfg(test)]
mod fuzz;

#[cfg(test)]
mod snapshots;

#[cfg(test)]
mod test_support {
	pub(super) const TS: &str = "code+moniker://";
	pub(super) const KINDS: &[&str] = &[
		"class",
		"method",
		"function",
		"module",
		"field",
		"param",
		"enum_constant",
	];
}
