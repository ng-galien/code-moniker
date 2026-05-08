
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dep {
	pub name: String,
	pub version: Option<String>,
	pub dep_kind: String,
	pub import_root: String,
}

#[derive(Debug)]
pub enum PyprojectError {
	Parse(toml::de::Error),
	Schema(String),
}

impl std::fmt::Display for PyprojectError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Parse(e) => write!(f, "pyproject.toml parse error: {e}"),
			Self::Schema(s) => write!(f, "pyproject.toml schema error: {s}"),
		}
	}
}

impl std::error::Error for PyprojectError {}

pub fn parse(content: &str) -> Result<Vec<Dep>, PyprojectError> {
	let value: toml::Value = toml::from_str(content).map_err(PyprojectError::Parse)?;
	let mut out = Vec::new();

	if let Some(project) = value.get("project").and_then(|v| v.as_table()) {
		if let Some(name) = project.get("name").and_then(|v| v.as_str()) {
			let version = project.get("version").and_then(|v| v.as_str()).map(str::to_string);
			out.push(Dep {
				name: name.to_string(),
				version,
				dep_kind: "package".to_string(),
				import_root: python_import_root(name),
			});
		}
		if let Some(deps) = project.get("dependencies").and_then(|v| v.as_array()) {
			for spec in deps.iter().filter_map(|v| v.as_str()) {
				if let Some(dep) = parse_pep508(spec, "normal") {
					out.push(dep);
				}
			}
		}
		if let Some(opt) = project.get("optional-dependencies").and_then(|v| v.as_table()) {
			for (group, list) in opt {
				let Some(arr) = list.as_array() else { continue };
				let kind = format!("optional:{group}");
				for spec in arr.iter().filter_map(|v| v.as_str()) {
					if let Some(dep) = parse_pep508(spec, &kind) {
						out.push(dep);
					}
				}
			}
		}
	}

	if let Some(poetry) = value
		.get("tool")
		.and_then(|t| t.get("poetry"))
		.and_then(|p| p.as_table())
	{
		if out.iter().all(|d| d.dep_kind != "package") {
			if let Some(name) = poetry.get("name").and_then(|v| v.as_str()) {
				let version = poetry.get("version").and_then(|v| v.as_str()).map(str::to_string);
				out.push(Dep {
					name: name.to_string(),
					version,
					dep_kind: "package".to_string(),
					import_root: python_import_root(name),
				});
			}
		}
		for (table_key, kind_label) in [
			("dependencies", "normal"),
			("dev-dependencies", "dev"),
		] {
			let Some(table) = poetry.get(table_key).and_then(|v| v.as_table()) else {
				continue;
			};
			for (name, spec) in table {
				if name == "python" {
					continue;
				}
				let version = crate::lang::rs::build::extract_version(spec);
				out.push(Dep {
					name: name.clone(),
					version,
					dep_kind: kind_label.to_string(),
					import_root: python_import_root(name),
				});
			}
		}
	}

	Ok(out)
}

fn parse_pep508(spec: &str, dep_kind: &str) -> Option<Dep> {
	let trimmed = spec.split(';').next()?.trim();
	if trimmed.is_empty() {
		return None;
	}
	let mut name_end = trimmed.len();
	for (i, ch) in trimmed.char_indices() {
		if matches!(ch, '=' | '<' | '>' | '!' | '~' | ' ' | '\t' | '[' | '(') {
			name_end = i;
			break;
		}
	}
	let name = trimmed[..name_end].trim();
	if name.is_empty() {
		return None;
	}
	let after_extras = match trimmed[name_end..].find(']') {
		Some(close) => trimmed[name_end + close + 1..].trim(),
		None => trimmed[name_end..].trim(),
	};
	let version = if after_extras.is_empty() {
		None
	} else {
		Some(after_extras.trim().to_string())
	};
	Some(Dep {
		name: name.to_string(),
		version,
		dep_kind: dep_kind.to_string(),
		import_root: python_import_root(name),
	})
}

