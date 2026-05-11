use regex::Regex;

use crate::cli::check::eval::Violation;
use crate::core::code_graph::{CodeGraph, DefRecord};
use crate::core::kinds::KIND_COMMENT;

/// Strip violations suppressed by `// code-moniker: ignore` (or `#`/`--`)
/// directives in comment-defs of the graph.
///
/// `ignore` (no `-file` suffix) suppresses violations on the next def whose
/// position starts at or after the comment's end byte. `ignore-file` applies
/// to every violation in the file. The optional `[id1, id2, ...]` list scopes
/// the suppression by rule-id suffix; without it, all rules are suppressed.
pub fn apply(graph: &CodeGraph, source: &str, violations: Vec<Violation>) -> Vec<Violation> {
	let directives = collect_directives(graph, source);
	if directives.is_empty() {
		return violations;
	}

	let file_scope: Vec<&Directive> = directives.iter().filter(|d| d.file_scope).collect();
	let line_scope: Vec<(&Directive, Option<(u32, u32)>)> = directives
		.iter()
		.filter(|d| !d.file_scope)
		.map(|d| (d, target_lines_for(graph, source, d)))
		.collect();

	violations
		.into_iter()
		.filter(|v| {
			!file_scope.iter().any(|d| matches_id(d, &v.rule_id))
				&& !line_scope.iter().any(|(d, target)| {
					matches_id(d, &v.rule_id)
						&& target.is_some_and(|(s, e)| v.lines.0 >= s && v.lines.0 <= e)
				})
		})
		.collect()
}

#[derive(Debug)]
struct Directive {
	comment_end_byte: u32,
	file_scope: bool,
	rule_filters: Vec<String>,
}

fn directive_re() -> &'static Regex {
	use std::sync::OnceLock;
	static RE: OnceLock<Regex> = OnceLock::new();
	RE.get_or_init(|| {
		Regex::new(r"(?://|#|--)\s*code-moniker:\s*ignore(-file)?(?:\[([^\]]+)\])?").unwrap()
	})
}

fn collect_directives(graph: &CodeGraph, source: &str) -> Vec<Directive> {
	let mut out = Vec::new();
	for d in graph.defs() {
		if d.kind.as_slice() != KIND_COMMENT {
			continue;
		}
		let Some((s, e)) = d.position else { continue };
		let Some(text) = source.get(s as usize..e as usize) else {
			continue;
		};
		let Some(caps) = directive_re().captures(text) else {
			continue;
		};
		let file_scope = caps.get(1).is_some();
		let rule_filters = caps
			.get(2)
			.map(|m| {
				m.as_str()
					.split(',')
					.map(|s| s.trim().to_string())
					.filter(|s| !s.is_empty())
					.collect()
			})
			.unwrap_or_default();
		out.push(Directive {
			comment_end_byte: e,
			file_scope,
			rule_filters,
		});
	}
	out
}

fn target_lines_for(graph: &CodeGraph, source: &str, dir: &Directive) -> Option<(u32, u32)> {
	let target = next_def_after(graph, dir.comment_end_byte)?;
	let (s, e) = target.position?;
	Some(crate::cli::lines::line_range(source, s, e))
}

fn next_def_after(graph: &CodeGraph, after_byte: u32) -> Option<&DefRecord> {
	let mut best: Option<&DefRecord> = None;
	for d in graph.defs() {
		if d.kind.as_slice() == KIND_COMMENT {
			continue;
		}
		let Some((s, _)) = d.position else { continue };
		if s < after_byte {
			continue;
		}
		match best {
			None => best = Some(d),
			Some(b) => {
				let bs = b.position.map(|p| p.0).unwrap_or(u32::MAX);
				if s < bs {
					best = Some(d);
				}
			}
		}
	}
	best
}

