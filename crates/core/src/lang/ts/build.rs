use serde_json::Value;

use crate::core::moniker::{Moniker, MonikerBuilder};

use super::kinds;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dep {
	pub name: String,
	pub version: Option<String>,
	pub dep_kind: String,
	pub import_root: String,
}

#[derive(Debug)]
pub enum PackageJsonError {
	Parse(serde_json::Error),
	Schema(String),
}

impl std::fmt::Display for PackageJsonError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Parse(e) => write!(f, "package.json parse error: {e}"),
			Self::Schema(s) => write!(f, "package.json schema error: {s}"),
		}
	}
}

impl std::error::Error for PackageJsonError {}

pub fn parse(content: &str) -> Result<Vec<Dep>, PackageJsonError> {
	let value: Value = serde_json::from_str(content).map_err(PackageJsonError::Parse)?;
	let obj = value
		.as_object()
		.ok_or_else(|| PackageJsonError::Schema("top-level value is not a JSON object".into()))?;
	let mut out = Vec::new();

	if let Some(name) = obj.get("name").and_then(Value::as_str) {
		let version = obj
			.get("version")
			.and_then(Value::as_str)
			.map(str::to_string);
		out.push(Dep {
			name: name.to_string(),
			version,
			dep_kind: "package".to_string(),
			import_root: ts_import_root(name),
		});
	}

	for (field, kind_label) in [
		("dependencies", "normal"),
		("devDependencies", "dev"),
		("peerDependencies", "peer"),
		("optionalDependencies", "optional"),
	] {
		let Some(table) = obj.get(field).and_then(Value::as_object) else {
			continue;
		};
		for (name, spec) in table {
			let version = extract_version(spec);
			out.push(Dep {
				name: name.clone(),
				version,
				dep_kind: kind_label.to_string(),
				import_root: ts_import_root(name),
			});
		}
	}

	Ok(out)
}

pub(crate) fn ts_import_root(name: &str) -> String {
	name.to_string()
}

pub fn package_moniker(project: &[u8], import_root: &str) -> Moniker {
	external_pkg_builder(project, import_root).build()
}

pub(in crate::lang::ts) fn external_pkg_builder(project: &[u8], pkg: &str) -> MonikerBuilder {
	let (head, tail) = split_package_specifier(pkg);
	let mut b = MonikerBuilder::new();
	b.project(project);
	b.segment(kinds::EXTERNAL_PKG, head.as_bytes());
	for piece in tail.split('/').filter(|s| !s.is_empty()) {
		b.segment(kinds::PATH, piece.as_bytes());
	}
	b
}

fn split_package_specifier(spec: &str) -> (&str, &str) {
	if let Some(after_scope) = spec.strip_prefix('@') {
		let mut parts = after_scope.splitn(3, '/');
		let scope = parts.next().unwrap_or("");
		let name = parts.next().unwrap_or("");
		let tail = parts.next().unwrap_or("");
		let head_end = if name.is_empty() {
			1 + scope.len()
		} else {
			1 + scope.len() + 1 + name.len()
		};
		(&spec[..head_end], tail)
	} else {
		match spec.find('/') {
			Some(i) => (&spec[..i], &spec[i + 1..]),
			None => (spec, ""),
		}
	}
}

fn extract_version(spec: &Value) -> Option<String> {
	match spec {
		Value::String(s) => Some(s.clone()),
		Value::Object(o) => o.get("version").and_then(Value::as_str).map(str::to_string),
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parse_minimal_package() {
		let json = r#"{ "name": "demo", "version": "0.1.0" }"#;
		let deps = parse(json).unwrap();
		assert_eq!(
			deps,
			vec![Dep {
				name: "demo".into(),
				version: Some("0.1.0".into()),
				dep_kind: "package".into(),
				import_root: "demo".into(),
			}]
		);
	}

	#[test]
	fn parse_normal_dep_keeps_version_string() {
		let json = r#"{
			"name": "demo",
			"version": "0.1.0",
			"dependencies": { "react": "^18.0.0" }
		}"#;
		let deps = parse(json).unwrap();
		assert!(deps.contains(&Dep {
			name: "react".into(),
			version: Some("^18.0.0".into()),
			dep_kind: "normal".into(),
			import_root: "react".into(),
		}));
	}

	#[test]
	fn parse_object_dep_with_version_field() {
		let json = r#"{
			"name": "demo",
			"version": "0.1.0",
			"dependencies": { "tsup": { "version": "8.0.0" } }
		}"#;
		let deps = parse(json).unwrap();
		assert!(
			deps.iter()
				.any(|d| d.name == "tsup" && d.version.as_deref() == Some("8.0.0"))
		);
	}

	#[test]
	fn parse_dev_peer_optional_kinds() {
		let json = r#"{
			"name": "demo",
			"version": "0.1.0",
			"devDependencies":      { "vitest":   "1.0.0" },
			"peerDependencies":     { "react":    "^18.0.0" },
			"optionalDependencies": { "fsevents": "2.0.0" }
		}"#;
		let deps = parse(json).unwrap();
		assert!(
			deps.iter()
				.any(|d| d.name == "vitest" && d.dep_kind == "dev")
		);
		assert!(
			deps.iter()
				.any(|d| d.name == "react" && d.dep_kind == "peer")
		);
		assert!(
			deps.iter()
				.any(|d| d.name == "fsevents" && d.dep_kind == "optional")
		);
	}

	#[test]
	fn parse_scoped_package_keeps_full_name_in_import_root() {
		let json = r#"{
			"name": "demo",
			"version": "0.1.0",
			"dependencies": { "@scope/pkg": "1.0.0" }
		}"#;
		let deps = parse(json).unwrap();
		let scoped = deps.iter().find(|d| d.name == "@scope/pkg").unwrap();
		assert_eq!(scoped.import_root, "@scope/pkg");
	}

	#[test]
	fn package_moniker_keeps_scoped_package_head() {
		let moniker = package_moniker(b"app", "@scope/pkg/sub/path");
		let segments = moniker.as_view().segments().collect::<Vec<_>>();
		assert_eq!(segments[0].kind, b"external_pkg");
		assert_eq!(segments[0].name, b"@scope/pkg");
		assert_eq!(segments[1].kind, b"path");
		assert_eq!(segments[1].name, b"sub");
		assert_eq!(segments[2].kind, b"path");
		assert_eq!(segments[2].name, b"path");
	}

	#[test]
	fn parse_invalid_json_returns_parse_error() {
		assert!(matches!(
			parse("{not json"),
			Err(PackageJsonError::Parse(_))
		));
	}

	#[test]
	fn parse_non_object_top_level_is_schema_error() {
		assert!(matches!(parse("[1,2,3]"), Err(PackageJsonError::Schema(_))));
	}

	#[test]
	fn parse_missing_name_omits_package_row() {
		let json = r#"{
			"private": true,
			"dependencies": { "react": "^18.0.0" }
		}"#;
		let deps = parse(json).unwrap();
		assert!(deps.iter().all(|d| d.dep_kind != "package"));
		assert!(deps.iter().any(|d| d.name == "react"));
	}
}
