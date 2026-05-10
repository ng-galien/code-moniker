use std::collections::HashMap;

use regex::Regex;

use crate::cli::check::config::{Config, ConfigError, KindRules, config_section};
use crate::cli::lines::line_range;
use crate::cli::render_uri;
use crate::core::code_graph::{CodeGraph, DefRecord};
use crate::core::kinds::KIND_COMMENT;
use crate::core::moniker::query::bare_callable_name;
use crate::core::uri::UriConfig;
use crate::lang::Lang;

#[derive(Debug, Clone)]
pub struct Violation {
	pub rule_id: String,
	pub moniker: String,
	pub kind: String,
	pub lines: (u32, u32),
	pub message: String,
	pub explanation: Option<String>,
}

pub fn evaluate(
	graph: &CodeGraph,
	source: &str,
	lang: Lang,
	cfg: &Config,
	scheme: &str,
) -> Result<Vec<Violation>, ConfigError> {
	let uri_cfg = UriConfig { scheme };
	let compiled = CompiledRules::for_lang(cfg, lang)?;
	let mut out = Vec::new();
	let parent_counts = parent_counts_by_kind(graph);
	let comment_ends = if compiled
		.by_kind
		.values()
		.any(|r| r.require_doc_for_vis.is_some())
	{
		comment_end_bytes(graph)
	} else {
		Vec::new()
	};

	for d in graph.defs() {
		let Ok(kind_str) = std::str::from_utf8(&d.kind) else {
			continue;
		};
		let Some(rules) = compiled.for_kind(kind_str) else {
			continue;
		};

		check_name_pattern(d, kind_str, source, lang, rules, &uri_cfg, &mut out);
		check_forbid_name_patterns(d, kind_str, source, lang, rules, &uri_cfg, &mut out);
		check_max_lines(d, kind_str, source, lang, rules, &uri_cfg, &mut out);
		check_require_doc_comment(
			d,
			kind_str,
			source,
			lang,
			rules,
			&uri_cfg,
			&comment_ends,
			&mut out,
		);
		if d.kind.as_slice() == KIND_COMMENT {
			check_comment_patterns(d, kind_str, source, lang, rules, &uri_cfg, &mut out);
		}
	}

	check_max_count_per_parent(
		graph,
		source,
		lang,
		&compiled,
		&uri_cfg,
		&parent_counts,
		&mut out,
	);

	Ok(out)
}

/// Sorted byte offsets where a comment def ends — used by require_doc_comment
/// to find a comment immediately preceding a target def in O(log n).
fn comment_end_bytes(graph: &CodeGraph) -> Vec<u32> {
	let mut v: Vec<u32> = graph
		.defs()
		.filter(|d| d.kind.as_slice() == KIND_COMMENT)
		.filter_map(|d| d.position.map(|(_, e)| e))
		.collect();
	v.sort_unstable();
	v
}

/// Whether the comment ending at `comment_end` (exclusive) and the def starting
/// at `def_start` are line-adjacent — comment's last line + 1 == def's first
/// line, or both are on the same line. Modifier keywords between AST nodes
/// (`export`, `pub`, `public`, …) sit on the def's first line in source, so
/// strict whitespace-between fails for `/** doc */\nexport class Foo`; the
/// line check matches what the reader sees.
fn comment_attaches_to(source: &str, comment_end: u32, def_start: u32) -> bool {
	if comment_end > def_start {
		return false;
	}
	let last_comment_byte = comment_end.saturating_sub(1);
	let (comment_line, _) = line_range(source, last_comment_byte, last_comment_byte + 1);
	let (def_line, _) = line_range(source, def_start, def_start + 1);
	def_line == comment_line || def_line == comment_line + 1
}

#[derive(Default)]
struct CompiledKindRules {
	name_re: Option<(String, Regex)>,
	forbid_name_res: Vec<(String, Regex)>,
	max_lines: Option<u32>,
	max_count_per_parent: Option<u32>,
	forbid_res: Vec<(String, Regex)>,
	allow_only_res: Vec<Regex>,
	require_doc_for_vis: Option<String>,
	messages: HashMap<String, String>,
}

