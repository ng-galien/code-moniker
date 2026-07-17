//! SDK-backed SQL extraction pipeline.
//!
//! The SQL semantic strategy still owns tree-sitter classification while this
//! compatibility phase translates its discoveries into the shared SDK IR.
//! This keeps the graph contract stable while SQL resolution moves onto the
//! same linkage and audit surfaces as SDK-native languages.

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::lang::canonical_walker::CanonicalWalker;
use crate::lang::sdk::{
	DiscoveredDef, DiscoveredFile, GraphEmitter, ImportTable, Namespace, RefHints, ResolvedRef,
	ScopeTree,
};

use super::Presets;
use super::canonicalize::compute_module_moniker;
use super::kinds;
use super::strategy::{Strategy, collect_callable_metadata, parse};

pub(super) fn extract(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	_deep: bool,
	_presets: &Presets,
) -> CodeGraph {
	let tree = parse(source);
	let module = compute_module_moniker(anchor, uri);
	let (def_cap, ref_cap) = CodeGraph::capacity_for_source(source.len());
	let mut legacy = CodeGraph::with_capacity(module.clone(), kinds::MODULE, def_cap, ref_cap);
	let (callable_metadata, search_paths) =
		collect_callable_metadata(tree.root_node(), source.as_bytes(), &module);
	let strategy = Strategy {
		module: module.clone(),
		source_str: source,
		emit_comments: true,
		search_paths: &search_paths,
	};
	CanonicalWalker::new(&strategy, source.as_bytes()).walk(tree.root_node(), &module, &mut legacy);

	let (discovered, refs) = into_sdk(legacy, module, &callable_metadata);
	GraphEmitter::emit(&discovered, &refs)
		.unwrap_or_else(|err| panic!("SQL SDK graph emission failed: {err}"))
}

fn into_sdk(
	graph: CodeGraph,
	root: Moniker,
	callables: &super::strategy::CallableMetadata,
) -> (DiscoveredFile, Vec<ResolvedRef>) {
	let defs = discovered_defs(&graph, &root, callables);
	let refs = resolved_refs(&graph);
	let discovered = DiscoveredFile::new(
		root.clone(),
		kinds::MODULE,
		defs,
		ScopeTree::new(root),
		ImportTable::default(),
	);
	(discovered, refs)
}

fn discovered_defs(
	graph: &CodeGraph,
	root: &Moniker,
	callables: &super::strategy::CallableMetadata,
) -> Vec<DiscoveredDef> {
	graph
		.defs()
		.skip(1)
		.map(|def| {
			let parent = def
				.parent
				.map(|index| graph.def_at(index).moniker.clone())
				.unwrap_or_else(|| root.clone());
			let name = def
				.moniker
				.as_view()
				.segments()
				.last()
				.map(|segment| segment.name.to_vec())
				.unwrap_or_default();
			let kind = static_kind(def.kind.as_ref());
			let (call_name, call_arity) = callables
				.get(&def.moniker)
				.cloned()
				.unwrap_or_else(|| callable_metadata(kind, &name));
			DiscoveredDef {
				moniker: def.moniker.clone(),
				parent,
				namespace: namespace_for(kind),
				name,
				kind,
				visibility: static_visibility(def.visibility.as_ref()),
				signature: def.signature.to_vec(),
				position: def.position,
				call_name,
				call_arity,
			}
		})
		.collect()
}

fn resolved_refs(graph: &CodeGraph) -> Vec<ResolvedRef> {
	graph
		.refs()
		.map(|reference| ResolvedRef {
			source: graph.def_at(reference.source).moniker.clone(),
			target: reference.target.clone(),
			kind: static_ref_kind(reference.kind.as_ref()),
			position: reference.position,
			confidence: static_confidence(reference.confidence.as_ref()),
			hints: RefHints {
				receiver_hint: reference.receiver_hint.to_vec(),
				alias: reference.alias.to_vec(),
				namespace: None,
				call_name: reference.call_name.to_vec(),
				call_arity: reference.call_arity,
			},
		})
		.collect()
}

fn callable_metadata(kind: &[u8], name: &[u8]) -> (Vec<u8>, Option<usize>) {
	if !matches!(kind, b"function" | b"procedure") {
		return (Vec::new(), None);
	}
	(
		crate::core::moniker::query::bare_callable_name(name).to_vec(),
		None,
	)
}

fn namespace_for(kind: &[u8]) -> Namespace {
	match kind {
		b"schema" | b"module" => Namespace::Module,
		b"table" | b"view" => Namespace::Type,
		_ => Namespace::Value,
	}
}

fn static_kind(kind: &[u8]) -> &'static [u8] {
	match kind {
		b"schema" => b"schema",
		b"table" => b"table",
		b"view" => b"view",
		b"type" => b"type",
		b"function" => b"function",
		b"procedure" => b"procedure",
		b"comment" => b"comment",
		other => panic!(
			"unsupported SQL SDK definition kind: {}",
			String::from_utf8_lossy(other)
		),
	}
}

fn static_ref_kind(kind: &[u8]) -> &'static [u8] {
	match kind {
		b"calls" => b"calls",
		b"uses_type" => b"uses_type",
		other => panic!(
			"unsupported SQL SDK reference kind: {}",
			String::from_utf8_lossy(other)
		),
	}
}

fn static_visibility(visibility: &[u8]) -> &'static [u8] {
	match visibility {
		b"" => b"",
		other => panic!(
			"unsupported SQL SDK visibility: {}",
			String::from_utf8_lossy(other)
		),
	}
}

fn static_confidence(confidence: &[u8]) -> &'static [u8] {
	match confidence {
		b"external" => b"external",
		b"name_match" => b"name_match",
		b"resolved" => b"resolved",
		b"" => b"",
		other => panic!(
			"unsupported SQL SDK confidence: {}",
			String::from_utf8_lossy(other)
		),
	}
}
