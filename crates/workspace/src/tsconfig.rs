use std::path::{Path, PathBuf};

use code_moniker_core::lang::ts::{PathAlias, TsSdkProfile};
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct TsResolution {
	pub aliases: Vec<PathAlias>,
	root: PathBuf,
	default_sdk_profile: TsSdkProfile,
	sdk_profiles: Vec<TsSdkScope>,
}

#[derive(Debug, Clone)]
struct TsSdkScope {
	root: PathBuf,
	config_path: PathBuf,
	profile: TsSdkProfile,
	selector: TsFileSelector,
}

#[derive(Debug, Clone, Default)]
struct TsFileSelector {
	files: Option<Vec<PathBuf>>,
	include: Option<Vec<Regex>>,
	exclude: Vec<Regex>,
}

impl Default for TsResolution {
	fn default() -> Self {
		Self {
			aliases: Vec::new(),
			root: PathBuf::new(),
			default_sdk_profile: TsSdkProfile::default(),
			sdk_profiles: Vec::new(),
		}
	}
}

impl TsResolution {
	pub fn sdk_profile_for(&self, path: &Path) -> &TsSdkProfile {
		let unresolved = if path.is_absolute() || self.root.as_os_str().is_empty() {
			path.to_path_buf()
		} else {
			self.root.join(path)
		};
		let absolute = normalize_scope_path(&unresolved);
		self.sdk_profiles
			.iter()
			.filter_map(|scope| {
				scope
					.selector
					.match_score(&scope.root, &absolute)
					.map(|score| (scope, score))
			})
			.max_by(|(left, left_score), (right, right_score)| {
				left.root
					.components()
					.count()
					.cmp(&right.root.components().count())
					.then_with(|| left_score.cmp(right_score))
					.then_with(|| left.config_path.cmp(&right.config_path))
			})
			.map(|(scope, _)| &scope.profile)
			.unwrap_or(&self.default_sdk_profile)
	}
}

impl TsFileSelector {
	fn from_options(options: &EffectiveSdkOptions) -> Self {
		Self {
			files: options.files.clone(),
			include: options.include.clone(),
			exclude: options.exclude.clone().unwrap_or_default(),
		}
	}

	fn match_score(&self, root: &Path, absolute: &Path) -> Option<usize> {
		let absolute_text = normalize_config_path(&absolute.to_string_lossy());
		if self
			.files
			.as_ref()
			.is_some_and(|files| files.iter().any(|file| file == absolute))
		{
			return Some(2_000_000 + absolute_text.len());
		}
		let excluded = self
			.exclude
			.iter()
			.any(|pattern| pattern.is_match(&absolute_text));
		if !excluded
			&& let Some(score) = self.include.as_ref().and_then(|patterns| {
				patterns
					.iter()
					.filter(|pattern| pattern.is_match(&absolute_text))
					.map(|pattern| pattern.as_str().len())
					.max()
			}) {
			return Some(1_000_000 + score);
		}
		if self.files.is_none() && self.include.is_none() && !excluded && absolute.starts_with(root)
		{
			return Some(1);
		}
		None
	}
}

fn normalize_scope_path(path: &Path) -> PathBuf {
	path.canonicalize().unwrap_or_else(|_| {
		path.parent()
			.and_then(|parent| parent.canonicalize().ok())
			.and_then(|parent| path.file_name().map(|name| parent.join(name)))
			.unwrap_or_else(|| path.to_path_buf())
	})
}

#[derive(Deserialize)]
struct RawTsConfig {
	#[serde(rename = "compilerOptions", default)]
	compiler_options: Option<RawCompilerOptions>,
	#[serde(default)]
	extends: Option<RawExtends>,
	#[serde(default)]
	files: Option<Vec<String>>,
	#[serde(default)]
	include: Option<Vec<String>>,
	#[serde(default)]
	exclude: Option<Vec<String>>,
	#[serde(default)]
	references: Vec<RawReference>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawExtends {
	One(String),
	Many(Vec<String>),
}

impl RawExtends {
	fn specifiers(&self) -> Box<dyn Iterator<Item = &str> + '_> {
		match self {
			Self::One(specifier) => Box::new(std::iter::once(specifier.as_str())),
			Self::Many(specifiers) => Box::new(specifiers.iter().map(String::as_str)),
		}
	}
}

