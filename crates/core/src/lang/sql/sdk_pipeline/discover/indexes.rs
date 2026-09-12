use super::*;

pub(super) fn emit_index(
	node: Node<'_>,
	source: &[u8],
	module: &Moniker,
	builder: &mut SqlBuilder,
) {
	let Some(relation) =
		find_child(node, "relation_expr").and_then(|n| find_descendant(n, "qualified_name"))
	else {
		return;
	};
	let Some(table) = relation_target(
		relation,
		source,
		module,
		module,
		&CallableSearchPaths::new(),
	) else {
		return;
	};
	let name = find_child(node, "opt_single_name")
		.or_else(|| find_child(node, "name"))
		.map(|n| canonical_identifier(node_slice(n, source)))
		.filter(|n| !n.is_empty())
		.unwrap_or_else(|| format!("@{}", node.start_byte()).into_bytes());
	let owner = table.parent().unwrap_or_else(|| module.clone());
	let index = extend_segment(&owner, b"index", &name);
	let method = find_child(node, "access_method_clause")
		.and_then(|n| find_child(n, "name"))
		.map(|n| canonical_identifier(node_slice(n, source)))
		.unwrap_or_else(|| b"btree".to_vec());
	if !builder.add_definition(index.clone(), b"index", method, node_position(node), module) {
		return;
	}
	builder.push_ref(resolved_ref(
		&index,
		table,
		crate::core::kinds::REF_MEMBER_OF,
		Some(node_position(relation)),
		kinds::CONF_NAME_MATCH,
		&[],
		None,
	));
	for (container, kind) in [
		("index_params", b"index_key".as_slice()),
		("opt_include", b"index_include".as_slice()),
	] {
		if let Some(params) = find_child(node, container) {
			let mut elements = Vec::new();
			collect_nodes(params, "index_elem", &mut elements);
			for (ordinal, element) in elements.into_iter().enumerate() {
				let signature = find_child(element, "ColId")
					.map(|n| column_signature(n, source))
					.unwrap_or_else(|| {
						[b"expression:".as_slice(), node_slice(element, source)].concat()
					});
				builder.add_definition(
					extend_segment_u32(&index, kind, ordinal as u32),
					kind,
					signature,
					node_position(element),
					&index,
				);
			}
		}
	}
	if let Some(predicate) = find_child(node, "where_clause") {
		builder.add_definition(
			extend_segment(&index, b"index_predicate", b"where"),
			b"index_predicate",
			node_slice(predicate, source).to_vec(),
			node_position(predicate),
			&index,
		);
	}
}

fn column_signature(node: Node<'_>, source: &[u8]) -> Vec<u8> {
	[
		b"column:".as_slice(),
		&canonical_identifier(node_slice(node, source)),
	]
	.concat()
}

pub(super) fn emit_foreign_key_columns(
	node: Node<'_>,
	source: &[u8],
	constraint: &Moniker,
	builder: &mut SqlBuilder,
) {
	let Some(references) = find_descendant(node, "kw_references") else {
		return;
	};
	let mut columns = Vec::new();
	collect_nodes(node, "columnElem", &mut columns);
	columns.retain(|n| n.end_byte() <= references.start_byte());
	let mut names: Vec<_> = columns
		.into_iter()
		.filter_map(|n| find_descendant(n, "ColId"))
		.collect();
	if names.is_empty() && node.kind() == "ColConstraint" {
		let mut parent = node.parent();
		while let Some(n) = parent {
			if n.kind() == "columnDef" {
				names.extend(find_child(n, "ColId"));
				break;
			}
			parent = n.parent();
		}
	}
	for (ordinal, name) in names.into_iter().enumerate() {
		builder.add_definition(
			extend_segment_u32(constraint, b"constraint_column", ordinal as u32),
			b"constraint_column",
			column_signature(name, source),
			node_position(name),
			constraint,
		);
	}
}

/// ALTER TABLE may arrive independently of CREATE TABLE in a catalog source set.
pub(super) fn emit_alter_constraints(
	node: Node<'_>,
	source: &[u8],
	module: &Moniker,
	builder: &mut SqlBuilder,
) {
	let Some(relation) =
		find_child(node, "relation_expr").and_then(|n| find_descendant(n, "qualified_name"))
	else {
		return;
	};
	let Some(table) = relation_target(
		relation,
		source,
		module,
		module,
		&CallableSearchPaths::new(),
	) else {
		return;
	};
	let mut constraints = Vec::new();
	collect_nodes(node, "TableConstraint", &mut constraints);
	for constraint in constraints {
		emit_constraint(constraint, source, &table, module, builder);
	}
}
