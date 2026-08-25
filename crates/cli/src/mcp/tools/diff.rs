use code_moniker_query::{
	ChangeReviewQuery, ChangeReviewRef, ChangeReviewResult, ChangeReviewSymbol, Page, Query,
	QueryResult,
};
use serde::Serialize;
use serde_json::{Value, json};

use super::common::{AgentOutputOptions, OutputBudget};
use super::scope::{append_call_bool_arg, append_call_number_arg, append_call_string_arg};
use super::{McpTool, OutputContract, OutputOptions, ToolDescriptor, ToolError, ToolResult};

use crate::mcp::context::McpContext;
use crate::presentation::TemplateOutput;
use crate::presentation::relationships as relationship_presentation;

pub(super) struct DiffTool;

impl DiffTool {
	pub(super) const NAME: &'static str = "code_moniker_diff";

	const DESCRIPTION: &'static str = concat!(
		"When to use: read the current git changes of the workspace as symbol-level facts ",
		"instead of line hunks - moved or renamed symbols, modified bodies, retargeted ",
		"imports and call sites, and residual (unattributed) edits.\n",
		"\n",
		"Semantic change review from code-moniker (scope HEAD..worktree).\n",
		"Facts only: kinds added/removed/body-modified/signature-changed/renamed/moved/",
		"attribute-changed with certain/candidate confidence, per-file dispositions and ",
		"hunk coverage. No importance judgment is applied."
	);

	const DEFAULT_MAX_ITEMS: usize = 50;

	fn input_schema() -> Value {
		json!({
			"type": "object",
			"properties": {
				"refs": {
					"type": "boolean",
					"description": "List individual reference facts instead of the collapsed count. Defaults false."
				},
				"max_items": {
					"type": "integer",
					"minimum": 1,
					"maximum": 500,
					"description": "Explicit bound for listed symbol and reference facts. Otherwise the volume profile selects 50, 150, or 500 facts; omitted facts produce a continuation."
				}
			},
			"additionalProperties": false
		})
	}
}

impl McpTool for DiffTool {
	fn descriptor(&self) -> ToolDescriptor {
		ToolDescriptor {
			name: Self::NAME,
			description: Self::DESCRIPTION,
			input_schema: Self::input_schema(),
		}
	}

	fn output_contract(&self) -> OutputContract {
		OutputContract::Agent
	}

	fn call(
		&self,
		context: &McpContext,
		arguments: &Value,
		output: OutputOptions,
	) -> Result<ToolResult, ToolError> {
		let output = output.agent_options();
		let detail_refs = optional_bool(arguments, "refs")?.unwrap_or(false);
		let max_items = diff_max_items(arguments, output.budget)?;
		let response = context
			.query_refreshed(
				Query::ChangeReview(ChangeReviewQuery { workspace: None }),
				Page::default(),
			)
			.map_err(ToolError::failed)?;
		let QueryResult::ChangeReview(result) = response.result else {
			return Err(ToolError::failed(anyhow::anyhow!(
				"unexpected change review response"
			)));
		};
		diff_template(&result, detail_refs, max_items, output)
			.map(ToolResult::templated)
			.map_err(ToolError::failed)
	}
}

fn diff_max_items(arguments: &Value, budget: OutputBudget) -> Result<usize, ToolError> {
	let volume_limit = diff_volume_limit(budget);
	Ok(optional_max_items(arguments)?
		.unwrap_or(volume_limit)
		.min(volume_limit))
}

fn optional_bool(arguments: &Value, name: &str) -> Result<Option<bool>, ToolError> {
	match arguments.get(name) {
		Some(Value::Bool(value)) => Ok(Some(*value)),
		Some(_) => Err(ToolError::failed(format!("{name} must be a boolean"))),
		None => Ok(None),
	}
}

fn optional_max_items(arguments: &Value) -> Result<Option<usize>, ToolError> {
	let Some(value) = arguments.get("max_items") else {
		return Ok(None);
	};
	let Some(value) = value.as_u64() else {
		return Err(ToolError::failed("max_items must be an unsigned integer"));
	};
	let value = value as usize;
	if !(1..=500).contains(&value) {
		return Err(ToolError::failed("max_items must be between 1 and 500"));
	}
	Ok(Some(value))
}

fn diff_volume_limit(budget: OutputBudget) -> usize {
	match budget {
		OutputBudget::Small => DiffTool::DEFAULT_MAX_ITEMS,
		OutputBudget::Medium => 150,
		OutputBudget::Full => 500,
	}
}

#[derive(Serialize)]
struct DiffView<'a> {
	volume: &'static str,
	max_items: usize,
	result: &'a ChangeReviewResult,
	files: Vec<DiffFileView<'a>>,
	files_omitted: usize,
	symbols: Vec<DiffSymbolView<'a>>,
	symbols_omitted: usize,
	refs: Option<&'a [ChangeReviewRef]>,
	refs_omitted: usize,
	next_call: Option<DiffNextCall>,
}

#[derive(Serialize)]
struct DiffFileView<'a> {
	path: String,
	disposition: &'a str,
	analyzable: bool,
	coverage_explained: bool,
}

#[derive(Serialize)]
struct DiffSymbolView<'a> {
	change_kind: &'a str,
	symbol_kind: &'a str,
	identity: &'a str,
	confidence: &'a str,
}

#[derive(Serialize)]
struct DiffNextCall {
	arguments: String,
}