#[derive(Deserialize)]
struct RawCompilerOptions {
	#[serde(rename = "baseUrl", default)]
	base_url: Option<String>,
	#[serde(default)]
	paths: std::collections::BTreeMap<String, Vec<String>>,
	#[serde(default)]
	lib: Option<Vec<String>>,
	#[serde(default)]
	target: Option<String>,
	#[serde(default)]
	types: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct RawReference {
	path: String,
}

const SKIP_DIR_NAMES: &[&str] = &["node_modules", "target", "dist", "build", "out"];

const MAX_REFERENCES_DEPTH: usize = 3;
const MAX_SDK_EXTENDS_DEPTH: usize = 32;

pub fn load(root: &Path) -> TsResolution {
	let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
	let mut aliases: Vec<PathAlias> = Vec::new();
	let entries = discover_tsconfigs(root);
	for entry in &entries {
		merge_from_file(entry, &canonical_root, &mut aliases, 0);
	}
	let sdk_profiles = entries
		.iter()
		.filter_map(|entry| sdk_scope_from_file(entry))
		.collect();
	TsResolution {
		aliases,
		root: canonical_root,
		default_sdk_profile: TsSdkProfile::default(),
		sdk_profiles,
	}
}

fn discover_tsconfigs(root: &Path) -> Vec<PathBuf> {
	let mut out = Vec::new();
	let mut pending = vec![root.to_path_buf()];
	while let Some(directory) = pending.pop() {
		let Ok(entries) = std::fs::read_dir(&directory) else {
			continue;
		};
		for entry in entries.flatten() {
			let path = entry.path();
			let Ok(file_type) = entry.file_type() else {
				continue;
			};
			if file_type.is_dir() {
				if !is_ignored_dir(&path) {
					pending.push(path);
				}
			} else if file_type.is_file() && is_tsconfig_path(&path) {
				out.push(path);
			}
		}
	}
	out.sort();
	out
}

pub(crate) fn is_tsconfig_path(path: &Path) -> bool {
	path.file_name()
		.and_then(|name| name.to_str())
		.is_some_and(|name| {
			name == "tsconfig.json" || name.starts_with("tsconfig.") && name.ends_with(".json")
		})
}

fn is_ignored_dir(path: &Path) -> bool {
	let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
		return false;
	};
	name.starts_with('.') || SKIP_DIR_NAMES.contains(&name)
}

fn merge_from_file(file: &Path, root: &Path, aliases: &mut Vec<PathAlias>, depth: usize) {
	if depth > MAX_REFERENCES_DEPTH {
		return;
	}
	let Ok(raw) = std::fs::read_to_string(file) else {
		return;
	};
	let stripped = strip_jsonc(&raw);
	let Ok(parsed) = serde_json::from_str::<RawTsConfig>(&stripped) else {
		return;
	};
	let file_dir = file.parent().unwrap_or(root);

	if let Some(opts) = parsed.compiler_options.as_ref() {
		let base_dir = match opts.base_url.as_deref() {
			Some(s) => file_dir.join(s),
			None => file_dir.to_path_buf(),
		};
		for (pattern, substitutions) in &opts.paths {
			let Some(first) = substitutions.first() else {
				continue;
			};
			let Some(substitution) = rebase_substitution(&base_dir, first, root) else {
				continue;
			};
			if !aliases.iter().any(|a| a.pattern == *pattern) {
				aliases.push(PathAlias {
					pattern: pattern.clone(),
					substitution,
				});
			}
		}
	}

	for r in parsed.references {
		let p = file_dir.join(&r.path);
		let resolved = if p.is_file() {
			p
		} else if p.is_dir() {
			p.join("tsconfig.json")
		} else if p.extension().is_none() {
			let with_ext = p.with_extension("json");
			if with_ext.is_file() {
				with_ext
			} else {
				continue;
			}
		} else {
			continue;
		};
		merge_from_file(&resolved, root, aliases, depth + 1);
	}
}

#[derive(Default)]
struct EffectiveSdkOptions {
	libraries: Option<Vec<String>>,
	target: Option<String>,
	files: Option<Vec<PathBuf>>,
	include: Option<Vec<Regex>>,
	exclude: Option<Vec<Regex>>,
}

