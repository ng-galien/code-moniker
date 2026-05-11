use serde_json::{Value, json};

use super::{EdgeKind, Lang};
use crate::core::code_graph::CodeGraph;
use crate::core::kinds::{REF_CALLS, REF_DI_REGISTER, REF_DI_REQUIRE, REF_IMPORTS_MODULE};
use crate::core::moniker::Moniker;
use crate::core::uri::{UriConfig, to_uri};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SerializeError {
	RootHasNoLangSegment {
		root: String,
	},
	UnknownLangSegment {
		lang: String,
	},
	LangMismatch {
		expected: &'static str,
		actual: String,
	},
	UriRender {
		reason: String,
	},
	Utf8 {
		what: &'static str,
	},
}

impl std::fmt::Display for SerializeError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::RootHasNoLangSegment { root } => write!(
				f,
				"graph root `{root}` has no `lang:` segment ; cannot infer the spec's `lang` field"
			),
			Self::UnknownLangSegment { lang } => write!(
				f,
				"graph root carries `lang:{lang}` which is not a recognised declarative profile"
			),
			Self::LangMismatch { expected, actual } => write!(
				f,
				"graph root carries `lang:{actual}` which does not match the typed extractor's `{expected}` (use the dynamic-dispatch entry point if you do not know the language ahead of time)"
			),
			Self::UriRender { reason } => write!(f, "moniker URI render error: {reason}"),
			Self::Utf8 { what } => write!(f, "{what} contains non-UTF-8 bytes"),
		}
	}
}

impl std::error::Error for SerializeError {}

pub fn graph_to_spec(graph: &CodeGraph) -> Result<Value, SerializeError> {
	let root = graph.root();
	let lang = lang_from_root(root)?;
	let cfg = UriConfig::default();
	let defs: Vec<&_> = graph.defs().collect();

	let mut symbols: Vec<Value> = Vec::with_capacity(defs.len().saturating_sub(1));
	for (i, d) in defs.iter().enumerate() {
		if i == 0 {
			continue;
		}
		let parent_moniker = defs
			.get(d.parent.unwrap_or(0))
			.map(|p| &p.moniker)
			.unwrap_or(root);
		let mut sym = serde_json::Map::with_capacity(5);
		sym.insert(
			"moniker".to_string(),
			Value::String(render(&d.moniker, &cfg)?),
		);
		sym.insert(
			"kind".to_string(),
			Value::String(utf8(&d.kind, "def kind")?.to_string()),
		);
		sym.insert(
			"parent".to_string(),
			Value::String(render(parent_moniker, &cfg)?),
		);
		if !d.visibility.is_empty() {
			sym.insert(
				"visibility".to_string(),
				Value::String(utf8(&d.visibility, "def visibility")?.to_string()),
			);
		}
		if !d.signature.is_empty() {
			sym.insert(
				"signature".to_string(),
				Value::String(utf8(&d.signature, "def signature")?.to_string()),
			);
		}
		symbols.push(Value::Object(sym));
	}

	let mut edges: Vec<Value> = Vec::with_capacity(graph.ref_count());
	for r in graph.refs() {
		let Some(canonical) = lift_ref_kind(&r.kind) else {
			continue;
		};
		let from_moniker = &defs
			.get(r.source)
			.ok_or_else(|| SerializeError::UriRender {
				reason: format!("ref source index {} out of bounds", r.source),
			})?
			.moniker;
		edges.push(json!({
			"from": render(from_moniker, &cfg)?,
			"kind": canonical.tag(),
			"to":   render(&r.target, &cfg)?,
		}));
	}

	Ok(json!({
		"root":    render(root, &cfg)?,
		"lang":    lang.tag(),
		"symbols": symbols,
		"edges":   edges,
	}))
}

