use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

use crate::lang::Lang;

const DEFAULT_PRESET: &str = include_str!("presets/default.toml");

/// Internal kinds emitted by extractors that are not part of `Lang::ALLOWED_KINDS`
/// but ARE legitimate rule targets.
const INTERNAL_KINDS: &[&str] = &["module", "local", "param", "comment"];

#[derive(Debug, Default, Deserialize, Clone)]
pub struct Config {
	#[serde(default)]
	pub default: LangRules,
	#[serde(default)]
	pub ts: LangRules,
	#[serde(default)]
	pub rust: LangRules,
	#[serde(default)]
	pub java: LangRules,
	#[serde(default)]
	pub python: LangRules,
	#[serde(default)]
	pub go: LangRules,
	#[serde(default)]
	pub cs: LangRules,
	#[serde(default)]
	pub sql: LangRules,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct LangRules {
	#[serde(flatten)]
	pub kinds: HashMap<String, KindRules>,
}

#[derive(Debug, Default, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct KindRules {
	pub name_pattern: Option<String>,
	pub forbid_name_patterns: Option<Vec<String>>,
	pub max_lines: Option<u32>,
	pub max_count_per_parent: Option<u32>,
	pub forbid_patterns: Option<Vec<String>>,
	pub allow_only_patterns: Option<Vec<String>>,
	/// Visibility name that triggers the doc-comment requirement, e.g. "public",
	/// "any". `None` disables the rule.
	pub require_doc_comment: Option<String>,
	pub messages: Option<HashMap<String, String>>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
	#[error("default preset embedded in the binary is invalid: {0}")]
	DefaultPresetInvalid(toml::de::Error),
	#[error("user config `{path}`: {error}")]
	UserConfig {
		path: String,
		error: toml::de::Error,
	},
	#[error("cannot read `{path}`: {error}")]
	Io { path: String, error: std::io::Error },
	#[error("invalid regex at `{at}` (`{pattern}`): {error}")]
	InvalidRegex {
		at: String,
		pattern: String,
		error: regex::Error,
	},
	#[error("unknown kind `{kind}` under `[{section}.{kind}]` (allowed: {allowed})")]
	UnknownKind {
		section: String,
		kind: String,
		allowed: String,
	},
}

pub fn load_default() -> Result<Config, ConfigError> {
	let cfg: Config = toml::from_str(DEFAULT_PRESET).map_err(ConfigError::DefaultPresetInvalid)?;
	validate(&cfg, "<embedded preset>")?;
	Ok(cfg)
}

/// Load the embedded defaults and merge `user_path` on top if it exists.
/// Missing user config is not an error — defaults stand alone.
pub fn load_with_overrides(user_path: Option<&Path>) -> Result<Config, ConfigError> {
	let mut cfg = load_default()?;
	if let Some(p) = user_path {
		if !p.exists() {
			return Ok(cfg);
		}
		let raw = std::fs::read_to_string(p).map_err(|error| ConfigError::Io {
			path: p.display().to_string(),
			error,
		})?;
		let user: Config = toml::from_str(&raw).map_err(|error| ConfigError::UserConfig {
			path: p.display().to_string(),
			error,
		})?;
		validate(&user, &p.display().to_string())?;
		merge_into(&mut cfg, user);
	}
	Ok(cfg)
}

fn merge_into(base: &mut Config, ov: Config) {
	merge_lang(&mut base.default, ov.default);
	merge_lang(&mut base.ts, ov.ts);
	merge_lang(&mut base.rust, ov.rust);
	merge_lang(&mut base.java, ov.java);
	merge_lang(&mut base.python, ov.python);
	merge_lang(&mut base.go, ov.go);
	merge_lang(&mut base.cs, ov.cs);
	merge_lang(&mut base.sql, ov.sql);
}

fn merge_lang(base: &mut LangRules, ov: LangRules) {
	for (kind, ov_rules) in ov.kinds {
		match base.kinds.get_mut(&kind) {
			Some(base_rules) => merge_kind(base_rules, ov_rules),
			None => {
				base.kinds.insert(kind, ov_rules);
			}
		}
	}
}