struct CompiledRules<'cfg> {
	by_kind: HashMap<&'cfg str, CompiledKindRules>,
}

impl<'cfg> CompiledRules<'cfg> {
	fn for_lang(cfg: &'cfg Config, lang: Lang) -> Result<Self, ConfigError> {
		let section = config_section(lang);
		let mut by_kind: HashMap<&str, CompiledKindRules> = HashMap::new();
		for (kind, rules) in cfg.for_lang(lang).kinds.iter() {
			by_kind.insert(kind.as_str(), compile(rules, section, kind)?);
		}
		for (kind, rules) in cfg.default.kinds.iter() {
			if !by_kind.contains_key(kind.as_str()) {
				by_kind.insert(kind.as_str(), compile(rules, "default", kind)?);
			}
		}
		Ok(Self { by_kind })
	}

	fn for_kind(&self, kind: &str) -> Option<&CompiledKindRules> {
		self.by_kind.get(kind)
	}
}

fn compile_one(p: &str, at: String) -> Result<Regex, ConfigError> {
	Regex::new(p).map_err(|error| ConfigError::InvalidRegex {
		at,
		pattern: p.to_string(),
		error,
	})
}

fn compile_pairs(
	patterns: Option<&[String]>,
	section: &str,
	kind: &str,
	field: &str,
) -> Result<Vec<(String, Regex)>, ConfigError> {
	patterns
		.unwrap_or(&[])
		.iter()
		.map(|p| {
			let re = compile_one(p, format!("{section}.{kind}.{field}"))?;
			Ok((p.clone(), re))
		})
		.collect()
}

fn compile(rules: &KindRules, section: &str, kind: &str) -> Result<CompiledKindRules, ConfigError> {
	let name_re = match rules.name_pattern.as_deref() {
		Some(p) => Some((
			p.to_string(),
			compile_one(p, format!("{section}.{kind}.name_pattern"))?,
		)),
		None => None,
	};
	let forbid_name_res = compile_pairs(
		rules.forbid_name_patterns.as_deref(),
		section,
		kind,
		"forbid_name_patterns",
	)?;
	let forbid_res = compile_pairs(
		rules.forbid_patterns.as_deref(),
		section,
		kind,
		"forbid_patterns",
	)?;
	let allow_only_res = rules
		.allow_only_patterns
		.as_deref()
		.unwrap_or(&[])
		.iter()
		.map(|p| compile_one(p, format!("{section}.{kind}.allow_only_patterns")))
		.collect::<Result<Vec<_>, _>>()?;
	Ok(CompiledKindRules {
		name_re,
		forbid_name_res,
		max_lines: rules.max_lines,
		max_count_per_parent: rules.max_count_per_parent,
		forbid_res,
		allow_only_res,
		require_doc_for_vis: rules.require_doc_comment.clone(),
		messages: rules.messages.clone().unwrap_or_default(),
	})
}

/// Substitute placeholders in a user-supplied message template.
fn render_template(tpl: &str, vars: &[(&str, &str)]) -> String {
	let mut out = tpl.to_string();
	for (k, v) in vars {
		let placeholder = format!("{{{k}}}");
		if out.contains(&placeholder) {
			out = out.replace(&placeholder, v);
		}
	}
	out
}

fn rule_id(lang: Lang, kind: &str, rule: &str) -> String {
	format!(
		"{}.{}.{}",
		crate::cli::check::config::config_section(lang),
		kind,
		rule
	)
}

fn lines_of(d: &DefRecord, source: &str) -> (u32, u32) {
	match d.position {
		Some((s, e)) => line_range(source, s, e),
		None => (0, 0),
	}
}

/// Last segment name with the typed-callable signature stripped (e.g. `bar(int)` → `bar`).
fn def_name(d: &DefRecord) -> Option<String> {
	let last = d.moniker.as_view().segments().last()?;
	let bare = bare_callable_name(last.name);
	std::str::from_utf8(bare).ok().map(|s| s.to_string())
}

