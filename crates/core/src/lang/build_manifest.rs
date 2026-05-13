//! Filename-keyed dispatch over the per-language manifest parsers, with
//! `package_moniker` attached to each declared dep.

use std::path::Path;

use crate::core::moniker::Moniker;
#[cfg(test)]
use crate::core::moniker::MonikerBuilder;
#[cfg(test)]
use crate::lang::kinds;
use crate::lang::{cs, go, java, python, rs, ts};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dep {
	pub package_moniker: Moniker,
	pub name: String,
	pub import_root: String,
	pub version: Option<String>,
	pub dep_kind: String,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Manifest {
	Cargo,
	PackageJson,
	PomXml,
	Pyproject,
	GoMod,
	Csproj,
}

impl Manifest {
	pub const ALL: &'static [Manifest] = &[
		Self::Cargo,
		Self::PackageJson,
		Self::PomXml,
		Self::Pyproject,
		Self::GoMod,
		Self::Csproj,
	];

	pub fn tag(self) -> &'static str {
		match self {
			Self::Cargo => "cargo",
			Self::PackageJson => "package_json",
			Self::PomXml => "pom_xml",
			Self::Pyproject => "pyproject",
			Self::GoMod => "go_mod",
			Self::Csproj => "csproj",
		}
	}

	pub fn for_filename(path: &Path) -> Option<Self> {
		let name = path.file_name()?.to_str()?;
		match name {
			"Cargo.toml" => Some(Self::Cargo),
			"package.json" => Some(Self::PackageJson),
			"pom.xml" => Some(Self::PomXml),
			"pyproject.toml" => Some(Self::Pyproject),
			"go.mod" => Some(Self::GoMod),
			_ if name.ends_with(".csproj") => Some(Self::Csproj),
			_ => None,
		}
	}
}

#[derive(Debug)]
pub enum ManifestError {
	Cargo(rs::build::CargoError),
	PackageJson(ts::build::PackageJsonError),
	PomXml(java::build::PomXmlError),
	Pyproject(python::build::PyprojectError),
	GoMod(go::build::GoModError),
	Csproj(cs::build::CsprojError),
}

impl std::fmt::Display for ManifestError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Cargo(e) => e.fmt(f),
			Self::PackageJson(e) => e.fmt(f),
			Self::PomXml(e) => e.fmt(f),
			Self::Pyproject(e) => e.fmt(f),
			Self::GoMod(e) => e.fmt(f),
			Self::Csproj(e) => e.fmt(f),
		}
	}
}

impl std::error::Error for ManifestError {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		match self {
			Self::Cargo(e) => Some(e),
			Self::PackageJson(e) => Some(e),
			Self::PomXml(e) => Some(e),
			Self::Pyproject(e) => Some(e),
			Self::GoMod(e) => Some(e),
			Self::Csproj(e) => Some(e),
		}
	}
}

pub fn parse(manifest: Manifest, project: &[u8], content: &str) -> Result<Vec<Dep>, ManifestError> {
	match manifest {
		Manifest::Cargo => rs::build::parse(content)
			.map_err(ManifestError::Cargo)
			.map(|v| {
				v.into_iter()
					.map(|d| project_into(manifest, project, d))
					.collect()
			}),
		Manifest::PackageJson => ts::build::parse(content)
			.map_err(ManifestError::PackageJson)
			.map(|v| {
				v.into_iter()
					.map(|d| project_into(manifest, project, d))
					.collect()
			}),
		Manifest::PomXml => java::build::parse(content)
			.map_err(ManifestError::PomXml)
			.map(|v| {
				v.into_iter()
					.map(|d| project_into(manifest, project, d))
					.collect()
			}),
		Manifest::Pyproject => python::build::parse(content)
			.map_err(ManifestError::Pyproject)
			.map(|v| {
				v.into_iter()
					.map(|d| project_into(manifest, project, d))
					.collect()
			}),
		Manifest::GoMod => go::build::parse(content)
			.map_err(ManifestError::GoMod)
			.map(|v| {
				v.into_iter()
					.map(|d| project_into(manifest, project, d))
					.collect()
			}),
		Manifest::Csproj => cs::build::parse(content)
			.map_err(ManifestError::Csproj)
			.map(|v| {
				v.into_iter()
					.map(|d| project_into(manifest, project, d))
					.collect()
			}),
	}
}

pub fn package_moniker(manifest: Manifest, project: &[u8], import_root: &str) -> Moniker {
	match manifest {
		Manifest::Cargo => rs::build::package_moniker(project, import_root),
		Manifest::PackageJson => ts::build::package_moniker(project, import_root),
		Manifest::PomXml => java::build::package_moniker(project, import_root),
		Manifest::Pyproject => python::build::package_moniker(project, import_root),
		Manifest::GoMod => go::build::package_moniker(project, import_root),
		Manifest::Csproj => cs::build::package_moniker(project, import_root),
	}
}

trait IntoDep {
	fn into_dep(self, manifest: Manifest, project: &[u8]) -> Dep;
}