impl EffectiveSdkOptions {
	fn merge(&mut self, next: Self) {
		if next.libraries.is_some() {
			self.libraries = next.libraries;
		}
		if next.target.is_some() {
			self.target = next.target;
		}
		if next.files.is_some() {
			self.files = next.files;
		}
		if next.include.is_some() {
			self.include = next.include;
		}
		if next.exclude.is_some() {
			self.exclude = next.exclude;
		}
	}
}

fn sdk_scope_from_file(file: &Path) -> Option<TsSdkScope> {
	let mut visited = std::collections::BTreeSet::new();
	let options = load_sdk_options(file, 0, &mut visited)?;
	let selector = TsFileSelector::from_options(&options);
	let profile = match options.libraries {
		Some(libraries) => TsSdkProfile::from_libraries(libraries),
		None => options
			.target
			.as_deref()
			.map(default_libraries_for_target)
			.map(TsSdkProfile::from_libraries)
			.unwrap_or_default(),
	};
	let root = file
		.parent()?
		.canonicalize()
		.unwrap_or_else(|_| file.parent().unwrap_or(Path::new(".")).to_path_buf());
	Some(TsSdkScope {
		root,
		config_path: normalize_scope_path(file),
		profile,
		selector,
	})
}

fn load_sdk_options(
	file: &Path,
	depth: usize,
	visited: &mut std::collections::BTreeSet<PathBuf>,
) -> Option<EffectiveSdkOptions> {
	if depth > MAX_SDK_EXTENDS_DEPTH {
		eprintln!(
			"code-moniker: TypeScript extends chain exceeds {MAX_SDK_EXTENDS_DEPTH} at {}",
			file.display(),
		);
		return None;
	}
	let normalized = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
	if !visited.insert(normalized.clone()) {
		eprintln!(
			"code-moniker: cyclic TypeScript extends chain at {}",
			file.display(),
		);
		return None;
	}
	let raw = std::fs::read_to_string(file).ok()?;
	let parsed = serde_json::from_str::<RawTsConfig>(&strip_jsonc(&raw)).ok()?;
	let mut effective = EffectiveSdkOptions::default();
	if let Some(extends) = parsed.extends.as_ref() {
		for specifier in extends.specifiers() {
			if let Some(parent) = resolve_extends(file, specifier)
				&& let Some(parent_options) = load_sdk_options(&parent, depth + 1, visited)
			{
				effective.merge(parent_options);
			}
		}
	}
	let config_dir = file
		.parent()
		.map(normalize_scope_path)
		.unwrap_or_else(|| PathBuf::from("."));
	let own_options = EffectiveSdkOptions {
		libraries: parsed
			.compiler_options
			.as_ref()
			.and_then(|options| options.lib.clone()),
		target: parsed
			.compiler_options
			.as_ref()
			.and_then(|options| options.target.clone()),
		files: parsed.files.map(|files| {
			files
				.into_iter()
				.map(|path| crate::path_util::lexical_path(&config_dir.join(path)))
				.collect()
		}),
		include: parsed
			.include
			.map(|patterns| compile_absolute_ts_globs(&config_dir, patterns)),
		exclude: parsed
			.exclude
			.map(|patterns| compile_absolute_ts_globs(&config_dir, patterns)),
	};
	effective.merge(own_options);
	if let Some(options) = parsed.compiler_options {
		let _ = options.types;
	}
	visited.remove(&normalized);
	Some(effective)
}

fn resolve_extends(file: &Path, specifier: &str) -> Option<PathBuf> {
	if specifier.starts_with('.') || Path::new(specifier).is_absolute() {
		let base = file.parent().unwrap_or(Path::new(".")).join(specifier);
		return resolve_config_candidate(&base);
	}
	resolve_package_extends(file, specifier)
}

fn resolve_package_extends(file: &Path, specifier: &str) -> Option<PathBuf> {
	let segments = specifier
		.split('/')
		.filter(|segment| !segment.is_empty())
		.collect::<Vec<_>>();
	let package_len = if segments.first()?.starts_with('@') {
		2
	} else {
		1
	};
	if segments.len() < package_len {
		return None;
	}
	let package_name = segments[..package_len].join("/");
	let subpath = segments[package_len..].join("/");
	let mut directory = file.parent();
	while let Some(current) = directory {
		let package_root = current.join("node_modules").join(&package_name);
		if package_root.is_dir() {
			if !subpath.is_empty() {
				return resolve_config_candidate(&package_root.join(subpath));
			}
			if let Some(target) = package_tsconfig_target(&package_root)
				&& let Some(resolved) = resolve_config_candidate(&package_root.join(target))
			{
				return Some(resolved);
			}
			return resolve_config_candidate(&package_root.join("tsconfig.json"));
		}
		directory = current.parent();
	}
	None
}

