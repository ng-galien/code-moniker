use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

use code_moniker_core::lang::Lang;
use serde::Serialize;

use crate::args::{
	DefaultRules, RulesArgs, RulesCommand, RulesEvalArgs, RulesFileArgs, RulesLearnArgs,
	RulesLearnFormat, RulesShowArgs, RulesShowFormat,
};
use code_moniker_check as check;

use crate::Exit;

pub fn run<W1: Write, W2: Write>(args: &RulesArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	let result = match &args.command {
		RulesCommand::Init(args) => init(args, stdout),
		RulesCommand::Disable(args) => set_default_rules(args, false, stdout),
		RulesCommand::Enable(args) => set_default_rules(args, true, stdout),
		RulesCommand::Show(args) => show(args, stdout),
		RulesCommand::Learn(args) => learn(args, stdout),
		RulesCommand::Eval(args) => eval(args, stdout),
	};
	match result {
		Ok(()) => Exit::Match,
		Err(e) => {
			let _ = writeln!(stderr, "code-moniker: {e:#}");
			Exit::UsageError
		}
	}
}

// Load a rules TOML fragment and apply an optional profile, the shared front of
// the `eval` and `show` pipelines.
fn load_rules(
	rules: Option<&Path>,
	default_rules: Option<DefaultRules>,
	profile: Option<&str>,
) -> anyhow::Result<check::Config> {
	ruleset_request(rules.map(Path::to_path_buf), default_rules, profile).load_config()
}

fn ruleset_request(
	rules: Option<PathBuf>,
	default_rules: Option<DefaultRules>,
	profile: Option<&str>,
) -> check::RuleSetRequest {
	check::RuleSetRequest::new(rules, crate::DEFAULT_SCHEME)
		.with_default_rules(check::DefaultRulesSelection::from_override(
			default_rules.map(DefaultRules::enabled),
		))
		.with_profile(profile.map(str::to_string))
}

// Evaluate a real rules TOML fragment (a .code-moniker.toml) against one
// in-memory sample, the same way `check` evaluates a file. The rule cell of the
// VSCode notebook is exactly this fragment, so what a developer authors here is
// what they paste into their project.
fn eval<W: Write>(args: &RulesEvalArgs, stdout: &mut W) -> anyhow::Result<()> {
	let lang = Lang::from_tag(&args.lang).with_context(|| {
		format!(
			"unknown language tag `{}` (known: {})",
			args.lang,
			Lang::ALL
				.iter()
				.map(|lang| lang.tag())
				.collect::<Vec<_>>()
				.join(", ")
		)
	})?;
	let rules = ruleset_request(
		Some(args.rules.clone()),
		args.default_rules,
		args.profile.as_deref(),
	);
	let source = read_source(args.source.as_deref())?;
	let anchor = args
		.source
		.clone()
		.unwrap_or_else(|| PathBuf::from(format!("sample.{}", lang.tag())));
	let source_report = rules.check_source(&source, &anchor, lang, false)?;
	let report = EvalReport {
		lang: lang.tag().to_string(),
		rules_file: args.rules.display().to_string(),
		total_rules: source_report.rules.len(),
		total_violations: source_report.violations.len(),
		rules: source_report.rules,
		violations: source_report.violations,
	};
	match args.format {
		RulesShowFormat::Text => write_eval_text(stdout, &report)?,
		RulesShowFormat::Json => {
			serde_json::to_writer_pretty(&mut *stdout, &report)?;
			stdout.write_all(b"\n")?;
		}
	}
	Ok(())
}

fn read_source(path: Option<&Path>) -> anyhow::Result<String> {
	match path {
		Some(path) => fs::read_to_string(path)
			.with_context(|| format!("cannot read source `{}`", path.display())),
		None => std::io::read_to_string(std::io::stdin()).context("cannot read source from stdin"),
	}
}

#[derive(Serialize)]
struct EvalReport {
	lang: String,
	rules_file: String,
	total_rules: usize,
	total_violations: usize,
	rules: Vec<check::CompiledRuleSpec>,
	violations: Vec<check::Violation>,
}

