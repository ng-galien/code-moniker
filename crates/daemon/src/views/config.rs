use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::model::{ViewDocument, ViewSpec};

const FRAGMENT_FILE_NAME: &str = "code-moniker.fragment.toml";
const ROOT_CONFIG_FILE_NAME: &str = ".code-moniker.toml";

#[derive(Debug, Default, Deserialize)]
struct RawViewFile {
	#[serde(default)]
	fragment: Option<String>,
	#[serde(default = "default_enabled")]
	enabled: bool,
	#[serde(default)]
	views: Vec<ViewSpec>,
}

fn default_enabled() -> bool {
	true
}

pub fn load(roots: &[PathBuf]) -> anyhow::Result<Vec<ViewDocument>> {
	let mut files = discover_view_files(roots);
	let mut views = Vec::new();
	for (root, path) in files.drain(..) {
		views.extend(parse_view_file(&root, &path)?);
	}
	validate_unique_ids(&views)?;
	views.sort_by(|a, b| a.spec.id.cmp(&b.spec.id));
	Ok(views)
}

fn discover_view_files(roots: &[PathBuf]) -> Vec<(PathBuf, PathBuf)> {
	let mut seen = HashSet::new();
	let mut files = Vec::new();
	for root in roots {
		let scan_root = scan_root(root);
		if seen.insert(scan_root.join(ROOT_CONFIG_FILE_NAME)) {
			let root_config = scan_root.join(ROOT_CONFIG_FILE_NAME);
			if root_config.is_file() {
				files.push((scan_root.clone(), root_config));
			}
		}
		for path in discover_fragment_paths(&scan_root) {
			if seen.insert(path.clone()) {
				files.push((scan_root.clone(), path));
			}
		}
	}
	files.sort_by(|a, b| a.1.cmp(&b.1));
	files
}

fn scan_root(path: &Path) -> PathBuf {
	if path.is_dir() {
		path.to_path_buf()
	} else {
		path.parent()
			.unwrap_or_else(|| Path::new("."))
			.to_path_buf()
	}
}

fn discover_fragment_paths(root: &Path) -> Vec<PathBuf> {
	let mut paths = ignore::WalkBuilder::new(root)
		.build()
		.filter_map(Result::ok)
		.filter(|entry| entry.file_type().is_some_and(|ty| ty.is_file()))
		.filter_map(|entry| {
			let path = entry.into_path();
			(path
				.file_name()
				.is_some_and(|name| name == FRAGMENT_FILE_NAME))
			.then_some(path)
		})
		.collect::<Vec<_>>();
	paths.sort();
	paths
}

fn parse_view_file(root: &Path, path: &Path) -> anyhow::Result<Vec<ViewDocument>> {
	let raw = std::fs::read_to_string(path)
		.map_err(|err| anyhow::anyhow!("cannot read view file {}: {err}", path.display()))?;
	let raw: RawViewFile = toml::from_str(&raw)
		.map_err(|err| anyhow::anyhow!("invalid view file {}: {err}", path.display()))?;
	if !raw.enabled || raw.views.is_empty() {
		return Ok(Vec::new());
	}
	let fragment = raw.fragment.unwrap_or_else(|| "root".to_string());
	let anchor = path.to_path_buf();
	let file_scope = file_scope(root, path);
	raw.views
		.into_iter()
		.map(|view| validate_view(anchor.clone(), file_scope.clone(), fragment.clone(), view))
		.collect()
}

fn validate_view(
	anchor: PathBuf,
	file_scope: String,
	fragment: String,
	view: ViewSpec,
) -> anyhow::Result<ViewDocument> {
	if !is_simple_id(&view.id) {
		anyhow::bail!(
			"view `{}` in {} must use a simple id",
			view.id,
			anchor.display()
		);
	}
	for boundary in &view.boundaries {
		if !is_simple_id(&boundary.id) {
			anyhow::bail!(
				"boundary `{}` in view `{}` must use a simple id",
				boundary.id,
				view.id
			);
		}
		if boundary.owns.is_empty() && boundary.rationale.is_none() {
			anyhow::bail!(
				"boundary `{}` in view `{}` must declare owns or rationale",
				boundary.id,
				view.id
			);
		}
		for rule in &boundary.forbid_rules {
			if rule.trim().is_empty() {
				anyhow::bail!(
					"boundary `{}` in view `{}` has an empty forbid rule",
					boundary.id,
					view.id
				);
			}
		}
	}
	for gotcha in &view.gotchas {
		if !is_simple_id(&gotcha.id) {
			anyhow::bail!(
				"gotcha `{}` in view `{}` must use a simple id",
				gotcha.id,
				view.id
			);
		}
		if gotcha.symbols.is_empty() && gotcha.rules.is_empty() && gotcha.check.is_none() {
			anyhow::bail!(
				"gotcha `{}` in view `{}` must reference symbols, rules, or check",
				gotcha.id,
				view.id
			);
		}
	}
	Ok(ViewDocument {
		fragment,
		anchor,
		scope_path: effective_scope(&file_scope, &view.scope),
		spec: view,
	})
}

fn validate_unique_ids(views: &[ViewDocument]) -> anyhow::Result<()> {
	let mut seen = HashSet::new();
	for view in views {
		if !seen.insert(view.spec.id.as_str()) {
			anyhow::bail!("duplicate view id `{}`", view.spec.id);
		}
	}
	Ok(())
}

fn file_scope(root: &Path, path: &Path) -> String {
	let parent = path.parent().unwrap_or_else(|| Path::new(""));
	let relative = parent.strip_prefix(root).unwrap_or(parent);
	normalize_path(relative)
}

fn effective_scope(file_scope: &str, declared: &str) -> String {
	let declared = declared.trim();
	if declared.is_empty() || declared == "." {
		file_scope.to_string()
	} else if file_scope.is_empty() {
		normalize_path(Path::new(declared))
	} else {
		normalize_path(&Path::new(file_scope).join(declared))
	}
}

fn normalize_path(path: &Path) -> String {
	path.to_string_lossy()
		.replace('\\', "/")
		.trim_start_matches("./")
		.trim_end_matches('/')
		.to_string()
}

fn is_simple_id(value: &str) -> bool {
	!value.is_empty()
		&& value
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}
