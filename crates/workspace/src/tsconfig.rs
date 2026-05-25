use std::path::{Path, PathBuf};

use code_moniker_core::lang::ts::PathAlias;
use serde::Deserialize;

#[derive(Debug, Clone, Default)]
pub struct TsResolution {
	pub aliases: Vec<PathAlias>,
}

#[derive(Deserialize)]
struct RawTsConfig {
	#[serde(rename = "compilerOptions", default)]
	compiler_options: Option<RawCompilerOptions>,
	#[serde(default)]
	references: Vec<RawReference>,
}

#[derive(Deserialize)]
struct RawCompilerOptions {
	#[serde(rename = "baseUrl", default)]
	base_url: Option<String>,
	#[serde(default)]
	paths: std::collections::BTreeMap<String, Vec<String>>,
}

#[derive(Deserialize)]
struct RawReference {
	path: String,
}

const TSCONFIG_CANDIDATES: &[&str] = &[
	"tsconfig.json",
	"tsconfig.app.json",
	"tsconfig.base.json",
	"tsconfig.web.json",
];

const SKIP_DIR_NAMES: &[&str] = &["node_modules", "target", "dist", "build", "out"];

const MAX_REFERENCES_DEPTH: usize = 3;

pub fn load(root: &Path) -> TsResolution {
	let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
	let mut aliases: Vec<PathAlias> = Vec::new();
	for entry in discover_tsconfigs(root) {
		merge_from_file(&entry, &canonical_root, &mut aliases, 0);
	}
	TsResolution { aliases }
}

fn discover_tsconfigs(root: &Path) -> Vec<PathBuf> {
	let mut out = Vec::new();
	push_tsconfigs_in(root, &mut out);
	if let Ok(entries) = std::fs::read_dir(root) {
		for entry in entries.flatten() {
			let path = entry.path();
			if path.is_dir() && !is_ignored_dir(&path) {
				push_tsconfigs_in(&path, &mut out);
			}
		}
	}
	out
}

fn push_tsconfigs_in(dir: &Path, out: &mut Vec<PathBuf>) {
	for name in TSCONFIG_CANDIDATES {
		let p = dir.join(name);
		if p.is_file() {
			out.push(p);
		}
	}
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
