use serde::Serialize;

use super::config::{
	Config, RuleSource, RuleSourceKind, RuleTaxonomy, is_canonical_rule_id,
	rule_source_for_compiled_id, scope_markers_belong_to_declared_components,
	taxonomy_alias_anchor,
};
use super::eval::CompiledRuleSpec;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RuleOrigin {
	pub kind: RuleOriginKind,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub source: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub fragment: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleOriginKind {
	EmbeddedDefault,
	Project,
	Fragment,
	External,
	Inline,
}

impl RuleOriginKind {
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::EmbeddedDefault => "embedded_default",
			Self::Project => "project",
			Self::Fragment => "fragment",
			Self::External => "external",
			Self::Inline => "inline",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RuleClassification {
	pub id: String,
	pub status: RuleClassificationStatus,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub pattern: Option<String>,
	pub components: Vec<String>,
	pub diagnostics: Vec<String>,
	pub candidate_patterns: Vec<String>,
	pub candidate_components: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RuleAliasUsage {
	pub name: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub effective_name: Option<String>,
	pub patterns: Vec<String>,
	pub components: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuleCorpusDiagnosticCode {
	MissingPatternAnchor,
	AmbiguousPatternAnchors,
	MissingComponentAnchor,
	RuleUsesNoAlias,
	AliasHasNoTaxonomyAnchor,
	AliasAnchorMissingFromRuleId,
	RuleComponentNotRepresentedByUsedAlias,
	InlineProjectSelectorCandidate,
}

impl RuleCorpusDiagnosticCode {
	pub const ALL: [Self; 8] = [
		Self::MissingPatternAnchor,
		Self::AmbiguousPatternAnchors,
		Self::MissingComponentAnchor,
		Self::RuleUsesNoAlias,
		Self::AliasHasNoTaxonomyAnchor,
		Self::AliasAnchorMissingFromRuleId,
		Self::RuleComponentNotRepresentedByUsedAlias,
		Self::InlineProjectSelectorCandidate,
	];

	pub const fn as_str(self) -> &'static str {
		match self {
			Self::MissingPatternAnchor => "missing-pattern-anchor",
			Self::AmbiguousPatternAnchors => "ambiguous-pattern-anchors",
			Self::MissingComponentAnchor => "missing-component-anchor",
			Self::RuleUsesNoAlias => "rule-uses-no-alias",
			Self::AliasHasNoTaxonomyAnchor => "alias-has-no-taxonomy-anchor",
			Self::AliasAnchorMissingFromRuleId => "alias-anchor-missing-from-rule-id",
			Self::RuleComponentNotRepresentedByUsedAlias => {
				"rule-component-not-represented-by-used-alias"
			}
			Self::InlineProjectSelectorCandidate => "inline-project-selector-candidate",
		}
	}

	pub const fn category(self) -> RuleCorpusDiagnosticCategory {
		match self {
			Self::MissingPatternAnchor
			| Self::AmbiguousPatternAnchors
			| Self::MissingComponentAnchor => RuleCorpusDiagnosticCategory::IdClassification,
			Self::RuleUsesNoAlias
			| Self::AliasHasNoTaxonomyAnchor
			| Self::AliasAnchorMissingFromRuleId
			| Self::RuleComponentNotRepresentedByUsedAlias => RuleCorpusDiagnosticCategory::AliasAlignment,
			Self::InlineProjectSelectorCandidate => {
				RuleCorpusDiagnosticCategory::MigrationSuggestion
			}
		}
	}

	pub const fn level(self) -> RuleCorpusDiagnosticLevel {
		match self {
			Self::MissingPatternAnchor
			| Self::AmbiguousPatternAnchors
			| Self::MissingComponentAnchor => RuleCorpusDiagnosticLevel::Nonconforming,
			Self::RuleUsesNoAlias
			| Self::AliasHasNoTaxonomyAnchor
			| Self::AliasAnchorMissingFromRuleId
			| Self::RuleComponentNotRepresentedByUsedAlias
			| Self::InlineProjectSelectorCandidate => RuleCorpusDiagnosticLevel::NeedsReview,
		}
	}

	pub const fn migration_action(self) -> &'static str {
		match self {
			Self::MissingPatternAnchor
			| Self::AmbiguousPatternAnchors
			| Self::MissingComponentAnchor => "review-rule-id-anchors",
			Self::RuleUsesNoAlias => "review-whether-rule-needs-alias",
			Self::AliasHasNoTaxonomyAnchor => "review-alias-taxonomy-anchors",
			Self::AliasAnchorMissingFromRuleId | Self::RuleComponentNotRepresentedByUsedAlias => {
				"align-rule-id-and-used-aliases"
			}
			Self::InlineProjectSelectorCandidate => "extract-inline-project-selector-into-alias",
		}
	}

	pub const fn guidance(self) -> &'static str {
		match self {
			Self::MissingPatternAnchor => {
				"The rule id must contain exactly one declared architectural pattern."
			}
			Self::AmbiguousPatternAnchors => {
				"The rule id contains more than one declared pattern; choose the pattern that states the enforced invariant."
			}
			Self::MissingComponentAnchor => {
				"The rule id must name at least one declared project component involved in the invariant."
			}
			Self::RuleUsesNoAlias => {
				"Review whether the expression hides a project-specific selector; metric and generic hygiene rules may legitimately use no alias."
			}
			Self::AliasHasNoTaxonomyAnchor => {
				"Review whether the alias is generic or should name a project component; generic aliases may legitimately have no taxonomy anchor."
			}
			Self::AliasAnchorMissingFromRuleId => {
				"A used alias names a taxonomy anchor absent from the rule id; add it only when that architectural party is material to the invariant."
			}
			Self::RuleComponentNotRepresentedByUsedAlias => {
				"A rule component is not represented by a used alias; verify that the expression still provides a clear coordinate to that component."
			}
			Self::InlineProjectSelectorCandidate => {
				"A raw project selector may deserve a stable alias when it represents a reusable project zone or symbol."
			}
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleCorpusDiagnosticCategory {
	IdClassification,
	AliasAlignment,
	MigrationSuggestion,
}

impl RuleCorpusDiagnosticCategory {
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::IdClassification => "id classification",
			Self::AliasAlignment => "alias alignment",
			Self::MigrationSuggestion => "migration suggestion",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleCorpusDiagnosticLevel {
	Nonconforming,
	NeedsReview,
}

impl RuleCorpusDiagnosticLevel {
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Nonconforming => "nonconforming",
			Self::NeedsReview => "needs_review",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RuleCorpusDiagnostic {
	pub code: RuleCorpusDiagnosticCode,
	pub category: RuleCorpusDiagnosticCategory,
	pub level: RuleCorpusDiagnosticLevel,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub alias: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub anchor: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RuleCorpusAnalysis {
	pub used_aliases: Vec<RuleAliasUsage>,
	pub diagnostics: Vec<RuleCorpusDiagnostic>,
	pub migration_actions: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleClassificationStatus {
	Classified,
	Unclassified,
	Invalid,
	TaxonomyNotDeclared,
}

impl RuleClassificationStatus {
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Classified => "classified",
			Self::Unclassified => "unclassified",
			Self::Invalid => "invalid",
			Self::TaxonomyNotDeclared => "taxonomy_not_declared",
		}
	}
}

#[derive(Clone, Debug, Serialize)]
pub struct RuleCorpusEntry {
	pub effective_id: String,
	#[serde(flatten)]
	pub rule: CompiledRuleSpec,
	pub origin: RuleOrigin,
	pub classification: RuleClassification,
	pub analysis: RuleCorpusAnalysis,
}

pub(crate) struct RuleCorpusContext<'a> {
	pub taxonomy: Option<&'a RuleTaxonomy>,
	pub config: &'a Config,
}

pub(crate) fn build_rule_corpus(
	specs: Vec<CompiledRuleSpec>,
	context: RuleCorpusContext<'_>,
) -> Vec<RuleCorpusEntry> {
	specs
		.into_iter()
		.map(|rule| {
			let (effective_id, declared_id, origin, local_aliases) =
				match rule_source_for_compiled_id(context.config, &rule.rule_id) {
					Some((effective_id, source)) => (
						effective_id,
						source.declared_id.as_str(),
						rule_origin(source),
						source.local_aliases.as_slice(),
					),
					None => (
						rule.rule_id.clone(),
						rule.rule_id.as_str(),
						RuleOrigin {
							kind: RuleOriginKind::External,
							source: None,
							fragment: None,
						},
						&[][..],
					),
				};
			let classification = classify_rule_id(declared_id, context.taxonomy);
			let analysis = analyze_rule_expressions(
				&rule.analysis_exprs,
				&classification,
				context.taxonomy,
				origin.fragment.as_deref(),
				local_aliases,
			);
			RuleCorpusEntry {
				effective_id,
				rule,
				origin,
				classification,
				analysis,
			}
		})
		.collect()
}

#[cfg(test)]
fn analyze_rule(
	expr: &str,
	classification: &RuleClassification,
	taxonomy: Option<&RuleTaxonomy>,
	fragment: Option<&str>,
	local_aliases: &[String],
) -> RuleCorpusAnalysis {
	analyze_rule_expressions(
		&[expr.to_string()],
		classification,
		taxonomy,
		fragment,
		local_aliases,
	)
}

fn analyze_rule_expressions(
	expressions: &[String],
	classification: &RuleClassification,
	taxonomy: Option<&RuleTaxonomy>,
	fragment: Option<&str>,
	local_aliases: &[String],
) -> RuleCorpusAnalysis {
	let mut diagnostics = Vec::new();
	if let Some(taxonomy) = taxonomy {
		match classification.candidate_patterns.as_slice() {
			[] => diagnostics.push(diagnostic(
				RuleCorpusDiagnosticCode::MissingPatternAnchor,
				None,
				None,
			)),
			[_] => {}
			_ => diagnostics.push(diagnostic(
				RuleCorpusDiagnosticCode::AmbiguousPatternAnchors,
				None,
				None,
			)),
		}
		if classification.candidate_components.is_empty() {
			diagnostics.push(diagnostic(
				RuleCorpusDiagnosticCode::MissingComponentAnchor,
				None,
				None,
			));
		}

		let used_aliases = direct_alias_names_in_expressions(expressions)
			.into_iter()
			.map(|effective_name| {
				let name = declared_alias_name(&effective_name, fragment, local_aliases);
				let patterns = maximal_matching_alias_terms(&name, &taxonomy.patterns);
				let components = maximal_matching_alias_terms(&name, &taxonomy.components);
				RuleAliasUsage {
					effective_name: (name != effective_name).then_some(effective_name),
					name,
					patterns,
					components,
				}
			})
			.collect::<Vec<_>>();
		append_alias_diagnostics(&mut diagnostics, classification, &used_aliases);
		append_expression_diagnostics(&mut diagnostics, expressions, used_aliases.is_empty());
		return RuleCorpusAnalysis {
			migration_actions: migration_actions(&diagnostics),
			used_aliases,
			diagnostics,
		};
	}

	let used_aliases = direct_alias_names_in_expressions(expressions)
		.into_iter()
		.map(|effective_name| {
			let name = declared_alias_name(&effective_name, fragment, local_aliases);
			RuleAliasUsage {
				effective_name: (name != effective_name).then_some(effective_name),
				name,
				patterns: Vec::new(),
				components: Vec::new(),
			}
		})
		.collect::<Vec<_>>();
	append_expression_diagnostics(&mut diagnostics, expressions, used_aliases.is_empty());
	RuleCorpusAnalysis {
		migration_actions: migration_actions(&diagnostics),
		used_aliases,
		diagnostics,
	}
}

fn append_alias_diagnostics(
	diagnostics: &mut Vec<RuleCorpusDiagnostic>,
	classification: &RuleClassification,
	used_aliases: &[RuleAliasUsage],
) {
	if used_aliases.is_empty() {
		return;
	}
	let mut alias_components: Vec<&String> = Vec::new();
	for alias in used_aliases {
		let alias_name = || Some(alias.name.clone());
		if alias.patterns.is_empty() && alias.components.is_empty() {
			diagnostics.push(diagnostic(
				RuleCorpusDiagnosticCode::AliasHasNoTaxonomyAnchor,
				alias_name(),
				None,
			));
		}
		for anchor in alias.patterns.iter().chain(&alias.components) {
			let present = classification.candidate_patterns.contains(anchor)
				|| classification.candidate_components.contains(anchor);
			if !present {
				diagnostics.push(diagnostic(
					RuleCorpusDiagnosticCode::AliasAnchorMissingFromRuleId,
					alias_name(),
					Some(anchor.clone()),
				));
			}
		}
		for component in &alias.components {
			if !alias_components.contains(&component) {
				alias_components.push(component);
			}
		}
	}
	for component in &classification.candidate_components {
		if !alias_components.contains(&component) {
			diagnostics.push(diagnostic(
				RuleCorpusDiagnosticCode::RuleComponentNotRepresentedByUsedAlias,
				None,
				Some(component.clone()),
			));
		}
	}
}

fn append_expression_diagnostics(
	diagnostics: &mut Vec<RuleCorpusDiagnostic>,
	expressions: &[String],
	uses_no_alias: bool,
) {
	if uses_no_alias {
		diagnostics.push(diagnostic(
			RuleCorpusDiagnosticCode::RuleUsesNoAlias,
			None,
			None,
		));
	}
	if expressions
		.iter()
		.any(|expr| contains_inline_project_selector(expr))
	{
		diagnostics.push(diagnostic(
			RuleCorpusDiagnosticCode::InlineProjectSelectorCandidate,
			None,
			None,
		));
	}
}

fn diagnostic(
	code: RuleCorpusDiagnosticCode,
	alias: Option<String>,
	anchor: Option<String>,
) -> RuleCorpusDiagnostic {
	RuleCorpusDiagnostic {
		code,
		category: code.category(),
		level: code.level(),
		alias,
		anchor,
	}
}

fn migration_actions(diagnostics: &[RuleCorpusDiagnostic]) -> Vec<String> {
	let mut actions = Vec::new();
	for diagnostic in diagnostics {
		let action = diagnostic.code.migration_action();
		if !actions.iter().any(|existing| existing == action) {
			actions.push(action.to_string());
		}
	}
	actions
}

fn direct_alias_names(expr: &str) -> Vec<String> {
	let bytes = expr.as_bytes();
	let mut aliases = Vec::new();
	let mut index = 0;
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
		if end > start {
			let alias = expr[start..end].to_string();
			if !aliases.contains(&alias) {
				aliases.push(alias);
			}
			index = end;
		} else {
			index += 1;
		}
	}
	aliases
}

fn direct_alias_names_in_expressions(expressions: &[String]) -> Vec<String> {
	let mut aliases = Vec::new();
	for expr in expressions {
		for alias in direct_alias_names(expr) {
			if !aliases.contains(&alias) {
				aliases.push(alias);
			}
		}
	}
	aliases
}

fn declared_alias_name(
	effective_name: &str,
	fragment: Option<&str>,
	local_aliases: &[String],
) -> String {
	let Some(fragment) = fragment else {
		return effective_name.to_string();
	};
	let namespace = fragment
		.bytes()
		.map(|byte| {
			if byte.is_ascii_alphanumeric() || byte == b'_' {
				byte as char
			} else {
				'_'
			}
		})
		.collect::<String>();
	local_aliases
		.iter()
		.find(|alias| effective_name == format!("{namespace}_{alias}"))
		.cloned()
		.unwrap_or_else(|| effective_name.to_string())
}

fn contains_inline_project_selector(expr: &str) -> bool {
	super::expr::contains_inline_project_selector(expr)
}

fn rule_origin(source: &RuleSource) -> RuleOrigin {
	RuleOrigin {
		kind: match source.kind {
			RuleSourceKind::EmbeddedDefault => RuleOriginKind::EmbeddedDefault,
			RuleSourceKind::Project => RuleOriginKind::Project,
			RuleSourceKind::Fragment => RuleOriginKind::Fragment,
			RuleSourceKind::External => RuleOriginKind::External,
			RuleSourceKind::Inline => RuleOriginKind::Inline,
		},
		source: source.source.clone(),
		fragment: source.fragment.clone(),
	}
}

pub fn classify_rule_id(id: &str, taxonomy: Option<&RuleTaxonomy>) -> RuleClassification {
	if id.contains('@') && taxonomy.is_none() {
		return RuleClassification {
			id: id.to_string(),
			status: RuleClassificationStatus::Invalid,
			pattern: None,
			components: Vec::new(),
			diagnostics: vec![
				"rule id uses `@` without a declared scoped component taxonomy".to_string(),
			],
			candidate_patterns: Vec::new(),
			candidate_components: Vec::new(),
		};
	}
	let Some(taxonomy) = taxonomy else {
		return RuleClassification {
			id: id.to_string(),
			status: RuleClassificationStatus::TaxonomyNotDeclared,
			pattern: None,
			components: Vec::new(),
			diagnostics: Vec::new(),
			candidate_patterns: Vec::new(),
			candidate_components: Vec::new(),
		};
	};
	let candidate_patterns = maximal_matching_terms(id, &taxonomy.patterns);
	let candidate_components = maximal_matching_terms_in_id_order(id, &taxonomy.components);
	if !is_canonical_rule_id(id) {
		return RuleClassification {
			id: id.to_string(),
			status: RuleClassificationStatus::Invalid,
			pattern: None,
			components: Vec::new(),
			diagnostics: vec![
				"rule id is not canonical kebab-case with optional component@scope anchors"
					.to_string(),
			],
			candidate_patterns,
			candidate_components,
		};
	}
	if !scope_markers_belong_to_declared_components(id, &taxonomy.components) {
		return RuleClassification {
			id: id.to_string(),
			status: RuleClassificationStatus::Invalid,
			pattern: None,
			components: Vec::new(),
			diagnostics: vec!["rule id uses `@` outside a declared scoped component".to_string()],
			candidate_patterns,
			candidate_components,
		};
	}
	let mut diagnostics = Vec::new();
	match candidate_patterns.as_slice() {
		[] => diagnostics.push("rule id must contain exactly one declared pattern".to_string()),
		[_] => {}
		patterns => diagnostics.push(format!(
			"rule id contains multiple declared patterns: {}",
			patterns.join(", ")
		)),
	}
	if candidate_components.is_empty() {
		diagnostics.push("rule id must contain one or more declared components".to_string());
	}
	let status = if diagnostics.is_empty() {
		RuleClassificationStatus::Classified
	} else {
		RuleClassificationStatus::Unclassified
	};
	RuleClassification {
		id: id.to_string(),
		status,
		pattern: (status == RuleClassificationStatus::Classified)
			.then(|| candidate_patterns[0].clone()),
		components: if status == RuleClassificationStatus::Classified {
			candidate_components.clone()
		} else {
			Vec::new()
		},
		diagnostics,
		candidate_patterns,
		candidate_components,
	}
}

fn maximal_matching_terms(id: &str, terms: &[String]) -> Vec<String> {
	maximal_matching_terms_in_words_with(
		&id.split('-').collect::<Vec<_>>(),
		terms,
		id_term_parts,
		false,
	)
}

fn maximal_matching_terms_in_id_order(id: &str, terms: &[String]) -> Vec<String> {
	maximal_matching_terms_in_words_with(
		&id.split('-').collect::<Vec<_>>(),
		terms,
		id_term_parts,
		true,
	)
}

fn maximal_matching_alias_terms(alias: &str, terms: &[String]) -> Vec<String> {
	maximal_matching_terms_in_words_with(
		&alias.split('_').collect::<Vec<_>>(),
		terms,
		alias_term_parts,
		false,
	)
}

fn maximal_matching_terms_in_words_with(
	words: &[&str],
	terms: &[String],
	term_parts: fn(&str) -> Vec<String>,
	preserve_position: bool,
) -> Vec<String> {
	let mut occurrences = Vec::new();
	for term in terms {
		let parts = term_parts(term);
		if parts.len() > words.len() {
			continue;
		}
		for start in 0..=words.len() - parts.len() {
			if words[start..start + parts.len()]
				.iter()
				.copied()
				.eq(parts.iter().map(String::as_str))
			{
				occurrences.push((term, start, start + parts.len()));
			}
		}
	}
	let mut matches = occurrences
		.iter()
		.filter(|(term, start, end)| {
			!occurrences.iter().any(|(other, other_start, other_end)| {
				term != other
					&& other_start <= start
					&& other_end >= end
					&& other_end - other_start > end - start
			})
		})
		.map(|(term, start, _)| (*start, (*term).clone()))
		.collect::<Vec<_>>();
	if preserve_position {
		matches.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
	} else {
		matches.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
	}
	let mut seen = std::collections::HashSet::new();
	matches
		.into_iter()
		.filter_map(|(_, term)| seen.insert(term.clone()).then_some(term))
		.collect()
}

fn id_term_parts(term: &str) -> Vec<String> {
	term.split('-').map(str::to_string).collect()
}

fn alias_term_parts(term: &str) -> Vec<String> {
	taxonomy_alias_anchor(term)
		.split('_')
		.map(str::to_string)
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	fn taxonomy() -> RuleTaxonomy {
		RuleTaxonomy {
			patterns: vec![
				"call-flow".to_string(),
				"dependency".to_string(),
				"dependency-injection".to_string(),
				"hygiene".to_string(),
			],
			components: vec![
				"index".to_string(),
				"roaring-bitmap".to_string(),
				"code".to_string(),
			],
		}
	}

	fn alignment_taxonomy() -> RuleTaxonomy {
		RuleTaxonomy {
			patterns: vec![
				"ownership".to_string(),
				"hygiene".to_string(),
				"dependency".to_string(),
				"dependency-injection".to_string(),
			],
			components: vec![
				"mcp".to_string(),
				"workspace".to_string(),
				"code".to_string(),
			],
		}
	}

	fn analysis_codes(analysis: &RuleCorpusAnalysis) -> Vec<RuleCorpusDiagnosticCode> {
		analysis
			.diagnostics
			.iter()
			.map(|diagnostic| diagnostic.code)
			.collect()
	}

	#[test]
	fn recovers_multi_word_semantic_anchors_anywhere_in_the_id() {
		let result = classify_rule_id(
			"runtime-roaring-bitmap-call-flow-is-backed-by-index",
			Some(&taxonomy()),
		);
		assert_eq!(result.status, RuleClassificationStatus::Classified);
		assert_eq!(result.pattern.as_deref(), Some("call-flow"));
		assert_eq!(result.components, vec!["roaring-bitmap", "index"]);
	}

	#[test]
	fn classifies_hygiene_and_code_without_a_generic_rule_exception() {
		let taxonomy = RuleTaxonomy {
			patterns: vec!["hygiene".to_string()],
			components: vec!["code".to_string()],
		};
		let result = classify_rule_id("hygiene-code-use-specific-names", Some(&taxonomy));
		assert_eq!(result.status, RuleClassificationStatus::Classified);
		assert_eq!(result.pattern.as_deref(), Some("hygiene"));
		assert_eq!(result.components, vec!["code"]);
	}

	#[test]
	fn rejects_ids_with_multiple_semantic_patterns() {
		let taxonomy = RuleTaxonomy {
			patterns: vec!["ownership".to_string(), "dependency".to_string()],
			components: vec!["daemon".to_string()],
		};
		let result = classify_rule_id("prefix-ownership-dependency-daemon", Some(&taxonomy));
		assert_eq!(result.status, RuleClassificationStatus::Unclassified);
		assert_eq!(result.pattern, None);
		assert_eq!(
			result.diagnostics,
			vec!["rule id contains multiple declared patterns: dependency, ownership"]
		);
		assert_eq!(result.candidate_patterns, vec!["dependency", "ownership"]);
		assert_eq!(result.candidate_components, vec!["daemon"]);
	}

	#[test]
	fn reports_missing_pattern_and_component_without_rejecting_the_corpus() {
		let result = classify_rule_id("historical-rule", Some(&taxonomy()));
		assert_eq!(result.status, RuleClassificationStatus::Unclassified);
		assert_eq!(
			result.diagnostics,
			vec![
				"rule id must contain exactly one declared pattern",
				"rule id must contain one or more declared components"
			]
		);
	}

	#[test]
	fn classifies_pattern_and_component_anchors_in_natural_order() {
		let taxonomy = RuleTaxonomy {
			patterns: vec!["ownership".to_string()],
			components: vec!["mcp".to_string()],
		};
		let result = classify_rule_id("mcp-runtime-ownership-is-on-server", Some(&taxonomy));
		assert_eq!(result.status, RuleClassificationStatus::Classified);
		assert_eq!(result.pattern.as_deref(), Some("ownership"));
		assert_eq!(result.components, vec!["mcp"]);
	}

	#[test]
	fn prefers_the_most_specific_declared_pattern() {
		let result = classify_rule_id(
			"mcp-runtime-dependency-injection-uses-daemon-context",
			Some(&RuleTaxonomy {
				patterns: taxonomy().patterns,
				components: vec!["mcp".to_string(), "daemon".to_string()],
			}),
		);
		assert_eq!(result.status, RuleClassificationStatus::Classified);
		assert_eq!(result.pattern.as_deref(), Some("dependency-injection"));
		assert_eq!(result.components, vec!["mcp", "daemon"]);
	}

	#[test]
	fn accepts_components_separated_from_the_pattern_and_each_other() {
		let result = classify_rule_id(
			"graph-corridor-call-flow-uses-roaring-bitmap-index",
			Some(&taxonomy()),
		);
		assert_eq!(result.status, RuleClassificationStatus::Classified);
		assert_eq!(result.pattern.as_deref(), Some("call-flow"));
		assert_eq!(result.components, vec!["roaring-bitmap", "index"]);
		assert_eq!(result.candidate_patterns, vec!["call-flow"]);
		assert_eq!(result.candidate_components, vec!["roaring-bitmap", "index"]);
	}

	#[test]
	fn rejects_a_second_pattern_anywhere_in_the_id() {
		let result = classify_rule_id("hygiene-code-dependency-use-contract", Some(&taxonomy()));
		assert_eq!(result.status, RuleClassificationStatus::Unclassified);
		assert_eq!(result.pattern, None);
		assert!(result.components.is_empty());
		assert_eq!(
			result.diagnostics,
			vec!["rule id contains multiple declared patterns: dependency, hygiene"]
		);
	}

	#[test]
	fn accepts_components_anywhere_in_the_id() {
		let result = classify_rule_id("hygiene-code-use-index", Some(&taxonomy()));
		assert_eq!(result.status, RuleClassificationStatus::Classified);
		assert_eq!(result.pattern.as_deref(), Some("hygiene"));
		assert_eq!(result.components, vec!["code", "index"]);
	}

	#[test]
	fn prefers_the_most_specific_declared_component() {
		let taxonomy = RuleTaxonomy {
			patterns: vec!["ownership".to_string()],
			components: vec!["index".to_string(), "source-index".to_string()],
		};
		let result = classify_rule_id(
			"source-index-ownership-stays-with-workspace",
			Some(&taxonomy),
		);
		assert_eq!(result.status, RuleClassificationStatus::Classified);
		assert_eq!(result.components, vec!["source-index"]);
		assert_eq!(result.candidate_components, vec!["source-index"]);
	}

	#[test]
	fn scoped_components_are_atomic_in_rule_ids_and_aliases() {
		let taxonomy = RuleTaxonomy {
			patterns: vec!["separation-of-concerns".to_string()],
			components: vec![
				"dsl".to_string(),
				"index".to_string(),
				"workspace".to_string(),
				"index@workspace".to_string(),
			],
		};
		let classification = classify_rule_id(
			"dsl-and-index@workspace-separation-of-concerns-preserves-local-evaluation",
			Some(&taxonomy),
		);
		assert_eq!(classification.status, RuleClassificationStatus::Classified);
		assert_eq!(classification.components, vec!["dsl", "index@workspace"]);

		let analysis = analyze_rule(
			"$dsl_evaluation AND $index_at_workspace_target",
			&classification,
			Some(&taxonomy),
			None,
			&[],
		);
		assert_eq!(analysis.used_aliases[0].components, vec!["dsl"]);
		assert_eq!(analysis.used_aliases[1].components, vec!["index@workspace"]);
		assert!(analysis.diagnostics.is_empty(), "{analysis:#?}");
	}

	#[test]
	fn rejects_scope_markers_outside_declared_components() {
		let taxonomy = RuleTaxonomy {
			patterns: vec!["hygiene".to_string()],
			components: vec![
				"dsl".to_string(),
				"index".to_string(),
				"workspace".to_string(),
			],
		};
		let classification =
			classify_rule_id("dsl-index@workspace-hygiene-is-explicit", Some(&taxonomy));
		assert_eq!(classification.status, RuleClassificationStatus::Invalid);
		assert_eq!(
			classification.diagnostics,
			vec!["rule id uses `@` outside a declared scoped component"]
		);
	}

	#[test]
	fn rejects_scope_markers_without_a_taxonomy() {
		let classification = classify_rule_id("index@workspace-ownership-is-explicit", None);
		assert_eq!(classification.status, RuleClassificationStatus::Invalid);
		assert_eq!(
			classification.diagnostics,
			vec!["rule id uses `@` without a declared scoped component taxonomy"]
		);
	}

	#[test]
	fn accepts_multiple_distinct_declared_scoped_components() {
		let taxonomy = RuleTaxonomy {
			patterns: vec!["ownership".to_string()],
			components: vec!["index@workspace".to_string(), "cache@daemon".to_string()],
		};
		let classification = classify_rule_id(
			"index@workspace-and-cache@daemon-ownership-is-explicit",
			Some(&taxonomy),
		);
		assert_eq!(classification.status, RuleClassificationStatus::Classified);
		assert_eq!(
			classification.components,
			vec!["index@workspace", "cache@daemon"]
		);

		let malformed = classify_rule_id(
			"index@workspace@daemon-ownership-is-explicit",
			Some(&taxonomy),
		);
		assert_eq!(malformed.status, RuleClassificationStatus::Invalid);
	}

	#[test]
	fn keeps_a_standalone_nested_pattern_as_a_second_pattern() {
		let result = classify_rule_id(
			"dependency-code-and-dependency-injection",
			Some(&taxonomy()),
		);
		assert_eq!(result.status, RuleClassificationStatus::Unclassified);
		assert_eq!(
			result.candidate_patterns,
			vec!["dependency", "dependency-injection"]
		);
	}

	#[test]
	fn aligns_direct_alias_anchors_without_reading_the_expanded_expression() {
		let taxonomy = alignment_taxonomy();
		let classification =
			classify_rule_id("mcp-runtime-ownership-is-on-server", Some(&taxonomy));
		let analysis = analyze_rule("$mcp_server", &classification, Some(&taxonomy), None, &[]);
		assert_eq!(analysis.used_aliases.len(), 1);
		assert_eq!(analysis.used_aliases[0].name, "mcp_server");
		assert_eq!(analysis.used_aliases[0].components, vec!["mcp"]);
		assert!(analysis.diagnostics.is_empty(), "{analysis:#?}");
	}

	#[test]
	fn reports_alias_anchors_missing_from_the_rule_and_rule_components_missing_from_aliases() {
		let taxonomy = alignment_taxonomy();
		let classification =
			classify_rule_id("mcp-runtime-ownership-is-on-server", Some(&taxonomy));
		let analysis = analyze_rule(
			"$workspace_runtime_target",
			&classification,
			Some(&taxonomy),
			None,
			&[],
		);
		assert_eq!(analysis.used_aliases[0].components, vec!["workspace"]);
		let codes = analysis_codes(&analysis);
		assert!(codes.contains(&RuleCorpusDiagnosticCode::AliasAnchorMissingFromRuleId));
		assert!(codes.contains(&RuleCorpusDiagnosticCode::RuleComponentNotRepresentedByUsedAlias));
		assert!(analysis.diagnostics.iter().any(|diagnostic| {
			diagnostic.anchor.as_deref() == Some("workspace")
				&& diagnostic.alias.as_deref() == Some("workspace_runtime_target")
		}));
	}

	#[test]
	fn generic_aliases_and_rules_without_aliases_are_review_only() {
		let taxonomy = alignment_taxonomy();
		let classification = classify_rule_id("code-hygiene-rejects-placeholders", Some(&taxonomy));
		let generic = analyze_rule(
			"$http_runtime_target",
			&classification,
			Some(&taxonomy),
			None,
			&[],
		);
		assert!(
			analysis_codes(&generic).contains(&RuleCorpusDiagnosticCode::AliasHasNoTaxonomyAnchor)
		);
		assert!(generic.diagnostics.iter().all(|diagnostic| {
			diagnostic.category == RuleCorpusDiagnosticCategory::AliasAlignment
				&& diagnostic.level == RuleCorpusDiagnosticLevel::NeedsReview
		}));

		let no_alias = analyze_rule(
			"name != 'placeholder'",
			&classification,
			Some(&taxonomy),
			None,
			&[],
		);
		assert_eq!(
			analysis_codes(&no_alias),
			vec![RuleCorpusDiagnosticCode::RuleUsesNoAlias]
		);
	}

	#[test]
	fn detects_inline_project_selectors_as_migration_suggestions() {
		let taxonomy = alignment_taxonomy();
		let classification = classify_rule_id("workspace-ownership-stays-local", Some(&taxonomy));
		let analysis = analyze_rule(
			"target ~ '**/external_pkg:other/**'",
			&classification,
			Some(&taxonomy),
			None,
			&[],
		);
		assert!(analysis.diagnostics.iter().any(|diagnostic| {
			diagnostic.code == RuleCorpusDiagnosticCode::InlineProjectSelectorCandidate
				&& diagnostic.category == RuleCorpusDiagnosticCategory::MigrationSuggestion
				&& diagnostic.level == RuleCorpusDiagnosticLevel::NeedsReview
		}));
		assert!(
			analysis
				.migration_actions
				.contains(&"extract-inline-project-selector-into-alias".to_string())
		);

		let alias_only_path = analyze_rule(
			"forbid path from ($workspace_source) to ($workspace_target)",
			&classification,
			Some(&taxonomy),
			None,
			&[],
		);
		assert!(
			!analysis_codes(&alias_only_path)
				.contains(&RuleCorpusDiagnosticCode::InlineProjectSelectorCandidate)
		);
	}

	#[test]
	fn restores_fragment_local_alias_names_before_anchor_analysis() {
		let taxonomy = alignment_taxonomy();
		let classification =
			classify_rule_id("mcp-runtime-ownership-is-on-server", Some(&taxonomy));
		let analysis = analyze_rule(
			"$cli_mcp_mcp_server",
			&classification,
			Some(&taxonomy),
			Some("cli-mcp"),
			&["mcp_server".to_string()],
		);
		assert_eq!(analysis.used_aliases[0].name, "mcp_server");
		assert_eq!(
			analysis.used_aliases[0].effective_name.as_deref(),
			Some("cli_mcp_mcp_server")
		);
		assert_eq!(analysis.used_aliases[0].components, vec!["mcp"]);
	}

	#[test]
	fn keeps_a_global_alias_that_shares_the_fragment_namespace_prefix() {
		let taxonomy = alignment_taxonomy();
		let classification = classify_rule_id("workspace-ownership-stays-local", Some(&taxonomy));
		let analysis = analyze_rule(
			"$cli_mcp_workspace",
			&classification,
			Some(&taxonomy),
			Some("cli-mcp"),
			&["mcp_server".to_string()],
		);
		assert_eq!(analysis.used_aliases[0].name, "cli_mcp_workspace");
		assert_eq!(analysis.used_aliases[0].effective_name, None);
		assert_eq!(
			analysis.used_aliases[0].components,
			vec!["mcp", "workspace"]
		);
	}

	#[test]
	fn inline_selector_candidates_ignore_aliases_literals_and_generic_predicates() {
		let taxonomy = alignment_taxonomy();
		let classification = classify_rule_id("code-hygiene-rejects-placeholders", Some(&taxonomy));
		for expr in [
			"$uri",
			"name = 'uri'",
			"name =~ ^uri$",
			"name =~ ^(uri=foo)$",
			"source.kind = 'test'",
			"target.name = 'call'",
		] {
			let analysis = analyze_rule(expr, &classification, Some(&taxonomy), None, &[]);
			assert!(
				!analysis_codes(&analysis)
					.contains(&RuleCorpusDiagnosticCode::InlineProjectSelectorCandidate),
				"unexpected inline selector diagnostic for {expr}"
			);
		}
	}

	#[test]
	fn inline_selector_candidates_follow_dsl_tokens_across_whitespace() {
		let taxonomy = alignment_taxonomy();
		let classification = classify_rule_id("workspace-ownership-stays-local", Some(&taxonomy));
		for expr in [
			"source   ~ '**/module:workspace/**'",
			"target\t~ '**/module:workspace/**'",
			"any(out_refs,\n target\n ~ '**/module:workspace/**')",
			"name =~ Repository$ disjoint uri ~ '**/dir:domain/**'",
		] {
			let analysis = analyze_rule(expr, &classification, Some(&taxonomy), None, &[]);
			assert!(
				analysis_codes(&analysis)
					.contains(&RuleCorpusDiagnosticCode::InlineProjectSelectorCandidate),
				"missing inline selector diagnostic for {expr}"
			);
		}
	}

	#[test]
	fn inline_selector_candidates_analyze_workspace_group_expressions_separately() {
		let taxonomy = alignment_taxonomy();
		let classification = classify_rule_id("workspace-ownership-stays-local", Some(&taxonomy));
		let analysis = analyze_rule_expressions(
			&[
				"uri ~ '**/dir:workspace/**'".to_string(),
				"member.count > 0".to_string(),
			],
			&classification,
			Some(&taxonomy),
			None,
			&[],
		);
		assert!(
			analysis_codes(&analysis)
				.contains(&RuleCorpusDiagnosticCode::InlineProjectSelectorCandidate)
		);
	}

	#[test]
	fn inline_selector_candidates_analyze_workspace_path_expressions_separately() {
		let taxonomy = alignment_taxonomy();
		let classification = classify_rule_id("workspace-ownership-stays-local", Some(&taxonomy));
		let analysis = analyze_rule_expressions(
			&[
				"$workspace_source".to_string(),
				"target ~ '**/external_pkg:other/**'".to_string(),
				"$workspace_boundary".to_string(),
			],
			&classification,
			Some(&taxonomy),
			None,
			&[],
		);
		assert!(
			analysis_codes(&analysis)
				.contains(&RuleCorpusDiagnosticCode::InlineProjectSelectorCandidate)
		);
		assert_eq!(
			analysis
				.used_aliases
				.iter()
				.map(|alias| alias.name.as_str())
				.collect::<Vec<_>>(),
			vec!["workspace_source", "workspace_boundary"]
		);
	}

	#[test]
	fn alias_anchor_matching_uses_exact_snake_case_terms() {
		let taxonomy = alignment_taxonomy();
		assert_eq!(
			maximal_matching_alias_terms("xmcp_server", &taxonomy.components),
			Vec::<String>::new()
		);
		assert_eq!(
			maximal_matching_alias_terms("dependency_injection_runtime", &taxonomy.patterns),
			vec!["dependency-injection"]
		);
	}
}
