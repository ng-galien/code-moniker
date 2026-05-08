
use roxmltree::{Document, Node};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dep {
	pub name: String,
	pub version: Option<String>,
	pub dep_kind: String,
	pub import_root: String,
}

#[derive(Debug)]
pub enum PomXmlError {
	Parse(String),
	Schema(String),
}

impl std::fmt::Display for PomXmlError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Parse(e) => write!(f, "pom.xml parse error: {e}"),
			Self::Schema(s) => write!(f, "pom.xml schema error: {s}"),
		}
	}
}

impl std::error::Error for PomXmlError {}

pub fn parse(content: &str) -> Result<Vec<Dep>, PomXmlError> {
	let doc = Document::parse(content).map_err(|e| PomXmlError::Parse(e.to_string()))?;
	let root = doc.root_element();
	if root.tag_name().name() != "project" {
		return Err(PomXmlError::Schema(format!(
			"top-level element is <{}>, expected <project>",
			root.tag_name().name()
		)));
	}

	let mut out = Vec::new();

	let group = direct_child_text(root, "groupId");
	let artifact = direct_child_text(root, "artifactId");
	if let Some(artifact) = artifact {
		let version = direct_child_text(root, "version").map(str::to_string);
		let coord = coord(group.unwrap_or(""), artifact);
		out.push(Dep {
			name: coord.clone(),
			version,
			dep_kind: "package".into(),
			import_root: coord,
		});
	}

	if let Some(deps_node) = direct_child(root, "dependencies") {
		for dep in deps_node.children().filter(is_dependency) {
			let g = direct_child_text(dep, "groupId").unwrap_or("");
			let a = direct_child_text(dep, "artifactId").unwrap_or("");
			if a.is_empty() {
				continue;
			}
			let version = direct_child_text(dep, "version").map(str::to_string);
			let scope = direct_child_text(dep, "scope")
				.map(str::to_string)
				.unwrap_or_else(|| "compile".into());
			let coord = coord(g, a);
			out.push(Dep {
				name: coord.clone(),
				version,
				dep_kind: scope,
				import_root: coord,
			});
		}
	}

	Ok(out)
}

fn coord(group: &str, artifact: &str) -> String {
	if group.is_empty() {
		artifact.to_string()
	} else {
		format!("{group}:{artifact}")
	}
}

fn is_dependency(n: &Node<'_, '_>) -> bool {
	n.is_element() && n.tag_name().name() == "dependency"
}

fn direct_child<'a, 'input>(
	parent: Node<'a, 'input>,
	name: &str,
) -> Option<Node<'a, 'input>> {
	parent
		.children()
		.find(|c| c.is_element() && c.tag_name().name() == name)
}

fn direct_child_text<'a>(parent: Node<'a, '_>, name: &str) -> Option<&'a str> {
	direct_child(parent, name)
		.and_then(|n| n.text().map(str::trim).filter(|s| !s.is_empty()))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parse_minimal_project() {
		let xml = r#"
            <project>
                <groupId>com.example</groupId>
                <artifactId>demo</artifactId>
                <version>0.1.0</version>
            </project>
        "#;
		let deps = parse(xml).unwrap();
		assert_eq!(
			deps,
			vec![Dep {
				name: "com.example:demo".into(),
				version: Some("0.1.0".into()),
				dep_kind: "package".into(),
				import_root: "com.example:demo".into(),
			}]
		);
	}

	#[test]
	fn parse_compile_dep_keeps_version() {
		let xml = r#"
            <project>
                <groupId>com.example</groupId>
                <artifactId>demo</artifactId>
                <version>0.1.0</version>
                <dependencies>
                    <dependency>
                        <groupId>com.google.guava</groupId>
                        <artifactId>guava</artifactId>
                        <version>33.0.0-jre</version>
                    </dependency>
                </dependencies>
            </project>
        "#;
		let deps = parse(xml).unwrap();
		assert!(deps.contains(&Dep {
			name: "com.google.guava:guava".into(),
			version: Some("33.0.0-jre".into()),
			dep_kind: "compile".into(),
			import_root: "com.google.guava:guava".into(),
		}));
	}

	#[test]
	fn parse_scope_test_tagged_dep_kind_test() {
		let xml = r#"
            <project>
                <groupId>com.example</groupId>
                <artifactId>demo</artifactId>
                <version>0.1.0</version>
                <dependencies>
                    <dependency>
                        <groupId>junit</groupId>
                        <artifactId>junit</artifactId>
                        <version>4.13.2</version>
                        <scope>test</scope>
                    </dependency>
                </dependencies>
            </project>
        "#;
		let deps = parse(xml).unwrap();
		let junit = deps.iter().find(|d| d.name == "junit:junit").unwrap();
		assert_eq!(junit.dep_kind, "test");
	}

	#[test]
	fn parse_dep_without_groupid_uses_artifact_only() {
		let xml = r#"
            <project>
                <artifactId>demo</artifactId>
                <dependencies>
                    <dependency>
                        <artifactId>orphan</artifactId>
                    </dependency>
                </dependencies>
            </project>
        "#;
		let deps = parse(xml).unwrap();
		assert!(deps.iter().any(|d| d.name == "orphan"));
	}

	#[test]
	fn parse_invalid_xml_returns_parse_error() {
		assert!(matches!(parse("<project>"), Err(PomXmlError::Parse(_))));
	}

	#[test]
	fn parse_non_project_root_is_schema_error() {
		assert!(matches!(
			parse("<settings></settings>"),
			Err(PomXmlError::Schema(_))
		));
	}
}
