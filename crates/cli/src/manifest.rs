use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::args::{ManifestArgs, ManifestFormat, OutputMode};
use code_moniker_core::core::uri::UriConfig;
use code_moniker_core::lang::build_manifest::{Dep, Manifest, parse};

const DEFAULT_PROJECT: &[u8] = b".";

pub fn run<W1: Write, W2: Write>(args: &ManifestArgs, stdout: &mut W1, stderr: &mut W2) -> i32 {
	match run_inner(args, stdout) {
		Ok(any) => {
			if any {
				0
			} else {
				1
			}
		}
		Err(e) => {
			let _ = writeln!(stderr, "code-moniker: {e:#}");
			2
		}
	}
}

fn run_inner<W: Write>(args: &ManifestArgs, stdout: &mut W) -> anyhow::Result<bool> {
	let path: &Path = &args.path;
	let scheme = args
		.scheme
		.as_deref()
		.unwrap_or(crate::DEFAULT_SCHEME)
		.to_string();
	let meta = std::fs::metadata(path)
		.map_err(|e| anyhow::anyhow!("cannot stat {}: {e}", path.display()))?;
	let entries = if meta.is_dir() {
		scan_dir(path)
	} else {
		scan_single(path)?
	};
	let any = !entries.is_empty();
	match args.mode() {
		OutputMode::Default => match args.format {
			ManifestFormat::Tsv => write_tsv(stdout, &entries, &scheme)?,
			ManifestFormat::Json => write_json(stdout, &entries, &scheme)?,
			#[cfg(feature = "pretty")]
			ManifestFormat::Tree => write_tree(stdout, &entries, &scheme)?,
		},
		OutputMode::Count => writeln!(stdout, "{}", entries.len())?,
		OutputMode::Quiet => {}
	}
	Ok(any)
}

struct Entry {
	manifest_uri: String,
	manifest_kind: Manifest,
	dep: Dep,
}

fn scan_single(path: &Path) -> anyhow::Result<Vec<Entry>> {
	let manifest = Manifest::for_filename(path).ok_or_else(|| {
		anyhow::anyhow!(
			"{}: filename not recognised as a build manifest (expected Cargo.toml / package.json / pom.xml / pyproject.toml / go.mod / *.csproj)",
			path.display()
		)
	})?;
	let content = std::fs::read_to_string(path)
		.map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
	let deps = parse(manifest, DEFAULT_PROJECT, &content)
		.map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
	let manifest_uri = path.display().to_string();
	Ok(deps
		.into_iter()
		.map(|dep| Entry {
			manifest_uri: manifest_uri.clone(),
			manifest_kind: manifest,
			dep,
		})
		.collect())
}

fn scan_dir(root: &Path) -> Vec<Entry> {
	use rayon::prelude::*;
	let manifests: Vec<(PathBuf, Manifest)> = ignore::WalkBuilder::new(root)
		.build()
		.filter_map(|e| e.ok())
		.filter(|e| e.file_type().is_some_and(|t| t.is_file()))
		.filter_map(|e| {
			let p = e.into_path();
			let m = Manifest::for_filename(&p)?;
			Some((p, m))
		})
		.collect();
	let mut entries: Vec<Entry> = manifests
		.par_iter()
		.flat_map(|(path, manifest)| {
			let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
			let content = match std::fs::read_to_string(path) {
				Ok(s) => s,
				Err(e) => {
					eprintln!("code-moniker: cannot read {}: {e}", path.display());
					return Vec::new();
				}
			};
			let deps = match parse(*manifest, DEFAULT_PROJECT, &content) {
				Ok(d) => d,
				Err(e) => {
					eprintln!("code-moniker: {}: {e}", path.display());
					return Vec::new();
				}
			};
			let manifest_uri = rel.display().to_string();
			deps.into_iter()
				.map(|dep| Entry {
					manifest_uri: manifest_uri.clone(),
					manifest_kind: *manifest,
					dep,
				})
				.collect()
		})
		.collect();
	enrich_cargo_workspace_members(&mut entries, root);
	entries.sort_by(|a, b| {
		a.manifest_uri
			.cmp(&b.manifest_uri)
			.then_with(|| a.dep.import_root.cmp(&b.dep.import_root))
			.then_with(|| a.dep.dep_kind.cmp(&b.dep.dep_kind))
	});
	entries
}

