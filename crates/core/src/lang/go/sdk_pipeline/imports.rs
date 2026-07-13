use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};
use crate::lang::sdk::{RefHints, ResolvedRef};
use crate::lang::tree_util::{node_position, node_slice};

use super::super::kinds;
use super::builtins::STDLIB_PACKAGES;
use super::discover::GoDiscover;
use super::syntax::{named_children, strip_string_quotes};

#[derive(Clone, Debug)]
pub(super) struct ImportedPackage {
	pub alias: Vec<u8>,
	pub target: Moniker,
	pub confidence: &'static [u8],
}

pub(super) fn collect_imports(state: &mut GoDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	for child in named_children(node) {
		match child.kind() {
			"import_declaration" => import_declaration(state, child, scope),
			"package_clause" => {}
			_ => {}
		}
	}
}

fn import_declaration(state: &mut GoDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	for child in named_children(node) {
		match child.kind() {
			"import_spec" => import_spec(state, child, scope),
			"import_spec_list" => {
				for spec in named_children(child) {
					if spec.kind() == "import_spec" {
						import_spec(state, spec, scope);
					}
				}
			}
			_ => {}
		}
	}
}

fn import_spec(state: &mut GoDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(path_node) = node.child_by_field_name("path") else {
		return;
	};
	let raw = std::str::from_utf8(node_slice(path_node, state.source)).unwrap_or("");
	let path = strip_string_quotes(raw);
	let pieces: Vec<&str> = path.split('/').filter(|piece| !piece.is_empty()).collect();
	if pieces.is_empty() {
		return;
	}

	let alias_text = node
		.child_by_field_name("name")
		.map(|name| node_slice(name, state.source))
		.and_then(|bytes| std::str::from_utf8(bytes).ok())
		.unwrap_or("");

	let confidence = stdlib_or_imported(&pieces);
	let target = external_package_target(state.root.as_view().project(), &pieces);

	let bind: Option<&[u8]> = match alias_text {
		"" => pieces.last().copied().map(str::as_bytes),
		"." | "_" => None,
		other => Some(other.as_bytes()),
	};
	if let Some(alias) = bind
		&& !alias.is_empty()
	{
		state.imports.push(ImportedPackage {
			alias: alias.to_vec(),
			target: target.clone(),
			confidence,
		});
	}

	state.push_ref(ResolvedRef {
		source: scope.clone(),
		target,
		kind: kinds::IMPORTS_MODULE,
		position: Some(node_position(node)),
		confidence,
		hints: RefHints {
			alias: alias_text.as_bytes().to_vec(),
			..RefHints::default()
		},
	});
}

pub(super) fn external_package_target(project: &[u8], pieces: &[&str]) -> Moniker {
	let mut builder = MonikerBuilder::new();
	builder.project(project);
	if let Some((head, tail)) = pieces.split_first() {
		builder.segment(kinds::EXTERNAL_PKG, head.as_bytes());
		for piece in tail {
			builder.segment(kinds::PATH, piece.as_bytes());
		}
	}
	builder.build()
}

fn stdlib_or_imported(pieces: &[&str]) -> &'static [u8] {
	if pieces.is_empty() {
		return kinds::CONF_IMPORTED;
	}
	if STDLIB_PACKAGES.binary_search(&pieces[0]).is_ok() {
		return kinds::CONF_EXTERNAL;
	}
	kinds::CONF_IMPORTED
}