fn project_into<T: IntoDep>(manifest: Manifest, project: &[u8], dep: T) -> Dep {
	dep.into_dep(manifest, project)
}

macro_rules! impl_into_dep {
	($($t:ty),* $(,)?) => {
		$(
			impl IntoDep for $t {
				fn into_dep(self, manifest: Manifest, project: &[u8]) -> Dep {
					let package_moniker = package_moniker(manifest, project, &self.import_root);
					Dep {
						package_moniker,
						name: self.name,
						import_root: self.import_root,
						version: self.version,
						dep_kind: self.dep_kind,
					}
				}
			}
		)*
	};
}

impl_into_dep!(
	rs::build::Dep,
	ts::build::Dep,
	java::build::Dep,
	python::build::Dep,
	go::build::Dep,
	cs::build::Dep,
);

#[cfg(test)]
mod tests {
	use super::*;
	use std::path::PathBuf;

	#[test]
	fn for_filename_recognises_each_manifest() {
		for (name, want) in [
			("Cargo.toml", Manifest::Cargo),
			("package.json", Manifest::PackageJson),
			("pom.xml", Manifest::PomXml),
			("pyproject.toml", Manifest::Pyproject),
			("go.mod", Manifest::GoMod),
			("MyApp.csproj", Manifest::Csproj),
		] {
			assert_eq!(
				Manifest::for_filename(&PathBuf::from(name)),
				Some(want),
				"{name}"
			);
		}
	}

	#[test]
	fn for_filename_ignores_unknown_and_directories() {
		assert!(Manifest::for_filename(&PathBuf::from("README.md")).is_none());
		assert!(Manifest::for_filename(&PathBuf::from("")).is_none());
	}

	#[test]
	fn package_moniker_round_trips_through_uri() {
		use crate::core::uri::{UriConfig, to_uri};
		let m = package_moniker(Manifest::PackageJson, b".", "react");
		let cfg = UriConfig {
			scheme: "code+moniker://",
		};
		let uri = to_uri(&m, &cfg).expect("utf-8 segments");
		assert_eq!(uri, "code+moniker://./external_pkg:react");
	}

	#[test]
	fn parse_cargo_includes_package_moniker_for_each_row() {
		let toml = r#"
			[package]
			name = "demo"
			version = "0.1.0"

			[dependencies]
			serde-json = "1.0"
		"#;
		let deps = parse(Manifest::Cargo, b".", toml).expect("ok");
		let demo = deps.iter().find(|d| d.name == "demo").unwrap();
		assert_eq!(
			demo.package_moniker,
			package_moniker(Manifest::Cargo, b".", "demo")
		);
		let sj = deps.iter().find(|d| d.name == "serde-json").unwrap();
		assert_eq!(sj.import_root, "serde_json");
		assert_eq!(
			sj.package_moniker,
			package_moniker(Manifest::Cargo, b".", "serde_json")
		);
	}

	#[test]
	fn parse_pyproject_normalises_import_root_in_moniker() {
		let toml = r#"
			[project]
			name = "demo"
			dependencies = ["requests-html >=1.0"]
		"#;
		let deps = parse(Manifest::Pyproject, b".", toml).expect("ok");
		let rh = deps
			.iter()
			.find(|d| d.name == "requests-html")
			.expect("dep parsed");
		assert_eq!(rh.import_root, "requests_html");
		assert_eq!(
			rh.package_moniker,
			package_moniker(Manifest::Pyproject, b".", "requests_html")
		);
	}

	#[test]
	fn package_moniker_splits_go_module_path_on_slash() {
		let m = package_moniker(Manifest::GoMod, b"app", "github.com/gorilla/mux");
		use crate::core::uri::{UriConfig, to_uri};
		let uri = to_uri(
			&m,
			&UriConfig {
				scheme: "code+moniker://",
			},
		)
		.expect("utf-8");
		assert_eq!(
			uri,
			"code+moniker://app/external_pkg:github.com/path:gorilla/path:mux"
		);
	}

	#[test]
	fn package_moniker_splits_csharp_namespace_on_dot() {
		let m = package_moniker(Manifest::Csproj, b"app", "Newtonsoft.Json");
		use crate::core::uri::{UriConfig, to_uri};
		let uri = to_uri(
			&m,
			&UriConfig {
				scheme: "code+moniker://",
			},
		)
		.expect("utf-8");
		assert_eq!(uri, "code+moniker://app/external_pkg:Newtonsoft/path:Json");
	}