fn write_eval_text<W: Write>(w: &mut W, report: &EvalReport) -> std::io::Result<()> {
	writeln!(
		w,
		"{} rule(s), {} violation(s) [{}]",
		report.total_rules, report.total_violations, report.lang
	)?;
	for rule in &report.rules {
		writeln!(w, "- {} ({})", rule.rule_id, rule.domain)?;
		if let Some(rationale) = &rule.rationale {
			writeln!(w, "    rationale: {}", one_line(rationale))?;
		}
	}
	for violation in &report.violations {
		writeln!(
			w,
			"L{}-L{} [{}] {}",
			violation.lines.0,
			violation.lines.1,
			violation.rule_id,
			one_line(&violation.message)
		)?;
		if let Some(explanation) = &violation.explanation {
			writeln!(w, "  -> {}", one_line(explanation))?;
		}
	}
	Ok(())
}

const LEARN_TOPIC_DOCUMENTS: &[&str] = &[
	include_str!("../../../../samples/learn/basics.cm.md"),
	include_str!("../../../../samples/learn/paths.cm.md"),
	include_str!("../../../../samples/learn/refs.cm.md"),
	include_str!("../../../../samples/learn/collections.cm.md"),
	include_str!("../../../../samples/learn/metrics.cm.md"),
	include_str!("../../../../samples/learn/profiles.cm.md"),
];

#[derive(Serialize)]
struct LearnTopic {
	name: String,
	title: String,
	summary: String,
	body: String,
}

#[derive(Serialize)]
struct LearnReport {
	topics: Vec<&'static LearnTopic>,
}

fn learn<W: Write>(args: &RulesLearnArgs, stdout: &mut W) -> anyhow::Result<()> {
	let topics = selected_learn_topics(args.topic.as_deref())?;
	match args.format {
		RulesLearnFormat::Text => write_learn_text(stdout, &topics)?,
		RulesLearnFormat::Json => {
			serde_json::to_writer_pretty(&mut *stdout, &LearnReport { topics })?;
			stdout.write_all(b"\n")?;
		}
	}
	Ok(())
}

fn selected_learn_topics(topic: Option<&str>) -> anyhow::Result<Vec<&'static LearnTopic>> {
	let Some(topic) = topic else {
		return Ok(learn_topics().iter().collect());
	};
	let normalized = topic.to_ascii_lowercase();
	learn_topics()
		.iter()
		.find(|candidate| candidate.name == normalized)
		.map(|topic| vec![topic])
		.with_context(|| {
			format!(
				"unknown DSL topic `{topic}` (known: {})",
				learn_topic_names().join(", ")
			)
		})
}

fn learn_topics() -> &'static [LearnTopic] {
	static TOPICS: std::sync::OnceLock<Vec<LearnTopic>> = std::sync::OnceLock::new();
	TOPICS.get_or_init(|| {
		LEARN_TOPIC_DOCUMENTS
			.iter()
			.map(|document| parse_learn_topic(document).expect("embedded learn topic parses"))
			.collect()
	})
}

fn parse_learn_topic(document: &str) -> anyhow::Result<LearnTopic> {
	let (front_matter, body) = document
		.strip_prefix("---\n")
		.and_then(|rest| rest.split_once("\n---\n"))
		.context("learn topic must start with front matter")?;
	let mut name = String::new();
	let mut title = String::new();
	let mut summary = String::new();
	for line in front_matter.lines() {
		let Some((key, value)) = line.split_once(':') else {
			continue;
		};
		match key.trim() {
			"name" => name = value.trim().to_string(),
			"title" => title = value.trim().to_string(),
			"summary" => summary = value.trim().to_string(),
			_ => {}
		}
	}
	if name.is_empty() || title.is_empty() || summary.is_empty() {
		bail!("learn topic front matter requires name, title, and summary");
	}
	Ok(LearnTopic {
		name,
		title,
		summary,
		body: body.to_string(),
	})
}

fn learn_topic_names() -> Vec<&'static str> {
	learn_topics()
		.iter()
		.map(|topic| topic.name.as_str())
		.collect()
}