fn enrich_cargo_workspace_members(entries: &mut [Entry], root: &Path) {
	use std::collections::HashMap;
	let package_by_manifest: HashMap<String, Dep> = entries
		.iter()
		.filter(|e| e.manifest_kind == Manifest::Cargo && e.dep.dep_kind == "package")
		.map(|e| (e.manifest_uri.clone(), e.dep.clone()))
		.collect();
	for e in entries
		.iter_mut()
		.filter(|e| e.manifest_kind == Manifest::Cargo && e.dep.dep_kind == "workspace_member")
	{
		let Some(path) = e.dep.path.as_deref() else {
			continue;
		};
		let member_manifest = root.join(path).join("Cargo.toml");
		let rel = member_manifest
			.strip_prefix(root)
			.unwrap_or(&member_manifest)
			.display()
			.to_string();
		let Some(pkg) = package_by_manifest.get(&rel) else {
			continue;
		};
		e.dep.name = pkg.name.clone();
		e.dep.import_root = pkg.import_root.clone();
		e.dep.version = pkg.version.clone();
		e.dep.package_moniker = pkg.package_moniker.clone();
	}
}

fn render(m: &code_moniker_core::core::moniker::Moniker, scheme: &str) -> String {
	crate::render_uri(m, &UriConfig { scheme })
}

fn write_tsv<W: Write>(w: &mut W, entries: &[Entry], scheme: &str) -> std::io::Result<()> {
	for e in entries {
		writeln!(
			w,
			"{moniker}\t{manifest}\t{name}\t{import_root}\t{version}\t{dep_kind}",
			moniker = render(&e.dep.package_moniker, scheme),
			manifest = e.manifest_uri,
			name = e.dep.name,
			import_root = e.dep.import_root,
			version = e.dep.version.as_deref().unwrap_or(""),
			dep_kind = e.dep.dep_kind,
		)?;
	}
	Ok(())
}

#[derive(Serialize)]
struct JsonRow<'a> {
	package_moniker: String,
	manifest_uri: &'a str,
	manifest_kind: &'static str,
	name: &'a str,
	import_root: &'a str,
	#[serde(skip_serializing_if = "Option::is_none")]
	version: Option<&'a str>,
	dep_kind: &'a str,
	#[serde(skip_serializing_if = "Option::is_none")]
	path: Option<&'a str>,
}

#[cfg(feature = "pretty")]
fn write_tree<W: Write>(w: &mut W, entries: &[Entry], scheme: &str) -> std::io::Result<()> {
	use std::collections::BTreeMap;
	let mut groups: BTreeMap<&str, Vec<&Entry>> = BTreeMap::new();
	for e in entries {
		groups.entry(e.manifest_uri.as_str()).or_default().push(e);
	}
	for (uri, rows) in groups {
		writeln!(w, "{uri}")?;
		let last = rows.len();
		for (i, e) in rows.iter().enumerate() {
			let glyph = if i + 1 == last { "└─" } else { "├─" };
			let version = e.dep.version.as_deref().unwrap_or("-");
			writeln!(
				w,
				"  {glyph} {name}  {version}  ({kind})  {moniker}",
				name = e.dep.import_root,
				kind = e.dep.dep_kind,
				moniker = render(&e.dep.package_moniker, scheme),
			)?;
		}
	}
	Ok(())
}