/// Field-by-field merge: each `Some` in `ov` overwrites `base`. Vec/HashMap
/// fields are replaced wholesale, not extended — to extend a default list,
/// the user must repeat the inherited entries explicitly.
fn merge_kind(base: &mut KindRules, ov: KindRules) {
	if ov.name_pattern.is_some() {
		base.name_pattern = ov.name_pattern;
	}
	if ov.forbid_name_patterns.is_some() {
		base.forbid_name_patterns = ov.forbid_name_patterns;
	}
	if ov.max_lines.is_some() {
		base.max_lines = ov.max_lines;
	}
	if ov.max_count_per_parent.is_some() {
		base.max_count_per_parent = ov.max_count_per_parent;
	}
	if ov.forbid_patterns.is_some() {
		base.forbid_patterns = ov.forbid_patterns;
	}
	if ov.allow_only_patterns.is_some() {
		base.allow_only_patterns = ov.allow_only_patterns;
	}
	if ov.require_doc_comment.is_some() {
		base.require_doc_comment = ov.require_doc_comment;
	}
	if let Some(ov_msgs) = ov.messages {
		match &mut base.messages {
			Some(base_msgs) => {
				for (k, v) in ov_msgs {
					base_msgs.insert(k, v);
				}
			}
			None => base.messages = Some(ov_msgs),
		}
	}
}

fn validate(cfg: &Config, path: &str) -> Result<(), ConfigError> {
	validate_lang_section(&cfg.default, "default", &allowed_kinds_set(None), path)?;
	for lang in Lang::ALL {
		let allowed = allowed_kinds_set(Some(*lang));
		validate_lang_section(cfg.for_lang(*lang), config_section(*lang), &allowed, path)?;
	}
	Ok(())
}

fn allowed_kinds_set(lang: Option<Lang>) -> Vec<&'static str> {
	let mut out: Vec<&'static str> = INTERNAL_KINDS.to_vec();
	if let Some(l) = lang {
		out.extend(l.allowed_kinds().iter().copied());
	} else {
		for l in Lang::ALL {
			out.extend(l.allowed_kinds().iter().copied());
		}
	}
	out.sort();
	out.dedup();
	out
}

/// TOML section / rule-id segment for a language. `Lang::Rs` aliases to
/// `rust` for readability — every other lang uses its `LANG_TAG` verbatim.
pub(crate) fn config_section(lang: Lang) -> &'static str {
	match lang {
		Lang::Rs => "rust",
		other => other.tag(),
	}
}

fn validate_lang_section(
	lr: &LangRules,
	section: &str,
	allowed: &[&str],
	_path: &str,
) -> Result<(), ConfigError> {
	for kind in lr.kinds.keys() {
		if !allowed.contains(&kind.as_str()) {
			return Err(ConfigError::UnknownKind {
				section: section.to_string(),
				kind: kind.clone(),
				allowed: allowed.join(", "),
			});
		}
	}
	Ok(())
}

impl Config {
	pub fn for_lang(&self, lang: Lang) -> &LangRules {
		match lang {
			Lang::Ts => &self.ts,
			Lang::Rs => &self.rust,
			Lang::Java => &self.java,
			Lang::Python => &self.python,
			Lang::Go => &self.go,
			Lang::Cs => &self.cs,
			#[cfg(any(feature = "pg14", feature = "pg15", feature = "pg16", feature = "pg17"))]
			Lang::Sql => &self.sql,
		}
	}

