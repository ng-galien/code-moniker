use std::collections::HashMap;
use std::path::Path;

use regex::Regex;
use serde::Deserialize;
use thiserror::Error;

use code_moniker_core::lang::Lang;

const DEFAULT_PRESET: &str = include_str!("presets/default.toml");

pub(crate) use code_moniker_core::lang::kinds::INTERNAL_KINDS;

/// Reserved keys under `[<lang>.…]` that aren't def kinds. `refs` is treated
/// as the per-lang ref rule list (parallel to top-level `[[refs.where]]`).
const RESERVED_LANG_KEYS: &[&str] = &["refs"];

#[derive(Debug, Default, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Config {
	#[serde(default)]
	pub aliases: HashMap<String, String>,
	#[serde(default)]
	pub refs: RefsRules,
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
	#[serde(default)]
	pub profiles: HashMap<String, Profile>,
}

#[derive(Debug, Default, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Profile {
	#[serde(default)]
	pub enable: Vec<String>,
	#[serde(default)]
	pub disable: Vec<String>,
}

#[derive(Debug, Default, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct RefsRules {
	#[serde(default, rename = "where")]
	pub rules: Vec<RuleEntry>,
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
	pub require_doc_comment: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct RuleEntry {
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
	#[error("alias cycle through `{chain}`")]
	AliasCycle { chain: String },
	#[error("unknown alias `${name}` referenced under `{at}`")]
	UnknownAlias { name: String, at: String },
	#[error("unknown profile `{name}` (known: {known})")]
	UnknownProfile { name: String, known: String },
	#[error("invalid regex `{pattern}` in profile `{profile}` ({field}): {error}")]
	BadProfileRegex {
		profile: String,
		field: &'static str,
		pattern: String,
		error: regex::Error,
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
	for (k, v) in ov.aliases {
		base.aliases.insert(k, v);
	}
	for (k, v) in ov.profiles {
		base.profiles.insert(k, v);
	}
	merge_refs(&mut base.refs, ov.refs);
	merge_lang(&mut base.default, ov.default);
	merge_lang(&mut base.ts, ov.ts);
	merge_lang(&mut base.rust, ov.rust);
	merge_lang(&mut base.java, ov.java);
	merge_lang(&mut base.python, ov.python);
	merge_lang(&mut base.go, ov.go);
	merge_lang(&mut base.cs, ov.cs);
	merge_lang(&mut base.sql, ov.sql);
}

fn merge_refs(base: &mut RefsRules, ov: RefsRules) {
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

/// Resolve every alias to its fully-expanded form. Reports a cycle when one
/// is detected and an unknown-alias error if a referenced `$name` doesn't
/// exist among the aliases (referenced names inside rule `expr` are
/// validated lazily at compile time, not here).
pub(crate) fn resolve_aliases(
	aliases: &HashMap<String, String>,
) -> Result<HashMap<String, String>, ConfigError> {
	let mut resolved: HashMap<String, String> = HashMap::new();
	for name in aliases.keys() {
		let mut stack: Vec<String> = Vec::new();
		resolve_one(name, aliases, &mut resolved, &mut stack)?;
	}
	Ok(resolved)
}

fn resolve_one(
	name: &str,
	src: &HashMap<String, String>,
	resolved: &mut HashMap<String, String>,
	stack: &mut Vec<String>,
) -> Result<String, ConfigError> {
	if let Some(v) = resolved.get(name) {
		return Ok(v.clone());
	}
	if stack.iter().any(|s| s == name) {
		stack.push(name.to_string());
		return Err(ConfigError::AliasCycle {
			chain: stack.join(" → "),
		});
	}
	let Some(body) = src.get(name) else {
		return Err(ConfigError::UnknownAlias {
			name: name.to_string(),
			at: format!("alias `{}`", stack.last().unwrap_or(&"<root>".to_string())),
		});
	};
	stack.push(name.to_string());
	let expanded = expand_refs(body, src, resolved, stack)?;
	stack.pop();
	resolved.insert(name.to_string(), expanded.clone());
	Ok(expanded)
}

fn expand_refs(
	body: &str,
	src: &HashMap<String, String>,
	resolved: &mut HashMap<String, String>,
	stack: &mut Vec<String>,
) -> Result<String, ConfigError> {
	let mut out = String::with_capacity(body.len());
	let bytes = body.as_bytes();
	let mut i = 0;
	while i < bytes.len() {
		if bytes[i] == b'$' {
			let start = i + 1;
			let mut j = start;
			while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
				j += 1;
			}
			if j > start {
				let name = &body[start..j];
				let expanded = resolve_one(name, src, resolved, stack)?;
				out.push('(');
				out.push_str(&expanded);
				out.push(')');
				i = j;
				continue;
			}
		}
		out.push(bytes[i] as char);
		i += 1;
	}
	Ok(out)
}

/// Substitute `$name` references in `expr` against an already-resolved alias
/// map. Unknown alias → error tagged with the rule location.
pub(crate) fn substitute_aliases(
	expr: &str,
	resolved: &HashMap<String, String>,
	at: &str,
) -> Result<String, ConfigError> {
	let mut out = String::with_capacity(expr.len());
	let bytes = expr.as_bytes();
	let mut i = 0;
	while i < bytes.len() {
		if bytes[i] == b'$' {
			let start = i + 1;
			let mut j = start;
			while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
				j += 1;
			}
			if j > start {
				let name = &expr[start..j];
				let Some(expanded) = resolved.get(name) else {
					return Err(ConfigError::UnknownAlias {
						name: name.to_string(),
						at: at.to_string(),
					});
				};
				out.push('(');
				out.push_str(expanded);
				out.push(')');
				i = j;
				continue;
			}
		}
		out.push(bytes[i] as char);
		i += 1;
	}
	Ok(out)
}

