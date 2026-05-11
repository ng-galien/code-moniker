use std::collections::HashSet;

use super::{DeclSymbol, DeclareError, DeclareSpec, EdgeKind};
use crate::core::code_graph::{CodeGraph, DefAttrs, RefAttrs};
use crate::core::kinds::{
	BIND_INJECT, BIND_LOCAL, BIND_NONE, ORIGIN_DECLARED, REF_CALLS, REF_DI_REGISTER,
	REF_DI_REQUIRE, REF_IMPORTS_MODULE,
};
use crate::core::moniker::{Moniker, MonikerBuilder};
use crate::core::uri::{UriConfig, to_uri};

pub fn build_graph(spec: &DeclareSpec) -> Result<CodeGraph, DeclareError> {
	let mut declared: HashSet<Moniker> = HashSet::with_capacity(spec.symbols.len() + 1);
	declared.insert(spec.root.clone());
	for (i, sym) in spec.symbols.iter().enumerate() {
		validate_kind_agreement(sym, i)?;
		if !declared.insert(sym.moniker.clone()) {
			return Err(DeclareError::DuplicateMoniker {
				moniker: render_uri(&sym.moniker),
			});
		}
	}

	for (i, sym) in spec.symbols.iter().enumerate() {
		if !declared.contains(&sym.parent) {
			return Err(DeclareError::UnknownParent {
				path: format!("$.symbols[{i}].parent"),
				parent: render_uri(&sym.parent),
			});
		}
	}

	let mut ordered: Vec<&DeclSymbol> = spec.symbols.iter().collect();
	ordered.sort_by_key(|s| s.moniker.as_bytes().len());

	let mut graph = CodeGraph::new(spec.root.clone(), b"module");

	for sym in &ordered {
		let attrs = DefAttrs {
			visibility: sym.visibility.as_deref().unwrap_or("").as_bytes(),
			signature: sym.signature.as_deref().unwrap_or("").as_bytes(),
			binding: b"",
			origin: ORIGIN_DECLARED,
		};
		graph
			.add_def_attrs(
				sym.moniker.clone(),
				sym.kind.as_bytes(),
				&sym.parent,
				None,
				&attrs,
			)
			.map_err(|e| DeclareError::GraphError(e.to_string()))?;
	}

	for (i, edge) in spec.edges.iter().enumerate() {
		if !declared.contains(&edge.from) {
			return Err(DeclareError::UnknownEdgeSource {
				path: format!("$.edges[{i}].from"),
				from: render_uri(&edge.from),
			});
		}
		let (ref_kind, binding_override) = lower_edge(edge.kind, &edge.from, &edge.to);
		let attrs = RefAttrs {
			binding: binding_override,
			..RefAttrs::default()
		};
		graph
			.add_ref_attrs(&edge.from, edge.to.clone(), ref_kind, None, &attrs)
			.map_err(|e| DeclareError::GraphError(e.to_string()))?;
	}

	Ok(graph)
}

fn validate_kind_agreement(sym: &DeclSymbol, idx: usize) -> Result<(), DeclareError> {
	let last_kind = sym
		.moniker
		.last_kind()
		.ok_or_else(|| DeclareError::InvalidMoniker {
			path: format!("$.symbols[{idx}].moniker"),
			value: render_uri(&sym.moniker),
			reason: "moniker has no segments (cannot extract last kind)".to_string(),
		})?;
	let last_kind_str =
		std::str::from_utf8(&last_kind).map_err(|_| DeclareError::InvalidMoniker {
			path: format!("$.symbols[{idx}].moniker"),
			value: render_uri(&sym.moniker),
			reason: "last segment kind is not UTF-8".to_string(),
		})?;
	if last_kind_str != sym.kind {
		return Err(DeclareError::KindMismatchMoniker {
			path: format!("$.symbols[{idx}]"),
			declared_kind: sym.kind.clone(),
			moniker_last_kind: last_kind_str.to_string(),
		});
	}
	Ok(())
}

fn lower_edge(kind: EdgeKind, from: &Moniker, to: &Moniker) -> (&'static [u8], &'static [u8]) {
	match kind {
		EdgeKind::DependsOn => (REF_IMPORTS_MODULE, b""),
		EdgeKind::Calls => {
			let binding = if shares_module(from, to) {
				BIND_LOCAL
			} else {
				BIND_NONE
			};
			(REF_CALLS, binding)
		}
		EdgeKind::InjectsProvide => (REF_DI_REGISTER, BIND_INJECT),
		EdgeKind::InjectsRequire => (REF_DI_REQUIRE, BIND_INJECT),
	}
}

fn shares_module(a: &Moniker, b: &Moniker) -> bool {
	let am = module_anchor_bytes(a);
	let bm = module_anchor_bytes(b);
	match (am, bm) {
		(Some(x), Some(y)) => x == y,
		_ => false,
	}
}

