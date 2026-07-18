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
			} else if let Some(target) =
				resolved_workspace_include_target(&state.root, inner, false, &state.presets)
			{
				(target, kinds::CONF_IMPORTED)
			} else {
				(
					system_include_target(&state.root, inner),
					kinds::CONF_EXTERNAL,
				)
			};
			push_include(state, scope, node, target, confidence);
		}
		"string_literal" => {
			let inner = text.trim_matches('"');
			if let Some(target) =
				resolved_workspace_include_target(&state.root, inner, true, &state.presets)
			{
				push_include(state, scope, node, target, kinds::CONF_IMPORTED);
			} else if quoted_include_escapes_workspace(&state.root, inner) {
				let target = declared_external_include_target(&state.root, "filesystem", inner);
				push_include(state, scope, node, target, kinds::CONF_EXTERNAL);
			} else if let Some(package) = &state.presets.external_include_package
				&& external_package_owns_include(package, inner)
			{
				let target = declared_external_include_target(&state.root, package, inner);
				push_declared_external_include(state, scope, node, target);
			} else {
				let target = workspace_include_target(&state.root, inner, true, &state.presets);
				push_include(state, scope, node, target, kinds::CONF_IMPORTED);
			}
		}
		_ => {}
	}
}

fn push_declared_external_include(
	state: &mut CDiscover<'_>,
	scope: &Moniker,
	node: Node<'_>,
	target: Moniker,
) {
	state.push_ref(ResolvedRef {
		source: scope.clone(),
		target,
		kind: kinds::IMPORTS_MODULE,
		position: Some(node_position(node)),
		confidence: kinds::CONF_EXTERNAL,
		hints: RefHints {
			receiver_hint: b"c_build_dependency".to_vec(),
			..RefHints::default()
		},
	});
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

fn declared_external_include_target(root: &Moniker, package: &str, include_path: &str) -> Moniker {
	let mut builder = MonikerBuilder::new();
	builder.project(root.as_view().project());
	builder.segment(kinds::EXTERNAL_PKG, package.as_bytes());
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
		builder.segment(kinds::PATH, name.as_bytes());
	}
	builder.build()
}

fn external_package_owns_include(package: &str, include_path: &str) -> bool {
	if package != "postgresql" {
		return false;
	}
	let first = include_path.split('/').next().unwrap_or(include_path);
	matches!(
		first,
		"access"
			| "catalog"
			| "common"
			| "executor"
			| "lib" | "libpq"
			| "nodes" | "parser"
			| "port" | "postmaster"
			| "replication"
			| "storage"
			| "tcop" | "utils"
	) || matches!(
		include_path,
		"c.h" | "fmgr.h" | "funcapi.h" | "miscadmin.h" | "plpgsql.h" | "postgres.h"
	)
}

fn quoted_include_escapes_workspace(root: &Moniker, include_path: &str) -> bool {
	let mut pieces = root
		.as_view()
		.segments()
		.filter(|segment| segment.kind == kinds::DIR)
		.map(|segment| String::from_utf8_lossy(segment.name).into_owned())
		.collect::<Vec<_>>();
	pieces.extend(
		include_path
			.split('/')
			.filter(|piece| !piece.is_empty())
			.map(str::to_owned),
	);
	normalize_path_pieces(pieces).is_none()
}

// Quoted includes are resolved relative to the source directory. Unknown
// angle-bracket roots retain their explicit project-relative path; configured
// `-I` search roots are intentionally left unresolved rather than guessed.
fn workspace_include_target(
	root: &Moniker,
	include_path: &str,
	relative: bool,
	presets: &super::super::Presets,
) -> Moniker {
	if let Some(target) = resolved_workspace_include_target(root, include_path, relative, presets) {
		return target;
	}
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
		let terminal = index == pieces.len() - 1;
		let kind = if terminal { kinds::MODULE } else { kinds::DIR };
		let name = if terminal {
			piece.strip_suffix(".c").unwrap_or(piece)
		} else {
			piece
		};
		segments.push((kind.to_vec(), name.as_bytes().to_vec()));
	}
	let mut builder = MonikerBuilder::new();
	builder.project(root.as_view().project());
	for (kind, name) in &segments {
		builder.segment(kind, name);
	}
	builder.build()
}

fn resolved_workspace_include_target(
	root: &Moniker,
	include_path: &str,
	relative: bool,
	presets: &super::super::Presets,
) -> Option<Moniker> {
	resolved_workspace_path(root, include_path, relative, presets)
		.map(|pieces| target_from_workspace_path(root, &pieces))
}

fn resolved_workspace_path(
	root: &Moniker,
	include_path: &str,
	relative: bool,
	presets: &super::super::Presets,
) -> Option<Vec<String>> {
	let include = include_path
		.split('/')
		.filter(|piece| !piece.is_empty())
		.map(str::to_string)
		.collect::<Vec<_>>();
	let mut candidates = Vec::new();
	if relative {
		let mut source_dir = root
			.as_view()
			.segments()
			.filter(|segment| segment.kind == kinds::DIR)
			.map(|segment| String::from_utf8_lossy(segment.name).into_owned())
			.collect::<Vec<_>>();
		source_dir.extend(include.iter().cloned());
		if let Some(candidate) = normalize_path_pieces(source_dir) {
			candidates.push(candidate);
		}
	}
	for include_root in &presets.include_paths {
		let mut candidate = include_root
			.split('/')
			.filter(|piece| !piece.is_empty() && *piece != ".")
			.map(str::to_string)
			.collect::<Vec<_>>();
		candidate.extend(include.iter().cloned());
		if let Some(candidate) = normalize_path_pieces(candidate) {
			candidates.push(candidate);
		}
	}
	if !relative {
		if let Some(candidate) = normalize_path_pieces(include) {
			candidates.push(candidate);
		}
	}
	candidates
		.into_iter()
		.find(|candidate| presets.workspace_files.contains(&candidate.join("/")))
}

fn normalize_path_pieces(pieces: Vec<String>) -> Option<Vec<String>> {
	let mut normalized = Vec::new();
	for piece in pieces {
		match piece.as_str() {
			"" | "." => {}
			".." => {
				normalized.pop()?;
			}
			_ => normalized.push(piece),
		}
	}
	Some(normalized)
}

fn target_from_workspace_path(root: &Moniker, pieces: &[String]) -> Moniker {
	let mut builder = MonikerBuilder::new();
	builder.project(root.as_view().project());
	for segment in root.as_view().segments() {
		if segment.kind == crate::lang::kinds::LANG {
			builder.segment(segment.kind, segment.name);
			break;
		}
		builder.segment(segment.kind, segment.name);
	}
	for (index, piece) in pieces.iter().enumerate() {
		let terminal = index == pieces.len() - 1;
		let kind = if terminal { kinds::MODULE } else { kinds::DIR };
		let name = if terminal {
			piece.strip_suffix(".c").unwrap_or(piece)
		} else {
			piece
		};
		builder.segment(kind, name.as_bytes());
	}
	builder.build()
}
