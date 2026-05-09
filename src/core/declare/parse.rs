use serde_json::{Map, Value};

use super::{DeclEdge, DeclSymbol, DeclareError, DeclareSpec, EdgeKind, Lang};
use crate::core::moniker::Moniker;

pub fn parse_spec(value: &Value) -> Result<DeclareSpec, DeclareError> {
	let obj = value.as_object().ok_or(DeclareError::NotAnObject("spec"))?;

	let lang_str = req_str(obj, "$", "lang")?;
	let lang =
		Lang::from_tag(lang_str).ok_or_else(|| DeclareError::UnknownLang(lang_str.to_string()))?;

	let root_str = req_str(obj, "$", "root")?;
	let root = parse_moniker_uri(root_str, "$.root")?;

	let symbols_val = obj.get("symbols").ok_or(DeclareError::MissingField {
		path: "$".to_string(),
		field: "symbols",
	})?;
	let symbols_arr = symbols_val.as_array().ok_or(DeclareError::InvalidType {
		path: "$.symbols".to_string(),
		expected: "array",
	})?;
	let symbols: Vec<DeclSymbol> = symbols_arr
		.iter()
		.enumerate()
		.map(|(i, v)| parse_symbol(v, &format!("$.symbols[{i}]"), lang))
		.collect::<Result<_, _>>()?;

	let edges = match obj.get("edges") {
		None | Some(Value::Null) => Vec::new(),
		Some(v) => {
			let arr = v.as_array().ok_or(DeclareError::InvalidType {
				path: "$.edges".to_string(),
				expected: "array",
			})?;
			arr.iter()
				.enumerate()
				.map(|(i, ev)| parse_edge(ev, &format!("$.edges[{i}]")))
				.collect::<Result<_, _>>()?
		}
	};

	Ok(DeclareSpec {
		root,
		lang,
		symbols,
		edges,
	})
}

fn parse_symbol(value: &Value, path: &str, lang: Lang) -> Result<DeclSymbol, DeclareError> {
	let obj = value.as_object().ok_or(DeclareError::InvalidType {
		path: path.to_string(),
		expected: "object",
	})?;

	let moniker_str = req_str(obj, path, "moniker")?;
	let moniker = parse_moniker_uri(moniker_str, &format!("{path}.moniker"))?;

	let kind = req_str(obj, path, "kind")?.to_string();
	if !lang.allowed_kinds().contains(&kind.as_str()) {
		return Err(DeclareError::KindNotInProfile { lang, kind });
	}

	let parent_str = req_str(obj, path, "parent")?;
	let parent = parse_moniker_uri(parent_str, &format!("{path}.parent"))?;

	let visibility = match obj.get("visibility") {
		None | Some(Value::Null) => None,
		Some(v) => {
			let s = v.as_str().ok_or(DeclareError::InvalidType {
				path: format!("{path}.visibility"),
				expected: "string",
			})?;
			if !lang.ignores_visibility() && !lang.allowed_visibilities().contains(&s) {
				return Err(DeclareError::VisibilityNotInProfile {
					lang,
					visibility: s.to_string(),
				});
			}
			Some(s.to_string())
		}
	};

	let signature = match obj.get("signature") {
		None | Some(Value::Null) => None,
		Some(v) => Some(
			v.as_str()
				.ok_or(DeclareError::InvalidType {
					path: format!("{path}.signature"),
					expected: "string",
				})?
				.to_string(),
		),
	};

	Ok(DeclSymbol {
		moniker,
		kind,
		parent,
		visibility,
		signature,
	})
}

fn parse_edge(value: &Value, path: &str) -> Result<DeclEdge, DeclareError> {
	let obj = value.as_object().ok_or(DeclareError::InvalidType {
		path: path.to_string(),
		expected: "object",
	})?;

	let from_str = req_str(obj, path, "from")?;
	let from = parse_moniker_uri(from_str, &format!("{path}.from"))?;

	let kind_str = req_str(obj, path, "kind")?;
	let kind = EdgeKind::from_tag(kind_str)
		.ok_or_else(|| DeclareError::UnknownEdgeKind(kind_str.to_string()))?;

	let to_str = req_str(obj, path, "to")?;
	let to = parse_moniker_uri(to_str, &format!("{path}.to"))?;

	Ok(DeclEdge { from, kind, to })
}

fn req_str<'a>(
	obj: &'a Map<String, Value>,
	path: &str,
	field: &'static str,
) -> Result<&'a str, DeclareError> {
	let v = obj.get(field).ok_or(DeclareError::MissingField {
		path: path.to_string(),
		field,
	})?;
	v.as_str().ok_or(DeclareError::InvalidType {
		path: format!("{path}.{field}"),
		expected: "string",
	})
}

