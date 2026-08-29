use std::collections::{BTreeMap, BTreeSet};
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
use crate::presentation::rules as rules_presentation;
use check::{
	RuleAliasUsage, RuleClassification, RuleClassificationStatus, RuleCorpusAnalysis,
	RuleCorpusDiagnosticCode, RuleCorpusDiagnosticLevel, RuleCorpusEntry, RuleOrigin, RuleTaxonomy,
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
	include_str!("../../assets/learn/basics.cm.md"),
	include_str!("../../assets/learn/taxonomy.cm.md"),
	include_str!("../../assets/learn/paths.cm.md"),
	include_str!("../../assets/learn/fragments.cm.md"),
	include_str!("../../assets/learn/refs.cm.md"),
	include_str!("../../assets/learn/collections.cm.md"),
	include_str!("../../assets/learn/domains.cm.md"),
	include_str!("../../assets/learn/metrics.cm.md"),
	include_str!("../../assets/learn/aggregates.cm.md"),
	include_str!("../../assets/learn/relations.cm.md"),
	include_str!("../../assets/learn/directives.cm.md"),
	include_str!("../../assets/learn/profiles.cm.md"),
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
			.map(|document| {
				parse_learn_topic(document)
					.unwrap_or_else(|err| panic!("embedded learn topic must parse: {err}"))
			})
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

pub(crate) fn learn_topic_names() -> Vec<&'static str> {
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
		if skipping {
			continue;
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
	let rules_file = path.display().to_string();
	let request = ruleset_request(Some(path), args.default_rules, args.profile.as_deref())
		.with_inline_rules(args.rules_inline.clone())
		.with_project_root(root.as_path());
	let cfg = request.load_config()?;
	validate_show_filters(args, cfg.rules.taxonomy.as_ref())?;
	let corpus = check::compiled_rule_corpus(&request, Lang::ALL.iter().copied())?;
	let full_summary = taxonomy_summary(cfg.rules.taxonomy.as_ref(), &corpus);
	let corpus = filter_corpus(corpus, args);
	let compiled_rows = corpus.len();
	let mut summary = taxonomy_summary(cfg.rules.taxonomy.as_ref(), &corpus);
	summary.unused_patterns = full_summary.unused_patterns;
	summary.unused_components = full_summary.unused_components;
	let details_requested = args.details || args.rule_id.is_some();
	let mut effective_rules = aggregate_effective_rules(corpus, args.by_language);
	let detail_page = details_requested.then(|| {
		let total = effective_rules.len();
		let offset = args.offset.unwrap_or(0).min(total);
		let limit = args.limit.unwrap_or(50);
		let rules = effective_rules
			.drain(offset..effective_rules.len().min(offset.saturating_add(limit)))
			.collect::<Vec<_>>();
		ShowDetailPage {
			offset,
			limit,
			returned: rules.len(),
			total,
			has_more: offset.saturating_add(rules.len()) < total,
			rules,
		}
	});
	let report = ShowReport {
		rules_file,
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
		compiled_rows,
		distinct_rules: summary.distinct_rules,
		taxonomy: cfg.rules.taxonomy.clone(),
		taxonomy_summary: summary,
		details: detail_page,
	};
	match args.format {
		RulesShowFormat::Text => write_show_text(stdout, &report, args)?,
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
	compiled_rows: usize,
	distinct_rules: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	taxonomy: Option<RuleTaxonomy>,
	taxonomy_summary: TaxonomySummary,
	#[serde(skip_serializing_if = "Option::is_none")]
	details: Option<ShowDetailPage>,
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
struct ShowDetailPage {
	offset: usize,
	limit: usize,
	returned: usize,
	total: usize,
	has_more: bool,
	rules: Vec<ShowRuleDetail>,
}

#[derive(Serialize)]
struct ShowRuleDetail {
	effective_id: String,
	id: String,
	origin: RuleOrigin,
	classification: RuleClassification,
	analysis: RuleCorpusAnalysis,
	languages: Vec<String>,
	domains: Vec<String>,
	compiled_rows: usize,
	severity: String,
	root: String,
	subject: String,
	plan: String,
	capabilities: Vec<String>,
	group_by: Vec<String>,
	kind: Option<String>,
	declared_expr: String,
	effective_expr: String,
	/// Backward-compatible alias for `effective_expr`.
	expr: String,
	expanded_expr: String,
	message: Option<String>,
	rationale: Option<String>,
	require_doc_comment: Option<String>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	projections: Vec<check::CompiledRuleSpec>,
}

#[derive(Default, Serialize)]
struct TaxonomySummary {
	distinct_rules: usize,
	classified_rules: usize,
	unclassified_rules: usize,
	invalid_rules: usize,
	pattern_counts: BTreeMap<String, usize>,
	component_counts: BTreeMap<String, usize>,
	origin_counts: BTreeMap<String, usize>,
	cross_tab: BTreeMap<String, BTreeMap<String, usize>>,
	unused_patterns: Vec<String>,
	unused_components: Vec<String>,
	unclassified_ids: Vec<String>,
	unclassified_ids_truncated: usize,
	migration_candidates: MigrationCandidates,
	needs_review_rules: usize,
	diagnostics: BTreeMap<String, DiagnosticSummary>,
}

#[derive(Default, Serialize)]
struct MigrationCandidates {
	rules: usize,
	rule_ids: Vec<String>,
	rule_ids_truncated: usize,
	pattern_counts: BTreeMap<String, usize>,
	component_counts: BTreeMap<String, usize>,
}

#[derive(Default, Serialize)]
struct DiagnosticSummary {
	rules: usize,
	occurrences: usize,
	examples: Vec<String>,
	examples_truncated: usize,
}

const SUMMARY_ID_SAMPLE_LIMIT: usize = 20;
const TEXT_DIAGNOSTIC_EXAMPLE_LIMIT: usize = 8;

fn validate_show_filters(
	args: &RulesShowArgs,
	taxonomy: Option<&RuleTaxonomy>,
) -> anyhow::Result<()> {
	if let Some(limit) = args.limit
		&& !(1..=200).contains(&limit)
	{
		bail!("--limit must be between 1 and 200");
	}
	if (!args.pattern.is_empty() || !args.component.is_empty()) && taxonomy.is_none() {
		bail!("pattern/component filters require `[rules.taxonomy]` in the project rules file");
	}
	if let Some(taxonomy) = taxonomy {
		for pattern in &args.pattern {
			if !taxonomy.patterns.contains(pattern) {
				bail!(
					"unknown rule pattern `{pattern}` (declared: {})",
					taxonomy.patterns.join(", ")
				);
			}
		}
		for component in &args.component {
			if !taxonomy.components.contains(component) {
				bail!(
					"unknown rule component `{component}` (declared: {})",
					taxonomy.components.join(", ")
				);
			}
		}
	}
	let known_origins = [
		"project",
		"fragment",
		"embedded_default",
		"external",
		"inline",
	];
	for origin in &args.origin {
		if !known_origins.contains(&origin.as_str()) {
			bail!(
				"unknown rule origin `{origin}` (known: {})",
				known_origins.join(", ")
			);
		}
	}
	Ok(())
}

fn aggregate_effective_rules(
	corpus: Vec<RuleCorpusEntry>,
	include_projections: bool,
) -> Vec<ShowRuleDetail> {
	let mut grouped = BTreeMap::<String, Vec<RuleCorpusEntry>>::new();
	for mut entry in corpus {
		let effective_id = std::mem::take(&mut entry.effective_id);
		grouped.entry(effective_id).or_default().push(entry);
	}
	grouped
		.into_iter()
		.map(|(effective_id, mut entries)| {
			entries.sort_by(|left, right| {
				left.rule
					.lang
					.cmp(&right.rule.lang)
					.then_with(|| left.rule.rule_id.cmp(&right.rule.rule_id))
			});
			let compiled_rows = entries.len();
			let languages = entries
				.iter()
				.map(|entry| entry.rule.lang.as_str())
				.collect::<BTreeSet<_>>()
				.into_iter()
				.map(str::to_string)
				.collect();
			let domains = entries
				.iter()
				.map(|entry| entry.rule.domain.as_str())
				.collect::<BTreeSet<_>>()
				.into_iter()
				.map(str::to_string)
				.collect();
			let projections = if include_projections {
				entries.iter().map(|entry| entry.rule.clone()).collect()
			} else {
				Vec::new()
			};
			let first = entries.remove(0);
			let classification = first.classification;
			let analysis = first.analysis;
			let effective_expr = first.rule.expr;
			let declared_expr = declared_expression(&effective_expr, &analysis.used_aliases);
			ShowRuleDetail {
				effective_id,
				id: classification.id.clone(),
				origin: first.origin,
				classification,
				analysis,
				languages,
				domains,
				compiled_rows,
				severity: first.rule.severity.as_str().to_string(),
				root: first.rule.root,
				subject: first.rule.subject,
				plan: first.rule.plan,
				capabilities: first.rule.capabilities,
				group_by: first.rule.group_by,
				kind: first.rule.kind,
				declared_expr,
				effective_expr: effective_expr.clone(),
				expr: effective_expr,
				expanded_expr: first.rule.expanded_expr,
				message: first.rule.message,
				rationale: first.rule.rationale,
				require_doc_comment: first.rule.require_doc_comment,
				projections,
			}
		})
		.collect()
}

fn declared_expression(expr: &str, aliases: &[RuleAliasUsage]) -> String {
	let bytes = expr.as_bytes();
	let mut rendered = String::with_capacity(expr.len());
	let mut index = 0;
	let mut copied_until = 0;
	while index < bytes.len() {
		if bytes[index] != b'$' {
			index += 1;
			continue;
		}
		let start = index + 1;
		let mut end = start;
		while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
			end += 1;
		}
		let effective_name = &expr[start..end];
		if let Some(alias) = aliases
			.iter()
			.find(|alias| alias.effective_name.as_deref() == Some(effective_name))
		{
			rendered.push_str(&expr[copied_until..start]);
			rendered.push_str(&alias.name);
			copied_until = end;
		}
		index = end.max(index + 1);
	}
	rendered.push_str(&expr[copied_until..]);
	rendered
}

fn filter_corpus(corpus: Vec<RuleCorpusEntry>, args: &RulesShowArgs) -> Vec<RuleCorpusEntry> {
	corpus
		.into_iter()
		.filter(|entry| {
			(args.pattern.is_empty()
				|| entry
					.classification
					.pattern
					.as_ref()
					.is_some_and(|pattern| args.pattern.contains(pattern)))
				&& args
					.component
					.iter()
					.all(|component| entry.classification.components.contains(component))
				&& (args.origin.is_empty()
					|| args
						.origin
						.iter()
						.any(|origin| origin == entry.origin.kind.as_str()))
		})
		.filter(|entry| {
			args.rule_id.as_ref().is_none_or(|id| {
				entry.rule.rule_id == *id
					|| entry.effective_id == *id
					|| entry.classification.id == *id
			})
		})
		.collect()
}

fn taxonomy_summary(
	taxonomy: Option<&RuleTaxonomy>,
	corpus: &[RuleCorpusEntry],
) -> TaxonomySummary {
	let mut summary = TaxonomySummary::default();
	for code in RuleCorpusDiagnosticCode::ALL {
		summary
			.diagnostics
			.insert(code.as_str().to_string(), DiagnosticSummary::default());
	}
	let mut seen = BTreeSet::new();
	for entry in corpus {
		if !seen.insert(entry.effective_id.as_str()) {
			continue;
		}
		*summary
			.origin_counts
			.entry(entry.origin.kind.as_str().to_string())
			.or_default() += 1;
		let mut diagnostic_codes = BTreeSet::new();
		if entry
			.analysis
			.diagnostics
			.iter()
			.any(|diagnostic| diagnostic.level == RuleCorpusDiagnosticLevel::NeedsReview)
		{
			summary.needs_review_rules += 1;
		}
		for diagnostic in &entry.analysis.diagnostics {
			let key = diagnostic.code.as_str().to_string();
			let diagnostic_summary = summary.diagnostics.entry(key).or_default();
			diagnostic_summary.occurrences += 1;
			if diagnostic_codes.insert(diagnostic.code) {
				diagnostic_summary.rules += 1;
			}
			let mut example = entry.classification.id.to_owned();
			if let Some(alias) = &diagnostic.alias {
				example.push_str(&format!(" (${}", alias));
				if let Some(anchor) = &diagnostic.anchor {
					example.push_str(&format!(": {anchor}"));
				}
				example.push(')');
			} else if let Some(anchor) = &diagnostic.anchor {
				example.push_str(&format!(" ({anchor})"));
			}
			push_bounded_id(
				&mut diagnostic_summary.examples,
				&mut diagnostic_summary.examples_truncated,
				&example,
			);
		}
		match entry.classification.status {
			RuleClassificationStatus::Classified => {
				summary.classified_rules += 1;
				if let Some(pattern) = &entry.classification.pattern {
					increment_count(&mut summary.pattern_counts, pattern);
					for component in &entry.classification.components {
						increment_count(&mut summary.component_counts, component);
						increment_cross_tab(&mut summary.cross_tab, pattern, component);
					}
				}
			}
			RuleClassificationStatus::Unclassified => {
				summary.unclassified_rules += 1;
				push_bounded_id(
					&mut summary.unclassified_ids,
					&mut summary.unclassified_ids_truncated,
					&entry.classification.id,
				);
				record_migration_candidate(&mut summary.migration_candidates, entry);
			}
			RuleClassificationStatus::Invalid => {
				summary.invalid_rules += 1;
				push_bounded_id(
					&mut summary.unclassified_ids,
					&mut summary.unclassified_ids_truncated,
					&entry.classification.id,
				);
				record_migration_candidate(&mut summary.migration_candidates, entry);
			}
			RuleClassificationStatus::TaxonomyNotDeclared => {}
		}
	}
	summary.distinct_rules = seen.len();
	if let Some(taxonomy) = taxonomy {
		summary.unused_patterns = taxonomy
			.patterns
			.iter()
			.filter(|pattern| !summary.pattern_counts.contains_key(*pattern))
			.map(ToString::to_string)
			.collect();
		summary.unused_components = taxonomy
			.components
			.iter()
			.filter(|component| !summary.component_counts.contains_key(*component))
			.map(ToString::to_string)
			.collect();
	}
	summary
}

fn record_migration_candidate(summary: &mut MigrationCandidates, entry: &RuleCorpusEntry) {
	if entry.classification.candidate_patterns.is_empty()
		&& entry.classification.candidate_components.is_empty()
	{
		return;
	}
	summary.rules += 1;
	push_bounded_id(
		&mut summary.rule_ids,
		&mut summary.rule_ids_truncated,
		&entry.classification.id,
	);
	for pattern in &entry.classification.candidate_patterns {
		increment_count(&mut summary.pattern_counts, pattern);
	}
	for component in &entry.classification.candidate_components {
		increment_count(&mut summary.component_counts, component);
	}
}

fn increment_count(counts: &mut BTreeMap<String, usize>, key: &str) {
	if let Some(count) = counts.get_mut(key) {
		*count += 1;
	} else {
		counts.insert(key.to_string(), 1);
	}
}

fn increment_cross_tab(
	cross_tab: &mut BTreeMap<String, BTreeMap<String, usize>>,
	pattern: &str,
	component: &str,
) {
	if let Some(components) = cross_tab.get_mut(pattern) {
		increment_count(components, component);
	} else {
		cross_tab.insert(
			pattern.to_string(),
			BTreeMap::from([(component.to_string(), 1)]),
		);
	}
}

fn push_bounded_id(ids: &mut Vec<String>, truncated: &mut usize, id: &str) {
	if ids.len() < SUMMARY_ID_SAMPLE_LIMIT {
		ids.push(id.to_string());
	} else {
		*truncated += 1;
	}
}

#[derive(Serialize)]
struct ShowTextContext<'a> {
	report: &'a ShowReport,
	focus: Vec<ShowTextFocus<'a>>,
	matrix: ShowTextMatrix<'a>,
	taxonomy_issues: Vec<ShowTextDiagnostic>,
	review_hints: Vec<ShowTextDiagnostic>,
	details: Option<ShowTextDetailPage<'a>>,
}

#[derive(Serialize)]
struct ShowTextFocus<'a> {
	label: &'static str,
	value: &'a str,
}