fn package_tsconfig_target(package_root: &Path) -> Option<String> {
	let raw = std::fs::read_to_string(package_root.join("package.json")).ok()?;
	let parsed = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
	parsed.get("tsconfig")?.as_str().map(str::to_owned)
}

fn resolve_config_candidate(base: &Path) -> Option<PathBuf> {
	if base.is_file() {
		return Some(base.to_path_buf());
	}
	if base.is_dir() {
		let candidate = base.join("tsconfig.json");
		return candidate.is_file().then_some(candidate);
	}
	if base.extension().is_none() {
		let candidate = base.with_extension("json");
		return candidate.is_file().then_some(candidate);
	}
	None
}

fn normalize_config_path(path: &str) -> String {
	path.trim_start_matches("./").replace('\\', "/")
}

fn compile_absolute_ts_globs(base: &Path, patterns: Vec<String>) -> Vec<Regex> {
	patterns
		.into_iter()
		.filter_map(|pattern| {
			let absolute = crate::path_util::lexical_path(&base.join(pattern));
			compile_ts_glob(&absolute.to_string_lossy())
		})
		.collect()
}

fn compile_ts_glob(pattern: &str) -> Option<Regex> {
	let normalized = normalize_config_path(pattern);
	if normalized.is_empty() {
		return None;
	}
	if !normalized.contains('*') && !normalized.contains('?') {
		let exact = regex::escape(normalized.trim_end_matches('/'));
		let suffix = if Path::new(&normalized).extension().is_none() {
			"(?:/.*)?"
		} else {
			""
		};
		return Regex::new(&format!("^{exact}{suffix}$")).ok();
	}
	let chars = normalized.chars().collect::<Vec<_>>();
	let mut regex = String::from("^");
	let mut index = 0;
	while index < chars.len() {
		match chars[index] {
			'*' if chars.get(index + 1) == Some(&'*') => {
				index += 2;
				if chars.get(index) == Some(&'/') {
					regex.push_str("(?:.*/)?");
					index += 1;
				} else {
					regex.push_str(".*");
				}
			}
			'*' => {
				regex.push_str("[^/]*");
				index += 1;
			}
			'?' => {
				regex.push_str("[^/]");
				index += 1;
			}
			character => {
				regex.push_str(&regex::escape(&character.to_string()));
				index += 1;
			}
		}
	}
	regex.push('$');
	Regex::new(&regex).ok()
}

fn default_libraries_for_target(target: &str) -> Vec<String> {
	let normalized = target.trim().to_ascii_lowercase();
	match normalized.as_str() {
		"es3" | "es5" => vec![
			"es5".into(),
			"dom".into(),
			"dom.iterable".into(),
			"scripthost".into(),
		],
		"es6" => vec!["es2015.full".into()],
		"latest" => vec!["esnext.full".into()],
		target => vec![format!("{target}.full")],
	}
}

fn rebase_substitution(base_dir: &Path, sub: &str, root: &Path) -> Option<String> {
	let (prefix, star, suffix) = match sub.find('*') {
		Some(i) => (&sub[..i], true, &sub[i + 1..]),
		None => (sub, false, ""),
	};
	let abs_prefix = base_dir.join(prefix);
	let canonical = abs_prefix.canonicalize().unwrap_or_else(|_| {
		base_dir
			.canonicalize()
			.unwrap_or_else(|_| base_dir.to_path_buf())
			.join(prefix)
	});
	let rel = canonical.strip_prefix(root).ok()?;
	let rel_str = rel.to_string_lossy();
	let mut out = String::from("./");
	out.push_str(&rel_str);
	if star {
		if !out.ends_with('/') && !rel_str.is_empty() {
			out.push('/');
		}
		out.push('*');
		out.push_str(suffix);
	}
	Some(out)
}