fn write_learn_text<W: Write>(w: &mut W, topics: &[&'static LearnTopic]) -> std::io::Result<()> {
	writeln!(w, "# code-moniker check DSL")?;
	writeln!(w, "# Topics: {}", learn_topic_names().join(", "))?;
	for topic in topics {
		writeln!(w)?;
		writeln!(w, "# --- {}: {} ---", topic.name, topic.title)?;
		writeln!(w, "# {}", topic.summary)?;
		let body = learn_text_body(&topic.body);
		write!(w, "{body}")?;
		if !body.ends_with('\n') {
			writeln!(w)?;
		}
	}
	Ok(())
}

fn learn_text_body(body: &str) -> String {
	let mut rendered = String::new();
	let mut skipping = false;
	for line in body.lines() {
		if line.trim_start().starts_with("```") {
			let info = line.trim_start().trim_start_matches('`').trim();
			if skipping {
				skipping = false;
				continue;
			}
			if info
				.split_whitespace()
				.any(|token| token == "cm:expect" || token.starts_with("cm:file="))
			{
				skipping = true;
				continue;
			}
			if info.split_whitespace().any(|token| token == "cm:rules") {
				let language = info
					.split_whitespace()
					.find(|token| !token.starts_with("cm:"))
					.unwrap_or("");
				rendered.push_str("```");
				rendered.push_str(language);
				rendered.push('\n');
				continue;
			}
		}
		rendered.push_str(line);
		rendered.push('\n');
	}
	rendered
}

fn show<W: Write>(args: &RulesShowArgs, stdout: &mut W) -> anyhow::Result<()> {
	let root = args
		.root
		.canonicalize()
		.with_context(|| format!("cannot resolve project root `{}`", args.root.display()))?;
	let path = resolve_from_root(&root, &args.rules);
	let cfg = load_rules(Some(&path), args.default_rules, args.profile.as_deref())?;
	let specs =
		check::compiled_specs_with_config(&cfg, Lang::ALL.iter().copied(), crate::DEFAULT_SCHEME)?;
	let mut rules_by_lang = BTreeMap::<String, Vec<check::CompiledRuleSpec>>::new();
	for spec in specs {
		rules_by_lang
			.entry(spec.lang.clone())
			.or_default()
			.push(spec);
	}
	let langs = Lang::ALL
		.iter()
		.map(|lang| ShowLang {
			lang: lang.tag().to_string(),
			rules: rules_by_lang.remove(lang.tag()).unwrap_or_default(),
		})
		.collect::<Vec<_>>();
	let total_rules = langs.iter().map(|lang| lang.rules.len()).sum();
	let report = ShowReport {
		rules_file: path.display().to_string(),
		default_rules: cfg.default_rules.unwrap_or(true),
		exclude: ShowExclude {
			uris: cfg.exclude.uris.to_vec(),
		},
		fragments: cfg
			.fragments
			.iter()
			.map(|fragment| ShowFragment {
				id: fragment.id.to_owned(),
				path: fragment.path.display().to_string(),
				enabled: fragment.enabled,
				declared_rules: fragment.declared_rules,
				active_rules: fragment.active_rules,
			})
			.collect(),
		profile: args.profile.as_deref().map(str::to_owned),
		total_rules,
		langs,
	};
	match args.format {
		RulesShowFormat::Text => write_show_text(stdout, &report)?,
		RulesShowFormat::Json => {
			serde_json::to_writer_pretty(&mut *stdout, &report)?;
			stdout.write_all(b"\n")?;
		}
	}
	Ok(())
}

#[derive(Serialize)]
struct ShowReport {
	rules_file: String,
	default_rules: bool,
	exclude: ShowExclude,
	fragments: Vec<ShowFragment>,
	profile: Option<String>,
	total_rules: usize,
	langs: Vec<ShowLang>,
}

#[derive(Serialize)]
struct ShowExclude {
	uris: Vec<String>,
}

#[derive(Serialize)]
struct ShowFragment {
	id: String,
	path: String,
	enabled: bool,
	declared_rules: usize,
	active_rules: usize,
}

#[derive(Serialize)]
struct ShowLang {
	lang: String,
	rules: Vec<check::CompiledRuleSpec>,
}

fn write_show_text<W: Write>(w: &mut W, report: &ShowReport) -> std::io::Result<()> {
	writeln!(w, "rules file: {}", report.rules_file)?;
	writeln!(w, "default rules: {}", report.default_rules)?;
	if report.exclude.uris.is_empty() {
		writeln!(w, "exclude.uris: <none>")?;
	} else {
		writeln!(w, "exclude.uris:")?;
		for uri in &report.exclude.uris {
			writeln!(w, "- {uri}")?;
		}
	}
	if report.fragments.is_empty() {
		writeln!(w, "fragments: <none>")?;
	} else {
		writeln!(w, "fragments: {}", report.fragments.len())?;
		for fragment in &report.fragments {
			let state = if fragment.enabled {
				"enabled"
			} else {
				"disabled"
			};
			writeln!(
				w,
				"- {} ({state}): {} active / {} declared rule(s) from {}",
				fragment.id, fragment.active_rules, fragment.declared_rules, fragment.path
			)?;
		}
	}
	writeln!(
		w,
		"profile: {}",
		report.profile.as_deref().unwrap_or("<none>")
	)?;
	writeln!(w, "compiled rules: {}", report.total_rules)?;
	for lang in &report.langs {
		writeln!(w)?;
		writeln!(w, "[{}] {} rule(s)", lang.lang, lang.rules.len())?;
		for rule in &lang.rules {
			writeln!(w, "- {} ({})", rule.rule_id, rule.domain)?;
			if rule.expr == rule.expanded_expr {
				writeln!(w, "  expr: {}", one_line(&rule.expr))?;
			} else {
				writeln!(w, "  expr: {}", one_line(&rule.expr))?;
				writeln!(w, "  expanded: {}", one_line(&rule.expanded_expr))?;
			}
			if let Some(message) = &rule.message {
				writeln!(w, "  message: {}", one_line(message))?;
			}
			if rule.severity.is_warn() {
				writeln!(w, "  severity: warn")?;
			}
			if let Some(rationale) = &rule.rationale {
				writeln!(w, "  rationale: {}", one_line(rationale))?;
			}
		}
	}
	Ok(())
}

fn one_line(value: &str) -> String {
	value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn init<W: Write>(args: &RulesFileArgs, stdout: &mut W) -> anyhow::Result<()> {
	let root = args
		.root
		.canonicalize()
		.with_context(|| format!("cannot resolve project root `{}`", args.root.display()))?;
	let path = resolve_from_root(&root, &args.rules);
	if path.exists() {
		bail!("`{}` already exists", path.display());
	}
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)
			.with_context(|| format!("cannot create `{}`", parent.display()))?;
	}
	let detected = detect_project(&root);
	let content = initial_config(&detected);
	fs::write(&path, content).with_context(|| format!("cannot write `{}`", path.display()))?;
	writeln!(
		stdout,
		"Created {} for {} project rules.",
		path.display(),
		detected.label()
	)?;
	Ok(())
}