fn lang_from_root(root: &Moniker) -> Result<Lang, SerializeError> {
	let cfg = UriConfig::default();
	let view = root.as_view();
	let lang_bytes = view
		.lang_segment()
		.ok_or_else(|| SerializeError::RootHasNoLangSegment {
			root: render(root, &cfg).unwrap_or_else(|_| "<unrenderable>".to_string()),
		})?;
	let lang_str = std::str::from_utf8(lang_bytes).map_err(|_| SerializeError::Utf8 {
		what: "lang segment",
	})?;
	Lang::from_tag(lang_str).ok_or_else(|| SerializeError::UnknownLangSegment {
		lang: lang_str.to_string(),
	})
}

fn lift_ref_kind(kind: &[u8]) -> Option<EdgeKind> {
	match kind {
		k if k == REF_IMPORTS_MODULE => Some(EdgeKind::DependsOn),
		k if k == REF_CALLS => Some(EdgeKind::Calls),
		k if k == REF_DI_REGISTER => Some(EdgeKind::InjectsProvide),
		k if k == REF_DI_REQUIRE => Some(EdgeKind::InjectsRequire),
		_ => None,
	}
}

fn render(m: &Moniker, cfg: &UriConfig<'_>) -> Result<String, SerializeError> {
	to_uri(m, cfg).map_err(|e| SerializeError::UriRender {
		reason: e.to_string(),
	})
}