pub(crate) fn python_import_root(name: &str) -> String {
	name.replace('-', "_").to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parse_pep621_project_emits_package_row() {
		let src = r#"
            [project]
            name = "demo"
            version = "0.2.0"
        "#;
		let deps = parse(src).unwrap();
		assert!(deps.contains(&Dep {
			name: "demo".into(),
			version: Some("0.2.0".into()),
			dep_kind: "package".into(),
			import_root: "demo".into(),
		}));
	}

	#[test]
	fn parse_pep621_dependencies_keeps_version_constraints() {
		let src = r#"
            [project]
            name = "demo"
            version = "0.1.0"
            dependencies = ["httpx==0.27.2", "anyio>=3.7"]
        "#;
		let deps = parse(src).unwrap();
		let httpx = deps.iter().find(|d| d.name == "httpx").unwrap();
		assert_eq!(httpx.version.as_deref(), Some("==0.27.2"));
		assert_eq!(httpx.dep_kind, "normal");
		let anyio = deps.iter().find(|d| d.name == "anyio").unwrap();
		assert_eq!(anyio.version.as_deref(), Some(">=3.7"));
	}

	#[test]
	fn parse_pep621_strips_extras_marker_and_environment() {
		let src = r#"
            [project]
            name = "demo"
            dependencies = ["requests[security]>=2.31; python_version >= '3.8'"]
        "#;
		let deps = parse(src).unwrap();
		let req = deps.iter().find(|d| d.name == "requests").unwrap();
		assert_eq!(req.version.as_deref(), Some(">=2.31"));
	}

	#[test]
	fn parse_pep621_optional_dependencies_emit_grouped_kind() {
		let src = r#"
            [project]
            name = "demo"
            [project.optional-dependencies]
            test = ["pytest>=7.0"]
            docs = ["sphinx"]
        "#;
		let deps = parse(src).unwrap();
		assert!(deps.iter().any(|d| d.name == "pytest" && d.dep_kind == "optional:test"));
		assert!(deps.iter().any(|d| d.name == "sphinx" && d.dep_kind == "optional:docs"));
	}

	#[test]
	fn parse_poetry_dependencies_skip_python_marker() {
		let src = r#"
            [tool.poetry]
            name = "demo"
            version = "0.1.0"
            [tool.poetry.dependencies]
            python = "^3.10"
            httpx = "^0.27"
            [tool.poetry.dev-dependencies]
            pytest = "^7.0"
        "#;
		let deps = parse(src).unwrap();
		assert!(!deps.iter().any(|d| d.name == "python"));
		assert!(deps.iter().any(|d| d.name == "httpx" && d.dep_kind == "normal"));
		assert!(deps.iter().any(|d| d.name == "pytest" && d.dep_kind == "dev"));
	}

	#[test]
	fn parse_poetry_table_uses_version_field() {
		let src = r#"
            [tool.poetry]
            name = "demo"
            [tool.poetry.dependencies]
            sqlalchemy = { version = "^2.0", extras = ["asyncio"] }
        "#;
		let deps = parse(src).unwrap();
		let sa = deps.iter().find(|d| d.name == "sqlalchemy").unwrap();
		assert_eq!(sa.version.as_deref(), Some("^2.0"));
	}

	#[test]
	fn parse_normalizes_hyphenated_import_root_to_underscore_lowercase() {
		let src = r#"
            [project]
            name = "Some-Project"
            dependencies = ["python-dateutil"]
        "#;
		let deps = parse(src).unwrap();
		let proj = deps.iter().find(|d| d.dep_kind == "package").unwrap();
		assert_eq!(proj.import_root, "some_project");
		let pd = deps.iter().find(|d| d.name == "python-dateutil").unwrap();
		assert_eq!(pd.import_root, "python_dateutil");
	}

	#[test]
	fn parse_invalid_toml_returns_parse_error() {
		assert!(matches!(parse("not toml ["), Err(PyprojectError::Parse(_))));
	}
}