#[derive(Default, Serialize)]
struct ShowTextMatrix<'a> {
	components: Vec<&'a str>,
	rows: Vec<ShowTextMatrixRow<'a>>,
}

#[derive(Serialize)]
struct ShowTextMatrixRow<'a> {
	pattern: &'a str,
	counts: Vec<usize>,
}

#[derive(Serialize)]
struct ShowTextDiagnostic {
	code: &'static str,
	category: &'static str,
	rules: usize,
	occurrences: usize,
	guidance: &'static str,
	examples: String,
	omitted: usize,
	action: &'static str,
}

#[derive(Serialize)]
struct ShowTextDetailPage<'a> {
	offset: usize,
	end: usize,
	limit: usize,
	total: usize,
	has_more: bool,
	rules: Vec<ShowTextRule<'a>>,
}

#[derive(Serialize)]
struct ShowTextRule<'a> {
	rule: &'a ShowRuleDetail,
	declared_expr: String,
	effective_expr: Option<String>,
	expanded_expr: Option<String>,
	message: Option<String>,
	rationale: Option<String>,
	diagnostics: Vec<ShowTextRuleDiagnostic>,
}

#[derive(Serialize)]
struct ShowTextRuleDiagnostic {
	label: &'static str,
	code: &'static str,
	category: &'static str,
	guidance: &'static str,
}