fn matches_id(dir: &Directive, rule_id: &str) -> bool {
	if dir.rule_filters.is_empty() {
		return true;
	}
	dir.rule_filters
		.iter()
		.any(|f| rule_id == f || rule_id.ends_with(&format!(".{f}")))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::cli::check::config::Config;
	use crate::cli::check::evaluate;
	use crate::cli::extract;
	use crate::lang::Lang;

	fn run(source: &str, cfg: &Config) -> Vec<Violation> {
		let graph = extract::extract(Lang::Ts, source, std::path::Path::new("test.ts"));
		let violations = evaluate(&graph, source, Lang::Ts, cfg, "code+moniker://")
			.expect("test config compiles");
		apply(&graph, source, violations)
	}

	fn cfg(s: &str) -> Config {
		toml::from_str(s).expect("test config must parse")
	}

	#[test]
	fn ignore_without_filter_drops_next_def_violations() {
		let cfg = cfg(r#"
			[[ts.class.where]]
			id   = "name-pascal"
			expr = "name =~ ^[A-Z][A-Za-z0-9]*$"
			"#);
		let source = "// code-moniker: ignore\nclass lower_bad {}\n";
		assert!(run(source, &cfg).is_empty());
	}

	#[test]
	fn ignore_with_specific_id_only_drops_matching_violations() {
		let cfg = cfg(r#"
			[[ts.class.where]]
			id   = "name-pascal"
			expr = "name =~ ^[A-Z][A-Za-z0-9]*$"

			[[ts.class.where]]
			id   = "max-lines"
			expr = "lines <= 1"
			"#);
		let source = "// code-moniker: ignore[name-pascal]\nclass lower_bad {\n}\n";
		let v = run(source, &cfg);
		let ids: Vec<&str> = v.iter().map(|x| x.rule_id.as_str()).collect();
		assert!(!ids.contains(&"ts.class.name-pascal"), "{ids:?}");
		assert!(
			ids.contains(&"ts.class.max-lines"),
			"max-lines should remain: {ids:?}"
		);
	}

	#[test]
	fn ignore_with_other_id_does_not_drop_violation() {
		let cfg = cfg(r#"
			[[ts.class.where]]
			id   = "name-pascal"
			expr = "name =~ ^[A-Z][A-Za-z0-9]*$"
			"#);
		let source = "// code-moniker: ignore[max-lines]\nclass lower_bad {}\n";
		let v = run(source, &cfg);
		assert_eq!(v.len(), 1);
		assert_eq!(v[0].rule_id, "ts.class.name-pascal");
	}

	#[test]
	fn ignore_file_drops_violations_anywhere() {
		let cfg = cfg(r#"
			[[ts.class.where]]
			id   = "name-pascal"
			expr = "name =~ ^[A-Z][A-Za-z0-9]*$"
			"#);
		let source = "// code-moniker: ignore-file\nclass lower_one {}\nclass another_lower {}\n";
		assert!(run(source, &cfg).is_empty());
	}

	#[test]
	fn ignore_file_with_filter_only_drops_listed_rules() {
		let cfg = cfg(r#"
			[[ts.class.where]]
			id   = "name-pascal"
			expr = "name =~ ^[A-Z][A-Za-z0-9]*$"

			[[ts.class.where]]
			id   = "max-lines"
			expr = "lines <= 1"
			"#);
		let source = "// code-moniker: ignore-file[name-pascal]\nclass lower_one {\n}\n";
		let v = run(source, &cfg);
		let ids: Vec<&str> = v.iter().map(|x| x.rule_id.as_str()).collect();
		assert!(!ids.contains(&"ts.class.name-pascal"), "{ids:?}");
		assert!(ids.contains(&"ts.class.max-lines"), "{ids:?}");
	}

	#[test]
	fn ignore_only_applies_to_immediate_next_def() {
		let cfg = cfg(r#"
			[[ts.class.where]]
			id   = "name-pascal"
			expr = "name =~ ^[A-Z][A-Za-z0-9]*$"
			"#);
		let source = "// code-moniker: ignore\nclass lower_one {}\nclass lower_two {}\n";
		let v = run(source, &cfg);
		let ids: Vec<&str> = v.iter().map(|x| x.rule_id.as_str()).collect();
		assert_eq!(v.len(), 1, "second class still flagged: {ids:?}");
	}

	#[test]
	fn ignore_directives_dont_self_flag_as_prose() {
		let cfg = cfg(r#"
			[[ts.comment.where]]
			id   = "allow-only"
			expr = '''text =~ ^\s*//\s*code-moniker:'''
			"#);
		let source = "// code-moniker: ignore\nclass Whatever {}\n";
		assert!(run(source, &cfg).is_empty());
	}
}