fn module_anchor_bytes(m: &Moniker) -> Option<Vec<u8>> {
	let view = m.as_view();
	let mut anchor = MonikerBuilder::new();
	anchor.project(view.project());
	let mut found = false;
	for seg in view.segments() {
		anchor.segment(seg.kind, seg.name);
		if seg.kind == b"module" {
			found = true;
			break;
		}
	}
	if found {
		Some(anchor.build().into_bytes())
	} else {
		None
	}
}

fn render_uri(m: &Moniker) -> String {
	let cfg = UriConfig::default();
	to_uri(m, &cfg).unwrap_or_else(|_| format!("{:?}", m.as_bytes()))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::declare::{parse_moniker_uri, parse_spec};
	use crate::core::kinds::{ORIGIN_DECLARED, ORIGIN_EXTRACTED};
	use crate::core::moniker::MonikerBuilder;
	use serde_json::json;

	fn parse_uri(uri: &str) -> Moniker {
		parse_moniker_uri(uri).unwrap()
	}

	fn build_from_json(v: serde_json::Value) -> Result<CodeGraph, DeclareError> {
		let spec = parse_spec(&v)?;
		build_graph(&spec)
	}

	fn java_minimal() -> serde_json::Value {
		json!({
			"root": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo",
			"lang": "java",
			"symbols": [
				{
					"moniker": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
					"kind": "class",
					"parent": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo",
					"visibility": "public"
				}
			]
		})
	}

	#[test]
	fn build_minimal_spec_yields_root_plus_one_def() {
		let g = build_from_json(java_minimal()).unwrap();
		assert_eq!(g.def_count(), 2);
		assert_eq!(g.ref_count(), 0);
	}

	#[test]
	fn every_declared_def_has_origin_declared() {
		let g = build_from_json(java_minimal()).unwrap();
		let class_def = g.defs().nth(1).unwrap();
		assert_eq!(class_def.origin, ORIGIN_DECLARED.to_vec());
	}

	#[test]
	fn root_def_keeps_origin_extracted_for_now() {
		let g = build_from_json(java_minimal()).unwrap();
		let root_def = g.defs().next().unwrap();
		assert_eq!(root_def.origin, ORIGIN_EXTRACTED.to_vec());
	}

	#[test]
	fn rejects_kind_mismatch_with_moniker_last_segment() {
		let v = json!({
			"root": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo",
			"lang": "java",
			"symbols": [{
				"moniker": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
				"kind": "interface",
				"parent": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo"
			}]
		});
		let err = build_from_json(v).unwrap_err();
		assert!(matches!(
			err,
			DeclareError::KindMismatchMoniker { ref declared_kind, ref moniker_last_kind, .. }
				if declared_kind == "interface" && moniker_last_kind == "class"
		));
	}

	#[test]
	fn rejects_unknown_parent() {
		let v = json!({
			"root": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo",
			"lang": "java",
			"symbols": [{
				"moniker": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo/method:bar()",
				"kind": "method",
				"parent": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:DoesNotExist"
			}]
		});
		let err = build_from_json(v).unwrap_err();
		assert!(matches!(err, DeclareError::UnknownParent { .. }));
	}

	#[test]
	fn rejects_duplicate_moniker_in_symbols() {
		let v = json!({
			"root": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo",
			"lang": "java",
			"symbols": [
				{
					"moniker": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
					"kind": "class",
					"parent": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo"
				},
				{
					"moniker": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
					"kind": "class",
					"parent": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo"
				}
			]
		});
		let err = build_from_json(v).unwrap_err();
		assert!(matches!(err, DeclareError::DuplicateMoniker { .. }));
	}

	#[test]
	fn out_of_order_symbols_are_topologically_sorted() {
		let v = json!({
			"root": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo",
			"lang": "java",
			"symbols": [
				{
					"moniker": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo/method:bar()",
					"kind": "method",
					"parent": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo"
				},
				{
					"moniker": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
					"kind": "class",
					"parent": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo"
				}
			]
		});
		let g = build_from_json(v).unwrap();
		assert_eq!(g.def_count(), 3);
	}

	#[test]
	fn calls_intra_module_get_local_binding() {
		let v = json!({
			"root": "code+moniker://app/srcset:main/lang:rs/module:svc",
			"lang": "rs",
			"symbols": [
				{
					"moniker": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
					"kind": "fn",
					"parent": "code+moniker://app/srcset:main/lang:rs/module:svc"
				},
				{
					"moniker": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:g()",
					"kind": "fn",
					"parent": "code+moniker://app/srcset:main/lang:rs/module:svc"
				}
			],
			"edges": [{
				"from": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "calls",
				"to":   "code+moniker://app/srcset:main/lang:rs/module:svc/fn:g()"
			}]
		});
		let g = build_from_json(v).unwrap();
		let r = g.refs().next().unwrap();
		assert_eq!(r.binding, b"local".to_vec());
	}

	#[test]
	fn calls_cross_module_get_none_binding() {
		let v = json!({
			"root": "code+moniker://app/srcset:main/lang:rs/module:svc",
			"lang": "rs",
			"symbols": [{
				"moniker": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "fn",
				"parent": "code+moniker://app/srcset:main/lang:rs/module:svc"
			}],
			"edges": [{
				"from": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "calls",
				"to":   "code+moniker://app/srcset:main/lang:rs/module:other/fn:g()"
			}]
		});
		let g = build_from_json(v).unwrap();
		let r = g.refs().next().unwrap();
		assert_eq!(r.binding, b"none".to_vec());
	}

	#[test]
	fn depends_on_lowers_to_imports_module_with_import_binding() {
		let v = json!({
			"root": "code+moniker://app/srcset:main/lang:rs/module:svc",
			"lang": "rs",
			"symbols": [{
				"moniker": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "fn",
				"parent": "code+moniker://app/srcset:main/lang:rs/module:svc"
			}],
			"edges": [{
				"from": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "depends_on",
				"to":   "code+moniker://app/external_pkg:cargo/path:serde"
			}]
		});
		let g = build_from_json(v).unwrap();
		let r = g.refs().next().unwrap();
		assert_eq!(r.kind, b"imports_module".to_vec());
		assert_eq!(r.binding, b"import".to_vec());
	}

	#[test]
	fn injects_provide_lowers_to_di_register_with_inject_binding() {
		let v = json!({
			"root": "code+moniker://app/srcset:main/lang:rs/module:svc",
			"lang": "rs",
			"symbols": [{
				"moniker": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "fn",
				"parent": "code+moniker://app/srcset:main/lang:rs/module:svc"
			}],
			"edges": [{
				"from": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "injects:provide",
				"to":   "code+moniker://app/srcset:main/lang:rs/module:other/trait:T"
			}]
		});
		let g = build_from_json(v).unwrap();
		let r = g.refs().next().unwrap();
		assert_eq!(r.kind, b"di_register".to_vec());
		assert_eq!(r.binding, b"inject".to_vec());
	}

	#[test]
	fn injects_require_lowers_to_di_require_with_inject_binding() {
		let v = json!({
			"root": "code+moniker://app/srcset:main/lang:rs/module:svc",
			"lang": "rs",
			"symbols": [{
				"moniker": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "fn",
				"parent": "code+moniker://app/srcset:main/lang:rs/module:svc"
			}],
			"edges": [{
				"from": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "injects:require",
				"to":   "code+moniker://app/srcset:main/lang:rs/module:other/trait:U"
			}]
		});
		let g = build_from_json(v).unwrap();
		let r = g.refs().next().unwrap();
		assert_eq!(r.kind, b"di_require".to_vec());
		assert_eq!(r.binding, b"inject".to_vec());
	}

	#[test]
	fn rejects_edge_from_undeclared_symbol() {
		let v = json!({
			"root": "code+moniker://app/srcset:main/lang:rs/module:svc",
			"lang": "rs",
			"symbols": [],
			"edges": [{
				"from": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:undeclared()",
				"kind": "calls",
				"to":   "code+moniker://app/srcset:main/lang:rs/module:other/fn:g()"
			}]
		});
		let err = build_from_json(v).unwrap_err();
		assert!(matches!(err, DeclareError::UnknownEdgeSource { .. }));
	}

	#[test]
	fn edge_to_unknown_target_is_accepted() {
		let v = json!({
			"root": "code+moniker://app/srcset:main/lang:rs/module:svc",
			"lang": "rs",
			"symbols": [{
				"moniker": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "fn",
				"parent": "code+moniker://app/srcset:main/lang:rs/module:svc"
			}],
			"edges": [{
				"from": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "calls",
				"to":   "code+moniker://app/srcset:main/lang:rs/module:never_extracted/fn:phantom()"
			}]
		});
		assert!(build_from_json(v).is_ok());
	}

	#[test]
	fn shares_module_handles_nested_class_in_module() {
		let svc_f =
			parse_uri("code+moniker://app/srcset:main/lang:rs/module:svc/class:C/method:f()");
		let svc_g =
			parse_uri("code+moniker://app/srcset:main/lang:rs/module:svc/class:C/method:g()");
		assert!(shares_module(&svc_f, &svc_g));
	}

	#[test]
	fn shares_module_returns_false_when_no_module_segment() {
		let java_a =
			parse_uri("code+moniker://app/srcset:main/lang:java/package:com/class:A/method:f()");
		let java_b =
			parse_uri("code+moniker://app/srcset:main/lang:java/package:com/class:A/method:g()");
		assert!(!shares_module(&java_a, &java_b));
	}

	#[test]
	fn declared_def_bind_matches_extracted_def_with_same_moniker() {
		let m1 = MonikerBuilder::new()
			.project(b"app")
			.segment(b"srcset", b"main")
			.segment(b"lang", b"java")
			.segment(b"package", b"com")
			.segment(b"module", b"Foo")
			.segment(b"class", b"Foo")
			.build();
		let m2 = MonikerBuilder::new()
			.project(b"app")
			.segment(b"srcset", b"main")
			.segment(b"lang", b"java")
			.segment(b"package", b"com")
			.segment(b"module", b"Foo")
			.segment(b"class", b"Foo")
			.build();
		assert!(m1.bind_match(&m2));
	}
}
