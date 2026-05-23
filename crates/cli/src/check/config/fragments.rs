use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use code_moniker_core::lang::Lang;

use super::{
	Config, ConfigError, FragmentInfo, KindRules, LangRules, RefsRules, RuleEntry, config_section,
};

const FRAGMENT_FILE_NAME: &str = "code-moniker.fragment.toml";

pub(super) struct FragmentFile {
	id: String,
	path: PathBuf,
	enabled: bool,
	config: Config,
	rules: usize,
	rule_keys: Vec<String>,
	local_aliases: Vec<String>,
	alias_keys: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFragmentConfig {
	fragment: String,
	#[serde(default = "default_enabled")]
	enabled: bool,
	#[serde(default)]
	aliases: HashMap<String, String>,
	#[serde(default)]
	refs: RefsRules,
	#[serde(default)]
	shape: HashMap<String, KindRules>,
	#[serde(default)]
	default: LangRules,
	#[serde(default)]
	ts: LangRules,
	#[serde(default)]
	rust: LangRules,
	#[serde(default)]
	java: LangRules,
	#[serde(default)]
	python: LangRules,
	#[serde(default)]
	go: LangRules,
	#[serde(default)]
	cs: LangRules,
	#[serde(default)]
	sql: LangRules,
}

fn default_enabled() -> bool {
	true
}

impl RawFragmentConfig {
	fn into_config(self) -> Config {
		Config {
			default_rules: None,
			aliases: self.aliases,
			refs: self.refs,
			shape: self.shape,
			default: self.default,
			ts: self.ts,
			rust: self.rust,
			java: self.java,
			python: self.python,
			go: self.go,
			cs: self.cs,
			sql: self.sql,
			profiles: HashMap::new(),
			fragments: Vec::new(),
		}
	}
}

pub(super) fn read(user_path: Option<&Path>) -> Result<Vec<FragmentFile>, ConfigError> {
	let Some(root) = user_path.and_then(fragment_root) else {
		return Ok(Vec::new());
	};
	if !root.is_dir() {
		return Ok(Vec::new());
	}
	let paths = discover_fragment_paths(&root);
	let mut fragments = Vec::with_capacity(paths.len());
	let mut seen: HashMap<String, String> = HashMap::new();
	for path in paths {
		let fragment = parse_fragment(&path)?;
		let path_display = path.display().to_string();
		if let Some(first) = seen.insert(fragment.id.clone(), path_display.clone()) {
			return Err(ConfigError::DuplicateFragment {
				id: fragment.id,
				first,
				second: path_display,
			});
		}
		fragments.push(fragment);
	}
	Ok(fragments)
}

pub(super) fn merge_into(
	base: &mut Config,
	fragments: Vec<FragmentFile>,
) -> Result<(), ConfigError> {
	if fragments.is_empty() {
		return Ok(());
	}
	let mut index = RuleIndex::from_config(base, "<effective config>");
	let global_aliases = base.aliases.clone();
	let mut alias_origins: HashMap<String, String> = base
		.aliases
		.keys()
		.map(|alias| (alias.clone(), "<effective config>".to_string()))
		.collect();
	for fragment in fragments {
		base.fragments.push(FragmentInfo {
			id: fragment.id.clone(),
			path: fragment.path.clone(),
			enabled: fragment.enabled,
			declared_rules: fragment.rules,
			active_rules: if fragment.enabled { fragment.rules } else { 0 },
			rule_keys: fragment.rule_keys.clone(),
		});
		validate_alias_boundaries(&fragment, &alias_origins)?;
		if !fragment.enabled {
			continue;
		}
		let mut alias_scope = global_aliases.clone();
		for (alias, body) in &fragment.config.aliases {
			alias_scope.insert(alias.clone(), body.clone());
		}
		let aliases = super::resolve_aliases(&alias_scope)?;
		validate_alias_references(&fragment, &aliases)?;
		index.insert_config(&fragment.config, &fragment.path.display().to_string())?;
		for alias in &fragment.alias_keys {
			alias_origins.insert(alias.clone(), fragment.path.display().to_string());
		}
		super::merge_into(base, fragment.config);
	}
	Ok(())
}

fn fragment_root(user_path: &Path) -> Option<PathBuf> {
	Some(
		user_path
			.parent()
			.filter(|parent| !parent.as_os_str().is_empty())
			.unwrap_or_else(|| Path::new("."))
			.to_path_buf(),
	)
}

fn discover_fragment_paths(root: &Path) -> Vec<PathBuf> {
	let mut paths: Vec<PathBuf> = ignore::WalkBuilder::new(root)
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
		.collect();
	paths.sort();
	paths
}

fn parse_fragment(path: &Path) -> Result<FragmentFile, ConfigError> {
	let raw = std::fs::read_to_string(path).map_err(|error| ConfigError::Io {
		path: path.display().to_string(),
		error,
	})?;
	let raw: RawFragmentConfig =
		toml::from_str(&raw).map_err(|error| ConfigError::FragmentConfig {
			path: path.display().to_string(),
			error,
		})?;
	let fragment = raw.fragment.trim().to_string();
	let enabled = raw.enabled;
	validate_fragment_id(path, &fragment)?;
	let mut config = raw.into_config();
	let local_aliases = namespace_aliases(&mut config, path, &fragment)?;
	let rules = count_rules(&config);
	ensure_no_require_doc(&config, path, &fragment)?;
	super::validate_structure(&config, &path.display().to_string())?;
	namespace_rule_ids(&mut config, path, &fragment)?;
	let rule_keys = collect_rule_keys(&config);
	let alias_keys = collect_alias_keys(&config);
	Ok(FragmentFile {
		id: fragment,
		path: path.to_path_buf(),
		enabled,
		config,
		rules,
		rule_keys,
		local_aliases,
		alias_keys,
	})
}

fn validate_fragment_id(path: &Path, id: &str) -> Result<(), ConfigError> {
	if is_simple_id(id) {
		return Ok(());
	}
	Err(ConfigError::InvalidFragmentId {
		path: path.display().to_string(),
		id: id.to_string(),
	})
}

fn validate_rule_id(path: &Path, fragment: &str, id: &str) -> Result<(), ConfigError> {
	if is_simple_id(id) {
		return Ok(());
	}
	Err(ConfigError::InvalidFragmentRuleId {
		path: path.display().to_string(),
		fragment: fragment.to_string(),
		id: id.to_string(),
	})
}

fn validate_alias_id(path: &Path, fragment: &str, alias: &str) -> Result<(), ConfigError> {
	if is_alias_id(alias) {
		return Ok(());
	}
	Err(ConfigError::InvalidFragmentAliasId {
		path: path.display().to_string(),
		fragment: fragment.to_string(),
		alias: alias.to_string(),
	})
}

fn is_simple_id(id: &str) -> bool {
	!id.is_empty()
		&& id
			.bytes()
			.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn is_alias_id(id: &str) -> bool {
	!id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn namespace_aliases(
	cfg: &mut Config,
	path: &Path,
	fragment: &str,
) -> Result<Vec<String>, ConfigError> {
	if cfg.aliases.is_empty() {
		return Ok(Vec::new());
	}
	let mut local_aliases: Vec<String> = cfg.aliases.keys().cloned().collect();
	local_aliases.sort();
	for alias in &local_aliases {
		validate_alias_id(path, fragment, alias)?;
	}
	let namespace = alias_namespace(fragment);
	let local_set: HashSet<String> = local_aliases.iter().cloned().collect();
	let aliases = std::mem::take(&mut cfg.aliases);
	cfg.aliases = aliases
		.into_iter()
		.map(|(alias, body)| {
			let key = effective_alias_id(&namespace, &alias);
			let body = rewrite_local_alias_refs(&body, &local_set, &namespace);
			(key, body)
		})
		.collect();
	rewrite_rule_alias_refs(cfg, &local_set, &namespace);
	Ok(local_aliases)
}

fn alias_namespace(fragment: &str) -> String {
	fragment
		.bytes()
		.map(|b| {
			if b.is_ascii_alphanumeric() || b == b'_' {
				b as char
			} else {
				'_'
			}
		})
		.collect()
}

fn effective_alias_id(namespace: &str, alias: &str) -> String {
	format!("{namespace}_{alias}")
}

fn rewrite_local_alias_refs(
	input: &str,
	local_aliases: &HashSet<String>,
	namespace: &str,
) -> String {
	let mut out = String::with_capacity(input.len());
	let bytes = input.as_bytes();
	let mut i = 0;
	let mut copied_until = 0;
	while i < bytes.len() {
		if bytes[i] == b'$' {
			let start = i + 1;
			let mut end = start;
			while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
				end += 1;
			}
			if end > start {
				let alias = &input[start..end];
				if local_aliases.contains(alias) {
					out.push_str(&input[copied_until..i]);
					out.push('$');
					out.push_str(&effective_alias_id(namespace, alias));
					copied_until = end;
				}
				i = end;
				continue;
			}
		}
		i += 1;
	}
	out.push_str(&input[copied_until..]);
	out
}

fn rewrite_rule_alias_refs(cfg: &mut Config, local_aliases: &HashSet<String>, namespace: &str) {
	rewrite_rules(&mut cfg.refs.rules, local_aliases, namespace);
	for rules in cfg.shape.values_mut() {
		rewrite_rules(&mut rules.rules, local_aliases, namespace);
	}
	rewrite_lang_rule_alias_refs(&mut cfg.default, local_aliases, namespace);
	for lang in Lang::ALL {
		rewrite_lang_rule_alias_refs(cfg.for_lang_mut(*lang), local_aliases, namespace);
	}
}

fn rewrite_lang_rule_alias_refs(
	rules: &mut LangRules,
	local_aliases: &HashSet<String>,
	namespace: &str,
) {
	for kind_rules in rules.shape.values_mut() {
		rewrite_rules(&mut kind_rules.rules, local_aliases, namespace);
	}
	for kind_rules in rules.kinds.values_mut() {
		rewrite_rules(&mut kind_rules.rules, local_aliases, namespace);
	}
}

fn rewrite_rules(rules: &mut [RuleEntry], local_aliases: &HashSet<String>, namespace: &str) {
	for rule in rules {
		rule.expr = rewrite_local_alias_refs(&rule.expr, local_aliases, namespace);
	}
}

fn ensure_no_require_doc(cfg: &Config, path: &Path, fragment: &str) -> Result<(), ConfigError> {
	for (shape, rules) in &cfg.shape {
		ensure_require_doc_absent(rules, &format!("shape.{shape}"), path, fragment)?;
	}
	ensure_lang_has_no_require_doc("default", &cfg.default, path, fragment)?;
	for lang in Lang::ALL {
		ensure_lang_has_no_require_doc(config_section(*lang), cfg.for_lang(*lang), path, fragment)?;
	}
	Ok(())
}

fn ensure_lang_has_no_require_doc(
	section: &str,
	rules: &LangRules,
	path: &Path,
	fragment: &str,
) -> Result<(), ConfigError> {
	for (shape, kind_rules) in &rules.shape {
		ensure_require_doc_absent(
			kind_rules,
			&format!("{section}.shape.{shape}"),
			path,
			fragment,
		)?;
	}
	for (kind, kind_rules) in &rules.kinds {
		ensure_require_doc_absent(kind_rules, &format!("{section}.{kind}"), path, fragment)?;
	}
	Ok(())
}

fn ensure_require_doc_absent(
	rules: &KindRules,
	at: &str,
	path: &Path,
	fragment: &str,
) -> Result<(), ConfigError> {
	if rules.require_doc_comment.is_some() {
		return Err(ConfigError::FragmentRequireDocUnsupported {
			path: path.display().to_string(),
			fragment: fragment.to_string(),
			at: at.to_string(),
		});
	}
	Ok(())
}

fn namespace_rule_ids(cfg: &mut Config, path: &Path, fragment: &str) -> Result<(), ConfigError> {
	namespace_rules(&mut cfg.refs.rules, "refs", path, fragment)?;
	for (shape, rules) in &mut cfg.shape {
		namespace_rules(&mut rules.rules, &format!("shape.{shape}"), path, fragment)?;
	}
	namespace_lang_rules("default", &mut cfg.default, path, fragment)?;
	for lang in Lang::ALL {
		namespace_lang_rules(
			config_section(*lang),
			cfg.for_lang_mut(*lang),
			path,
			fragment,
		)?;
	}
	Ok(())
}

fn namespace_lang_rules(
	section: &str,
	rules: &mut LangRules,
	path: &Path,
	fragment: &str,
) -> Result<(), ConfigError> {
	for (shape, kind_rules) in &mut rules.shape {
		namespace_rules(
			&mut kind_rules.rules,
			&format!("{section}.shape.{shape}"),
			path,
			fragment,
		)?;
	}
	for (kind, kind_rules) in &mut rules.kinds {
		namespace_rules(
			&mut kind_rules.rules,
			&format!("{section}.{kind}"),
			path,
			fragment,
		)?;
	}
	Ok(())
}

fn namespace_rules(
	rules: &mut [RuleEntry],
	at: &str,
	path: &Path,
	fragment: &str,
) -> Result<(), ConfigError> {
	for rule in rules {
		let Some(local_id) = rule.id.as_deref() else {
			return Err(ConfigError::FragmentRuleMissingId {
				path: path.display().to_string(),
				fragment: fragment.to_string(),
				at: at.to_string(),
			});
		};
		validate_rule_id(path, fragment, local_id)?;
		rule.id = Some(format!("{fragment}.{local_id}"));
	}
	Ok(())
}

fn count_rules(cfg: &Config) -> usize {
	let mut count = cfg.refs.rules.len();
	count += cfg
		.shape
		.values()
		.map(|rules| rules.rules.len())
		.sum::<usize>();
	count += count_lang_rules(&cfg.default);
	for lang in Lang::ALL {
		count += count_lang_rules(cfg.for_lang(*lang));
	}
	count
}

fn count_lang_rules(rules: &LangRules) -> usize {
	rules
		.shape
		.values()
		.map(|kind_rules| kind_rules.rules.len())
		.sum::<usize>()
		+ rules
			.kinds
			.values()
			.map(|kind_rules| kind_rules.rules.len())
			.sum::<usize>()
}

fn collect_alias_keys(cfg: &Config) -> Vec<String> {
	let mut aliases: Vec<String> = cfg.aliases.keys().cloned().collect();
	aliases.sort();
	aliases
}

fn validate_alias_boundaries(
	fragment: &FragmentFile,
	alias_origins: &HashMap<String, String>,
) -> Result<(), ConfigError> {
	let path = fragment.path.display().to_string();
	for alias in &fragment.local_aliases {
		if alias_origins.contains_key(alias) {
			return Err(ConfigError::FragmentAliasShadowsExisting {
				path: path.clone(),
				fragment: fragment.id.clone(),
				alias: alias.clone(),
			});
		}
	}
	for alias in &fragment.alias_keys {
		if let Some(existing) = alias_origins.get(alias) {
			return Err(ConfigError::FragmentAliasCollision {
				alias: alias.clone(),
				path: path.clone(),
				existing: existing.clone(),
			});
		}
	}
	Ok(())
}

fn validate_alias_references(
	fragment: &FragmentFile,
	aliases: &HashMap<String, String>,
) -> Result<(), ConfigError> {
	for_each_rule_key(&fragment.config, |rule_id, rule| {
		let at = format!("{}:{rule_id}", fragment.path.display());
		super::substitute_aliases(&rule.expr, aliases, &at).map(|_| ())
	})
}

struct RuleIndex {
	origins: HashMap<String, String>,
}

impl RuleIndex {
	fn from_config(cfg: &Config, origin: &str) -> Self {
		let mut index = Self {
			origins: HashMap::new(),
		};
		let _ = for_each_rule_key(cfg, |rule_id, _rule| {
			index
				.origins
				.entry(rule_id)
				.or_insert_with(|| origin.to_string());
			Ok(())
		});
		index
	}

	fn insert_config(&mut self, cfg: &Config, origin: &str) -> Result<(), ConfigError> {
		for_each_rule_key(cfg, |rule_id, _rule| {
			if let Some(existing) = self.origins.get(&rule_id) {
				return Err(ConfigError::FragmentRuleCollision {
					rule_id,
					path: origin.to_string(),
					existing: existing.clone(),
				});
			}
			self.origins.insert(rule_id, origin.to_string());
			Ok(())
		})
	}
}

fn for_each_rule_key(
	cfg: &Config,
	mut visit: impl FnMut(String, &RuleEntry) -> Result<(), ConfigError>,
) -> Result<(), ConfigError> {
	for_each_rule_list("refs", &cfg.refs.rules, &mut visit)?;
	for (shape, rules) in &cfg.shape {
		for_each_rule_list(&format!("shape.{shape}"), &rules.rules, &mut visit)?;
	}
	for_each_lang_rule_key("default", &cfg.default, &mut visit)?;
	for lang in Lang::ALL {
		for_each_lang_rule_key(config_section(*lang), cfg.for_lang(*lang), &mut visit)?;
	}
	Ok(())
}

fn collect_rule_keys(cfg: &Config) -> Vec<String> {
	let mut out = Vec::new();
	let _ = for_each_rule_key(cfg, |rule_id, _rule| {
		out.push(rule_id);
		Ok(())
	});
	out.sort();
	out
}

fn for_each_lang_rule_key(
	section: &str,
	rules: &LangRules,
	visit: &mut impl FnMut(String, &RuleEntry) -> Result<(), ConfigError>,
) -> Result<(), ConfigError> {
	for (shape, kind_rules) in &rules.shape {
		for_each_rule_list(
			&format!("{section}.shape.{shape}"),
			&kind_rules.rules,
			visit,
		)?;
	}
	for (kind, kind_rules) in &rules.kinds {
		for_each_rule_list(&format!("{section}.{kind}"), &kind_rules.rules, visit)?;
	}
	Ok(())
}

fn for_each_rule_list(
	prefix: &str,
	rules: &[RuleEntry],
	visit: &mut impl FnMut(String, &RuleEntry) -> Result<(), ConfigError>,
) -> Result<(), ConfigError> {
	for rule in rules {
		if let Some(id) = &rule.id {
			visit(format!("{prefix}.{id}"), rule)?;
		}
	}
	Ok(())
}