/// Aliases are resolved first so cycles surface before any kind / visibility check.
fn validate(cfg: &Config, path: &str) -> Result<(), ConfigError> {
	resolve_aliases(&cfg.aliases)?;
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
		if RESERVED_LANG_KEYS.contains(&kind.as_str()) {
			continue;
		}
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
			Lang::Sql => &self.sql,
		}
	}

	pub fn for_lang_mut(&mut self, lang: Lang) -> &mut LangRules {
		match lang {
			Lang::Ts => &mut self.ts,
			Lang::Rs => &mut self.rust,
			Lang::Java => &mut self.java,
			Lang::Python => &mut self.python,
			Lang::Go => &mut self.go,
			Lang::Cs => &mut self.cs,
			Lang::Sql => &mut self.sql,
		}
	}

	pub fn rules_for(&self, lang: Lang, kind: &str) -> Option<&KindRules> {
		self.for_lang(lang)
			.kinds
			.get(kind)
			.or_else(|| self.default.kinds.get(kind))
	}

	pub fn apply_profile(&mut self, name: &str) -> Result<(), ConfigError> {
		let profile = self
			.profiles
			.get(name)
			.ok_or_else(|| ConfigError::UnknownProfile {
				name: name.to_string(),
				known: self.known_profiles(),
			})?
			.clone();
		let enable = compile_patterns(&profile.enable, name, "enable")?;
		let disable = compile_patterns(&profile.disable, name, "disable")?;
		filter_rules(&mut self.refs.rules, "refs", &enable, &disable);
		filter_lang(&mut self.default, "default", &enable, &disable);
		for lang in Lang::ALL {
			filter_lang(
				self.for_lang_mut(*lang),
				config_section(*lang),
				&enable,
				&disable,
			);
		}
		Ok(())
	}

	fn known_profiles(&self) -> String {
		let mut names: Vec<&str> = self.profiles.keys().map(|s| s.as_str()).collect();
		names.sort();
		names.join(", ")
	}
}

impl RuleEntry {
	pub(crate) fn fallback_id(&self, idx: usize) -> String {
		self.id.clone().unwrap_or_else(|| format!("where_{idx}"))
	}
}

fn compile_patterns(
	patterns: &[String],
	profile: &str,
	field: &'static str,
) -> Result<Vec<Regex>, ConfigError> {
	patterns
		.iter()
		.map(|p| {
			Regex::new(p).map_err(|error| ConfigError::BadProfileRegex {
				profile: profile.to_string(),
				field,
				pattern: p.clone(),
				error,
			})
		})
		.collect()
}

fn filter_lang(lr: &mut LangRules, section: &str, enable: &[Regex], disable: &[Regex]) {
	for (kind, kr) in lr.kinds.iter_mut() {
		let prefix = format!("{section}.{kind}");
		filter_rules(&mut kr.rules, &prefix, enable, disable);
	}
}

