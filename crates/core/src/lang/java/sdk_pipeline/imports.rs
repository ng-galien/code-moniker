use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};
use crate::lang::sdk::{RefHints, ResolvedRef};
use crate::lang::tree_util::node_position;

use super::super::kinds;
use super::discover::JavaDiscover;
use super::syntax::{named_children, path_pieces};

#[derive(Clone, Debug)]
pub(super) struct ImportedSymbol {
	pub name: Vec<u8>,
	pub target: Moniker,
	pub confidence: &'static [u8],
}

pub(super) fn collect_imports(state: &mut JavaDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	for child in named_children(node) {
		if child.kind() == "import_declaration" {
			import_declaration(state, child, scope);
		} else {
			collect_imports(state, child, scope);
		}
	}
}

fn import_declaration(state: &mut JavaDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let mut wildcard = false;
	let mut path_node = None;
	for child in named_children(node) {
		match child.kind() {
			"asterisk" => wildcard = true,
			"identifier" | "scoped_identifier" => path_node = Some(child),
			_ => {}
		}
	}
	let Some(path_node) = path_node else {
		return;
	};
	let pieces = path_pieces(path_node, state.source);
	if pieces.is_empty() {
		return;
	}
	let str_pieces = pieces
		.iter()
		.map(|piece| std::str::from_utf8(piece).unwrap_or(""))
		.collect::<Vec<_>>();
	let confidence = external_or_imported(&str_pieces);
	let target = if wildcard {
		wildcard_target(&state.root, &str_pieces, confidence)
	} else {
		symbol_target(&state.root, &str_pieces, confidence)
	};
	let kind = if wildcard {
		kinds::IMPORTS_MODULE
	} else {
		kinds::IMPORTS_SYMBOL
	};
	state.push_ref(ResolvedRef {
		source: scope.clone(),
		target: target.clone(),
		kind,
		position: Some(node_position(node)),
		confidence,
		hints: RefHints::default(),
	});
	if !wildcard && let Some(name) = pieces.last() {
		state.imports.push(ImportedSymbol {
			name: name.clone(),
			target,
			confidence,
		});
	}
}

pub(super) fn wildcard_target(module: &Moniker, pieces: &[&str], confidence: &[u8]) -> Moniker {
	if confidence == kinds::CONF_IMPORTED && !pieces.is_empty() {
		let mut builder = project_regime_builder(module);
		builder.segment(crate::lang::kinds::LANG, b"java");
		for piece in pieces {
			builder.segment(kinds::PACKAGE, piece.as_bytes());
		}
		return builder.build();
	}
	external_package_target(module.as_view().project(), pieces)
}

pub(super) fn symbol_target(module: &Moniker, pieces: &[&str], confidence: &[u8]) -> Moniker {
	if confidence == kinds::CONF_IMPORTED && !pieces.is_empty() {
		let mut builder = project_regime_builder(module);
		builder.segment(crate::lang::kinds::LANG, b"java");
		let last = pieces.len() - 1;
		for (index, piece) in pieces.iter().enumerate() {
			let kind = if index == last {
				kinds::MODULE
			} else {
				kinds::PACKAGE
			};
			builder.segment(kind, piece.as_bytes());
		}
		builder.segment(kinds::PATH, pieces[last].as_bytes());
		return builder.build();
	}
	external_package_target(module.as_view().project(), pieces)
}

pub(super) fn same_package_symbol_target(module: &Moniker, name: &[u8]) -> Moniker {
	let view = module.as_view();
	let mut builder = MonikerBuilder::new();
	builder.project(view.project());
	for segment in view.segments() {
		if segment.kind == kinds::MODULE {
			break;
		}
		builder.segment(segment.kind, segment.name);
	}
	builder.segment(kinds::MODULE, name);
	builder.segment(kinds::PATH, name);
	builder.build()
}

pub(super) fn java_lang_target(module: &Moniker, name: &[u8]) -> Moniker {
	symbol_target(
		module,
		&["java", "lang", std::str::from_utf8(name).unwrap_or("")],
		kinds::CONF_EXTERNAL,
	)
}

pub(super) fn java_external_target_shape(target: &Moniker) -> bool {
	target
		.as_view()
		.segments()
		.any(|segment| segment.kind == kinds::EXTERNAL_PKG)
}

fn project_regime_builder(module: &Moniker) -> MonikerBuilder {
	let view = module.as_view();
	let mut builder = MonikerBuilder::new();
	builder.project(view.project());
	for segment in view.segments() {
		if segment.kind == crate::lang::kinds::LANG {
			break;
		}
		builder.segment(segment.kind, segment.name);
	}
	builder
}

fn external_package_target(project: &[u8], pieces: &[&str]) -> Moniker {
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

fn external_or_imported(pieces: &[&str]) -> &'static [u8] {
	if pieces.is_empty() {
		return kinds::CONF_IMPORTED;
	}
	match pieces[0] {
		"java" | "javax" | "kotlin" | "sun" => kinds::CONF_EXTERNAL,
		"com" if pieces.get(1).copied() == Some("sun") => kinds::CONF_EXTERNAL,
		_ => kinds::CONF_IMPORTED,
	}
}
