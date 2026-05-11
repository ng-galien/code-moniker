use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

use crate::lang::Lang;

const DEFAULT_PRESET: &str = include_str!("presets/default.toml");

/// Internal kinds emitted by extractors that are not part of `Lang::ALLOWED_KINDS`
/// but ARE legitimate rule targets.
pub(crate) const INTERNAL_KINDS: &[&str] = &["module", "local", "param", "comment"];

#[derive(Debug, Default, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
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
	#[serde(default, rename = "where")]
	pub rules: Vec<RuleEntry>,
	/// Visibility name that triggers the doc-comment requirement, e.g. "public",
	/// "any". `None` disables the rule. Spatial check (annotation-aware), not an
	/// expression — lives outside the `where` DSL.
	pub require_doc_comment: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct RuleEntry {
	/// Stable rule-id used in violation reports and suppression directives.
	/// When absent, the engine derives `where_<index>` from the rule's
	/// position in the per-kind list.
	#[serde(default)]
	pub id: Option<String>,
	pub expr: String,
	#[serde(default)]
	pub message: Option<String>,
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
	#[error("invalid expression at `{at}`: {error}")]
	InvalidExpr {
		at: String,
		error: super::expr::ParseError,
	},
	#[error("unknown kind `{kind}` under `[{section}.{kind}]` (allowed: {allowed})")]
	UnknownKind {
		section: String,
		kind: String,
		allowed: String,
	},
	#[error(
		"require_doc_comment = `{value}` under `[{section}.{kind}]` is not a recognised visibility for that language (allowed: {allowed})"
	)]
	UnknownDocVisibility {
		section: String,
		kind: String,
		value: String,
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

/// `where` rules are concatenated when both sides supply entries: an entry
/// from `ov` whose `id` matches an existing base entry replaces that base
/// entry; otherwise it's appended. `require_doc_comment` overrides if set.
fn merge_kind(base: &mut KindRules, ov: KindRules) {
	for ov_rule in ov.rules {
		match ov_rule
			.id
			.as_deref()
			.and_then(|id| base.rules.iter().position(|r| r.id.as_deref() == Some(id)))
		{
			Some(idx) => base.rules[idx] = ov_rule,
			None => base.rules.push(ov_rule),
		}
	}
	if ov.require_doc_comment.is_some() {
		base.require_doc_comment = ov.require_doc_comment;
	}
}

fn validate(cfg: &Config, path: &str) -> Result<(), ConfigError> {
	validate_lang_section(
		&cfg.default,
		"default",
		&allowed_kinds_set(None),
		None,
		path,
	)?;
	for lang in Lang::ALL {
		let allowed = allowed_kinds_set(Some(*lang));
		validate_lang_section(
			cfg.for_lang(*lang),
			config_section(*lang),
			&allowed,
			Some(*lang),
			path,
		)?;
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

/// Kinds legitimately usable in DSL `count(<kind>)` for `lang` — `lang`'s
/// extractor vocabulary plus internal kinds (`module`, `local`, `param`,
/// `comment`).
pub(crate) fn allowed_kinds_for(lang: Lang) -> Vec<&'static str> {
	allowed_kinds_set(Some(lang))
}

/// `lang`'s visibility vocabulary plus `"any"`. `"any"` is a special token
/// that means "ignore the visibility and require a doc comment everywhere".
fn allowed_doc_vis_for(lang: Lang) -> Vec<&'static str> {
	let mut out: Vec<&'static str> = vec!["any"];
	out.extend(lang.allowed_visibilities().iter().copied());
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
	lang: Option<Lang>,
	_path: &str,
) -> Result<(), ConfigError> {
	for (kind, kr) in lr.kinds.iter() {
		if !allowed.contains(&kind.as_str()) {
			return Err(ConfigError::UnknownKind {
				section: section.to_string(),
				kind: kind.clone(),
				allowed: allowed.join(", "),
			});
		}
		if let (Some(value), Some(l)) = (&kr.require_doc_comment, lang) {
			let allowed_vis = allowed_doc_vis_for(l);
			if !allowed_vis.contains(&value.as_str()) {
				return Err(ConfigError::UnknownDocVisibility {
					section: section.to_string(),
					kind: kind.clone(),
					value: value.clone(),
					allowed: allowed_vis.join(", "),
				});
			}
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
	}

	#[test]
	fn ts_class_ships_at_least_one_rule_in_default() {
		let cfg = load_default().unwrap();
		let r = cfg.rules_for(Lang::Ts, "class").expect("ts.class present");
		assert!(!r.rules.is_empty(), "preset must ship rules for ts.class");
	}

	#[test]
	fn rules_for_falls_back_to_default_section() {
		let cfg = parse(
			r#"
			[[default.module.where]]
			id   = "stub"
			expr = "lines <= 99"

			[[ts.class.where]]
			expr = "name =~ ^X"
			"#,
		)
		.unwrap();
		let r = cfg
			.rules_for(Lang::Ts, "module")
			.expect("falls back to default.module");
		assert_eq!(r.rules.len(), 1);
		assert_eq!(r.rules[0].id.as_deref(), Some("stub"));
	}

	#[test]
	fn override_with_same_id_replaces_preset_rule() {
		let user = parse(
			r#"
			[[ts.function.where]]
			id   = "max-lines"
			expr = "lines <= 999"
			"#,
		)
		.unwrap();
		let mut base = parse(
			r#"
			[[ts.function.where]]
			id   = "name-camel"
			expr = "name =~ ^[a-z]"

			[[ts.function.where]]
			id   = "max-lines"
			expr = "lines <= 60"
			"#,
		)
		.unwrap();
		merge_into(&mut base, user);
		let f = base.rules_for(Lang::Ts, "function").unwrap();
		assert_eq!(f.rules.len(), 2, "id-matched override replaces in place");
		let max_lines = f
			.rules
			.iter()
			.find(|r| r.id.as_deref() == Some("max-lines"))
			.unwrap();
		assert!(max_lines.expr.contains("999"), "user override applied");
		assert!(
			f.rules
				.iter()
				.any(|r| r.id.as_deref() == Some("name-camel")),
			"sibling rule preserved"
		);
	}

	#[test]
	fn override_with_new_id_appends_to_preset() {
		let user = parse(
			r#"
			[[ts.class.where]]
			id   = "extra"
			expr = "name !~ ^Internal"
			"#,
		)
		.unwrap();
		let mut base = parse(
			r#"
			[[ts.class.where]]
			id   = "name-pascal"
			expr = "name =~ ^[A-Z]"
			"#,
		)
		.unwrap();
		merge_into(&mut base, user);
		let r = base.rules_for(Lang::Ts, "class").unwrap();
		assert_eq!(r.rules.len(), 2);
	}

	#[test]
	fn unknown_field_in_kind_rules_is_rejected() {
		let r = toml::from_str::<Config>(
			r#"
			[ts.function]
			max_lines = 10
			"#,
		);
		assert!(r.is_err(), "deny_unknown_fields rejects legacy fields");
	}

	#[test]
	fn unknown_top_level_lang_section_is_rejected() {
		let r = toml::from_str::<Config>(
			r#"
			[[typescript.class.where]]
			expr = "name =~ ^[A-Z]"
			"#,
		);
		assert!(
			r.is_err(),
			"deny_unknown_fields must reject unknown lang sections"
		);
	}

	#[test]
	fn unknown_require_doc_visibility_is_rejected() {
		let r = parse(
			r#"
			[ts.class]
			require_doc_comment = "publc"
			"#,
		);
		match r {
			Err(ConfigError::UnknownDocVisibility { value, .. }) => assert_eq!(value, "publc"),
			other => panic!("expected UnknownDocVisibility, got {other:?}"),
		}
	}

	#[test]
	fn doc_visibility_any_is_accepted() {
		let r = parse(
			r#"
			[ts.class]
			require_doc_comment = "any"
			"#,
		);
		assert!(r.is_ok(), "any is always valid");
	}

	#[test]
	fn unknown_kind_section_is_rejected() {
		let r = parse(
			r#"
			[[ts.classs.where]]
			expr = "name =~ ^X"
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
	fn default_preset_ships_at_least_one_rule_per_language() {
		let cfg = load_default().unwrap();
		for lang in Lang::ALL {
			let lr = cfg.for_lang(*lang);
			assert!(
				!lr.kinds.is_empty(),
				"{} should ship at least one default rule",
				lang.tag()
			);
		}
	}
}