fn set_default_rules<W: Write>(
	args: &RulesFileArgs,
	enabled: bool,
	stdout: &mut W,
) -> anyhow::Result<()> {
	let root = args
		.root
		.canonicalize()
		.with_context(|| format!("cannot resolve project root `{}`", args.root.display()))?;
	let path = resolve_from_root(&root, &args.rules);
	let raw = if path.exists() {
		fs::read_to_string(&path).with_context(|| format!("cannot read `{}`", path.display()))?
	} else {
		String::new()
	};
	if !raw.trim().is_empty() {
		parse_toml(&raw, &path)?;
	}
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)
			.with_context(|| format!("cannot create `{}`", parent.display()))?;
	}
	let next = set_top_level_default_rules(&raw, enabled)?;
	parse_toml(&next, &path)?;
	fs::write(&path, next).with_context(|| format!("cannot write `{}`", path.display()))?;
	let state = if enabled { "enabled" } else { "disabled" };
	writeln!(
		stdout,
		"Embedded default rules {state} in {}.",
		path.display()
	)?;
	Ok(())
}

fn resolve_from_root(root: &Path, path: &Path) -> PathBuf {
	if path.is_absolute() {
		path.to_path_buf()
	} else {
		root.join(path)
	}
}

fn parse_toml(raw: &str, path: &Path) -> anyhow::Result<toml::Value> {
	raw.parse::<toml::Value>()
		.with_context(|| format!("`{}` is not valid TOML", path.display()))
}

fn set_top_level_default_rules(raw: &str, enabled: bool) -> anyhow::Result<String> {
	let flag = format!("default_rules = {enabled}");
	if raw.trim().is_empty() {
		return Ok(format!("{flag}\n"));
	}

	let mut lines: Vec<String> = raw.lines().map(str::to_string).collect();
	let first_table = lines
		.iter()
		.position(|line| line.trim_start().starts_with('['))
		.unwrap_or(lines.len());

	for line in &mut lines[..first_table] {
		let trimmed = line.trim_start();
		if let Some(rest) = trimmed.strip_prefix("default_rules")
			&& rest.trim_start().starts_with('=')
		{
			let indent = &line[..line.len() - trimmed.len()];
			*line = format!("{indent}{flag}");
			return Ok(finish_lines(lines, raw.ends_with('\n')));
		}
	}

	lines.insert(first_table, flag);
	Ok(finish_lines(lines, true))
}

