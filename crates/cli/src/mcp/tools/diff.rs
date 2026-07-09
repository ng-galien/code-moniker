use std::fmt::Write as _;

use code_moniker_query::{
	ChangeReviewQuery, ChangeReviewResult, Page, Query, QueryRequest, QueryResult,
};
use serde_json::{Value, json};

use super::{McpTool, ToolDescriptor, ToolError, ToolResult};

use crate::mcp::context::McpContext;

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
					"description": "Bound for listed symbol and reference facts. Defaults 50; truncation is reported."
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

	fn call(&self, context: &McpContext, arguments: &Value) -> Result<ToolResult, ToolError> {
		let detail_refs = arguments
			.get("refs")
			.and_then(Value::as_bool)
			.unwrap_or(false);
		let max_items = arguments
			.get("max_items")
			.and_then(Value::as_u64)
			.map(|value| value as usize)
			.unwrap_or(Self::DEFAULT_MAX_ITEMS);
		let response = context
			.query(QueryRequest {
				query: Query::ChangeReview(ChangeReviewQuery { workspace: None }),
				consistency: code_moniker_query::Consistency::RefreshIfStale,
				page: Page::default(),
			})
			.map_err(ToolError::failed)?;
		let QueryResult::ChangeReview(result) = response.result else {
			return Err(ToolError::failed(anyhow::anyhow!(
				"unexpected change review response"
			)));
		};
		Ok(ToolResult {
			text: render_review(&result, detail_refs, max_items),
			is_error: false,
		})
	}
}

fn render_review(result: &ChangeReviewResult, detail_refs: bool, max_items: usize) -> String {
	let mut out = String::new();
	let _ = writeln!(out, "scope: {}", result.scope);
	let _ = writeln!(
		out,
		"summary: files {} ({} analyzable) symbols {} refs {} ({} retargeted) residual {}",
		result.summary.files,
		result.summary.analyzable_files,
		result.summary.symbol_changes,
		result.summary.ref_changes,
		result.summary.retargeted_refs,
		result.summary.residual_files
	);
	for file in &result.files {
		let _ = writeln!(out, "{}", file_line(file));
	}
	render_symbols(&mut out, result, max_items);
	render_refs(&mut out, result, detail_refs, max_items);
	for diagnostic in &result.diagnostics {
		let _ = writeln!(out, "diagnostic: {diagnostic}");
	}
	out
}

fn file_line(file: &code_moniker_query::ChangeReviewFile) -> String {
	let path = match (&file.old_path, &file.new_path) {
		(Some(old), Some(new)) if old != new => format!("{old} -> {new}"),
		(_, Some(new)) => new.clone(),
		(Some(old), None) => old.clone(),
		(None, None) => "<unknown>".to_string(),
	};
	format!(
		"- {path} {}{}{}",
		file.disposition,
		if file.analyzable {
			""
		} else {
			" (not analyzable)"
		},
		if file.coverage_explained {
			""
		} else {
			" [residual]"
		}
	)
}

fn render_symbols(out: &mut String, result: &ChangeReviewResult, max_items: usize) {
	for change in result.symbol_changes.iter().take(max_items) {
		let Some(side) = change.new.as_ref().or(change.old.as_ref()) else {
			continue;
		};
		let _ = writeln!(
			out,
			"  {} {} {} [{}]",
			change.kind, side.kind, side.name, change.confidence
		);
	}
	if result.symbol_changes.len() > max_items {
		let _ = writeln!(
			out,
			"  truncated: +{} symbol fact(s)",
			result.symbol_changes.len() - max_items
		);
	}
}

fn render_refs(out: &mut String, result: &ChangeReviewResult, detail_refs: bool, max_items: usize) {
	if !detail_refs {
		return;
	}
	for change in result.ref_changes.iter().take(max_items) {
		let _ = writeln!(out, "  {} {} {}", change.kind, change.ref_kind, change.file);
	}
	if result.ref_changes.len() > max_items {
		let _ = writeln!(
			out,
			"  truncated: +{} ref fact(s)",
			result.ref_changes.len() - max_items
		);
	}
}