fn strip_jsonc(src: &str) -> String {
	let bytes = src.as_bytes();
	let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
	let mut i = 0;
	while i < bytes.len() {
		let b = bytes[i];
		if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
			while i < bytes.len() && bytes[i] != b'\n' {
				i += 1;
			}
		} else if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
			i += 2;
			while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
				i += 1;
			}
			i = (i + 2).min(bytes.len());
		} else if b == b'"' {
			out.push(b);
			i += 1;
			while i < bytes.len() && bytes[i] != b'"' {
				if bytes[i] == b'\\' && i + 1 < bytes.len() {
					out.push(bytes[i]);
					out.push(bytes[i + 1]);
					i += 2;
				} else {
					out.push(bytes[i]);
					i += 1;
				}
			}
			if i < bytes.len() {
				out.push(bytes[i]);
				i += 1;
			}
		} else {
			out.push(b);
			i += 1;
		}
	}
	String::from_utf8(out).unwrap_or_else(|_| src.to_string())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;
	use tempfile::tempdir;

	#[test]
	fn load_picks_aliases_from_root_tsconfig() {
		let tmp = tempdir().unwrap();
		fs::write(
			tmp.path().join("tsconfig.json"),
			r#"{"compilerOptions": {"paths": {"@/*": ["./src/*"]}}}"#,
		)
		.unwrap();
		let r = load(tmp.path());
		assert_eq!(r.aliases.len(), 1);
		assert_eq!(r.aliases[0].pattern, "@/*");
		assert_eq!(r.aliases[0].substitution, "./src/*");
	}

	#[test]
	fn load_picks_aliases_from_nested_tsconfig() {
		let tmp = tempdir().unwrap();
		fs::create_dir_all(tmp.path().join("web/src")).unwrap();
		fs::write(
			tmp.path().join("web/tsconfig.app.json"),
			r#"{"compilerOptions": {"paths": {"@/*": ["./src/*"]}}}"#,
		)
		.unwrap();
		let r = load(tmp.path());
		let pattern_hit = r
			.aliases
			.iter()
			.any(|a| a.pattern == "@/*" && a.substitution.ends_with("web/src/*"));
		assert!(
			pattern_hit,
			"alias from nested tsconfig must be rebased to project root: {:?}",
			r.aliases
		);
	}

	#[test]
	fn load_strips_jsonc_comments() {
		let tmp = tempdir().unwrap();
		fs::write(
			tmp.path().join("tsconfig.json"),
			"{\n  // a comment\n  \"compilerOptions\": { \"paths\": { \"@/*\": [\"./src/*\"] } } /* trailing */\n}",
		)
		.unwrap();
		let r = load(tmp.path());
		assert_eq!(r.aliases.len(), 1);
	}

	#[test]
	fn load_empty_when_no_tsconfig() {
		let tmp = tempdir().unwrap();
		let r = load(tmp.path());
		assert!(r.aliases.is_empty());
	}

	#[test]
	fn load_selects_nearest_sdk_profile_without_cross_runtime_pollution() {
		let tmp = tempdir().unwrap();
		fs::create_dir_all(tmp.path().join("server")).unwrap();
		fs::create_dir_all(tmp.path().join("worker")).unwrap();
		fs::write(
			tmp.path().join("tsconfig.json"),
			r#"{"compilerOptions":{"target":"ES2022","lib":["ES2022","DOM"]}}"#,
		)
		.unwrap();
		fs::write(
			tmp.path().join("server/tsconfig.json"),
			r#"{"compilerOptions":{"target":"ES2022","lib":["ES2022"]}}"#,
		)
		.unwrap();
		fs::write(
			tmp.path().join("worker/tsconfig.json"),
			r#"{"compilerOptions":{"target":"ES2022","lib":["ES2022","WebWorker"]}}"#,
		)
		.unwrap();

		let resolution = load(tmp.path());
		let dom = resolution.sdk_profile_for(&tmp.path().join("src/app.ts"));
		let server = resolution.sdk_profile_for(&tmp.path().join("server/main.ts"));
		let worker = resolution.sdk_profile_for(&tmp.path().join("worker/main.ts"));

		assert!(dom.is_global_value(b"document"));
		assert!(!server.is_global_value(b"document"));
		assert!(server.is_global_type(b"Promise"));
		assert!(worker.is_global_value(b"self"));
		assert!(!worker.is_global_value(b"document"));
	}

	#[test]
	fn load_inherits_and_overrides_sdk_libraries_from_local_extends() {
		let tmp = tempdir().unwrap();
		fs::create_dir_all(tmp.path().join("server")).unwrap();
		fs::create_dir_all(tmp.path().join("web")).unwrap();
		fs::write(
			tmp.path().join("tsconfig.base.json"),
			r#"{"compilerOptions":{"target":"ES2022","lib":["ES2022"],"types":["node"]}}"#,
		)
		.unwrap();
		fs::write(
			tmp.path().join("server/tsconfig.json"),
			r#"{"extends":"../tsconfig.base.json"}"#,
		)
		.unwrap();
		fs::write(
			tmp.path().join("web/tsconfig.app.json"),
			r#"{"extends":"../tsconfig.base.json","compilerOptions":{"lib":["ES2022","DOM"]}}"#,
		)
		.unwrap();

		let resolution = load(tmp.path());
		let server = resolution.sdk_profile_for(&tmp.path().join("server/main.ts"));
		let web = resolution.sdk_profile_for(&tmp.path().join("web/main.ts"));

		assert!(server.is_global_type(b"Promise"));
		assert!(!server.is_global_value(b"document"));
		assert!(
			!server.is_global_value(b"process"),
			"`types: [\"node\"]` selects declaration packages, not TypeScript SDK libraries",
		);
		assert!(web.is_global_type(b"Promise"));
		assert!(web.is_global_value(b"document"));
	}

	#[test]
	fn load_selects_same_directory_profiles_by_files_and_include() {
		let tmp = tempdir().unwrap();
		fs::create_dir_all(tmp.path().join("src")).unwrap();
		fs::write(
			tmp.path().join("tsconfig.app.json"),
			r#"{
				"compilerOptions":{"target":"ES2022","lib":["ES2022","DOM"]},
				"include":["src/**/*.ts"]
			}"#,
		)
		.unwrap();
		fs::write(
			tmp.path().join("tsconfig.node.json"),
			r#"{
				"compilerOptions":{"target":"ES2022","lib":["ES2022"]},
				"files":["vite.config.ts"]
			}"#,
		)
		.unwrap();

		let resolution = load(tmp.path());
		assert!(
			resolution
				.sdk_profile_for(&tmp.path().join("src/app.ts"))
				.is_global_value(b"document"),
			"the app include must select the DOM profile",
		);
		assert!(
			!resolution
				.sdk_profile_for(&tmp.path().join("vite.config.ts"))
				.is_global_value(b"document"),
			"the explicit Node file must not inherit the sibling DOM profile",
		);
	}

	#[test]
	fn load_resolves_package_extends_from_node_modules() {
		let tmp = tempdir().unwrap();
		let preset = tmp.path().join("node_modules/@tsconfig/node20");
		fs::create_dir_all(&preset).unwrap();
		fs::write(
			preset.join("tsconfig.json"),
			r#"{"compilerOptions":{"target":"ES2022","lib":["ES2022"]}}"#,
		)
		.unwrap();
		fs::write(
			tmp.path().join("tsconfig.json"),
			r#"{"extends":"@tsconfig/node20/tsconfig.json"}"#,
		)
		.unwrap();

		let resolution = load(tmp.path());
		let profile = resolution.sdk_profile_for(&tmp.path().join("server.ts"));
		assert!(profile.is_global_type(b"Promise"));
		assert!(
			!profile.is_global_value(b"document"),
			"an npm-resolved Node preset must not fall back to the default DOM profile",
		);
	}

	#[test]
	fn load_merges_extends_arrays_in_order() {
		let tmp = tempdir().unwrap();
		fs::write(
			tmp.path().join("base.json"),
			r#"{"compilerOptions":{"target":"ES2022","lib":["ES2022","DOM"]}}"#,
		)
		.unwrap();
		fs::write(
			tmp.path().join("node.json"),
			r#"{"compilerOptions":{"lib":["ES2022"]}}"#,
		)
		.unwrap();
		fs::write(
			tmp.path().join("tsconfig.json"),
			r#"{"extends":["./base.json","./node.json"]}"#,
		)
		.unwrap();

		let profile = load(tmp.path())
			.sdk_profile_for(&tmp.path().join("server.ts"))
			.clone();
		assert!(profile.is_global_type(b"Promise"));
		assert!(
			!profile.is_global_value(b"document"),
			"later bases in an extends array must override earlier bases",
		);
	}

	#[test]
	fn load_keeps_the_declaring_config_as_inherited_selector_origin() {
		let tmp = tempdir().unwrap();
		fs::create_dir_all(tmp.path().join("packages/app")).unwrap();
		fs::create_dir_all(tmp.path().join("shared/private")).unwrap();
		fs::write(
			tmp.path().join("tsconfig.base.json"),
			r#"{
				"compilerOptions":{"lib":["ES2022","DOM"]},
				"files":["special.ts"],
				"include":["shared/**/*.ts"],
				"exclude":["shared/private/**/*.ts"]
			}"#,
		)
		.unwrap();
		fs::write(
			tmp.path().join("packages/app/tsconfig.json"),
			r#"{
				"extends":"../../tsconfig.base.json",
				"compilerOptions":{"lib":["ES2022"]}
			}"#,
		)
		.unwrap();

		let resolution = load(tmp.path());
		for path in ["special.ts", "shared/public.ts"] {
			assert!(
				!resolution
					.sdk_profile_for(&tmp.path().join(path))
					.is_global_value(b"document"),
				"{path} must use the child Node profile selected from its base config origin",
			);
		}
		assert!(
			resolution
				.sdk_profile_for(&tmp.path().join("shared/private/secret.ts"))
				.is_global_value(b"document"),
			"the inherited exclusion must remain anchored to the base config",
		);
	}

	#[test]
	fn load_supports_defensively_bounded_deep_extends_chains() {
		let tmp = tempdir().unwrap();
		fs::create_dir_all(tmp.path().join("app")).unwrap();
		fs::write(
			tmp.path().join("tsconfig.level0.json"),
			r#"{"compilerOptions":{"lib":["ES2022"]}}"#,
		)
		.unwrap();
		for level in 1..=5 {
			fs::write(
				tmp.path().join(format!("tsconfig.level{level}.json")),
				format!(r#"{{"extends":"./tsconfig.level{}.json"}}"#, level - 1),
			)
			.unwrap();
		}
		fs::write(
			tmp.path().join("app/tsconfig.json"),
			r#"{"extends":"../tsconfig.level5.json"}"#,
		)
		.unwrap();

		let profile = load(tmp.path())
			.sdk_profile_for(&tmp.path().join("app/server.ts"))
			.clone();
		assert!(profile.is_global_type(b"Promise"));
		assert!(
			!profile.is_global_value(b"document"),
			"a valid extends chain deeper than project-reference traversal must keep its base libs",
		);
	}

	#[test]
	fn load_matches_unicode_include_and_exclude_patterns() {
		let tmp = tempdir().unwrap();
		fs::create_dir_all(tmp.path().join("src/équipe/privé")).unwrap();
		fs::write(
			tmp.path().join("tsconfig.json"),
			r#"{
				"compilerOptions":{"lib":["ES2022"]},
				"include":["src/équipe/**/*.ts"],
				"exclude":["src/équipe/privé/**/*.ts"]
			}"#,
		)
		.unwrap();

		let resolution = load(tmp.path());
		assert!(
			!resolution
				.sdk_profile_for(&tmp.path().join("src/équipe/public.ts"))
				.is_global_value(b"document"),
			"a Unicode include must select the Node profile",
		);
		assert!(
			resolution
				.sdk_profile_for(&tmp.path().join("src/équipe/privé/secret.ts"))
				.is_global_value(b"document"),
			"a Unicode exclude must keep the file outside that profile",
		);
	}

	#[test]
	fn load_ignores_node_modules() {
		let tmp = tempdir().unwrap();
		fs::create_dir_all(tmp.path().join("node_modules/foo")).unwrap();
		fs::write(
			tmp.path().join("node_modules/foo/tsconfig.json"),
			r#"{"compilerOptions": {"paths": {"!polluted/*": ["./*"]}}}"#,
		)
		.unwrap();
		let r = load(tmp.path());
		assert!(
			r.aliases.iter().all(|a| a.pattern != "!polluted/*"),
			"node_modules tsconfigs must not pollute aliases: {:?}",
			r.aliases
		);
	}

	#[test]
	fn strip_jsonc_preserves_utf8_multibyte() {
		let src = "{ \"k\": \"é à\" } // 中文";
		let out = strip_jsonc(src);
		assert!(out.contains("é à"), "UTF-8 multibyte preserved: {out:?}");
	}
}