fn finish_lines(lines: Vec<String>, trailing_newline: bool) -> String {
	let mut out = lines.join("\n");
	if trailing_newline {
		out.push('\n');
	}
	out
}

#[derive(Default)]
struct DetectedProject {
	java: bool,
	ts: bool,
	rust: bool,
	python: bool,
	go: bool,
	cs: bool,
}

impl DetectedProject {
	fn label(&self) -> &'static str {
		let count = [self.java, self.ts, self.rust, self.python, self.go, self.cs]
			.into_iter()
			.filter(|detected| *detected)
			.count();
		match count {
			0 => "generic",
			1 if self.java => "java",
			1 if self.ts => "typescript",
			1 if self.rust => "rust",
			1 if self.python => "python",
			1 if self.go => "go",
			1 if self.cs => "csharp",
			_ => "multi-language",
		}
	}
}

fn detect_project(root: &Path) -> DetectedProject {
	let mut detected = DetectedProject {
		java: root.join("pom.xml").exists()
			|| root.join("build.gradle").exists()
			|| root.join("build.gradle.kts").exists(),
		ts: root.join("package.json").exists() || root.join("tsconfig.json").exists(),
		rust: root.join("Cargo.toml").exists(),
		python: root.join("pyproject.toml").exists(),
		go: root.join("go.mod").exists(),
		cs: false,
	};
	detected.cs = fs::read_dir(root).is_ok_and(|entries| {
		entries.filter_map(Result::ok).any(|entry| {
			entry
				.path()
				.extension()
				.and_then(|ext| ext.to_str())
				.is_some_and(|ext| ext.eq_ignore_ascii_case("csproj"))
		})
	});
	detected
}

fn initial_config(detected: &DetectedProject) -> String {
	let mut out = String::from(
		"# code-moniker project rules\n\
		 # This file is loaded automatically by `code-moniker check`.\n\n\
		 default_rules = true\n\n\
		 [aliases]\n",
	);
	let mut wrote = false;
	if detected.java {
		wrote = true;
		out.push_str(
			"java_main = \"moniker ~ '**/srcset:main/**'\"\n\
			 java_test = \"moniker ~ '**/srcset:test/**'\"\n",
		);
	}
	if detected.ts {
		wrote = true;
		out.push_str(
			"ts_src = \"moniker ~ '**/dir:src/**'\"\n\
			 ts_test = \"moniker ~ '**/dir:test/**' OR moniker ~ '**/dir:tests/**'\"\n",
		);
	}
	if detected.rust {
		wrote = true;
		out.push_str(
			"rust_src = \"moniker ~ '**/dir:src/**'\"\n\
			 rust_tests = \"moniker ~ '**/dir:tests/**'\"\n",
		);
	}
	if detected.python {
		wrote = true;
		out.push_str(
			"python_package = \"moniker ~ '**/dir:src/**'\"\n\
			 python_tests = \"moniker ~ '**/dir:test/**' OR moniker ~ '**/dir:tests/**'\"\n",
		);
	}
	if detected.go {
		wrote = true;
		out.push_str("go_package = \"moniker ~ '**/lang:go/**'\"\n");
	}
	if detected.cs {
		wrote = true;
		out.push_str(
			"cs_src = \"moniker ~ '**/lang:cs/**'\"\n\
			 cs_tests = \"moniker ~ '**/dir:Tests/**' OR moniker ~ '**/dir:tests/**'\"\n",
		);
	}
	if !wrote {
		out.push_str("src = \"moniker ~ '**/dir:src/**'\"\n");
	}
	out.push('\n');
	out.push_str(
		"# Add project-specific rules here. Example:\n\
		 # [[refs.where]]\n\
		 # id = \"domain-no-infra\"\n\
		 # expr = \"source ~ '**/dir:domain/**' => NOT target ~ '**/dir:infrastructure/**'\"\n",
	);
	out
}

#[cfg(test)]
mod tests {
	use clap::Parser;
	use tempfile::tempdir;

	use crate::args::Cli;
	use crate::{Exit, run};

	#[test]
	fn rules_init_creates_canonical_file_with_detected_aliases() {
		let dir = tempdir().unwrap();
		std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"init",
			dir.path().to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);