fn write_json<W: Write>(w: &mut W, entries: &[Entry], scheme: &str) -> anyhow::Result<()> {
	let rows: Vec<JsonRow<'_>> = entries
		.iter()
		.map(|e| JsonRow {
			package_moniker: render(&e.dep.package_moniker, scheme),
			manifest_uri: &e.manifest_uri,
			manifest_kind: e.manifest_kind.tag(),
			name: &e.dep.name,
			import_root: &e.dep.import_root,
			version: e.dep.version.as_deref(),
			dep_kind: &e.dep.dep_kind,
			path: e.dep.path.as_deref(),
		})
		.collect();
	serde_json::to_writer_pretty(&mut *w, &rows)?;
	w.write_all(b"\n")?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;

	fn args_for(path: PathBuf, format: ManifestFormat) -> ManifestArgs {
		ManifestArgs {
			path,
			format,
			count: false,
			quiet: false,
			scheme: None,
		}
	}

	#[test]
	fn single_file_emits_tsv_row_per_dep() {
		let tmp = tempfile::tempdir().unwrap();
		let p = tmp.path().join("package.json");
		fs::write(
			&p,
			r#"{"name":"demo","version":"0.1.0","dependencies":{"react":"^18"}}"#,
		)
		.unwrap();
		let args = args_for(p, ManifestFormat::Tsv);
		let mut out = Vec::new();
		let mut err = Vec::new();
		assert_eq!(run(&args, &mut out, &mut err), 0);
		let text = String::from_utf8(out).unwrap();
		assert!(text.contains("code+moniker://./external_pkg:demo"));
		assert!(text.contains("code+moniker://./external_pkg:react"));
		assert!(text.contains("\treact\t"));
		assert!(text.contains("\tnormal"));
	}

	#[test]
	fn single_file_emits_json_array() {
		let tmp = tempfile::tempdir().unwrap();
		let p = tmp.path().join("Cargo.toml");
		fs::write(
			&p,
			"[package]\nname=\"demo\"\nversion=\"0.1.0\"\n\n[dependencies]\nserde-json = \"1\"\n",
		)
		.unwrap();
		let args = args_for(p, ManifestFormat::Json);
		let mut out = Vec::new();
		let mut err = Vec::new();
		assert_eq!(run(&args, &mut out, &mut err), 0);
		let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
		let rows = v.as_array().expect("array");
		assert!(rows.iter().any(|r| r["import_root"] == "serde_json"
			&& r["package_moniker"] == "code+moniker://./external_pkg:serde_json"));
		assert!(rows.iter().any(|r| r["manifest_kind"] == "cargo"));
	}

	#[test]
	fn cargo_workspace_json_exposes_members_and_path_dependencies() {
		let tmp = tempfile::tempdir().unwrap();
		let p = tmp.path().join("Cargo.toml");
		fs::write(
			&p,
			r#"
[workspace]
members = ["crates/core", "crates/cli"]

[workspace.dependencies]
code-moniker-core = { path = "crates/core", version = "0.2.0" }
serde = "1"
"#,
		)
		.unwrap();
		let args = args_for(p, ManifestFormat::Json);
		let mut out = Vec::new();
		let mut err = Vec::new();
		assert_eq!(run(&args, &mut out, &mut err), 0);
		let rows: Vec<serde_json::Value> = serde_json::from_slice(&out).unwrap();
		assert!(rows.iter().any(|r| {
			r["name"] == "crates/core"
				&& r["dep_kind"] == "workspace_member"
				&& r["path"] == "crates/core"
		}));
		assert!(rows.iter().any(|r| {
			r["name"] == "code-moniker-core"
				&& r["dep_kind"] == "path"
				&& r["path"] == "crates/core"
				&& r["import_root"] == "code_moniker_core"
		}));
		assert!(
			rows.iter()
				.any(|r| r["name"] == "serde" && r["dep_kind"] == "workspace")
		);
	}

	#[test]
	fn dir_mode_enriches_workspace_member_rows_from_member_packages() {
		let tmp = tempfile::tempdir().unwrap();
		let root = tmp.path();
		fs::write(
			root.join("Cargo.toml"),
			r#"
[workspace]
members = ["crates/core"]
"#,
		)
		.unwrap();
		fs::create_dir_all(root.join("crates/core")).unwrap();
		fs::write(
			root.join("crates/core/Cargo.toml"),
			r#"
[package]
name = "code-moniker-core"
version = "0.2.0"
"#,
		)
		.unwrap();
		let args = args_for(root.to_path_buf(), ManifestFormat::Json);
		let mut out = Vec::new();
		let mut err = Vec::new();
		assert_eq!(run(&args, &mut out, &mut err), 0);
		let rows: Vec<serde_json::Value> = serde_json::from_slice(&out).unwrap();
		let member = rows
			.iter()
			.find(|r| r["dep_kind"] == "workspace_member")
			.expect("workspace member row");
		assert_eq!(member["name"], "code-moniker-core");
		assert_eq!(member["version"], "0.2.0");
		assert_eq!(member["import_root"], "code_moniker_core");
		assert_eq!(
			member["package_moniker"],
			"code+moniker://./external_pkg:code_moniker_core"
		);
		assert_eq!(member["path"], "crates/core");
	}

	#[test]
	fn dir_mode_walks_every_manifest_kind() {
		let tmp = tempfile::tempdir().unwrap();
		let root = tmp.path();
		fs::write(
			root.join("package.json"),
			r#"{"name":"a","dependencies":{"react":"^18"}}"#,
		)
		.unwrap();
		fs::create_dir_all(root.join("sub")).unwrap();
		fs::write(
			root.join("sub/Cargo.toml"),
			"[package]\nname=\"b\"\nversion=\"0\"\n\n[dependencies]\nserde = \"1\"\n",
		)
		.unwrap();
		let args = args_for(root.to_path_buf(), ManifestFormat::Tsv);
		let mut out = Vec::new();
		let mut err = Vec::new();
		assert_eq!(run(&args, &mut out, &mut err), 0);
		let text = String::from_utf8(out).unwrap();
		assert!(text.contains("\tpackage.json\t"));
		assert!(text.contains("\tsub/Cargo.toml\t"));
		assert!(text.contains("external_pkg:react"));
		assert!(text.contains("external_pkg:serde"));
	}

	#[test]
	fn unknown_filename_reports_usage_error() {
		let tmp = tempfile::tempdir().unwrap();
		let p = tmp.path().join("README.md");
		fs::write(&p, "").unwrap();
		let args = args_for(p, ManifestFormat::Tsv);
		let mut out = Vec::new();
		let mut err = Vec::new();
		assert_eq!(run(&args, &mut out, &mut err), 2);
		let err_text = String::from_utf8(err).unwrap();
		assert!(err_text.contains("filename not recognised"));
	}

	#[test]
	fn empty_dir_exits_with_no_match() {
		let tmp = tempfile::tempdir().unwrap();
		let args = args_for(tmp.path().to_path_buf(), ManifestFormat::Tsv);
		let mut out = Vec::new();
		let mut err = Vec::new();
		assert_eq!(run(&args, &mut out, &mut err), 1);
	}

	#[test]
	fn count_mode_prints_total_rows() {
		let tmp = tempfile::tempdir().unwrap();
		let p = tmp.path().join("package.json");
		fs::write(&p, r#"{"name":"demo","dependencies":{"a":"1","b":"2"}}"#).unwrap();
		let mut args = args_for(p, ManifestFormat::Tsv);
		args.count = true;
		let mut out = Vec::new();
		let mut err = Vec::new();
		assert_eq!(run(&args, &mut out, &mut err), 0);
		assert_eq!(String::from_utf8(out).unwrap(), "3\n");
	}

	#[test]
	fn quiet_mode_emits_nothing() {
		let tmp = tempfile::tempdir().unwrap();
		let p = tmp.path().join("package.json");
		fs::write(&p, r#"{"name":"demo"}"#).unwrap();
		let mut args = args_for(p, ManifestFormat::Tsv);
		args.quiet = true;
		let mut out = Vec::new();
		let mut err = Vec::new();
		assert_eq!(run(&args, &mut out, &mut err), 0);
		assert!(out.is_empty());
	}

	#[cfg(feature = "pretty")]
	#[test]
	fn tree_format_groups_rows_by_manifest() {
		let tmp = tempfile::tempdir().unwrap();
		let p = tmp.path().join("package.json");
		fs::write(
			&p,
			r#"{"name":"demo","dependencies":{"react":"^18"},"devDependencies":{"vitest":"1"}}"#,
		)
		.unwrap();
		let args = args_for(p, ManifestFormat::Tree);
		let mut out = Vec::new();
		let mut err = Vec::new();
		assert_eq!(run(&args, &mut out, &mut err), 0);
		let text = String::from_utf8(out).unwrap();
		assert!(text.contains("package.json\n"), "{text}");
		assert!(text.contains("react"), "{text}");
		assert!(text.contains("└─"), "{text}");
		assert!(text.contains("(dev)"), "{text}");
		assert!(
			text.contains("code+moniker://./external_pkg:react"),
			"{text}"
		);
	}

	#[test]
	fn custom_scheme_round_trips_in_tsv() {
		let tmp = tempfile::tempdir().unwrap();
		let p = tmp.path().join("package.json");
		fs::write(&p, r#"{"name":"demo","dependencies":{"react":"^18"}}"#).unwrap();
		let mut args = args_for(p, ManifestFormat::Tsv);
		args.scheme = Some("acme://".into());
		let mut out = Vec::new();
		let mut err = Vec::new();
		assert_eq!(run(&args, &mut out, &mut err), 0);
		let text = String::from_utf8(out).unwrap();
		assert!(text.contains("acme://./external_pkg:react"));
	}
}
