use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};
use crate::lang::sdk::{RefHints, ResolvedRef};
use crate::lang::tree_util::{node_position, node_slice};

use super::super::kinds;
use super::builtins::is_stdlib_header_path;
use super::discover::CDiscover;
use super::syntax::strip_header_suffix;

pub(super) fn collect_include(state: &mut CDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(path_node) = node.child_by_field_name("path") else {
		return;
	};
	let raw = node_slice(path_node, state.source);
	let text = std::str::from_utf8(raw).unwrap_or("").trim();
	match path_node.kind() {
		"system_lib_string" => {
			let inner = text.trim_start_matches('<').trim_end_matches('>');
			let (target, confidence) = if is_stdlib_header_path(inner) {
				(
					system_include_target(&state.root, inner),
					kinds::CONF_EXTERNAL,
				)
			} else {
				(
					workspace_include_target(&state.root, inner, false),
					kinds::CONF_IMPORTED,
				)
			};
			push_include(state, scope, node, target, confidence);
		}
		"string_literal" => {
			let inner = text.trim_matches('"');
			let target = workspace_include_target(&state.root, inner, true);
			push_include(state, scope, node, target, kinds::CONF_IMPORTED);
		}
		_ => {}
	}
}

fn push_include(
	state: &mut CDiscover<'_>,
	scope: &Moniker,
	node: Node<'_>,
	target: Moniker,
	confidence: &'static [u8],
) {
	state.push_ref(ResolvedRef {
		source: scope.clone(),
		target,
		kind: kinds::IMPORTS_MODULE,
		position: Some(node_position(node)),
		confidence,
		hints: RefHints::default(),
	});
}

// `<sys/types.h>` → external_pkg:sys/path:types — the last piece drops `.h`.
fn system_include_target(root: &Moniker, include_path: &str) -> Moniker {
	let mut builder = MonikerBuilder::new();
	builder.project(root.as_view().project());
	let pieces = include_path
		.split('/')
		.filter(|piece| !piece.is_empty())
		.collect::<Vec<_>>();
	for (index, piece) in pieces.iter().enumerate() {
		let name = if index == pieces.len() - 1 {
			strip_header_suffix(piece)
		} else {
			piece
		};
		let kind = if index == 0 {
			kinds::EXTERNAL_PKG
		} else {
			kinds::PATH
		};
		builder.segment(kind, name.as_bytes());
	}
	builder.build()
}

// Quoted includes are resolved relative to the source directory. Unknown
// angle-bracket roots retain their explicit project-relative path; configured
// `-I` search roots are intentionally left unresolved rather than guessed.
fn workspace_include_target(root: &Moniker, include_path: &str, relative: bool) -> Moniker {
	let mut segments = if relative {
		let mut root_segments = root
			.as_view()
			.segments()
			.map(|segment| (segment.kind.to_vec(), segment.name.to_vec()))
			.collect::<Vec<_>>();
		if root_segments
			.last()
			.is_some_and(|(kind, _)| kind.as_slice() == kinds::MODULE)
		{
			root_segments.pop();
		}
		root_segments
	} else {
		vec![(crate::lang::kinds::LANG.to_vec(), b"c".to_vec())]
	};
	let pieces = include_path
		.split('/')
		.filter(|piece| !piece.is_empty() && *piece != ".")
		.collect::<Vec<_>>();
	for (index, piece) in pieces.iter().enumerate() {
		if *piece == ".." {
			if segments
				.last()
				.is_some_and(|(kind, _)| kind.as_slice() == kinds::DIR)
			{
				segments.pop();
			}
			continue;
		}
		let kind = if index == pieces.len() - 1 {
			kinds::MODULE
		} else {
			kinds::DIR
		};
		segments.push((kind.to_vec(), piece.as_bytes().to_vec()));
	}
	let mut builder = MonikerBuilder::new();
	builder.project(root.as_view().project());
	for (kind, name) in &segments {
		builder.segment(kind, name);
	}
	builder.build()
}
