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
	pub is_static: bool,
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
	let mut static_import = false;
	let mut path_node = None;
	for index in 0..node.child_count() {
		let Some(child) = node.child(index as u32) else {
			continue;
		};
		match child.kind() {
			"asterisk" => wildcard = true,
			"static" => static_import = true,
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
			target: if static_import {
				static_owner_target(&state.root, &str_pieces, confidence).unwrap_or(target)
			} else {
				target
			},
			confidence,
			is_static: static_import,
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
	external_or_sdk_target(module.as_view().project(), pieces)
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
	external_or_sdk_target(module.as_view().project(), pieces)
}

fn static_owner_target(module: &Moniker, pieces: &[&str], confidence: &[u8]) -> Option<Moniker> {
	let owner = pieces.get(..pieces.len().checked_sub(1)?)?;
	if owner.is_empty() {
		return None;
	}
	if confidence == kinds::CONF_IMPORTED {
		let mut builder = project_regime_builder(module);
		builder.segment(crate::lang::kinds::LANG, b"java");
		let last = owner.len() - 1;
		for (index, piece) in owner.iter().enumerate() {
			let kind = if index == last {
				kinds::MODULE
			} else {
				kinds::PACKAGE
			};
			builder.segment(kind, piece.as_bytes());
		}
		return Some(builder.build());
	}
	Some(external_or_sdk_target(module.as_view().project(), owner))
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
	target.as_view().segments().next().is_some_and(|segment| {
		matches!(segment.kind, kinds::EXTERNAL_PKG | crate::lang::kinds::SDK)
	})
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

pub(super) fn external_or_sdk_target(project: &[u8], pieces: &[&str]) -> Moniker {
	if !is_java_sdk_path(pieces) {
		return external_package_target(project, pieces);
	}
	let mut builder = crate::lang::sdk::sdk_target_builder(project, b"java");
	for piece in pieces {
		builder.segment(kinds::PATH, piece.as_bytes());
	}
	builder.build()
}

fn is_java_sdk_path(pieces: &[&str]) -> bool {
	matches!(pieces.first().copied(), Some("java" | "sun")) || is_javax_sdk_path(pieces)
}

pub(super) fn external_or_imported(pieces: &[&str]) -> &'static [u8] {
	if pieces.is_empty() {
		return kinds::CONF_IMPORTED;
	}
	if is_java_sdk_path(pieces) {
		kinds::CONF_EXTERNAL
	} else {
		kinds::CONF_IMPORTED
	}
}

// Conservative Java SE/JDK 21 ownership. `javax` is a shared historical
// namespace: JPA, Servlet, Validation, JAX-RS and JAXB are supplied by project
// dependencies on current JDKs, so the root itself is not enough evidence.
fn is_javax_sdk_path(pieces: &[&str]) -> bool {
	matches!(
		pieces,
		["javax", "accessibility", ..]
			| ["javax", "annotation", "processing", ..]
			| ["javax", "crypto", ..]
			| ["javax", "imageio", ..]
			| ["javax", "lang", "model", ..]
			| ["javax", "management", ..]
			| ["javax", "naming", ..]
			| ["javax", "net", ..]
			| ["javax", "print", ..]
			| ["javax", "rmi", "ssl", ..]
			| ["javax", "script", ..]
			| ["javax", "security", ..]
			| ["javax", "smartcardio", ..]
			| ["javax", "sound", ..]
			| ["javax", "sql", ..]
			| ["javax", "swing", ..]
			| ["javax", "tools", ..]
			| ["javax", "transaction", "xa", ..]
			| ["javax", "xml"]
			| ["javax", "xml", "catalog", ..]
			| ["javax", "xml", "crypto", ..]
			| ["javax", "xml", "datatype", ..]
			| ["javax", "xml", "namespace", ..]
			| ["javax", "xml", "parsers", ..]
			| ["javax", "xml", "stream", ..]
			| ["javax", "xml", "transform", ..]
			| ["javax", "xml", "validation", ..]
			| ["javax", "xml", "xpath", ..]
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn sdk_root_preserves_java_namespace_and_excludes_kotlin() {
		let java = external_or_sdk_target(b"app", &["java", "lang", "String"]);
		let java_segments = java.as_view().segments().collect::<Vec<_>>();
		assert_eq!(
			(java_segments[0].kind, java_segments[0].name),
			(crate::lang::kinds::SDK, b"java".as_slice())
		);
		assert_eq!(
			(java_segments[1].kind, java_segments[1].name),
			(kinds::PATH, b"java".as_slice())
		);

		let kotlin = external_or_sdk_target(b"app", &["kotlin", "collections", "List"]);
		let kotlin_root = kotlin.as_view().segments().next().unwrap();
		assert_eq!(
			(kotlin_root.kind, kotlin_root.name),
			(kinds::EXTERNAL_PKG, b"kotlin".as_slice())
		);
	}

	#[test]
	fn javax_sdk_ownership_is_conservative() {
		let crypto = external_or_sdk_target(b"app", &["javax", "crypto", "Cipher"]);
		assert_eq!(
			crypto.as_view().segments().next().unwrap().kind,
			crate::lang::kinds::SDK,
		);

		for third_party in [
			&["javax", "persistence", "Entity"][..],
			&["javax", "servlet", "Servlet"][..],
			&["javax", "validation", "Validator"][..],
			&["javax", "ws", "rs", "Path"][..],
			&["javax", "xml", "bind", "JAXBContext"][..],
		] {
			let target = external_or_sdk_target(b"app", third_party);
			assert_eq!(
				target.as_view().segments().next().unwrap().kind,
				kinds::EXTERNAL_PKG,
				"{third_party:?} must remain manifest-owned",
			);
			assert_eq!(external_or_imported(third_party), kinds::CONF_IMPORTED);
		}
	}

	#[test]
	fn com_sun_namespace_remains_manifest_owned() {
		for third_party in [
			&["com", "sun", "jersey", "api", "client", "Client"][..],
			&["com", "sun", "xml", "bind", "Marshaller"][..],
		] {
			let target = external_or_sdk_target(b"app", third_party);
			assert_eq!(
				target.as_view().segments().next().unwrap().kind,
				kinds::EXTERNAL_PKG,
				"{third_party:?} must remain manifest-owned",
			);
			assert_eq!(external_or_imported(third_party), kinds::CONF_IMPORTED);
		}
	}
}