fn check_name_pattern(
	d: &DefRecord,
	kind: &str,
	source: &str,
	lang: Lang,
	rules: &CompiledKindRules,
	cfg: &UriConfig<'_>,
	out: &mut Vec<Violation>,
) {
	let Some((pat, re)) = &rules.name_re else {
		return;
	};
	let Some(name) = def_name(d) else { return };
	if !re.is_match(&name) {
		let moniker = render_uri(&d.moniker, cfg);
		let explanation = rules.messages.get("name_pattern").map(|tpl| {
			render_template(
				tpl,
				&[
					("name", &name),
					("kind", kind),
					("pattern", pat),
					("moniker", &moniker),
				],
			)
		});
		out.push(Violation {
			rule_id: rule_id(lang, kind, "name_pattern"),
			moniker,
			kind: kind.to_string(),
			lines: lines_of(d, source),
			message: format!("name `{name}` does not match `{pat}`"),
			explanation,
		});
	}
}

fn check_forbid_name_patterns(
	d: &DefRecord,
	kind: &str,
	source: &str,
	lang: Lang,
	rules: &CompiledKindRules,
	cfg: &UriConfig<'_>,
	out: &mut Vec<Violation>,
) {
	if rules.forbid_name_res.is_empty() {
		return;
	}
	let Some(name) = def_name(d) else { return };
	for (pat, re) in &rules.forbid_name_res {
		if re.is_match(&name) {
			let moniker = render_uri(&d.moniker, cfg);
			let explanation = rules.messages.get("forbid_name_patterns").map(|tpl| {
				render_template(
					tpl,
					&[
						("name", &name),
						("kind", kind),
						("pattern", pat),
						("moniker", &moniker),
					],
				)
			});
			out.push(Violation {
				rule_id: rule_id(lang, kind, "forbid_name_patterns"),
				moniker,
				kind: kind.to_string(),
				lines: lines_of(d, source),
				message: format!("name `{name}` matches forbidden pattern `{pat}`"),
				explanation,
			});
			break;
		}
	}
}

#[allow(clippy::too_many_arguments)]
fn check_require_doc_comment(
	d: &DefRecord,
	kind: &str,
	source: &str,
	lang: Lang,
	rules: &CompiledKindRules,
	cfg: &UriConfig<'_>,
	comment_ends: &[u32],
	out: &mut Vec<Violation>,
) {
	let Some(filter) = &rules.require_doc_for_vis else {
		return;
	};
	let vis = std::str::from_utf8(&d.visibility).unwrap_or("");
	if filter != "any" && filter != vis {
		return;
	}
	let Some((start, _)) = d.position else { return };

	// comment_ends is sorted ascending; partition_point(|e| *e <= start) returns
	// the index of the first element strictly greater than start, so idx-1 is
	// the largest end <= start.
	let idx = comment_ends.partition_point(|&end| end <= start);
	let has_doc = idx > 0 && comment_attaches_to(source, comment_ends[idx - 1], start);
	if has_doc {
		return;
	}

	let moniker = render_uri(&d.moniker, cfg);
	let name = def_name(d).unwrap_or_default();
	let explanation = rules.messages.get("require_doc_comment").map(|tpl| {
		render_template(
			tpl,
			&[("name", &name), ("kind", kind), ("moniker", &moniker)],
		)
	});
	out.push(Violation {
		rule_id: rule_id(lang, kind, "require_doc_comment"),
		moniker,
		kind: kind.to_string(),
		lines: lines_of(d, source),
		message: format!("{kind} `{name}` is missing a doc comment immediately before it"),
		explanation,
	});
}

