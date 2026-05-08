
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dep {
	pub name: String,
	pub version: Option<String>,
	pub dep_kind: String,
	pub import_root: String,
}

#[derive(Debug)]
pub enum CargoError {
	Parse(toml::de::Error),
	Schema(String),
}

impl std::fmt::Display for CargoError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Parse(e) => write!(f, "Cargo.toml parse error: {e}"),
			Self::Schema(s) => write!(f, "Cargo.toml schema error: {s}"),
		}
	}
}

impl std::error::Error for CargoError {}

pub fn parse(content: &str) -> Result<Vec<Dep>, CargoError> {
	let value: toml::Value = toml::from_str(content).map_err(CargoError::Parse)?;
	let mut out = Vec::new();

	if let Some(pkg) = value.get("package").and_then(|v| v.as_table()) {
		let name = pkg
			.get("name")
			.and_then(|v| v.as_str())
			.ok_or_else(|| CargoError::Schema("[package].name missing or not a string".into()))?;
		let version = pkg.get("version").and_then(|v| v.as_str()).map(str::to_string);
		out.push(Dep {
			name: name.to_string(),
			version,
			dep_kind: "package".to_string(),
			import_root: rust_import_root(name),
		});
	}

	for (kind_table, kind_label) in [
		("dependencies", "normal"),
		("dev-dependencies", "dev"),
		("build-dependencies", "build"),
	] {
		let Some(table) = value.get(kind_table).and_then(|v| v.as_table()) else {
			continue;
		};
		for (name, spec) in table {
			let version = extract_version(spec);
			out.push(Dep {
				name: name.clone(),
				version,
				dep_kind: kind_label.to_string(),
				import_root: rust_import_root(name),
			});
		}
	}

	Ok(out)
}

pub(crate) fn rust_import_root(name: &str) -> String {
	name.replace('-', "_")
}

pub(crate) fn extract_version(spec: &toml::Value) -> Option<String> {
	match spec {
		toml::Value::String(s) => Some(s.clone()),
		toml::Value::Table(t) => t.get("version").and_then(|v| v.as_str()).map(str::to_string),
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parse_minimal_package() {
		let toml = r#"
            [package]
            name = "demo"
            version = "0.1.0"
        "#;
		let deps = parse(toml).unwrap();
		assert_eq!(deps, vec![Dep {
			name: "demo".into(),
			version: Some("0.1.0".into()),
			dep_kind: "package".into(),
			import_root: "demo".into(),
		}]);
	}

	#[test]
	fn parse_string_dep_keeps_version() {
		let toml = r#"
            [package]
            name = "demo"
            version = "1.0.0"

            [dependencies]
            serde = "1.0"
        "#;
		let deps = parse(toml).unwrap();
		assert!(deps.contains(&Dep {
			name: "serde".into(),
			version: Some("1.0".into()),
			dep_kind: "normal".into(),
			import_root: "serde".into(),
		}));
	}

	#[test]
	fn parse_table_dep_uses_version_field() {
		let toml = r#"
            [package]
            name = "demo"
            version = "1.0.0"

            [dependencies]
            tokio = { version = "1.40", features = ["full"] }
        "#;
		let deps = parse(toml).unwrap();
		assert!(deps.contains(&Dep {
			name: "tokio".into(),
			version: Some("1.40".into()),
			dep_kind: "normal".into(),
			import_root: "tokio".into(),
		}));
	}

	#[test]
	fn parse_path_dep_has_no_version() {
		let toml = r#"
            [package]
            name = "demo"
            version = "1.0.0"

            [dependencies]
            local_lib = { path = "../local_lib" }
        "#;
		let deps = parse(toml).unwrap();
		assert!(deps.contains(&Dep {
			name: "local_lib".into(),
			version: None,
			dep_kind: "normal".into(),
			import_root: "local_lib".into(),
		}));
	}

	#[test]
	fn parse_dev_and_build_dependencies_kinds() {
		let toml = r#"
            [package]
            name = "demo"
            version = "1.0.0"

            [dev-dependencies]
            criterion = "0.5"

            [build-dependencies]
            cc = "1.0"
        "#;
		let deps = parse(toml).unwrap();
		assert!(deps.iter().any(|d| d.name == "criterion" && d.dep_kind == "dev"));
		assert!(deps.iter().any(|d| d.name == "cc" && d.dep_kind == "build"));
	}

	#[test]
	fn parse_hyphenated_name_normalizes_import_root() {
		let toml = r#"
            [package]
            name = "demo"
            version = "1.0.0"

            [dependencies]
            tree-sitter = "0.26"
            multi-word-name = "1.0"
        "#;
		let deps = parse(toml).unwrap();
		let ts = deps.iter().find(|d| d.name == "tree-sitter").unwrap();
		assert_eq!(ts.import_root, "tree_sitter");
		let mw = deps.iter().find(|d| d.name == "multi-word-name").unwrap();
		assert_eq!(mw.import_root, "multi_word_name");
	}

	#[test]
	fn parse_invalid_toml_returns_error() {
		assert!(matches!(
			parse("not [valid toml"),
			Err(CargoError::Parse(_))
		));
	}

	#[test]
	fn parse_missing_package_name_is_schema_error() {
		let toml = r#"
            [package]
            version = "1.0.0"
        "#;
		assert!(matches!(parse(toml), Err(CargoError::Schema(_))));
	}
}