fn write_show_text<W: Write>(
	w: &mut W,
	report: &ShowReport,
	args: &RulesShowArgs,
) -> anyhow::Result<()> {
	let context = show_text_context(report, args);
	w.write_all(rules_presentation::show(&context)?.as_bytes())?;
	Ok(())
}

fn show_text_context<'a>(report: &'a ShowReport, args: &'a RulesShowArgs) -> ShowTextContext<'a> {
	let mut focus = Vec::new();
	for pattern in &args.pattern {
		focus.push(ShowTextFocus {
			label: "pattern",
			value: pattern,
		});
	}
	for component in &args.component {
		focus.push(ShowTextFocus {
			label: "component",
			value: component,
		});
	}
	for origin in &args.origin {
		focus.push(ShowTextFocus {
			label: "origin",
			value: origin,
		});
	}
	if let Some(rule_id) = &args.rule_id {
		focus.push(ShowTextFocus {
			label: "rule",
			value: rule_id,
		});
	}
	if let Some(profile) = &args.profile {
		focus.push(ShowTextFocus {
			label: "profile",
			value: profile,
		});
	}

	let components = report
		.taxonomy
		.as_ref()
		.map(|taxonomy| {
			taxonomy
				.components
				.iter()
				.filter(|component| {
					report
						.taxonomy_summary
						.component_counts
						.contains_key(*component)
				})
				.map(String::as_str)
				.collect::<Vec<_>>()
		})
		.unwrap_or_default();
	let rows = report
		.taxonomy_summary
		.cross_tab
		.iter()
		.map(|(pattern, counts)| ShowTextMatrixRow {
			pattern,
			counts: components
				.iter()
				.map(|component| counts.get(*component).copied().unwrap_or(0))
				.collect(),
		})
		.collect();

	let details = report.details.as_ref().map(|page| ShowTextDetailPage {
		offset: page.offset,
		end: page.offset + page.returned,
		limit: page.limit,
		total: page.total,
		has_more: page.has_more,
		rules: page.rules.iter().map(show_text_rule).collect(),
	});

	ShowTextContext {
		report,
		focus,
		matrix: ShowTextMatrix { components, rows },
		taxonomy_issues: show_text_diagnostics(report, RuleCorpusDiagnosticLevel::Nonconforming),
		review_hints: show_text_diagnostics(report, RuleCorpusDiagnosticLevel::NeedsReview),
		details,
	}
}