fn check_max_lines(
	d: &DefRecord,
	kind: &str,
	source: &str,
	lang: Lang,
	rules: &CompiledKindRules,
	cfg: &UriConfig<'_>,
	out: &mut Vec<Violation>,
) {
	let Some(limit) = rules.max_lines else { return };
	let Some((s, e)) = d.position else { return };
	let (start_line, end_line) = line_range(source, s, e);
	let lines = end_line - start_line + 1;
	if lines > limit {
		let moniker = render_uri(&d.moniker, cfg);
		let name = def_name(d).unwrap_or_default();
		let lines_s = lines.to_string();
		let limit_s = limit.to_string();
		let explanation = rules.messages.get("max_lines").map(|tpl| {
			render_template(
				tpl,
				&[
					("name", &name),
					("kind", kind),
					("lines", &lines_s),
					("limit", &limit_s),
					("moniker", &moniker),
				],
			)
		});
		out.push(Violation {
			rule_id: rule_id(lang, kind, "max_lines"),
			moniker,
			kind: kind.to_string(),
			lines: (start_line, end_line),
			message: format!("{kind} spans {lines} lines, max is {limit}"),
			explanation,
		});
	}
}

fn check_comment_patterns(
	d: &DefRecord,
	kind: &str,
	source: &str,
	lang: Lang,
	rules: &CompiledKindRules,
	cfg: &UriConfig<'_>,
	out: &mut Vec<Violation>,
) {
	let Some((s, e)) = d.position else { return };
	let text = source.get(s as usize..e as usize).unwrap_or("");

	for (pat, re) in &rules.forbid_res {
		if re.is_match(text) {
			let moniker = render_uri(&d.moniker, cfg);
			let explanation = rules.messages.get("forbid_patterns").map(|tpl| {
				render_template(
					tpl,
					&[("kind", kind), ("pattern", pat), ("moniker", &moniker)],
				)
			});
			out.push(Violation {
				rule_id: rule_id(lang, kind, "forbid_patterns"),
				moniker,
				kind: kind.to_string(),
				lines: lines_of(d, source),
				message: format!("comment matches forbidden pattern `{pat}`"),
				explanation,
			});
			break;
		}
	}

	if !rules.allow_only_res.is_empty() && !rules.allow_only_res.iter().any(|re| re.is_match(text))
	{
		let moniker = render_uri(&d.moniker, cfg);
		let explanation = rules
			.messages
			.get("allow_only_patterns")
			.map(|tpl| render_template(tpl, &[("kind", kind), ("moniker", &moniker)]));
		out.push(Violation {
			rule_id: rule_id(lang, kind, "allow_only_patterns"),
			moniker,
			kind: kind.to_string(),
			lines: lines_of(d, source),
			message: "prose comment forbidden — only directives in the allow-list are permitted"
				.to_string(),
			explanation,
		});
	}
}

fn parent_counts_by_kind(graph: &CodeGraph) -> HashMap<(usize, &[u8]), u32> {
	let mut m: HashMap<(usize, &[u8]), u32> = HashMap::new();
	for d in graph.defs() {
		if let Some(p) = d.parent {
			*m.entry((p, d.kind.as_slice())).or_insert(0) += 1;
		}
	}
	m
}