	/// Effective rules for `(lang, kind)`, falling back to `default.<kind>`
	/// when the language has nothing defined for that kind.
	pub fn rules_for(&self, lang: Lang, kind: &str) -> Option<&KindRules> {
		self.for_lang(lang)
			.kinds
			.get(kind)
			.or_else(|| self.default.kinds.get(kind))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn parse(s: &str) -> Result<Config, ConfigError> {
		let cfg: Config = toml::from_str(s).map_err(|e| ConfigError::UserConfig {
			path: "<test>".to_string(),
			error: e,
		})?;
		validate(&cfg, "<test>")?;
		Ok(cfg)
	}

	#[test]
	fn embedded_default_parses() {
		let cfg = load_default().expect("default preset must parse");
		assert!(cfg.ts.kinds.contains_key("class"));
		assert!(cfg.ts.kinds.contains_key("function"));
		assert!(cfg.ts.kinds.contains_key("comment"));
	}

	#[test]
	fn ts_class_has_pascal_case_pattern_in_default() {
		let cfg = load_default().unwrap();
		let r = cfg.rules_for(Lang::Ts, "class").expect("ts.class present");
		let pat = r.name_pattern.as_deref().unwrap();
		assert!(
			pat.contains("[A-Z]"),
			"expected PascalCase regex, got {pat}"
		);
	}

	#[test]
	fn rules_for_falls_back_to_default_section() {
		let cfg = parse(
			r#"
			[default.module]
			max_lines = 99

			[ts.class]
			name_pattern = "x"
			"#,
		)
		.unwrap();
		assert_eq!(
			cfg.rules_for(Lang::Ts, "module").and_then(|r| r.max_lines),
			Some(99),
		);
	}

	#[test]
	fn override_max_lines_keeps_inherited_name_pattern() {
		let user = parse(
			r#"
			[ts.function]
			max_lines = 10
			"#,
		)
		.unwrap();
		let mut base = load_default().unwrap();
		let pattern_before = base
			.rules_for(Lang::Ts, "function")
			.unwrap()
			.name_pattern
			.clone();
		assert!(
			pattern_before.is_some(),
			"preset should ship a TS function name pattern"
		);

		merge_into(&mut base, user);
		let merged = base.rules_for(Lang::Ts, "function").unwrap();
		assert_eq!(merged.max_lines, Some(10), "override applied");
		assert_eq!(
			merged.name_pattern, pattern_before,
			"name_pattern from preset must survive the merge"
		);
	}

	#[test]
	fn override_messages_extends_inherited_keys() {
		let user = parse(
			r#"
			[ts.class.messages]
			max_lines = "custom"
			"#,
		)
		.unwrap();
		let mut base = load_default().unwrap();
		merge_into(&mut base, user);
		let merged = base.rules_for(Lang::Ts, "class").unwrap();
		let msgs = merged.messages.as_ref().expect("messages present");
		assert_eq!(msgs.get("max_lines").map(|s| s.as_str()), Some("custom"));
		assert!(
			msgs.contains_key("name_pattern"),
			"preset name_pattern message must survive: {msgs:?}"
		);
	}

	#[test]
	fn unknown_field_in_kind_rules_is_rejected() {
		let r = toml::from_str::<Config>(
			r#"
			[ts.function]
			max_lines = 10
			bogus_unknown_field = true
			"#,
		);
		assert!(r.is_err(), "deny_unknown_fields should reject typos");
	}

	#[test]
	fn unknown_kind_section_is_rejected() {
		let r = parse(
			r#"
			[ts.classs]
			name_pattern = "x"
			"#,
		);
		match r {
			Err(ConfigError::UnknownKind { kind, .. }) => assert_eq!(kind, "classs"),
			other => panic!("expected UnknownKind, got {other:?}"),
		}
	}

	#[test]
	fn missing_user_file_is_not_an_error() {
		let cfg = load_with_overrides(Some(Path::new("/no/such/file.toml")))
			.expect("missing file falls back to defaults");
		assert!(cfg.ts.kinds.contains_key("class"));
	}

	#[test]
	fn malformed_user_file_returns_user_config_error() {
		let dir = tempfile::tempdir().unwrap();
		let p = dir.path().join("bad.toml");
		std::fs::write(&p, "this is not toml = = =").unwrap();
		match load_with_overrides(Some(&p)) {
			Err(ConfigError::UserConfig { .. }) => {}
			other => panic!("expected UserConfig error, got {other:?}"),
		}
	}

	#[test]
	fn default_section_per_language_is_loadable() {
		let cfg = load_default().unwrap();
		for lang in Lang::ALL {
			let lr = cfg.for_lang(*lang);
			assert!(
				lr.kinds.contains_key("comment"),
				"{} must define comment rules in the default preset",
				lang.tag()
			);
		}
	}
}
