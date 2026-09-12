use crate::check::expr::{Atom, CollectionExpr, Domain, Lhs, LhsExpr, Node, NumberExpr, Op, Rhs};
use std::collections::BTreeSet;

pub(in crate::check) fn validate(node: &Node) -> Result<Vec<String>, ()> {
	let mut capabilities = BTreeSet::new();
	validate_node(node, &mut capabilities)?;
	Ok(capabilities.into_iter().collect())
}

fn validate_node(node: &Node, caps: &mut BTreeSet<String>) -> Result<(), ()> {
	match node {
		Node::Atom(atom) => validate_atom(atom, caps),
		Node::And(nodes) | Node::Or(nodes) => {
			for node in nodes {
				validate_node(node, caps)?;
			}
			Ok(())
		}
		Node::Not(node) => validate_node(node, caps),
		Node::Implies(a, b) => {
			validate_node(a, caps)?;
			validate_node(b, caps)
		}
		Node::Quantifier { domain, filter, .. } => {
			validate_domain(domain)?;
			caps.insert("structure.quantifier".into());
			validate_node(filter, caps)
		}
		_ => Err(()),
	}
}

fn validate_atom(atom: &Atom, caps: &mut BTreeSet<String>) -> Result<(), ()> {
	match &atom.lhs {
		LhsExpr::Attr(lhs) => validate_projection(*lhs)?,
		LhsExpr::Number(n) => validate_number(n, caps)?,
		LhsExpr::Collection(c) => validate_collection(c)?,
		_ => return Err(()),
	}
	match &atom.rhs {
		Rhs::Projection(lhs) | Rhs::CurrentProjection(lhs) => validate_projection(*lhs)?,
		Rhs::Number(n) => validate_number(n, caps)?,
		Rhs::Collection(c) => validate_collection(c)?,
		Rhs::Str(_) | Rhs::RegexStr(_) | Rhs::Moniker(_) | Rhs::PathPattern(_) => {}
		_ => return Err(()),
	}
	if atom.op == Op::Prefix {
		// Multiset algebra has no source ordering contract.
		if !matches!(
			(&atom.lhs, &atom.rhs),
			(
				LhsExpr::Collection(CollectionExpr::Projection(_)),
				Rhs::Collection(CollectionExpr::Projection(_))
			)
		) {
			return Err(());
		}
		if let (
			LhsExpr::Collection(CollectionExpr::Projection(a)),
			Rhs::Collection(CollectionExpr::Projection(b)),
		) = (&atom.lhs, &atom.rhs)
		{
			if !matches!(&a.domain, Domain::Children(_) | Domain::ChildrenByShape(_))
				|| !matches!(&b.domain, Domain::Children(_) | Domain::ChildrenByShape(_))
			{
				return Err(());
			}
		}
		caps.insert("collection.prefix".into());
	}
	caps.insert("structure.projection".into());
	Ok(())
}

fn validate_number(number: &NumberExpr, caps: &mut BTreeSet<String>) -> Result<(), ()> {
	match number {
		NumberExpr::Literal(_) => Ok(()),
		NumberExpr::Count { domain, filter } => {
			validate_domain(domain)?;
			caps.insert("structure.count".into());
			if let Some(filter) = filter {
				validate_node(filter, caps)?;
			}
			Ok(())
		}
		NumberExpr::Size(c) => validate_collection(c),
		_ => Err(()),
	}
}

fn validate_collection(collection: &CollectionExpr) -> Result<(), ()> {
	match collection {
		CollectionExpr::Projection(p) => {
			validate_domain(&p.domain)?;
			validate_projection(Lhs::from_projection_name(&p.path.join(".")).ok_or(())?)
		}
		CollectionExpr::Unique(inner) => validate_collection(inner),
		CollectionExpr::Binary { left, right, .. } => {
			validate_collection(left)?;
			validate_collection(right)
		}
		_ => Err(()),
	}
}

fn validate_domain(domain: &Domain) -> Result<(), ()> {
	match domain {
		Domain::Children(_)
		| Domain::ChildrenByShape(_)
		| Domain::OutRefs
		| Domain::InRefs
		| Domain::SourceInRefs
		| Domain::SourceOutRefs
		| Domain::TargetInRefs
		| Domain::TargetOutRefs => Ok(()),
		_ => Err(()),
	}
}

fn validate_projection(lhs: Lhs) -> Result<(), ()> {
	match lhs {
		Lhs::Srcset
		| Lhs::SourceSrcset
		| Lhs::TargetSrcset
		| Lhs::SourceVisibility
		| Lhs::TargetVisibility
		| Lhs::Name
		| Lhs::Kind
		| Lhs::Shape
		| Lhs::Signature
		| Lhs::Visibility
		| Lhs::Moniker
		| Lhs::SourceName
		| Lhs::SourceKind
		| Lhs::SourceShape
		| Lhs::SourceSignature
		| Lhs::SourceMoniker
		| Lhs::TargetName
		| Lhs::TargetKind
		| Lhs::TargetShape
		| Lhs::TargetSignature
		| Lhs::TargetMoniker => Ok(()),
		_ => Err(()),
	}
}