fn utf8<'a>(bytes: &'a [u8], what: &'static str) -> Result<&'a str, SerializeError> {
	std::str::from_utf8(bytes).map_err(|_| SerializeError::Utf8 { what })
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::declare::{declare_from_json_value, parse_spec};
	use serde_json::json;

	fn round_trip(input: Value) -> Value {
		let g = declare_from_json_value(&input).unwrap();
		graph_to_spec(&g).unwrap()
	}

	#[test]
	fn lang_field_is_inferred_from_root_lang_segment() {
		let v = json!({
			"root": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo",
			"lang": "java",
			"symbols": []
		});
		let out = round_trip(v);
		assert_eq!(out.get("lang").unwrap().as_str().unwrap(), "java");
	}

	#[test]
	fn root_field_is_preserved() {
		let root = "code+moniker://app/srcset:main/lang:java/package:com/module:Foo";
		let v = json!({
			"root": root,
			"lang": "java",
			"symbols": []
		});
		let out = round_trip(v);
		assert_eq!(out.get("root").unwrap().as_str().unwrap(), root);
	}

	#[test]
	fn symbols_are_emitted_for_each_non_root_def() {
		let v = json!({
			"root": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo",
			"lang": "java",
			"symbols": [
				{
					"moniker": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
					"kind": "class",
					"parent": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo",
					"visibility": "public"
				},
				{
					"moniker": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo/method:bar()",
					"kind": "method",
					"parent": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
					"visibility": "public",
					"signature": "bar(): void"
				}
			]
		});
		let out = round_trip(v);
		let symbols = out.get("symbols").unwrap().as_array().unwrap();
		assert_eq!(symbols.len(), 2);
	}

	#[test]
	fn edges_lift_canonical_kinds() {
		let v = json!({
			"root": "code+moniker://app/srcset:main/lang:rs/module:svc",
			"lang": "rs",
			"symbols": [{
				"moniker": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "fn",
				"parent": "code+moniker://app/srcset:main/lang:rs/module:svc"
			}],
			"edges": [
				{ "from": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				  "kind": "depends_on",
				  "to":   "code+moniker://app/external_pkg:cargo/path:serde" },
				{ "from": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				  "kind": "calls",
				  "to":   "code+moniker://app/srcset:main/lang:rs/module:other/fn:g()" },
				{ "from": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				  "kind": "injects:provide",
				  "to":   "code+moniker://app/srcset:main/lang:rs/module:di/trait:Repo" },
				{ "from": "code+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				  "kind": "injects:require",
				  "to":   "code+moniker://app/srcset:main/lang:rs/module:di/trait:Bus" }
			]
		});
		let out = round_trip(v);
		let edges = out.get("edges").unwrap().as_array().unwrap();
		assert_eq!(edges.len(), 4);
		let kinds: Vec<&str> = edges
			.iter()
			.map(|e| e.get("kind").unwrap().as_str().unwrap())
			.collect();
		assert!(kinds.contains(&"depends_on"));
		assert!(kinds.contains(&"calls"));
		assert!(kinds.contains(&"injects:provide"));
		assert!(kinds.contains(&"injects:require"));
	}

	#[test]
	fn non_canonical_ref_kinds_are_dropped() {
		use crate::core::code_graph::CodeGraph;
		use crate::core::moniker::MonikerBuilder;
		let root = MonikerBuilder::new()
			.project(b"app")
			.segment(b"srcset", b"main")
			.segment(b"lang", b"rs")
			.segment(b"module", b"svc")
			.build();
		let foo = MonikerBuilder::from_view(root.as_view())
			.segment(b"fn", b"f()")
			.build();
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(foo.clone(), b"fn", &root, None).unwrap();
		g.add_ref(
			&foo,
			MonikerBuilder::new()
				.project(b"app")
				.segment(b"srcset", b"main")
				.segment(b"lang", b"rs")
				.segment(b"module", b"svc")
				.segment(b"struct", b"X")
				.build(),
			b"uses_type",
			None,
		)
		.unwrap();

		let out = graph_to_spec(&g).unwrap();
		let edges = out.get("edges").unwrap().as_array().unwrap();
		assert!(edges.is_empty(), "non-canonical refs should be dropped");
	}

	#[test]
	fn errors_when_root_has_no_lang_segment() {
		// Build a graph whose root is a project-regime moniker (no lang:).
		use crate::core::code_graph::CodeGraph;
		use crate::core::moniker::MonikerBuilder;
		let root = MonikerBuilder::new()
			.project(b"app")
			.segment(b"srcset", b"main")
			.build();
		let g = CodeGraph::new(root, b"srcset");
		let err = graph_to_spec(&g).unwrap_err();
		assert!(matches!(err, SerializeError::RootHasNoLangSegment { .. }));
	}

	#[test]
	fn round_trip_preserves_structure() {
		let original = json!({
			"root": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo",
			"lang": "java",
			"symbols": [
				{
					"moniker": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
					"kind": "class",
					"parent": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo",
					"visibility": "public"
				},
				{
					"moniker": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo/method:bar()",
					"kind": "method",
					"parent": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
					"visibility": "public"
				}
			],
			"edges": [
				{
					"from": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo/method:bar()",
					"kind": "calls",
					"to":   "code+moniker://app/srcset:main/lang:java/package:com/module:Other/class:Other/method:baz()"
				}
			]
		});
		let g1 = declare_from_json_value(&original).unwrap();
		let spec1 = graph_to_spec(&g1).unwrap();
		// Re-parse the output: it must still be a valid spec.
		let _ = parse_spec(&spec1).unwrap();
		let g2 = declare_from_json_value(&spec1).unwrap();
		let spec2 = graph_to_spec(&g2).unwrap();
		// Second round must equal first round (idempotent).
		assert_eq!(spec1, spec2);
	}

	#[test]
	fn declared_origin_preserved_after_round_trip() {
		let v = json!({
			"root": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo",
			"lang": "java",
			"symbols": [{
				"moniker": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
				"kind": "class",
				"parent": "code+moniker://app/srcset:main/lang:java/package:com/module:Foo",
				"visibility": "public"
			}]
		});
		let g1 = declare_from_json_value(&v).unwrap();
		let spec = graph_to_spec(&g1).unwrap();
		let g2 = declare_from_json_value(&spec).unwrap();
		// The class def in g2 must have origin=declared (because re-declared).
		let class_def = g2.defs().nth(1).unwrap();
		assert_eq!(
			class_def.origin,
			crate::core::kinds::ORIGIN_DECLARED.to_vec()
		);
	}
}
