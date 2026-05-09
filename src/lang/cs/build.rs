#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dep {
	pub name: String,
	pub version: Option<String>,
	pub dep_kind: String,
	pub import_root: String,
}

#[derive(Debug)]
pub enum CsprojError {
	Parse(roxmltree::Error),
}

impl std::fmt::Display for CsprojError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Parse(e) => write!(f, ".csproj parse error: {e}"),
		}
	}
}

impl std::error::Error for CsprojError {}

pub fn parse(content: &str) -> Result<Vec<Dep>, CsprojError> {
	let doc = roxmltree::Document::parse(content).map_err(CsprojError::Parse)?;
	let root = doc.root_element();
	let mut out = Vec::new();

	if let Some(name) = project_self_name(root) {
		let version = property_value(root, "Version");
		out.push(Dep {
			name: name.clone(),
			version,
			dep_kind: "package".into(),
			import_root: name,
		});
	}

	for node in root.descendants() {
		match node.tag_name().name() {
			"PackageReference" => {
				let Some(name) = node.attribute("Include") else {
					continue;
				};
				let version = node
					.attribute("Version")
					.map(str::to_string)
					.or_else(|| element_text(node, "Version"));
				out.push(Dep {
					name: name.into(),
					version,
					dep_kind: "normal".into(),
					import_root: name.into(),
				});
			}
			"ProjectReference" => {
				let Some(path) = node.attribute("Include") else {
					continue;
				};
				let stem = project_path_stem(path);
				out.push(Dep {
					name: stem.clone(),
					version: None,
					dep_kind: "project".into(),
					import_root: stem,
				});
			}
			_ => {}
		}
	}

	Ok(out)
}

fn project_self_name(root: roxmltree::Node<'_, '_>) -> Option<String> {
	property_value(root, "AssemblyName").or_else(|| property_value(root, "RootNamespace"))
}

fn property_value(root: roxmltree::Node<'_, '_>, tag: &str) -> Option<String> {
	root.descendants()
		.find(|n| n.is_element() && n.tag_name().name() == tag)
		.and_then(|n| n.text())
		.map(|s| s.trim().to_string())
		.filter(|s| !s.is_empty())
}

fn element_text(node: roxmltree::Node<'_, '_>, tag: &str) -> Option<String> {
	node.children()
		.find(|n| n.is_element() && n.tag_name().name() == tag)
		.and_then(|n| n.text())
		.map(|s| s.trim().to_string())
		.filter(|s| !s.is_empty())
}

fn project_path_stem(path: &str) -> String {
	let leaf = path.rsplit(['/', '\\']).next().unwrap_or(path);
	leaf.strip_suffix(".csproj").unwrap_or(leaf).to_string()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parse_empty_project_returns_empty_vec() {
		let xml = r#"<Project Sdk="Microsoft.NET.Sdk"></Project>"#;
		assert!(parse(xml).unwrap().is_empty());
	}

	#[test]
	fn parse_self_name_from_assembly_name() {
		let xml = r#"<Project Sdk="Microsoft.NET.Sdk">
			<PropertyGroup>
				<AssemblyName>MyApp</AssemblyName>
				<Version>1.2.3</Version>
			</PropertyGroup>
		</Project>"#;
		let deps = parse(xml).unwrap();
		let pkg = deps.iter().find(|d| d.dep_kind == "package").unwrap();
		assert_eq!(pkg.name, "MyApp");
		assert_eq!(pkg.version.as_deref(), Some("1.2.3"));
		assert_eq!(pkg.import_root, "MyApp");
	}

	#[test]
	fn parse_falls_back_to_root_namespace() {
		let xml = r#"<Project>
			<PropertyGroup>
				<RootNamespace>Acme</RootNamespace>
			</PropertyGroup>
		</Project>"#;
		let deps = parse(xml).unwrap();
		assert!(
			deps.iter()
				.any(|d| d.dep_kind == "package" && d.name == "Acme")
		);
	}

	#[test]
	fn parse_package_reference_attribute_version() {
		let xml = r#"<Project>
			<ItemGroup>
				<PackageReference Include="Newtonsoft.Json" Version="13.0.1" />
			</ItemGroup>
		</Project>"#;
		let deps = parse(xml).unwrap();
		let pkg = deps.iter().find(|d| d.name == "Newtonsoft.Json").unwrap();
		assert_eq!(pkg.version.as_deref(), Some("13.0.1"));
		assert_eq!(pkg.dep_kind, "normal");
		assert_eq!(pkg.import_root, "Newtonsoft.Json");
	}

	#[test]
	fn parse_package_reference_element_version() {
		let xml = r#"<Project>
			<ItemGroup>
				<PackageReference Include="Serilog">
					<Version>3.0.0</Version>
				</PackageReference>
			</ItemGroup>
		</Project>"#;
		let deps = parse(xml).unwrap();
		let pkg = deps.iter().find(|d| d.name == "Serilog").unwrap();
		assert_eq!(pkg.version.as_deref(), Some("3.0.0"));
	}

	#[test]
	fn parse_project_reference_strips_path_and_extension() {
		let xml = r#"<Project>
			<ItemGroup>
				<ProjectReference Include="..\Other\Other.csproj" />
			</ItemGroup>
		</Project>"#;
		let deps = parse(xml).unwrap();
		let pr = deps.iter().find(|d| d.dep_kind == "project").unwrap();
		assert_eq!(pr.name, "Other");
		assert!(pr.version.is_none());
	}

	#[test]
	fn parse_project_reference_handles_unix_paths() {
		let xml = r#"<Project>
			<ItemGroup>
				<ProjectReference Include="../Other/Other.csproj" />
			</ItemGroup>
		</Project>"#;
		let deps = parse(xml).unwrap();
		assert!(
			deps.iter()
				.any(|d| d.name == "Other" && d.dep_kind == "project")
		);
	}

	#[test]
	fn parse_invalid_xml_returns_parse_error() {
		assert!(matches!(parse("<not closed"), Err(CsprojError::Parse(_))));
	}
}