fn parse_moniker_uri(uri: &str, path: &str) -> Result<Moniker, DeclareError> {
	if !uri.contains("://") {
		return Err(DeclareError::InvalidMoniker {
			path: path.to_string(),
			value: uri.to_string(),
			reason: "URI must contain `://`".to_string(),
		});
	}
	super::parse_moniker_uri(uri).map_err(|e| DeclareError::InvalidMoniker {
		path: path.to_string(),
		value: uri.to_string(),
		reason: e.to_string(),
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	fn minimal_spec() -> Value {
		json!({
			"root": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
			"lang": "java",
			"symbols": [
				{
					"moniker": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
					"kind": "class",
					"parent": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
					"visibility": "public"
				}
			]
		})
	}

	#[test]
	fn parses_minimal_java_spec() {
		let s = parse_spec(&minimal_spec()).unwrap();
		assert_eq!(s.lang, Lang::Java);
		assert_eq!(s.symbols.len(), 1);
		assert_eq!(s.symbols[0].kind, "class");
		assert!(s.edges.is_empty());
	}

	#[test]
	fn rejects_missing_root() {
		let mut v = minimal_spec();
		v.as_object_mut().unwrap().remove("root");
		let err = parse_spec(&v).unwrap_err();
		assert!(matches!(
			err,
			DeclareError::MissingField { field, .. } if field == "root"
		));
	}

	#[test]
	fn rejects_missing_lang() {
		let mut v = minimal_spec();
		v.as_object_mut().unwrap().remove("lang");
		let err = parse_spec(&v).unwrap_err();
		assert!(matches!(
			err,
			DeclareError::MissingField { field, .. } if field == "lang"
		));
	}

	#[test]
	fn rejects_unknown_lang() {
		let v = json!({
			"root": "pcm+moniker://app/foo:bar",
			"lang": "cobol",
			"symbols": []
		});
		let err = parse_spec(&v).unwrap_err();
		assert!(matches!(err, DeclareError::UnknownLang(s) if s == "cobol"));
	}

	#[test]
	fn rejects_kind_outside_profile() {
		let v = json!({
			"root": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
			"lang": "java",
			"symbols": [{
				"moniker": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo/trait:Foo",
				"kind": "trait",
				"parent": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo"
			}]
		});
		let err = parse_spec(&v).unwrap_err();
		assert!(matches!(err, DeclareError::KindNotInProfile { ref kind, .. } if kind == "trait"));
	}

	#[test]
	fn rejects_visibility_outside_profile() {
		let v = json!({
			"root": "pcm+moniker://app/srcset:main/lang:ts/dir:src/module:foo",
			"lang": "ts",
			"symbols": [{
				"moniker": "pcm+moniker://app/srcset:main/lang:ts/dir:src/module:foo/class:Bar",
				"kind": "class",
				"parent": "pcm+moniker://app/srcset:main/lang:ts/dir:src/module:foo",
				"visibility": "package"
			}]
		});
		let err = parse_spec(&v).unwrap_err();
		assert!(matches!(
			err,
			DeclareError::VisibilityNotInProfile { ref visibility, .. } if visibility == "package"
		));
	}

	#[test]
	fn ts_accepts_module_visibility() {
		let v = json!({
			"root": "pcm+moniker://app/srcset:main/lang:ts/dir:src/module:foo",
			"lang": "ts",
			"symbols": [{
				"moniker": "pcm+moniker://app/srcset:main/lang:ts/dir:src/module:foo/class:Bar",
				"kind": "class",
				"parent": "pcm+moniker://app/srcset:main/lang:ts/dir:src/module:foo",
				"visibility": "module"
			}]
		});
		assert!(parse_spec(&v).is_ok());
	}

	#[test]
	fn python_accepts_module_visibility() {
		let v = json!({
			"root": "pcm+moniker://app/srcset:main/lang:python/package:acme/module:util",
			"lang": "python",
			"symbols": [{
				"moniker": "pcm+moniker://app/srcset:main/lang:python/package:acme/module:util/class:Helper",
				"kind": "class",
				"parent": "pcm+moniker://app/srcset:main/lang:python/package:acme/module:util",
				"visibility": "module"
			}]
		});
		assert!(parse_spec(&v).is_ok());
	}

	#[test]
	fn go_accepts_module_visibility_replaces_package() {
		let v = json!({
			"root": "pcm+moniker://app/srcset:main/lang:go/package:foo/module:svc",
			"lang": "go",
			"symbols": [{
				"moniker": "pcm+moniker://app/srcset:main/lang:go/package:foo/module:svc/func:helper()",
				"kind": "func",
				"parent": "pcm+moniker://app/srcset:main/lang:go/package:foo/module:svc",
				"visibility": "module"
			}]
		});
		assert!(parse_spec(&v).is_ok());
	}

	#[test]
	fn go_rejects_legacy_package_visibility() {
		let v = json!({
			"root": "pcm+moniker://app/srcset:main/lang:go/package:foo/module:svc",
			"lang": "go",
			"symbols": [{
				"moniker": "pcm+moniker://app/srcset:main/lang:go/package:foo/module:svc/func:helper()",
				"kind": "func",
				"parent": "pcm+moniker://app/srcset:main/lang:go/package:foo/module:svc",
				"visibility": "package"
			}]
		});
		let err = parse_spec(&v).unwrap_err();
		assert!(matches!(
			err,
			DeclareError::VisibilityNotInProfile { ref visibility, .. } if visibility == "package"
		));
	}

	#[test]
	fn sql_ignores_visibility_field() {
		let v = json!({
			"root": "pcm+moniker://app/srcset:db/lang:sql/schema:public",
			"lang": "sql",
			"symbols": [{
				"moniker": "pcm+moniker://app/srcset:db/lang:sql/schema:public/function:do_thing(uuid)",
				"kind": "function",
				"parent": "pcm+moniker://app/srcset:db/lang:sql/schema:public",
				"visibility": "anything"
			}]
		});
		assert!(parse_spec(&v).is_ok());
	}

	#[test]
	fn rejects_unknown_edge_kind() {
		let v = json!({
			"root": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
			"lang": "java",
			"symbols": [],
			"edges": [{
				"from": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
				"kind": "extends",
				"to": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Bar"
			}]
		});
		let err = parse_spec(&v).unwrap_err();
		assert!(matches!(err, DeclareError::UnknownEdgeKind(s) if s == "extends"));
	}

	#[test]
	fn parses_all_four_canonical_edge_kinds() {
		let v = json!({
			"root": "pcm+moniker://app/srcset:main/lang:rs/module:foo",
			"lang": "rs",
			"symbols": [{
				"moniker": "pcm+moniker://app/srcset:main/lang:rs/module:foo/fn:f()",
				"kind": "fn",
				"parent": "pcm+moniker://app/srcset:main/lang:rs/module:foo"
			}],
			"edges": [
				{ "from": "pcm+moniker://app/srcset:main/lang:rs/module:foo/fn:f()",
				  "kind": "depends_on",
				  "to":   "pcm+moniker://app/external_pkg:cargo/path:serde" },
				{ "from": "pcm+moniker://app/srcset:main/lang:rs/module:foo/fn:f()",
				  "kind": "calls",
				  "to":   "pcm+moniker://app/srcset:main/lang:rs/module:foo/fn:g()" },
				{ "from": "pcm+moniker://app/srcset:main/lang:rs/module:foo/fn:f()",
				  "kind": "injects:provide",
				  "to":   "pcm+moniker://app/srcset:main/lang:rs/module:bar/trait:T" },
				{ "from": "pcm+moniker://app/srcset:main/lang:rs/module:foo/fn:f()",
				  "kind": "injects:require",
				  "to":   "pcm+moniker://app/srcset:main/lang:rs/module:bar/trait:U" }
			]
		});
		let s = parse_spec(&v).unwrap();
		assert_eq!(s.edges.len(), 4);
		assert_eq!(s.edges[0].kind, EdgeKind::DependsOn);
		assert_eq!(s.edges[1].kind, EdgeKind::Calls);
		assert_eq!(s.edges[2].kind, EdgeKind::InjectsProvide);
		assert_eq!(s.edges[3].kind, EdgeKind::InjectsRequire);
	}

	#[test]
	fn rejects_invalid_moniker_uri() {
		let v = json!({
			"root": "not-a-uri",
			"lang": "java",
			"symbols": []
		});
		let err = parse_spec(&v).unwrap_err();
		assert!(matches!(err, DeclareError::InvalidMoniker { .. }));
	}

	#[test]
	fn missing_edges_treated_as_empty() {
		let s = parse_spec(&minimal_spec()).unwrap();
		assert!(s.edges.is_empty());
	}

	#[test]
	fn null_edges_treated_as_empty() {
		let mut v = minimal_spec();
		v.as_object_mut()
			.unwrap()
			.insert("edges".to_string(), Value::Null);
		let s = parse_spec(&v).unwrap();
		assert!(s.edges.is_empty());
	}
}