fn diff_template(
	result: &ChangeReviewResult,
	detail_refs: bool,
	max_items: usize,
	output: AgentOutputOptions,
) -> anyhow::Result<TemplateOutput> {
	let files = result
		.files
		.iter()
		.take(max_items)
		.map(diff_file_view)
		.collect::<Vec<_>>();
	let files_omitted = result.files.len().saturating_sub(max_items);
	let symbols = result
		.symbol_changes
		.iter()
		.take(max_items)
		.filter_map(diff_symbol_view)
		.collect::<Vec<_>>();
	let symbols_omitted = result.symbol_changes.len().saturating_sub(max_items);
	let refs = detail_refs.then(|| &result.ref_changes[..result.ref_changes.len().min(max_items)]);
	let refs_omitted = if detail_refs {
		result.ref_changes.len().saturating_sub(max_items)
	} else {
		0
	};
	let next_call = diff_next_call(
		detail_refs,
		max_items,
		output,
		files_omitted > 0 || symbols_omitted > 0 || refs_omitted > 0,
	);
	let view = DiffView {
		volume: output.budget.as_str(),
		max_items,
		result,
		files,
		files_omitted,
		symbols,
		symbols_omitted,
		refs,
		refs_omitted,
		next_call,
	};
	relationship_presentation::diff(&view)
}

fn diff_file_view(file: &code_moniker_query::ChangeReviewFile) -> DiffFileView<'_> {
	let path = match (&file.old_path, &file.new_path) {
		(Some(old), Some(new)) if old != new => format!("{old} -> {new}"),
		(_, Some(new)) => new.to_string(),
		(Some(old), None) => old.to_string(),
		(None, None) => "unknown".to_string(),
	};
	DiffFileView {
		path,
		disposition: &file.disposition,
		analyzable: file.analyzable,
		coverage_explained: file.coverage_explained,
	}
}

fn diff_symbol_view(change: &ChangeReviewSymbol) -> Option<DiffSymbolView<'_>> {
	let side = change.new.as_ref().or(change.old.as_ref())?;
	Some(DiffSymbolView {
		change_kind: &change.kind,
		symbol_kind: &side.kind,
		identity: &side.identity,
		confidence: &change.confidence,
	})
}

fn diff_next_call(
	detail_refs: bool,
	max_items: usize,
	output: AgentOutputOptions,
	incomplete: bool,
) -> Option<DiffNextCall> {
	if !incomplete || max_items >= 500 {
		return None;
	}
	let next_budget = match output.budget {
		OutputBudget::Small => OutputBudget::Medium,
		OutputBudget::Medium | OutputBudget::Full => OutputBudget::Full,
	};
	let next_limit = max_items
		.saturating_mul(2)
		.max(diff_volume_limit(next_budget))
		.min(500);
	let mut arguments = String::new();
	append_call_bool_arg(&mut arguments, "refs", detail_refs);
	append_call_number_arg(&mut arguments, "max_items", next_limit);
	append_call_bool_arg(&mut arguments, "compact", output.compact);
	append_call_string_arg(&mut arguments, "budget", next_budget.as_str());
	Some(DiffNextCall { arguments })
}

#[cfg(test)]
mod tests {
	use code_moniker_query::{ChangeReviewSide, ChangeReviewSymbol};

	use super::*;
	use crate::presentation::RenderOptions;

	fn added_method(identity: &str) -> ChangeReviewSymbol {
		ChangeReviewSymbol {
			kind: "added".to_string(),
			confidence: "certain".to_string(),
			body_changed: false,
			signature_changed: false,
			visibility_changed: false,
			header_changed: false,
			file_moved: false,
			old: None,
			new: Some(ChangeReviewSide {
				identity: identity.to_string(),
				file: "crates/check/src/check/command.rs".to_string(),
				kind: "method".to_string(),
				name: "source_catalog(root:&Path)".to_string(),
				visibility: "private".to_string(),
				lines: None,
				test_artifact: false,
			}),
		}
	}

	#[test]
	fn symbol_facts_carry_their_identity() {
		let result = ChangeReviewResult {
			scope: "HEAD..worktree".to_string(),
			summary: Default::default(),
			files: Vec::new(),
			symbol_changes: vec![
				added_method(
					"module:command/struct:FsCheckWorkspace/method:source_catalog(root:&Path)",
				),
				added_method(
					"module:command/struct:MemoryCheckWorkspace/method:source_catalog(root:&Path)",
				),
			],
			ref_changes: Vec::new(),
			diagnostics: Vec::new(),
		};
		let out = diff_template(
			&result,
			false,
			10,
			AgentOutputOptions {
				compact: false,
				budget: OutputBudget::Small,
			},
		)
		.expect("diff template")
		.render(RenderOptions {
			compact: false,
			scheme: "code+moniker://",
			runtime: None,
		})
		.expect("render diff");
		crate::presentation::tests::validate_agent_markdown(&out, "Semantic diff", false)
			.expect("valid diff Markdown");
		assert!(
			out.contains("struct:FsCheckWorkspace/method:source_catalog"),
			"each symbol fact must carry its identity, got:\n{out}"
		);
		assert!(
			out.contains("struct:MemoryCheckWorkspace/method:source_catalog"),
			"same-name facts must stay distinguishable, got:\n{out}"
		);
	}

	#[test]
	fn diff_volume_profiles_bound_fact_projection_before_rendering() {
		assert_eq!(diff_volume_limit(OutputBudget::Small), 50);
		assert_eq!(diff_volume_limit(OutputBudget::Medium), 150);
		assert_eq!(diff_volume_limit(OutputBudget::Full), 500);
		assert_eq!(
			super::diff_max_items(&serde_json::json!({"max_items": 500}), OutputBudget::Small)
				.expect("small cap"),
			50
		);
	}
}
