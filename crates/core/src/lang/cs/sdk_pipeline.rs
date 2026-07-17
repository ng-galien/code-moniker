//! SDK-backed C# extraction pipeline.
//!
//! The C# semantic strategy still owns tree-sitter classification while this
//! compatibility phase translates its discoveries into the shared SDK IR.
//! Keeping that boundary explicit lets the semantic passes move independently
//! without changing the public graph contract.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::lang::canonical_walker::CanonicalWalker;
use crate::lang::sdk::{
	DiscoveredDef, DiscoveredFile, GraphEmitter, ImportTable, Namespace, RefHints, ResolvedRef,
	ScopeTree,
};

use super::canonicalize::compute_module_moniker;
use super::kinds;
use super::strategy::{Strategy, collect_callable_table, collect_type_table};

pub(super) fn extract(uri: &str, source: &str, anchor: &Moniker, deep: bool) -> CodeGraph {
	let tree = super::parse(source);
	let module = compute_module_moniker(anchor, uri);
	let (def_cap, ref_cap) = CodeGraph::capacity_for_source(source.len());
	let mut legacy = CodeGraph::with_capacity(module.clone(), kinds::MODULE, def_cap, ref_cap);
	let mut type_table = HashMap::new();
	collect_type_table(
		tree.root_node(),
		source.as_bytes(),
		&module,
		&mut type_table,
	);
	let mut callable_table = HashMap::new();
	let mut callable_metadata = HashMap::new();
	collect_callable_table(
		tree.root_node(),
		source.as_bytes(),
		&module,
		&mut callable_table,
		&mut callable_metadata,
	);
	let strategy = Strategy {
		module: module.clone(),
		source_bytes: source.as_bytes(),
		deep,
		imports: RefCell::new(HashMap::new()),
		local_scope: RefCell::new(Vec::new()),
		type_table,
		callable_table,
	};
	CanonicalWalker::new(&strategy, source.as_bytes()).walk(tree.root_node(), &module, &mut legacy);

	let (discovered, refs) = into_sdk(legacy, module, &callable_metadata);
	GraphEmitter::emit(&discovered, &refs)
		.unwrap_or_else(|err| panic!("C# SDK graph emission failed: {err}"))
}

fn into_sdk(
	graph: CodeGraph,
	root: Moniker,
	callables: &HashMap<Moniker, (Vec<u8>, Option<usize>)>,
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
	callables: &HashMap<Moniker, (Vec<u8>, Option<usize>)>,
) -> Vec<DiscoveredDef> {
	graph
		.defs()
		.enumerate()
		.skip(1)
		.map(|(_, def)| {
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
			let (call_name, call_arity) =
				callables.get(&def.moniker).cloned().unwrap_or_else(|| {
					callable_metadata(kind, &name, def.call_name.as_ref(), def.call_arity)
				});
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

fn callable_metadata(
	kind: &[u8],
	name: &[u8],
	call_name: &[u8],
	call_arity: Option<usize>,
) -> (Vec<u8>, Option<usize>) {
	if !call_name.is_empty() || call_arity.is_some() {
		return (call_name.to_vec(), call_arity);
	}
	if !matches!(kind, b"method" | b"constructor") {
		return (Vec::new(), None);
	}
	let bare = crate::core::moniker::query::bare_callable_name(name).to_vec();
	(bare, None)
}

fn namespace_for(kind: &[u8]) -> Namespace {
	match kind {
		b"class" | b"interface" | b"struct" | b"record" | b"enum" | b"delegate" => Namespace::Type,
		b"module" => Namespace::Module,
		_ => Namespace::Value,
	}
}

fn static_kind(kind: &[u8]) -> &'static [u8] {
	match kind {
		b"class" => b"class",
		b"interface" => b"interface",
		b"struct" => b"struct",
		b"record" => b"record",
		b"enum" => b"enum",
		b"delegate" => b"delegate",
		b"method" => b"method",
		b"constructor" => b"constructor",
		b"field" => b"field",
		b"property" => b"property",
		b"event" => b"event",
		b"enum_constant" => b"enum_constant",
		b"param" => b"param",
		b"local" => b"local",
		b"comment" => b"comment",
		other => panic!(
			"unsupported C# SDK definition kind: {}",
			String::from_utf8_lossy(other)
		),
	}
}

fn static_ref_kind(kind: &[u8]) -> &'static [u8] {
	match kind {
		b"annotates" => b"annotates",
		b"calls" => b"calls",
		b"extends" => b"extends",
		b"imports_module" => b"imports_module",
		b"instantiates" => b"instantiates",
		b"method_call" => b"method_call",
		b"typed_as" => b"typed_as",
		b"uses_type" => b"uses_type",
		other => panic!(
			"unsupported C# SDK reference kind: {}",
			String::from_utf8_lossy(other)
		),
	}
}

fn static_visibility(visibility: &[u8]) -> &'static [u8] {
	match visibility {
		b"public" => b"public",
		b"protected" => b"protected",
		b"package" => b"package",
		b"private" => b"private",
		b"" => b"",
		other => panic!(
			"unsupported C# SDK visibility: {}",
			String::from_utf8_lossy(other)
		),
	}
}

fn static_confidence(confidence: &[u8]) -> &'static [u8] {
	match confidence {
		b"external" => b"external",
		b"imported" => b"imported",
		b"local" => b"local",
		b"name_match" => b"name_match",
		b"resolved" => b"resolved",
		b"unresolved" => b"unresolved",
		b"" => b"",
		other => panic!(
			"unsupported C# SDK confidence: {}",
			String::from_utf8_lossy(other)
		),
	}
}