fn check_max_count_per_parent(
	graph: &CodeGraph,
	source: &str,
	lang: Lang,
	compiled: &CompiledRules<'_>,
	uri_cfg: &UriConfig<'_>,
	counts: &HashMap<(usize, &[u8]), u32>,
	out: &mut Vec<Violation>,
) {
	for ((parent_idx, kind_bytes), count) in counts {
		let Ok(kind_str) = std::str::from_utf8(kind_bytes) else {
			continue;
		};
		let Some(rules) = compiled.for_kind(kind_str) else {
			continue;
		};
		let Some(limit) = rules.max_count_per_parent else {
			continue;
		};
		if *count > limit {
			let parent = graph.def_at(*parent_idx);
			let moniker = render_uri(&parent.moniker, uri_cfg);
			let parent_name = def_name(parent).unwrap_or_default();
			let count_s = count.to_string();
			let limit_s = limit.to_string();
			let explanation = rules.messages.get("max_count_per_parent").map(|tpl| {
				render_template(
					tpl,
					&[
						("name", &parent_name),
						("kind", kind_str),
						("count", &count_s),
						("limit", &limit_s),
						("moniker", &moniker),
					],
				)
			});
			out.push(Violation {
				rule_id: rule_id(lang, kind_str, "max_count_per_parent"),
				moniker,
				kind: std::str::from_utf8(&parent.kind).unwrap_or("").to_string(),
				lines: lines_of(parent, source),
				message: format!("contains {count} {kind_str}s, max is {limit}"),
				explanation,
			});
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::cli::check::config::Config;
	use crate::core::moniker::{Moniker, MonikerBuilder};

	const SCHEME: &str = "ts+moniker://";

	fn cfg_from(s: &str) -> Config {
		toml::from_str(s).expect("test config must parse")
	}

	fn build_module(name: &[u8]) -> Moniker {
		let mut b = MonikerBuilder::new();
		b.project(b".");
		b.segment(b"lang", b"ts");
		b.segment(b"module", name);
		b.build()
	}

	fn child(parent: &Moniker, kind: &[u8], name: &[u8]) -> Moniker {
		let mut b = MonikerBuilder::from_view(parent.as_view());
		b.segment(kind, name);
		b.build()
	}

	#[test]
	fn no_rules_means_no_violations() {
		let cfg: Config = Config::default();
		let module = build_module(b"a");
		let g = CodeGraph::new(module, b"module");
		let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		assert!(v.is_empty());
	}

	#[test]
	fn name_pattern_violation_uses_rule_id() {
		let cfg = cfg_from(
			r#"
			[ts.class]
			name_pattern = "^[A-Z][A-Za-z0-9]*$"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let bad = child(&module, b"class", b"lower_case_bad");
		g.add_def(bad, b"class", &module, Some((0, 10))).unwrap();

		let v =
			evaluate(&g, "lowerCaseClass\n", Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		assert_eq!(v.len(), 1);
		assert_eq!(v[0].rule_id, "ts.class.name_pattern");
		assert!(v[0].message.contains("lower_case_bad"));
	}

	#[test]
	fn name_pattern_passes_when_matching() {
		let cfg = cfg_from(
			r#"
			[ts.class]
			name_pattern = "^[A-Z][A-Za-z0-9]*$"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let good = child(&module, b"class", b"GoodName");
		g.add_def(good, b"class", &module, Some((0, 10))).unwrap();

		let v = evaluate(&g, "anything\n", Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		assert!(v.is_empty(), "expected no violations, got {v:?}");
	}

	#[test]
	fn max_lines_violation_reports_actual_line_count() {
		let cfg = cfg_from(
			r#"
			[ts.function]
			max_lines = 2
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let foo = child(&module, b"function", b"foo");
		g.add_def(foo, b"function", &module, Some((0, 14))).unwrap();

		let source = "line1\nline2\nline3\n";
		let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		assert_eq!(v.len(), 1);
		assert_eq!(v[0].rule_id, "ts.function.max_lines");
		assert!(v[0].message.contains("3 lines"));
		assert!(v[0].message.contains("max is 2"));
	}

	#[test]
	fn comment_allow_only_blocks_prose() {
		let cfg = cfg_from(
			r#"
			[ts.comment]
			allow_only_patterns = ['^\s*//\s*TODO[: ]']
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let prose = child(&module, b"comment", b"5");
		g.add_def(prose, b"comment", &module, Some((0, 14)))
			.unwrap();

		let source = "// some prose\n";
		let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		assert_eq!(v.len(), 1);
		assert_eq!(v[0].rule_id, "ts.comment.allow_only_patterns");
	}

	#[test]
	fn comment_allow_only_passes_for_whitelisted() {
		let cfg = cfg_from(
			r#"
			[ts.comment]
			allow_only_patterns = ['^\s*//\s*TODO[: ]']
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let todo = child(&module, b"comment", b"5");
		let source = "// TODO: fix\n";
		g.add_def(todo, b"comment", &module, Some((0, source.len() as u32)))
			.unwrap();

		let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		assert!(v.is_empty(), "TODO comment should pass: {v:?}");
	}

	#[test]
	fn comment_forbid_pattern_triggers_even_when_whitelisted() {
		let cfg = cfg_from(
			r#"
			[ts.comment]
			allow_only_patterns = ['^\s*//']
			forbid_patterns     = ['eslint-disable']
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let bad = child(&module, b"comment", b"5");
		g.add_def(bad, b"comment", &module, Some((0, 21))).unwrap();

		let source = "// eslint-disable foo\n";
		let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		let ids: Vec<&str> = v.iter().map(|x| x.rule_id.as_str()).collect();
		assert!(ids.contains(&"ts.comment.forbid_patterns"), "{ids:?}");
		assert!(!ids.contains(&"ts.comment.allow_only_patterns"), "{ids:?}");
	}

	#[test]
	fn max_count_per_parent_groups_by_class() {
		let cfg = cfg_from(
			r#"
			[ts.method]
			max_count_per_parent = 2
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let foo = child(&module, b"class", b"Foo");
		g.add_def(foo.clone(), b"class", &module, Some((0, 100)))
			.unwrap();
		g.add_def(child(&foo, b"method", b"a"), b"method", &foo, Some((1, 5)))
			.unwrap();
		g.add_def(child(&foo, b"method", b"b"), b"method", &foo, Some((6, 10)))
			.unwrap();
		g.add_def(
			child(&foo, b"method", b"c"),
			b"method",
			&foo,
			Some((11, 15)),
		)
		.unwrap();

		let bar = child(&module, b"class", b"Bar");
		g.add_def(bar.clone(), b"class", &module, Some((20, 50)))
			.unwrap();
		g.add_def(
			child(&bar, b"method", b"x"),
			b"method",
			&bar,
			Some((21, 25)),
		)
		.unwrap();

		let v = evaluate(&g, "", Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		assert_eq!(v.len(), 1, "Foo over the limit, Bar under: {v:?}");
		assert_eq!(v[0].rule_id, "ts.method.max_count_per_parent");
		assert!(v[0].moniker.contains("class:Foo"));
	}

	#[test]
	fn rust_lang_uses_rust_segment_in_rule_ids() {
		let cfg = cfg_from(
			r#"
			[rust.fn]
			name_pattern = "^[a-z_]+$"
			"#,
		);
		let mut b = MonikerBuilder::new();
		b.project(b".");
		b.segment(b"lang", b"rs");
		b.segment(b"module", b"a");
		let module = b.build();
		let mut g = CodeGraph::new(module.clone(), b"module");
		let bad = child(&module, b"fn", b"BadCase");
		g.add_def(bad, b"fn", &module, Some((0, 10))).unwrap();

		let v = evaluate(&g, "anything\n", Lang::Rs, &cfg, "rs+moniker://")
			.expect("test config compiles");
		assert_eq!(v.len(), 1);
		assert_eq!(v[0].rule_id, "rust.fn.name_pattern");
	}

	#[test]
	fn explanation_renders_template_with_placeholders() {
		let cfg = cfg_from(
			r#"
			[ts.class]
			name_pattern = "^[A-Z][A-Za-z0-9]*$"

			[ts.class.messages]
			name_pattern = "Rename `{name}` to match `{pattern}` ({kind})."
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let bad = child(&module, b"class", b"lower_one");
		g.add_def(bad, b"class", &module, Some((0, 10))).unwrap();

		let v = evaluate(&g, "anything\n", Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		assert_eq!(v.len(), 1);
		let exp = v[0].explanation.as_deref().expect("explanation rendered");
		assert!(exp.contains("`lower_one`"), "{exp}");
		assert!(exp.contains("^[A-Z][A-Za-z0-9]*$"), "{exp}");
		assert!(exp.contains("(class)"), "{exp}");
	}

	#[test]
	fn missing_message_for_rule_leaves_explanation_none() {
		let cfg = cfg_from(
			r#"
			[ts.class]
			name_pattern = "^[A-Z][A-Za-z0-9]*$"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let bad = child(&module, b"class", b"lower_one");
		g.add_def(bad, b"class", &module, Some((0, 10))).unwrap();

		let v = evaluate(&g, "anything\n", Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		assert_eq!(v.len(), 1);
		assert!(v[0].explanation.is_none());
	}

	#[test]
	fn max_lines_message_substitutes_lines_and_limit() {
		let cfg = cfg_from(
			r#"
			[ts.function]
			max_lines = 2

			[ts.function.messages]
			max_lines = "{name}: {lines} > {limit}"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let foo = child(&module, b"function", b"foo");
		g.add_def(foo, b"function", &module, Some((0, 14))).unwrap();

		let v = evaluate(&g, "a\nb\nc\n", Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		assert_eq!(v.len(), 1);
		assert_eq!(v[0].explanation.as_deref(), Some("foo: 3 > 2"));
	}

	#[test]
	fn unknown_message_keys_are_silently_ignored() {
		let cfg = cfg_from(
			r#"
			[ts.class]
			name_pattern = "^[A-Z][A-Za-z0-9]*$"

			[ts.class.messages]
			name_pattern   = "ok"
			fictional_rule = "should be ignored, not crash"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let bad = child(&module, b"class", b"lower_one");
		g.add_def(bad, b"class", &module, Some((0, 10))).unwrap();

		let v = evaluate(&g, "anything\n", Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		assert_eq!(v.len(), 1);
		assert_eq!(v[0].explanation.as_deref(), Some("ok"));
	}

	#[test]
	fn forbid_name_patterns_blocks_dumping_ground_names() {
		let cfg = cfg_from(
			r#"
			[ts.function]
			forbid_name_patterns = ["^helper$", "^utils?$", "^manager$"]
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let bad = child(&module, b"function", b"helper");
		g.add_def(bad, b"function", &module, Some((0, 10))).unwrap();
		let ok = child(&module, b"function", b"renderWidget");
		g.add_def(ok, b"function", &module, Some((11, 20))).unwrap();

		let v = evaluate(&g, "anything\n", Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		assert_eq!(v.len(), 1);
		assert_eq!(v[0].rule_id, "ts.function.forbid_name_patterns");
		assert!(v[0].message.contains("helper"));
		assert!(v[0].message.contains("^helper$"));
	}

	#[test]
	fn forbid_name_patterns_message_template_carries_pattern() {
		let cfg = cfg_from(
			r#"
			[ts.function]
			forbid_name_patterns = ["^helper$"]

			[ts.function.messages]
			forbid_name_patterns = "Rename `{name}` (matched `{pattern}`)."
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let bad = child(&module, b"function", b"helper");
		g.add_def(bad, b"function", &module, Some((0, 10))).unwrap();

		let v = evaluate(&g, "anything\n", Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		assert_eq!(
			v[0].explanation.as_deref(),
			Some("Rename `helper` (matched `^helper$`).")
		);
	}

	#[test]
	fn require_doc_comment_flags_undocumented_public() {
		let cfg = cfg_from(
			r#"
			[ts.class]
			require_doc_comment = "public"
			"#,
		);
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let mut b = MonikerBuilder::from_view(module.as_view());
		b.segment(b"class", b"Foo");
		let foo = b.build();
		let source = "class Foo {}\n";
		let attrs = crate::core::code_graph::DefAttrs {
			visibility: b"public",
			..crate::core::code_graph::DefAttrs::default()
		};
		g.add_def_attrs(
			foo,
			b"class",
			&module,
			Some((0, source.len() as u32)),
			&attrs,
		)
		.unwrap();

		let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		assert_eq!(v.len(), 1);
		assert_eq!(v[0].rule_id, "ts.class.require_doc_comment");
	}

	#[test]
	fn require_doc_comment_passes_when_comment_immediately_precedes() {
		let cfg = cfg_from(
			r#"
			[ts.class]
			require_doc_comment = "public"
			"#,
		);
		let source = "/** doc */\nclass Foo {}\n";
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");

		let mut b = MonikerBuilder::from_view(module.as_view());
		b.segment(b"comment", b"0");
		let cmt = b.build();
		g.add_def(cmt, b"comment", &module, Some((0, 10))).unwrap();

		let mut b = MonikerBuilder::from_view(module.as_view());
		b.segment(b"class", b"Foo");
		let foo = b.build();
		let attrs = crate::core::code_graph::DefAttrs {
			visibility: b"public",
			..crate::core::code_graph::DefAttrs::default()
		};
		g.add_def_attrs(foo, b"class", &module, Some((11, 23)), &attrs)
			.unwrap();

		let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		assert!(
			v.is_empty(),
			"comment immediately before should pass: {v:?}"
		);
	}

	#[test]
	fn require_doc_comment_skips_non_matching_visibility() {
		let cfg = cfg_from(
			r#"
			[ts.class]
			require_doc_comment = "public"
			"#,
		);
		let source = "class Foo {}\n";
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let mut b = MonikerBuilder::from_view(module.as_view());
		b.segment(b"class", b"Foo");
		let foo = b.build();
		let attrs = crate::core::code_graph::DefAttrs {
			visibility: b"private",
			..crate::core::code_graph::DefAttrs::default()
		};
		g.add_def_attrs(foo, b"class", &module, Some((0, 12)), &attrs)
			.unwrap();

		let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		assert!(v.is_empty(), "private def should not be flagged: {v:?}");
	}

	#[test]
	fn require_doc_comment_any_filter_applies_regardless_of_visibility() {
		let cfg = cfg_from(
			r#"
			[ts.class]
			require_doc_comment = "any"
			"#,
		);
		let source = "class Foo {}\n";
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");
		let mut b = MonikerBuilder::from_view(module.as_view());
		b.segment(b"class", b"Foo");
		let foo = b.build();
		g.add_def(foo, b"class", &module, Some((0, 12))).unwrap();

		let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		assert_eq!(v.len(), 1);
	}

	#[test]
	fn require_doc_comment_rejects_when_comment_is_too_far_above_def() {
		let cfg = cfg_from(
			r#"
			[ts.class]
			require_doc_comment = "public"
			"#,
		);
		let source = "/** doc */\n\n\nclass Foo {}\n";
		let module = build_module(b"a");
		let mut g = CodeGraph::new(module.clone(), b"module");

		let mut b = MonikerBuilder::from_view(module.as_view());
		b.segment(b"comment", b"0");
		let cmt = b.build();
		g.add_def(cmt, b"comment", &module, Some((0, 10))).unwrap();

		let mut b = MonikerBuilder::from_view(module.as_view());
		b.segment(b"class", b"Foo");
		let foo = b.build();
		let attrs = crate::core::code_graph::DefAttrs {
			visibility: b"public",
			..crate::core::code_graph::DefAttrs::default()
		};
		g.add_def_attrs(foo, b"class", &module, Some((13, 26)), &attrs)
			.unwrap();

		let v = evaluate(&g, source, Lang::Ts, &cfg, SCHEME).expect("test config compiles");
		assert_eq!(
			v.len(),
			1,
			"comment 3 lines above def must not count as its doc: {v:?}"
		);
	}

	#[test]
	fn invalid_regex_returns_compile_error_with_location() {
		let cfg = cfg_from(
			r#"
			[ts.class]
			name_pattern = "[unclosed"
			"#,
		);
		let module = build_module(b"a");
		let g = CodeGraph::new(module, b"module");
		match evaluate(&g, "", Lang::Ts, &cfg, SCHEME) {
			Err(ConfigError::InvalidRegex { at, pattern, .. }) => {
				assert_eq!(pattern, "[unclosed");
				assert_eq!(at, "ts.class.name_pattern");
			}
			other => panic!("expected InvalidRegex, got {other:?}"),
		}
	}

	#[test]
	fn invalid_regex_in_forbid_list_also_returns_error() {
		let cfg = cfg_from(
			r#"
			[ts.function]
			forbid_name_patterns = ["ok", "[broken"]
			"#,
		);
		let module = build_module(b"a");
		let g = CodeGraph::new(module, b"module");
		match evaluate(&g, "", Lang::Ts, &cfg, SCHEME) {
			Err(ConfigError::InvalidRegex { pattern, .. }) => assert_eq!(pattern, "[broken"),
			other => panic!("expected InvalidRegex, got {other:?}"),
		}
	}
}