fn show_text_diagnostics(
	report: &ShowReport,
	level: RuleCorpusDiagnosticLevel,
) -> Vec<ShowTextDiagnostic> {
	RuleCorpusDiagnosticCode::ALL
		.into_iter()
		.filter(|code| code.level() == level)
		.filter_map(|code| {
			let summary = report.taxonomy_summary.diagnostics.get(code.as_str())?;
			(summary.occurrences > 0).then(|| {
				let examples = summary
					.examples
					.iter()
					.take(TEXT_DIAGNOSTIC_EXAMPLE_LIMIT)
					.map(String::as_str)
					.collect::<Vec<_>>()
					.join(", ");
				let omitted = summary
					.examples
					.len()
					.saturating_sub(TEXT_DIAGNOSTIC_EXAMPLE_LIMIT)
					+ summary.examples_truncated;
				ShowTextDiagnostic {
					code: code.as_str(),
					category: code.category().as_str(),
					rules: summary.rules,
					occurrences: summary.occurrences,
					guidance: code.guidance(),
					examples,
					omitted,
					action: code.migration_action(),
				}
			})
		})
		.collect()
}

fn show_text_rule(rule: &ShowRuleDetail) -> ShowTextRule<'_> {
	ShowTextRule {
		rule,
		declared_expr: one_line(&rule.declared_expr),
		effective_expr: (rule.declared_expr != rule.effective_expr)
			.then(|| one_line(&rule.effective_expr)),
		expanded_expr: (rule.effective_expr != rule.expanded_expr)
			.then(|| one_line(&rule.expanded_expr)),
		message: rule.message.as_deref().map(one_line),
		rationale: rule.rationale.as_deref().map(one_line),
		diagnostics: rule
			.analysis
			.diagnostics
			.iter()
			.map(|diagnostic| ShowTextRuleDiagnostic {
				label: if diagnostic.level == RuleCorpusDiagnosticLevel::Nonconforming {
					"taxonomy issue"
				} else {
					"review hint"
				},
				code: diagnostic.code.as_str(),
				category: diagnostic.category.as_str(),
				guidance: diagnostic.code.guidance(),
			})
			.collect(),
	}
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
			"java_main = \"srcset = 'main'\"\n\
			 java_test = \"srcset = 'test'\"\n",
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
		assert!(config.contains("java_main = \"srcset = 'main'\""));
		assert!(config.contains("java_test = \"srcset = 'test'\""));
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
			"--details",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out = String::from_utf8(stdout).unwrap();
		assert!(out.contains("default rules: false"), "{out}");
		assert!(out.contains("profile: only-keep"), "{out}");
		assert!(out.contains("ts.class.keep"), "{out}");
		assert!(
			out.contains("expanded expr: `(moniker ~ '**/dir:src/**') => name =~ ^[A-Z]`"),
			"{out}"
		);
		assert!(
			out.contains(
				"ADR-001: generated types are exempt, but source classes stay PascalCase."
			),
			"{out}"
		);
		assert!(out.contains("severity: `warn`"), "{out}");
		assert!(!out.contains("ts.class.drop"), "{out}");
		crate::presentation::tests::validate_agent_markdown(&out, "Project rules", false)
			.expect("rules show Markdown");
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
		assert!(!out.contains("import { Router }"), "{out}");
	}

	#[test]
	fn rules_learn_prints_taxonomy_topic() {
		let cli = Cli::parse_from(["code-moniker", "rules", "learn", "taxonomy"]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out = String::from_utf8(stdout).unwrap();
		assert!(out.contains("# --- taxonomy: Project taxonomy"), "{out}");
		assert!(out.contains("[rules.taxonomy]"), "{out}");
		assert!(out.contains("Scoped Components Are Atomic"), "{out}");
		assert!(out.contains("zero is not a conformance target"), "{out}");
		assert!(!out.contains("cm:file="), "{out}");
		assert!(!out.contains("import { snapshot }"), "{out}");
		assert!(!out.contains(" @ src/workspace/editor.ts"), "{out}");
	}

	#[test]
	fn rules_learn_prints_all_dsl_topics() {
		let cli = Cli::parse_from(["code-moniker", "rules", "learn"]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out = String::from_utf8(stdout).unwrap();
		assert!(
			out.contains(
				"# Topics: basics, taxonomy, paths, fragments, refs, collections, domains, metrics, aggregates, relations, directives, profiles"
			),
			"{out}"
		);
		assert!(out.contains("# --- basics:"), "{out}");
		assert!(out.contains("# --- taxonomy:"), "{out}");
		assert!(out.contains("# --- profiles:"), "{out}");
		assert!(!out.contains("cm:expect"), "{out}");
	}

	#[test]
	fn rules_learn_embeds_every_learn_topic_document() {
		let learn_dir =
			std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/learn");
		let embedded_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/learn");
		let topic_names_in = |dir: &std::path::Path| {
			let mut names: Vec<String> = std::fs::read_dir(dir)
				.expect("learn samples directory")
				.filter_map(|entry| {
					let path = entry.expect("learn sample entry").path();
					let name = path.file_name()?.to_str()?;
					name.strip_suffix(".cm.md").map(str::to_string)
				})
				.collect();
			names.sort();
			names
		};
		let packaged = topic_names_in(&embedded_dir);
		let mut embedded = super::learn_topic_names();
		embedded.sort();
		assert_eq!(
			embedded,
			packaged.iter().map(String::as_str).collect::<Vec<_>>(),
			"`rules learn` must expose every packaged learn document"
		);
		if learn_dir.is_dir() {
			let source_topics = topic_names_in(&learn_dir);
			assert_eq!(
				packaged, source_topics,
				"packaged learn topics drifted from the repository corpus"
			);
			for name in &packaged {
				let file_name = format!("{name}.cm.md");
				let source = std::fs::read_to_string(learn_dir.join(&file_name))
					.expect("repository learn sample");
				let embedded = std::fs::read_to_string(embedded_dir.join(&file_name))
					.expect("packaged learn sample");
				assert_eq!(
					embedded, source,
					"packaged learn sample {file_name} drifted from its repository source"
				);
			}
		}
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
			"--details",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		assert_eq!(out["default_rules"], false);
		assert!(out["compiled_rows"].as_u64().unwrap() >= 1);
		assert_eq!(out["taxonomy_summary"]["unclassified_rules"], 0);
		assert_eq!(
			out["taxonomy_summary"]["unclassified_ids"],
			serde_json::json!([])
		);
		let rule = out["details"]["rules"]
			.as_array()
			.unwrap()
			.iter()
			.find(|rule| rule["effective_id"] == "refs.domain-no-infra")
			.expect("domain rule is present");
		assert_eq!(
			rule["rationale"],
			"ADR-002: the domain layer must stay independent from infrastructure details."
		);
		assert_eq!(rule["severity"], "error");
		assert_eq!(rule["classification"]["status"], "taxonomy_not_declared");
	}

	#[test]
	fn rules_show_maps_and_filters_the_effective_taxonomy() {
		let dir = tempdir().unwrap();
		std::fs::write(
			dir.path().join(".code-moniker.toml"),
			r#"
			default_rules = false

			[rules.taxonomy]
			patterns = ["ownership", "hygiene"]
			components = ["daemon", "code"]

			[[rust.fn.where]]
			id = "ownership-daemon-use-facade"
			expr = "name != 'forbidden'"

			[[rust.fn.where]]
			id = "hygiene-code-use-specific-names"
			expr = "name != 'placeholder'"

			[[rust.fn.where]]
			id = "legacy_rule"
			expr = "name != 'legacy'"

			[[rust.fn.where]]
			id = "legacy-ownership-hygiene-daemon"
			expr = "name != 'migration'"
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
			"--details",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		assert_eq!(out["distinct_rules"], 4);
		assert_eq!(out["taxonomy_summary"]["classified_rules"], 2);
		assert_eq!(out["taxonomy_summary"]["unclassified_rules"], 1);
		assert_eq!(out["taxonomy_summary"]["invalid_rules"], 1);
		assert_eq!(
			out["taxonomy_summary"]["cross_tab"]["ownership"]["daemon"],
			1
		);
		assert_eq!(out["taxonomy_summary"]["pattern_counts"]["ownership"], 1);
		assert_eq!(out["taxonomy_summary"]["component_counts"]["daemon"], 1);
		assert_eq!(
			out["taxonomy_summary"]["migration_candidates"]["pattern_counts"]["ownership"],
			1
		);
		assert_eq!(
			out["taxonomy_summary"]["migration_candidates"]["component_counts"]["daemon"],
			1
		);
		let rules = out["details"]["rules"].as_array().unwrap();
		let classified = rules
			.iter()
			.find(|rule| rule["classification"]["pattern"] == "ownership")
			.unwrap();
		assert_eq!(classified["origin"]["kind"], "project");
		assert_eq!(classified["classification"]["pattern"], "ownership");
		assert_eq!(
			classified["classification"]["components"],
			serde_json::json!(["daemon"])
		);

		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"show",
			dir.path().to_str().unwrap(),
			"--pattern",
			"ownership",
			"--component",
			"daemon",
			"--format",
			"json",
		]);
		stdout.clear();
		stderr.clear();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let filtered: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		assert_eq!(filtered["distinct_rules"], 1);
		assert_eq!(filtered["compiled_rows"], 1);
		assert_eq!(
			filtered["taxonomy_summary"]["unused_patterns"],
			serde_json::json!([])
		);
		assert_eq!(
			filtered["taxonomy_summary"]["unused_components"],
			serde_json::json!([])
		);

		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"show",
			dir.path().to_str().unwrap(),
		]);
		stdout.clear();
		stderr.clear();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let text = String::from_utf8(stdout).unwrap();
		assert!(
			text.contains("taxonomy conformance: 2 classified; 1 unclassified; 1 invalid"),
			"{text}"
		);
		assert!(text.contains("taxonomy issues:"), "{text}");
		assert!(
			text.contains("`missing-pattern-anchor` [`id classification`]"),
			"{text}"
		);
		assert!(
			text.contains("`ambiguous-pattern-anchors` [`id classification`]"),
			"{text}"
		);
		crate::presentation::tests::validate_agent_markdown(&text, "Project rules", true)
			.expect("rules taxonomy Markdown");
		let project = dir.path().canonicalize().unwrap();
		let normalized = text.replace(project.to_str().unwrap(), "<PROJECT>");
		insta::assert_snapshot!("rules_show_taxonomy_markdown", normalized);
	}

	#[test]
	fn rules_show_filters_scoped_components_without_lexical_rollup() {
		let dir = tempdir().unwrap();
		std::fs::write(
			dir.path().join(".code-moniker.toml"),
			r#"
			default_rules = false

			[rules.taxonomy]
			patterns = ["separation-of-concerns"]
			components = ["dsl", "index", "workspace", "index@workspace"]

			[aliases]
			dsl_evaluation = "name = 'evaluation'"
			index_at_workspace_target = "name = 'snapshot'"

			[[rust.fn.where]]
			id = "dsl-and-index@workspace-separation-of-concerns-preserves-local-evaluation"
			expr = "$dsl_evaluation AND $index_at_workspace_target"
			"#,
		)
		.unwrap();

		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"show",
			dir.path().to_str().unwrap(),
			"--component",
			"index@workspace",
			"--format",
			"json",
			"--details",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		assert_eq!(out["distinct_rules"], 1);
		assert_eq!(
			out["details"]["rules"][0]["classification"]["components"],
			serde_json::json!(["dsl", "index@workspace"])
		);
		assert_eq!(
			out["taxonomy_summary"]["component_counts"],
			serde_json::json!({"dsl": 1, "index@workspace": 1})
		);
		assert_eq!(out["taxonomy_summary"]["needs_review_rules"], 0);

		for parent in ["index", "workspace"] {
			let cli = Cli::parse_from([
				"code-moniker",
				"rules",
				"show",
				dir.path().to_str().unwrap(),
				"--component",
				parent,
				"--format",
				"json",
			]);
			stdout.clear();
			stderr.clear();
			assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
			let filtered: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
			assert_eq!(filtered["distinct_rules"], 0, "parent filter `{parent}`");
			assert_eq!(filtered["compiled_rows"], 0, "parent filter `{parent}`");
		}
	}

	#[test]
	fn rules_show_reports_alias_alignment_and_migration_diagnostics() {
		let dir = tempdir().unwrap();
		std::fs::write(
			dir.path().join(".code-moniker.toml"),
			r#"
			default_rules = false

			[rules.taxonomy]
			patterns = ["ownership", "hygiene"]
			components = ["mcp", "workspace", "code"]

			[aliases]
			mcp_server = "name = 'server'"
			workspace_runtime_target = "name = 'workspace'"
			http_runtime_target = "name = 'http'"

			[[rust.fn.where]]
			id = "mcp-runtime-ownership-is-on-server"
			expr = "$mcp_server"

			[[rust.fn.where]]
			id = "mcp-runtime-ownership-needs-alignment"
			expr = "$workspace_runtime_target"

			[[rust.fn.where]]
			id = "code-hygiene-uses-generic-selector"
			expr = "$http_runtime_target"

			[[rust.fn.where]]
			id = "code-hygiene-rejects-placeholder-names"
			expr = "name != 'placeholder'"

			[[refs.where]]
			id = "workspace-ownership-target-is-local"
			expr = "target ~ '**/external_pkg:other/**'"
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
			"--details",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		let diagnostics = &out["taxonomy_summary"]["diagnostics"];
		assert_eq!(out["taxonomy_summary"]["classified_rules"], 5);
		assert_eq!(diagnostics["missing-pattern-anchor"]["rules"], 0);
		assert_eq!(diagnostics["ambiguous-pattern-anchors"]["rules"], 0);
		assert_eq!(diagnostics["missing-component-anchor"]["rules"], 0);
		assert_eq!(diagnostics["rule-uses-no-alias"]["rules"], 2);
		assert_eq!(
			diagnostics["alias-has-no-taxonomy-anchor"]["occurrences"],
			1
		);
		assert_eq!(
			diagnostics["alias-anchor-missing-from-rule-id"]["occurrences"],
			1
		);
		assert_eq!(
			diagnostics["rule-component-not-represented-by-used-alias"]["occurrences"],
			2
		);
		assert_eq!(diagnostics["inline-project-selector-candidate"]["rules"], 1);

		let rules = out["details"]["rules"].as_array().unwrap();
		let aligned = rules
			.iter()
			.find(|rule| rule["id"] == "mcp-runtime-ownership-is-on-server")
			.unwrap();
		assert_eq!(aligned["classification"]["pattern"], "ownership");
		assert_eq!(
			aligned["classification"]["components"],
			serde_json::json!(["mcp"])
		);
		assert_eq!(aligned["analysis"]["used_aliases"][0]["name"], "mcp_server");
		assert_eq!(
			aligned["analysis"]["used_aliases"][0]["components"],
			serde_json::json!(["mcp"])
		);
		assert_eq!(aligned["analysis"]["diagnostics"], serde_json::json!([]));

		let no_alias = rules
			.iter()
			.find(|rule| rule["id"] == "code-hygiene-rejects-placeholder-names")
			.unwrap();
		assert!(
			no_alias["analysis"]["diagnostics"]
				.as_array()
				.unwrap()
				.iter()
				.any(|diagnostic| diagnostic["code"] == "rule-uses-no-alias"
					&& diagnostic["level"] == "needs_review")
		);

		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"show",
			dir.path().to_str().unwrap(),
		]);
		stdout.clear();
		stderr.clear();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let text = String::from_utf8(stdout).unwrap();
		assert!(
			text.contains(
				"review hints: 4 distinct rule(s) (advisory; zero is not a conformance target)"
			),
			"{text}"
		);
		assert!(
			text.contains("`rule-uses-no-alias` [`alias alignment`]: 2 rule(s), 2 occurrence(s)"),
			"{text}"
		);
		assert!(
			text.contains("metric and generic hygiene rules may legitimately"),
			"{text}"
		);
		assert!(
			text.contains("code-hygiene-rejects-placeholder-names"),
			"{text}"
		);
		assert!(
			text.contains("workspace-ownership-target-is-local"),
			"{text}"
		);
		assert!(
			text.contains("action: review-whether-rule-needs-alias"),
			"{text}"
		);
		assert!(text.contains("next: add --details"), "{text}");
		assert!(text.contains("focus: add `--component NAME`"), "{text}");
		assert!(!text.contains("needs review:"), "{text}");
	}

	#[test]
	fn rules_show_distinguishes_declared_effective_and_expanded_expressions() {
		let dir = tempdir().unwrap();
		std::fs::create_dir_all(dir.path().join("runtime")).unwrap();
		std::fs::write(
			dir.path().join(".code-moniker.toml"),
			r#"
			default_rules = false

			[rules.taxonomy]
			patterns = ["ownership"]
			components = ["daemon"]
			"#,
		)
		.unwrap();
		std::fs::write(
			dir.path().join("runtime/code-moniker.fragment.toml"),
			r#"
			fragment = "runtime"

			[aliases]
			daemon_boundary = "name != 'forbidden'"

			[[rust.fn.where]]
			id = "daemon-ownership-stays-local"
			expr = "$daemon_boundary"
			"#,
		)
		.unwrap();

		let json_cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"show",
			dir.path().to_str().unwrap(),
			"--format",
			"json",
			"--details",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();
		assert_eq!(run(&json_cli, &mut stdout, &mut stderr), Exit::Match);
		let out: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		let rule = &out["details"]["rules"][0];
		assert_eq!(rule["declared_expr"], "$daemon_boundary");
		assert_eq!(rule["effective_expr"], "$runtime_daemon_boundary");
		assert_eq!(rule["expr"], rule["effective_expr"]);
		assert!(
			rule["expanded_expr"]
				.as_str()
				.unwrap()
				.contains("name != 'forbidden'")
		);

		let text_cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"show",
			dir.path().to_str().unwrap(),
			"--details",
		]);
		stdout.clear();
		stderr.clear();
		assert_eq!(run(&text_cli, &mut stdout, &mut stderr), Exit::Match);
		let text = String::from_utf8(stdout).unwrap();
		assert!(text.contains("declared expr: `$daemon_boundary`"), "{text}");
		assert!(
			text.contains("effective expr: `$runtime_daemon_boundary`"),
			"{text}"
		);
		assert!(
			text.contains("expanded expr: `(name != 'forbidden')`"),
			"{text}"
		);
	}

	#[test]
	fn rules_show_distinguishes_fragment_and_embedded_origins() {
		let dir = tempdir().unwrap();
		std::fs::create_dir_all(dir.path().join("module/src")).unwrap();
		std::fs::write(
			dir.path().join(".code-moniker.toml"),
			r#"
			default_rules = true

			[rules.taxonomy]
			patterns = ["ownership"]
			components = ["daemon"]
			"#,
		)
		.unwrap();
		std::fs::write(
			dir.path().join("module/src/code-moniker.fragment.toml"),
			r#"
			fragment = "fn"

			[[rust.fn.where]]
			id = "ownership-daemon-use-boundary"
			expr = "name != 'forbidden'"
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
			"--details",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		assert!(
			out["taxonomy_summary"]["origin_counts"]["embedded_default"]
				.as_u64()
				.unwrap() > 0
		);
		assert_eq!(out["taxonomy_summary"]["origin_counts"]["fragment"], 1);
		let fragment = out["details"]["rules"]
			.as_array()
			.unwrap()
			.iter()
			.find(|rule| rule["origin"]["kind"] == "fragment")
			.unwrap();
		assert_eq!(fragment["origin"]["fragment"], "fn");
		assert_eq!(fragment["classification"]["pattern"], "ownership");
	}

	#[test]
	fn rules_show_reports_inline_override_as_the_effective_origin() {
		let dir = tempdir().unwrap();
		std::fs::write(
			dir.path().join(".code-moniker.toml"),
			r#"
			default_rules = false

			[rules.taxonomy]
			patterns = ["ownership"]
			components = ["daemon"]

			[[rust.fn.where]]
			id = "ownership-daemon-use-boundary"
			expr = "name != 'project'"
			"#,
		)
		.unwrap();
		let inline = r#"
			[[rust.fn.where]]
			id = "ownership-daemon-use-boundary"
			expr = "name != 'inline'"
			"#;

		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"show",
			dir.path().to_str().unwrap(),
			"--rules-inline",
			inline,
			"--format",
			"json",
			"--details",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		let rule = out["details"]["rules"]
			.as_array()
			.unwrap()
			.iter()
			.find(|rule| rule["classification"]["id"] == "ownership-daemon-use-boundary")
			.unwrap();
		assert_eq!(rule["origin"]["kind"], "inline");
		assert_eq!(rule["expr"], "name != 'inline'");
	}

	#[test]
	fn rules_show_keeps_anonymous_overlay_identity_after_merge() {
		let dir = tempdir().unwrap();
		std::fs::write(
			dir.path().join(".code-moniker.toml"),
			r#"
			default_rules = false

			[[refs.where]]
			expr = "source != target"
			"#,
		)
		.unwrap();
		let inline = r#"
			[[refs.where]]
			expr = "source = target"
			"#;

		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"show",
			dir.path().to_str().unwrap(),
			"--rules-inline",
			inline,
			"--format",
			"json",
			"--details",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		assert_eq!(out["distinct_rules"], 2);
		let inline_rule = out["details"]["rules"]
			.as_array()
			.unwrap()
			.iter()
			.find(|rule| rule["effective_id"] == "refs.where_1")
			.expect("anonymous inline rule keeps its post-merge identity");
		assert_eq!(inline_rule["origin"]["kind"], "inline");
		assert_eq!(inline_rule["compiled_rows"], super::Lang::ALL.len());
	}

	#[test]
	fn rules_show_validates_the_exact_declared_id_including_dots() {
		let dir = tempdir().unwrap();
		std::fs::write(
			dir.path().join(".code-moniker.toml"),
			r#"
			default_rules = false

			[rules.taxonomy]
			patterns = ["ownership"]
			components = ["daemon"]

			[[rust.fn.where]]
			id = "legacy.ownership-daemon-use-boundary"
			expr = "name != 'forbidden'"
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
			"--details",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		let rule = &out["details"]["rules"][0];
		assert_eq!(rule["id"], "legacy.ownership-daemon-use-boundary");
		assert_eq!(rule["classification"]["status"], "invalid");
		assert_eq!(rule["classification"]["pattern"], serde_json::Value::Null);
	}

	#[test]
	fn rules_show_counts_one_declared_rule_across_compiled_language_rows() {
		let dir = tempdir().unwrap();
		std::fs::write(
			dir.path().join(".code-moniker.toml"),
			r#"
			default_rules = false

			[rules.taxonomy]
			patterns = ["dependency"]
			components = ["query"]

			[[refs.where]]
			id = "dependency-query-use-contract"
			expr = "source != target"
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
		assert_eq!(out["compiled_rows"], super::Lang::ALL.len());
		assert_eq!(out["distinct_rules"], 1);
		assert_eq!(out["taxonomy_summary"]["classified_rules"], 1);
		assert!(out.get("details").is_none());

		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"show",
			dir.path().to_str().unwrap(),
			"--format",
			"json",
			"--details",
			"--by-language",
			"--limit",
			"1",
		]);
		stdout.clear();
		stderr.clear();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let detailed: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		assert_eq!(detailed["details"]["total"], 1);
		assert_eq!(detailed["details"]["returned"], 1);
		assert_eq!(
			detailed["details"]["rules"][0]["compiled_rows"],
			super::Lang::ALL.len()
		);
		assert_eq!(
			detailed["details"]["rules"][0]["projections"]
				.as_array()
				.unwrap()
				.len(),
			super::Lang::ALL.len()
		);
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
			"--details",
			"--by-language",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let out: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		assert_eq!(out["distinct_rules"], 2);
		let rust_ids: Vec<_> = out["details"]["rules"]
			.as_array()
			.unwrap()
			.iter()
			.flat_map(|rule| rule["projections"].as_array().unwrap())
			.filter(|projection| projection["lang"] == "rs")
			.map(|projection| projection["rule_id"].as_str().unwrap().to_string())
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