	#[test]
	fn parse_dispatches_each_manifest_kind() {
		let cases: Vec<(Manifest, &str, &str)> = vec![
			(
				Manifest::Cargo,
				r#"[package]
name = "x"
version = "0""#,
				"x",
			),
			(Manifest::PackageJson, r#"{"name":"x","version":"0"}"#, "x"),
			(
				Manifest::GoMod,
				r#"module x
go 1.21"#,
				"x",
			),
		];
		for (m, content, head) in cases {
			let deps =
				parse(m, b".", content).unwrap_or_else(|e| panic!("{} parse failed: {e}", m.tag()));
			assert!(
				deps.iter().any(|d| d.import_root == head),
				"{} did not yield head {head}",
				m.tag()
			);
		}
	}

	#[test]
	fn parse_propagates_per_lang_error_variant() {
		let err = parse(Manifest::Cargo, b".", "not [valid toml").unwrap_err();
		assert!(matches!(err, ManifestError::Cargo(_)));
		let err = parse(Manifest::PackageJson, b".", "{not json").unwrap_err();
		assert!(matches!(err, ManifestError::PackageJson(_)));
	}

	fn first_external_target(
		g: &crate::core::code_graph::CodeGraph,
		head_name: &str,
	) -> Option<Moniker> {
		g.refs()
			.find(|r| {
				let mut segs = r.target.as_view().segments();
				match segs.next() {
					Some(s) => s.kind == kinds::EXTERNAL_PKG && s.name == head_name.as_bytes(),
					None => false,
				}
			})
			.map(|r| r.target.clone())
	}

	/// `package_moniker(import_root)` must be `@>`-ancestor of (or equal
	/// to) the ref target the extractor emits for the same import. Python
	/// uses `os` because only stdlib goes through `external_pkg`; Java is
	/// excluded since non-stdlib imports use `lang:java/package:…`.
	#[test]
	fn package_moniker_binds_extractor_ref_per_language() {
		use crate::lang::{cs, go, python, rs, ts};
		let anchor = MonikerBuilder::new().project(b"app").build();

		struct Case {
			lang: &'static str,
			manifest: Manifest,
			extractor_head: &'static str,
			import_root: &'static str,
			run: fn(&Moniker) -> crate::core::code_graph::CodeGraph,
		}

		fn run_ts(a: &Moniker) -> crate::core::code_graph::CodeGraph {
			ts::extract(
				"util.ts",
				"import { x } from 'react';",
				a,
				false,
				&ts::Presets::default(),
			)
		}
		fn run_rs(a: &Moniker) -> crate::core::code_graph::CodeGraph {
			rs::extract(
				"util.rs",
				"use serde_json;",
				a,
				false,
				&rs::Presets::default(),
			)
		}
		fn run_python(a: &Moniker) -> crate::core::code_graph::CodeGraph {
			python::extract("m.py", "import os\n", a, false, &python::Presets::default())
		}
		fn run_go(a: &Moniker) -> crate::core::code_graph::CodeGraph {
			go::extract(
				"foo.go",
				"package foo\nimport \"github.com/gorilla/mux\"\n",
				a,
				false,
				&go::Presets::default(),
			)
		}
		fn run_cs(a: &Moniker) -> crate::core::code_graph::CodeGraph {
			cs::extract(
				"F.cs",
				"using Newtonsoft.Json;\n",
				a,
				false,
				&cs::Presets::default(),
			)
		}

		let cases = [
			Case {
				lang: "ts",
				manifest: Manifest::PackageJson,
				extractor_head: "react",
				import_root: "react",
				run: run_ts,
			},
			Case {
				lang: "rs",
				manifest: Manifest::Cargo,
				extractor_head: "serde_json",
				import_root: "serde_json",
				run: run_rs,
			},
			Case {
				lang: "python",
				manifest: Manifest::Pyproject,
				extractor_head: "os",
				import_root: "os",
				run: run_python,
			},
			Case {
				lang: "go",
				manifest: Manifest::GoMod,
				extractor_head: "github.com",
				import_root: "github.com/gorilla/mux",
				run: run_go,
			},
			Case {
				lang: "cs",
				manifest: Manifest::Csproj,
				extractor_head: "Newtonsoft",
				import_root: "Newtonsoft.Json",
				run: run_cs,
			},
		];

		for case in cases {
			let g = (case.run)(&anchor);
			let target = first_external_target(&g, case.extractor_head).unwrap_or_else(|| {
				panic!(
					"lang={}: no ref target with head external_pkg:{}",
					case.lang, case.extractor_head
				)
			});
			let pkg = package_moniker(case.manifest, b"app", case.import_root);
			assert!(
				pkg.as_view().is_ancestor_of(&target.as_view()) || pkg == target,
				"lang={}: package_moniker({}) must be @>-ancestor of ref target (pkg={:?} target={:?})",
				case.lang,
				case.import_root,
				pkg.as_bytes(),
				target.as_bytes(),
			);
		}
	}

	#[test]
	fn ts_scoped_package_moniker_binds_extractor_ref() {
		use crate::lang::ts;
		let anchor = MonikerBuilder::new().project(b"app").build();
		let g = ts::extract(
			"util.ts",
			"import x from '@scope/pkg';",
			&anchor,
			false,
			&ts::Presets::default(),
		);
		let target = first_external_target(&g, "@scope/pkg").expect("scoped ref");
		let pkg = package_moniker(Manifest::PackageJson, b"app", "@scope/pkg");
		assert!(
			pkg.as_view().is_ancestor_of(&target.as_view()) || pkg == target,
			"scoped pkg must bind extractor ref via @>"
		);
	}
}