		let config = std::fs::read_to_string(dir.path().join(".code-moniker.toml")).unwrap();
		assert!(config.contains("default_rules = true"));
		assert!(config.contains("java_main = \"moniker ~ '**/srcset:main/**'\""));
		assert!(config.contains("java_test = \"moniker ~ '**/srcset:test/**'\""));
		assert!(!config.contains("code-moniker.toml"));
	}

	#[test]
	fn rules_disable_and_enable_toggle_default_rules() {
		let dir = tempdir().unwrap();
		std::fs::write(
			dir.path().join(".code-moniker.toml"),
			"# local rules\n\n[aliases]\nfoo = \"name = Foo\"\n",
		)
		.unwrap();

		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"disable",
			dir.path().to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let config = std::fs::read_to_string(dir.path().join(".code-moniker.toml")).unwrap();
		assert!(config.contains("default_rules = false\n"));
		assert!(config.contains("[aliases]"));

		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"enable",
			dir.path().to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let config = std::fs::read_to_string(dir.path().join(".code-moniker.toml")).unwrap();
		assert!(config.contains("default_rules = true\n"));
	}

	#[test]
	fn rules_show_prints_effective_profiled_rules() {
		let dir = tempdir().unwrap();
		std::fs::write(
			dir.path().join(".code-moniker.toml"),
			r#"
			default_rules = false

			[aliases]
			src = "moniker ~ '**/dir:src/**'"

			[[ts.class.where]]
			id = "keep"
			severity = "warn"
			expr = "$src => name =~ ^[A-Z]"
			message = "keep this rule"
			rationale = "ADR-001: generated types are exempt, but source classes stay PascalCase."

			[[ts.class.where]]
			id = "drop"
			expr = "name =~ ^X"

			[profiles.only-keep]
			enable = ["^ts\\.class\\.keep$"]
			"#,
		)
		.unwrap();
		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"show",
			dir.path().to_str().unwrap(),
			"--profile",
			"only-keep",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out = String::from_utf8(stdout).unwrap();
		assert!(out.contains("default rules: false"), "{out}");
		assert!(out.contains("profile: only-keep"), "{out}");
		assert!(out.contains("ts.class.keep"), "{out}");
		assert!(
			out.contains("expanded: (moniker ~ '**/dir:src/**') => name =~ ^[A-Z]"),
			"{out}"
		);
		assert!(
			out.contains(
				"rationale: ADR-001: generated types are exempt, but source classes stay PascalCase."
			),
			"{out}"
		);
		assert!(out.contains("severity: warn"), "{out}");
		assert!(!out.contains("ts.class.drop"), "{out}");
	}

	#[test]
	fn rules_learn_prints_dsl_topic() {
		let cli = Cli::parse_from(["code-moniker", "rules", "learn", "refs"]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out = String::from_utf8(stdout).unwrap();
		assert!(out.contains("# --- refs: Reference rules ---"), "{out}");
		assert!(out.contains("[[refs.where]]"), "{out}");
		assert!(out.contains("source.*"), "{out}");
		assert!(!out.contains("cm:file="), "{out}");
	}

	#[test]
	fn rules_learn_prints_all_dsl_topics() {
		let cli = Cli::parse_from(["code-moniker", "rules", "learn"]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out = String::from_utf8(stdout).unwrap();
		assert!(
			out.contains("# Topics: basics, paths, refs, collections, metrics, profiles"),
			"{out}"
		);
		assert!(out.contains("# --- basics:"), "{out}");
		assert!(out.contains("# --- profiles:"), "{out}");
		assert!(!out.contains("cm:expect"), "{out}");
	}

	#[test]
	fn rules_learn_embeds_every_learn_topic_document() {
		let learn_dir =
			std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/learn");
		let mut on_disk: Vec<String> = std::fs::read_dir(learn_dir)
			.expect("learn samples directory")
			.filter_map(|entry| {
				let path = entry.expect("learn sample entry").path();
				let name = path.file_name()?.to_str()?;
				name.strip_suffix(".cm.md").map(str::to_string)
			})
			.collect();
		on_disk.sort();
		let mut embedded = super::learn_topic_names();
		embedded.sort();
		assert_eq!(
			embedded,
			on_disk.iter().map(String::as_str).collect::<Vec<_>>(),
			"`rules learn` must expose every samples/learn/*.cm.md document"
		);
		for topic in super::learn_topics() {
			assert!(!topic.title.is_empty(), "{}: missing title", topic.name);
			assert!(!topic.summary.is_empty(), "{}: missing summary", topic.name);
			assert!(!topic.body.is_empty(), "{}: empty body", topic.name);
		}
	}

	#[test]
	fn rules_learn_json_reports_topics() {
		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"learn",
			"paths",
			"--format",
			"json",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		let topics = out["topics"].as_array().unwrap();
		assert_eq!(topics.len(), 1);
		assert_eq!(topics[0]["name"], "paths");
		assert_eq!(topics[0]["title"], "Moniker path patterns and aliases");
		assert!(
			topics[0]["body"].as_str().unwrap().contains("[aliases]"),
			"{out:#}"
		);
	}

	#[test]
	fn rules_learn_rejects_unknown_topic() {
		let cli = Cli::parse_from(["code-moniker", "rules", "learn", "kotlin"]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::UsageError);
		let err = String::from_utf8(stderr).unwrap();
		assert!(err.contains("unknown DSL topic `kotlin`"), "{err}");
		assert!(err.contains("refs"), "{err}");
	}

	#[test]
	fn rules_show_json_reports_compiled_rules() {
		let dir = tempdir().unwrap();
		std::fs::write(
			dir.path().join(".code-moniker.toml"),
			r#"
			default_rules = false

			[[refs.where]]
			id = "domain-no-infra"
			expr = "source ~ '**/dir:domain/**' => NOT target ~ '**/dir:infra/**'"
			rationale = "ADR-002: the domain layer must stay independent from infrastructure details."
			"#,
		)
		.unwrap();
		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"show",
			dir.path().to_str().unwrap(),
			"--format",
			"json",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		assert_eq!(out["default_rules"], false);
		assert!(out["total_rules"].as_u64().unwrap() >= 1);
		assert!(
			out["langs"]
				.as_array()
				.unwrap()
				.iter()
				.any(|lang| lang["lang"] == "ts"
					&& lang["rules"]
						.as_array()
						.unwrap()
						.iter()
						.any(|rule| rule["rule_id"] == "refs.domain-no-infra")),
			"{out:#}"
		);
		let rule = out["langs"]
			.as_array()
			.unwrap()
			.iter()
			.flat_map(|lang| lang["rules"].as_array().unwrap())
			.find(|rule| rule["rule_id"] == "refs.domain-no-infra")
			.expect("domain rule is present");
		assert_eq!(
			rule["rationale"],
			"ADR-002: the domain layer must stay independent from infrastructure details."
		);
		assert_eq!(rule["severity"], "error");
	}

	fn write_eval_inputs(
		rules: &str,
		sample_name: &str,
		sample: &str,
	) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
		let dir = tempdir().unwrap();
		let rules_path = dir.path().join("rules.toml");
		std::fs::write(&rules_path, rules).unwrap();
		let sample_path = dir.path().join(sample_name);
		std::fs::write(&sample_path, sample).unwrap();
		(dir, rules_path, sample_path)
	}

	const SNAKE_RULE: &str = "default_rules = false\n\n\
		[[rust.fn.where]]\n\
		id = \"snake-case\"\n\
		expr = \"name =~ ^[a-z][a-z0-9_]*$\"\n\
		message = \"Function `{name}` should be snake_case.\"\n\
		rationale = \"Rust API guidelines: free functions use snake_case.\"\n";

	#[test]
	fn rules_eval_reports_real_toml_rule_json() {
		let (_dir, rules, sample) =
			write_eval_inputs(SNAKE_RULE, "sample.rs", "fn DoThing() {}\nfn good() {}\n");
		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"eval",
			"--rules",
			rules.to_str().unwrap(),
			"--lang",
			"rs",
			"--format",
			"json",
			sample.to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		assert_eq!(out["lang"], "rs");
		assert_eq!(out["total_rules"], 1);
		assert_eq!(out["rules"][0]["rule_id"], "rust.fn.snake-case");
		assert_eq!(
			out["rules"][0]["rationale"],
			"Rust API guidelines: free functions use snake_case."
		);
		assert_eq!(out["total_violations"], 1);
		let violations = out["violations"].as_array().unwrap();
		assert_eq!(violations.len(), 1);
		assert_eq!(violations[0]["rule_id"], "rust.fn.snake-case");
		assert!(
			violations[0]["explanation"]
				.as_str()
				.unwrap()
				.contains("snake_case"),
			"{out:#}"
		);
	}

	#[test]
	fn rules_eval_clean_source_has_no_violations() {
		let (_dir, rules, sample) =
			write_eval_inputs(SNAKE_RULE, "sample.rs", "fn good_name() {}\n");
		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"eval",
			"--rules",
			rules.to_str().unwrap(),
			"--lang",
			"rs",
			"--format",
			"json",
			sample.to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		assert_eq!(out["total_violations"], 0);
		assert!(out["violations"].as_array().unwrap().is_empty());
	}

	#[test]
	fn rules_eval_text_shows_rationale_and_message() {
		let (_dir, rules, sample) = write_eval_inputs(SNAKE_RULE, "sample.rs", "fn DoThing() {}\n");
		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"eval",
			"--rules",
			rules.to_str().unwrap(),
			"--lang",
			"rs",
			sample.to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out = String::from_utf8(stdout).unwrap();
		assert!(out.contains("1 rule(s), 1 violation(s) [rs]"), "{out}");
		assert!(
			out.contains("rationale: Rust API guidelines: free functions use snake_case."),
			"{out}"
		);
		assert!(
			out.contains("-> Function `DoThing` should be snake_case."),
			"{out}"
		);
	}

	#[test]
	fn rules_eval_supports_aliases_and_multiple_rules() {
		let rules = "default_rules = false\n\n\
			[aliases]\n\
			public_fn = \"visibility = 'public'\"\n\n\
			[[rust.fn.where]]\n\
			id = \"snake\"\n\
			expr = \"name =~ ^[a-z]\"\n\n\
			[[rust.fn.where]]\n\
			id = \"public-documented\"\n\
			expr = \"$public_fn => name !~ ^_\"\n";
		let (_dir, rules, sample) = write_eval_inputs(rules, "sample.rs", "pub fn _Bad() {}\n");
		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"eval",
			"--rules",
			rules.to_str().unwrap(),
			"--lang",
			"rs",
			"--format",
			"json",
			sample.to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		assert_eq!(out["total_rules"], 2);
		// `_Bad` breaks both the snake-case rule and the public-fn rule.
		assert_eq!(out["total_violations"], 2);
	}

	#[test]
	fn rules_eval_rejects_unknown_language() {
		let (_dir, rules, sample) = write_eval_inputs(SNAKE_RULE, "sample.kt", "fun x() {}\n");
		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"eval",
			"--rules",
			rules.to_str().unwrap(),
			"--lang",
			"kotlin",
			sample.to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::UsageError);
		let err = String::from_utf8(stderr).unwrap();
		assert!(err.contains("unknown language tag `kotlin`"), "{err}");
	}

	#[test]
	fn rules_eval_rejects_invalid_rules_toml() {
		let (_dir, rules, sample) = write_eval_inputs(
			"[[rust.fn.where]]\nid = \"bad\"\nexpr = \"name =~~ (\"\n",
			"sample.rs",
			"fn x() {}\n",
		);
		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"eval",
			"--rules",
			rules.to_str().unwrap(),
			"--lang",
			"rs",
			sample.to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::UsageError);
		let err = String::from_utf8(stderr).unwrap();
		assert!(err.contains("code-moniker:"), "{err}");
	}

	#[test]
	fn rules_show_skips_default_kinds_not_emitted_by_lang() {
		let dir = tempdir().unwrap();
		std::fs::write(
			dir.path().join(".code-moniker.toml"),
			r#"
			default_rules = false

			[[default.class.where]]
			id = "class-rule"
			expr = "name =~ ^[A-Z]"

			[[default.function.where]]
			id = "function-rule"
			expr = "name =~ ^[a-z]"
			"#,
		)
		.unwrap();
		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"show",
			dir.path().to_str().unwrap(),
			"--format",
			"json",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		let langs = out["langs"].as_array().unwrap();
		let rust_ids: Vec<_> = langs.iter().find(|lang| lang["lang"] == "rs").unwrap()["rules"]
			.as_array()
			.unwrap()
			.iter()
			.map(|rule| rule["rule_id"].as_str().unwrap().to_string())
			.collect();
		assert!(
			!rust_ids.iter().any(|id| id == "rs.class.class-rule"),
			"Rust cannot emit class defs: {rust_ids:?}"
		);
		assert!(
			!rust_ids.iter().any(|id| id == "rs.function.function-rule"),
			"Rust cannot emit function defs: {rust_ids:?}"
		);
	}
}