fn filter_rules(rules: &mut Vec<RuleEntry>, prefix: &str, enable: &[Regex], disable: &[Regex]) {
	if rules.is_empty() || (enable.is_empty() && disable.is_empty()) {
		return;
	}
	let mut idx = 0;
	rules.retain(|r| {
		let full = format!("{prefix}.{}", r.fallback_id(idx));
		idx += 1;
		(enable.is_empty() || enable.iter().any(|re| re.is_match(&full)))
			&& !disable.iter().any(|re| re.is_match(&full))
	});
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
	fn alias_section_parses() {
		let cfg = parse(
			r#"
			[aliases]
			domain = "moniker ~ '**/module:domain/**'"
			"#,
		)
		.unwrap();
		assert_eq!(
			cfg.aliases.get("domain").map(|s| s.as_str()),
			Some("moniker ~ '**/module:domain/**'"),
		);
	}

	#[test]
	fn alias_cycle_is_rejected() {
		let r = parse(
			r#"
			[aliases]
			a = "$b"
			b = "$a"
			"#,
		);
		match r {
			Err(ConfigError::AliasCycle { chain }) => {
				assert!(chain.contains("a") && chain.contains("b"), "{chain}");
			}
			other => panic!("expected AliasCycle, got {other:?}"),
		}
	}

	#[test]
	fn alias_chain_resolves() {
		let cfg = parse(
			r#"
			[aliases]
			a = "name = 'X'"
			b = "$a OR name = 'Y'"
			c = "$b AND lines <= 10"
			"#,
		)
		.unwrap();
		let resolved = resolve_aliases(&cfg.aliases).unwrap();
		let final_c = resolved.get("c").unwrap();
		assert!(final_c.contains("name = 'X'"), "{final_c}");
		assert!(final_c.contains("name = 'Y'"), "{final_c}");
		assert!(final_c.contains("lines <= 10"), "{final_c}");
	}

	#[test]
	fn alias_substitution_wraps_in_parens() {
		// `$x OR Y` → `(<x-body>) OR Y` so precedence is preserved.
		let mut src = HashMap::new();
		src.insert("x".to_string(), "A AND B".to_string());
		let resolved = resolve_aliases(&src).unwrap();
		let out = substitute_aliases("$x OR C", &resolved, "test").unwrap();
		assert_eq!(out, "(A AND B) OR C");
	}

	#[test]
	fn unknown_alias_is_rejected_at_substitution() {
		let resolved = HashMap::new();
		match substitute_aliases("$bogus AND name = 'X'", &resolved, "ts.class.r1") {
			Err(ConfigError::UnknownAlias { name, at }) => {
				assert_eq!(name, "bogus");
				assert_eq!(at, "ts.class.r1");
			}
			other => panic!("expected UnknownAlias, got {other:?}"),
		}
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
	fn profile_enable_filters_in() {
		let mut cfg = parse(
			r#"
			[[ts.class.where]]
			id   = "keep"
			expr = "lines <= 99"

			[[ts.class.where]]
			id   = "drop"
			expr = "lines <= 99"

			[profiles.only_keep]
			enable = ["\\.keep$"]
			"#,
		)
		.unwrap();
		cfg.apply_profile("only_keep").unwrap();
		let r = cfg.rules_for(Lang::Ts, "class").unwrap();
		assert_eq!(r.rules.len(), 1);
		assert_eq!(r.rules[0].id.as_deref(), Some("keep"));
	}

	#[test]
	fn profile_disable_filters_out() {
		let mut cfg = parse(
			r#"
			[[ts.class.where]]
			id   = "keep"
			expr = "lines <= 99"

			[[ts.class.where]]
			id   = "drop"
			expr = "lines <= 99"

			[profiles.drop_one]
			disable = ["\\.drop$"]
			"#,
		)
		.unwrap();
		cfg.apply_profile("drop_one").unwrap();
		let r = cfg.rules_for(Lang::Ts, "class").unwrap();
		assert_eq!(r.rules.len(), 1);
		assert_eq!(r.rules[0].id.as_deref(), Some("keep"));
	}

	#[test]
	fn profile_enable_then_disable() {
		let mut cfg = parse(
			r#"
			[[ts.class.where]]
			id   = "a"
			expr = "lines <= 99"

			[[ts.class.where]]
			id   = "b"
			expr = "lines <= 99"

			[[ts.class.where]]
			id   = "c"
			expr = "lines <= 99"

			[profiles.p]
			enable  = ["ts\\.class\\.(a|b)$"]
			disable = ["ts\\.class\\.b$"]
			"#,
		)
		.unwrap();
		cfg.apply_profile("p").unwrap();
		let r = cfg.rules_for(Lang::Ts, "class").unwrap();
		assert_eq!(r.rules.len(), 1);
		assert_eq!(r.rules[0].id.as_deref(), Some("a"));
	}

	#[test]
	fn profile_filters_refs_top_level() {
		let mut cfg = parse(
			r#"
			[[refs.where]]
			id   = "stay"
			expr = "kind = 'call'"

			[[refs.where]]
			id   = "go"
			expr = "kind = 'call'"

			[profiles.p]
			disable = ["^refs\\.go$"]
			"#,
		)
		.unwrap();
		cfg.apply_profile("p").unwrap();
		assert_eq!(cfg.refs.rules.len(), 1);
		assert_eq!(cfg.refs.rules[0].id.as_deref(), Some("stay"));
	}

	#[test]
	fn profile_filters_per_lang_refs() {
		let mut cfg = parse(
			r#"
			[[ts.refs.where]]
			id   = "stay"
			expr = "kind = 'call'"

			[[ts.refs.where]]
			id   = "go"
			expr = "kind = 'call'"

			[profiles.p]
			disable = ["^ts\\.refs\\.go$"]
			"#,
		)
		.unwrap();
		cfg.apply_profile("p").unwrap();
		let r = cfg.ts.kinds.get("refs").unwrap();
		assert_eq!(r.rules.len(), 1);
		assert_eq!(r.rules[0].id.as_deref(), Some("stay"));
	}

	#[test]
	fn profile_filters_default_section() {
		let mut cfg = parse(
			r#"
			[[default.module.where]]
			id   = "stay"
			expr = "lines <= 99"

			[[default.module.where]]
			id   = "go"
			expr = "lines <= 99"

			[profiles.p]
			disable = ["^default\\.module\\.go$"]
			"#,
		)
		.unwrap();
		cfg.apply_profile("p").unwrap();
		let r = cfg.default.kinds.get("module").unwrap();
		assert_eq!(r.rules.len(), 1);
		assert_eq!(r.rules[0].id.as_deref(), Some("stay"));
	}

	#[test]
	fn unknown_profile_returns_error() {
		let mut cfg = parse(
			r#"
			[profiles.known]
			disable = []
			"#,
		)
		.unwrap();
		match cfg.apply_profile("nope") {
			Err(ConfigError::UnknownProfile { name, known }) => {
				assert_eq!(name, "nope");
				assert!(known.contains("known"), "{known}");
			}
			other => panic!("expected UnknownProfile, got {other:?}"),
		}
	}

	#[test]
	fn bad_regex_returns_error() {
		let mut cfg = parse(
			r#"
			[profiles.p]
			enable = ["(unclosed"]
			"#,
		)
		.unwrap();
		match cfg.apply_profile("p") {
			Err(ConfigError::BadProfileRegex {
				profile,
				field,
				pattern,
				..
			}) => {
				assert_eq!(profile, "p");
				assert_eq!(field, "enable");
				assert_eq!(pattern, "(unclosed");
			}
			other => panic!("expected BadProfileRegex, got {other:?}"),
		}
	}

	#[test]
	fn fallback_where_n_id_matches() {
		let mut cfg = parse(
			r#"
			[[ts.class.where]]
			expr = "lines <= 99"

			[[ts.class.where]]
			expr = "lines <= 99"

			[profiles.p]
			disable = ["^ts\\.class\\.where_0$"]
			"#,
		)
		.unwrap();
		cfg.apply_profile("p").unwrap();
		let r = cfg.rules_for(Lang::Ts, "class").unwrap();
		assert_eq!(r.rules.len(), 1);
	}

	#[test]
	fn user_profile_overrides_preset_by_name() {
		let user = parse(
			r#"
			[profiles.bugfix]
			enable  = ["^user$"]
			disable = []
			"#,
		)
		.unwrap();
		let mut base = parse(
			r#"
			[profiles.bugfix]
			enable  = ["^base$"]
			disable = []
			"#,
		)
		.unwrap();
		merge_into(&mut base, user);
		let p = base.profiles.get("bugfix").unwrap();
		assert_eq!(p.enable, vec!["^user$".to_string()]);
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
