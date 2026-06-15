use std::collections::BTreeMap;

use code_moniker_query::{
	Query, QueryRequest, QueryResult, SymbolDetailResult, TreeChildrenQuery, TreeChildrenResult,
	ViewBoundaryDto, ViewDetailResult, ViewEvidenceDto, ViewGotchaDto, ViewListResult,
	ViewReadQuery, ViewReadResult, ViewRuleDto, ViewRuleRefDto,
};
use code_moniker_workspace::snapshot::{SourceCatalog, SourceFileRecord, SourceUnit, SymbolRecord};
use serde_json::{Value, json};

use super::common::{is_workspace_uri, normalize_workspace_uri};
use super::scope::{
	Paging, ScopeFilter, append_call_cursor_arg, append_call_number_arg, append_call_string_arg,
	path_prefix,
};
use super::{McpTool, ToolDescriptor, ToolError, ToolResult};
use crate::language_kinds;
use crate::mcp::context::McpContext;
use crate::views::{self, MonikerDisplay};

const DEFAULT_READ_URI: &str = "workspace";
const MAX_DEPTH: usize = 20;

pub(in crate::mcp) struct ReadTool;

impl ReadTool {
	pub(super) const NAME: &'static str = "code_moniker_read";

	const DESCRIPTION: &'static str = concat!(
		"When to use: default entry point to explore the current code-moniker UI workspace. ",
		"The same verb starts at the workspace root, expands an explorer tree, or reads code from an exact symbol URI.\n",
		"\n",
		"Read from code-moniker.\n",
		"  workspace                — workspace summary, language vocabulary, concentration indicators, and explorer page\n",
		"  workspace/views          — project-defined contextual views for agents\n",
		"  code+moniker://workspace — same root with an explicit URI\n",
		"  code+moniker://...       — symbol URI returned by code_moniker_symbols; reads the source slice around that symbol\n",
		"Use path/lang to scope discovery, depth to expand the explorer, limit/cursor for paging, and moniker_format when a view should expose resolved monikers. Pair with code_moniker_symbols when you need symbol rows."
	);

	fn input_schema() -> Value {
		json!({
			"type": "object",
			"properties": {
				"uri": {
					"type": "string",
					"description": "workspace | code+moniker://workspace | exact symbol URI returned by code_moniker_symbols"
				},
				"depth": {
					"type": "integer",
					"minimum": 0,
					"maximum": MAX_DEPTH,
					"description": "Explorer depth to render."
				},
				"path": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Relative file glob(s), OR-combined. Example: crates/cli/src/mcp/**"
				},
				"lang": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Language tag(s), OR-combined. Example: rs, java"
				},
				"limit": {
					"type": "integer",
					"minimum": 1,
					"maximum": super::scope::MAX_LIMIT,
					"description": "Maximum explorer rows to emit."
				},
				"cursor": {
					"oneOf": [{ "type": "integer" }, { "type": "string" }],
					"description": "Opaque row offset returned in next calls."
				},
				"context_lines": {
					"type": "integer",
					"minimum": 0,
					"maximum": 20,
					"description": "Extra lines around a symbol source slice."
				},
				"moniker_format": {
					"type": "string",
					"enum": ["none", "compact", "uri"],
					"description": "For workspace/views reads, optionally display resolved evidence monikers."
				},
				"include_code": {
					"type": "boolean",
					"description": "For workspace/views reads, include source snippets for resolved evidence."
				}
			},
			"required": ["uri"],
			"additionalProperties": false
		})
	}
}

impl McpTool for ReadTool {
	fn descriptor(&self) -> ToolDescriptor {
		ToolDescriptor {
			name: Self::NAME,
			description: Self::DESCRIPTION,
			input_schema: Self::input_schema(),
		}
	}

	fn call(&self, context: &McpContext, arguments: &Value) -> Result<ToolResult, ToolError> {
		let request = ReadRequest::from_arguments(arguments).map_err(ToolError::failed)?;
		let text = read_resource(context, &request).map_err(ToolError::failed)?;
		Ok(ToolResult {
			text,
			is_error: false,
		})
	}
}

struct ReadRequest {
	uri: String,
	depth: usize,
	context_lines: usize,
	include_code: bool,
	moniker_display: MonikerDisplay,
	scope: ScopeFilter,
	paging: Paging,
}

impl ReadRequest {
	fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		Ok(Self {
			uri: arguments
				.get("uri")
				.and_then(Value::as_str)
				.unwrap_or(DEFAULT_READ_URI)
				.to_string(),
			depth: arguments
				.get("depth")
				.and_then(Value::as_u64)
				.unwrap_or(2)
				.min(MAX_DEPTH as u64) as usize,
			context_lines: arguments
				.get("context_lines")
				.and_then(Value::as_u64)
				.unwrap_or(2)
				.min(20) as usize,
			include_code: arguments
				.get("include_code")
				.and_then(Value::as_bool)
				.unwrap_or(false),
			moniker_display: MonikerDisplay::parse(
				arguments.get("moniker_format").and_then(Value::as_str),
			)?,
			scope: ScopeFilter::from_arguments(arguments)?,
			paging: Paging::from_arguments(arguments)?,
		})
	}
}

fn read_resource(context: &McpContext, request: &ReadRequest) -> anyhow::Result<String> {
	if is_workspace_uri(&request.uri, context.scheme(), DEFAULT_READ_URI) {
		return read_workspace(
			context,
			&request.uri,
			request.depth,
			&request.scope,
			request.paging,
		);
	}
	if views::is_views_uri(&request.uri, context.scheme()) {
		return read_view(
			context,
			&request.uri,
			request.context_lines,
			request.include_code,
			request.moniker_display,
		);
	}
	read_symbol(context, &request.uri, request.context_lines)
}

fn read_workspace(
	context: &McpContext,
	uri: &str,
	depth: usize,
	scope: &ScopeFilter,
	paging: Paging,
) -> anyhow::Result<String> {
	let response = context.query(QueryRequest {
		query: Query::TreeChildren(TreeChildrenQuery {
			workspace: None,
			path: scope.paths.clone(),
			depth,
			lang: scope.langs.clone(),
			projection: Vec::new(),
		}),
		consistency: code_moniker_query::Consistency::Current,
		page: paging.daemon_page(),
	})?;
	let QueryResult::TreeChildren(result) = response.result else {
		anyhow::bail!("unexpected daemon response for workspace read");
	};
	Ok(render_daemon_explorer_lmnav(DaemonExplorerRender {
		scheme: context.scheme(),
		request_uri: uri,
		depth,
		scope,
		paging,
		next_cursor: response.next_cursor.as_ref(),
		result: &result,
	}))
}

fn read_symbol(context: &McpContext, uri: &str, context_lines: usize) -> anyhow::Result<String> {
	let response = context.query(QueryRequest::new(Query::SymbolDetail(
		code_moniker_query::SymbolDetailQuery {
			workspace: None,
			uri: uri.to_string(),
			context_lines,
		},
	)))?;
	let QueryResult::SymbolDetail(result) = response.result else {
		anyhow::bail!("unexpected daemon response for symbol read");
	};
	Ok(render_daemon_symbol_source_lmnav(context.scheme(), &result))
}

fn read_view(
	context: &McpContext,
	uri: &str,
	context_lines: usize,
	include_code: bool,
	moniker_display: MonikerDisplay,
) -> anyhow::Result<String> {
	let response = context.query(QueryRequest::new(Query::ViewRead(ViewReadQuery {
		uri: uri.to_string(),
		scheme: Some(context.scheme().to_string()),
		context_lines,
		include_code,
	})))?;
	let QueryResult::ViewRead(result) = response.result else {
		anyhow::bail!("unexpected daemon response for view read");
	};
	Ok(render_daemon_view_lmnav(
		context.scheme(),
		&result,
		moniker_display,
	))
}

const VIEWS_URI: &str = "workspace/views";

fn render_daemon_view_lmnav(
	scheme: &str,
	result: &ViewReadResult,
	moniker_display: MonikerDisplay,
) -> String {
	match result {
		ViewReadResult::List(list) => render_view_list(scheme, list),
		ViewReadResult::Detail(detail) => render_view_detail(scheme, detail, moniker_display),
	}
}

fn render_view_list(scheme: &str, list: &ViewListResult) -> String {
	let mut output = String::new();
	output.push_str(&format!("uri: {scheme}{VIEWS_URI}\n"));
	output.push_str("completeness: full\n");
	output.push_str(&format!("views: {}\n\n", list.views.len()));
	output.push_str("views:\n");
	if list.views.is_empty() {
		output.push_str("  <empty>\n");
	} else {
		for view in &list.views {
			output.push_str(&format!("  - {}\n", view.id));
			if let Some(title) = &view.title {
				output.push_str(&format!("    title: {title}\n"));
			}
			output.push_str(&format!("    fragment: {}\n", view.fragment));
			output.push_str(&format!("    anchor: {}\n", view.anchor));
			output.push_str(&format!("    scope: {}\n", view_scope_label(&view.scope)));
		}
	}
	output.push_str("\nnext:\n");
	for view in list.views.iter().take(12) {
		output.push_str(&format!(
			"  - code_moniker_read uri=\"{scheme}{VIEWS_URI}/{}\"\n",
			view.id
		));
	}
	output
}

fn render_view_detail(
	scheme: &str,
	detail: &ViewDetailResult,
	moniker_display: MonikerDisplay,
) -> String {
	let mut output = String::new();
	render_view_header(&mut output, scheme, detail);
	render_view_rule_catalog(&mut output, &detail.rules);
	render_view_boundaries(&mut output, detail, moniker_display);
	render_view_gotchas(&mut output, detail, moniker_display);
	render_view_next(&mut output, scheme, detail);
	output
}

fn render_view_header(output: &mut String, scheme: &str, detail: &ViewDetailResult) {
	output.push_str(&format!("uri: {scheme}{VIEWS_URI}/{}\n", detail.id));
	output.push_str("completeness: full\n");
	output.push_str(&format!("view: {}\n", detail.id));
	if let Some(title) = &detail.title {
		output.push_str(&format!("title: {title}\n"));
	}
	output.push_str(&format!("fragment: {}\n", detail.fragment));
	output.push_str(&format!("anchor: {}\n", detail.anchor));
	output.push_str(&format!("scope: {}\n", view_scope_label(&detail.scope)));
	if let Some(intent) = &detail.intent {
		output.push_str(&format!("intent: {intent}\n"));
	}
	if let Some(summary) = &detail.summary {
		output.push_str("\nsummary:\n");
		render_view_text_block(output, summary, "  ");
	}
}

fn render_view_rule_catalog(output: &mut String, rules: &[ViewRuleDto]) {
	if rules.is_empty() {
		return;
	}
	output.push_str("\nrules:\n");
	for rule in rules {
		output.push_str(&format!(
			"  - {} [{}] domain={}\n",
			rule.id, rule.severity, rule.domain
		));
		if let Some(rationale) = &rule.rationale {
			output.push_str("    rationale:\n");
			render_view_text_block(output, rationale, "      ");
		}
	}
}

fn render_view_boundaries(
	output: &mut String,
	detail: &ViewDetailResult,
	moniker_display: MonikerDisplay,
) {
	output.push_str("\nboundaries:\n");
	if detail.boundaries.is_empty() {
		output.push_str("  <empty>\n");
		return;
	}
	for boundary in &detail.boundaries {
		render_view_boundary(output, boundary, moniker_display);
	}
}

fn render_view_boundary(
	output: &mut String,
	boundary: &ViewBoundaryDto,
	moniker_display: MonikerDisplay,
) {
	output.push_str(&format!("  - {}\n", boundary.id));
	render_view_list_block(output, "owns", &boundary.owns, "    ");
	render_view_forbids(output, boundary, "    ");
	if let Some(rationale) = &boundary.rationale {
		output.push_str("    rationale:\n");
		render_view_text_block(output, rationale, "      ");
	}
	render_view_rule_refs(output, "rules", &boundary.rule_refs, "    ");
	render_view_evidence(
		output,
		&boundary.evidence,
		&boundary.missing,
		moniker_display,
		"    ",
	);
}

fn render_view_gotchas(
	output: &mut String,
	detail: &ViewDetailResult,
	moniker_display: MonikerDisplay,
) {
	output.push_str("\ngotchas:\n");
	if detail.gotchas.is_empty() {
		output.push_str("  <empty>\n");
		return;
	}
	for gotcha in &detail.gotchas {
		render_view_gotcha(output, gotcha, moniker_display);
	}
}

fn render_view_gotcha(
	output: &mut String,
	gotcha: &ViewGotchaDto,
	moniker_display: MonikerDisplay,
) {
	output.push_str(&format!("  - {}\n", gotcha.id));
	output.push_str("    rationale:\n");
	render_view_text_block(output, &gotcha.rationale, "      ");
	if let Some(check) = &gotcha.check {
		output.push_str(&format!("    check: {check}\n"));
	}
	render_view_rule_refs(output, "rules", &gotcha.rule_refs, "    ");
	render_view_evidence(
		output,
		&gotcha.evidence,
		&gotcha.missing,
		moniker_display,
		"    ",
	);
}

fn render_view_evidence(
	output: &mut String,
	evidence: &[ViewEvidenceDto],
	missing: &[String],
	moniker_display: MonikerDisplay,
	indent: &str,
) {
	if evidence.is_empty() && missing.is_empty() {
		return;
	}
	output.push_str(indent);
	output.push_str("evidence:\n");
	for item in evidence {
		render_view_evidence_item(output, item, moniker_display, indent);
	}
	for selector in missing {
		output.push_str(indent);
		output.push_str(&format!("  - selector: {selector}\n"));
		output.push_str(indent);
		output.push_str("    status: missing\n");
	}
}

fn render_view_evidence_item(
	output: &mut String,
	item: &ViewEvidenceDto,
	moniker_display: MonikerDisplay,
	indent: &str,
) {
	output.push_str(indent);
	output.push_str(&format!("  - selector: {}\n", item.selector));
	output.push_str(indent);
	output.push_str(&format!("    label: {}\n", item.label));
	if let Some(moniker) = moniker_display.render(&item.moniker) {
		output.push_str(indent);
		output.push_str(&format!("    moniker: {moniker}\n"));
	}
	output.push_str(indent);
	output.push_str(&format!("    file: {}\n", item.file));
	if let Some((start, end)) = item.slice {
		output.push_str(indent);
		output.push_str(&format!("    slice: L{start}-L{end}\n"));
	}
	if !item.code.is_empty() {
		output.push_str(indent);
		output.push_str("    code:\n");
		for line in &item.code {
			let marker = if item
				.active_slice
				.is_some_and(|(start, end)| start <= line.number && line.number <= end)
			{
				">"
			} else {
				" "
			};
			output.push_str(indent);
			output.push_str(&format!(
				"      {marker} {:>4} | {}\n",
				line.number, line.text
			));
		}
	}
}

fn render_view_rule_refs(
	output: &mut String,
	label: &str,
	rule_refs: &[ViewRuleRefDto],
	indent: &str,
) {
	if rule_refs.is_empty() {
		return;
	}
	output.push_str(indent);
	output.push_str(label);
	output.push_str(":\n");
	for rule_ref in rule_refs {
		output.push_str(indent);
		if rule_ref.present {
			output.push_str(&format!("  - {}\n", rule_ref.id));
		} else {
			output.push_str(&format!("  - {} [missing]\n", rule_ref.id));
		}
	}
}

fn render_view_forbids(output: &mut String, boundary: &ViewBoundaryDto, indent: &str) {
	if boundary.forbids.is_empty() {
		return;
	}
	output.push_str(indent);
	output.push_str("forbids:\n");
	for value in &boundary.forbids {
		output.push_str(indent);
		output.push_str(&format!("  - {value}\n"));
	}
	output.push_str(indent);
	if boundary.forbid_rules.is_empty() {
		output.push_str("forbids_status: advisory\n");
	} else {
		output.push_str("forbids_status: enforced_by_rules\n");
		render_view_list_block(output, "forbid_rules", &boundary.forbid_rules, indent);
	}
}

fn render_view_list_block(output: &mut String, label: &str, values: &[String], indent: &str) {
	if values.is_empty() {
		return;
	}
	output.push_str(indent);
	output.push_str(label);
	output.push_str(":\n");
	for value in values {
		output.push_str(indent);
		output.push_str(&format!("  - {value}\n"));
	}
}

fn render_view_text_block(output: &mut String, text: &str, indent: &str) {
	for line in text.trim().lines() {
		output.push_str(indent);
		output.push_str(line.trim());
		output.push('\n');
	}
}

fn render_view_next(output: &mut String, scheme: &str, detail: &ViewDetailResult) {
	output.push_str("\nnext:\n");
	output.push_str(&format!(
		"  - code_moniker_symbols uri=\"{scheme}workspace\" path=\"{}**\" limit=50\n",
		view_next_scope_path(&detail.scope)
	));
	output.push_str(&format!(
		"  - code_moniker_rules uri=\"{scheme}workspace\" action=\"list\" limit=50\n"
	));
}

fn view_scope_label(scope: &str) -> &str {
	if scope.is_empty() { "." } else { scope }
}

fn view_next_scope_path(scope: &str) -> String {
	if scope.is_empty() {
		String::new()
	} else {
		format!("{scope}/")
	}
}

fn render_daemon_symbol_source_lmnav(scheme: &str, result: &SymbolDetailResult) -> String {
	let symbol = &result.symbol;
	let mut output = String::new();
	output.push_str(&format!("uri: {}\n", symbol.uri));
	if result.source.is_some() {
		output.push_str("completeness: full\n");
	} else {
		output.push_str(
			"completeness: partial (symbol has no line range; showing first available lines)\n",
		);
	}
	output.push_str(&format!("file: {}\n", symbol.file));
	output.push_str(&format!("language: {}\n", symbol.language));
	output.push_str(&format!("kind: {}\n", symbol.kind));
	output.push_str(&format!("name: {}\n", symbol.name));
	if let Some((start, end)) = symbol.line_range {
		output.push_str(&format!("range: {start}-{end}\n"));
	}
	if let Some(source) = &result.source {
		output.push_str(&format!(
			"slice: {}-{}\n\n",
			source.first_line, source.last_line
		));
		output.push_str("code:\n");
		for line in &source.lines {
			output.push_str(&format!("  {:>4} | {}\n", line.number, line.text));
		}
	}
	output.push_str("\nnext:\n");
	output.push_str(&format!(
		"  - code_moniker_symbols uri=\"{scheme}workspace\""
	));
	append_call_string_arg(&mut output, "name", &symbol.name);
	append_call_number_arg(&mut output, "limit", 20);
	output.push('\n');
	output.push_str(&format!(
		"  - code_moniker_symbols uri=\"{scheme}workspace\""
	));
	append_call_string_arg(&mut output, "path", &symbol.file);
	append_call_number_arg(&mut output, "limit", 50);
	output.push('\n');
	output
}

struct DaemonExplorerRender<'a> {
	scheme: &'a str,
	request_uri: &'a str,
	depth: usize,
	scope: &'a ScopeFilter,
	paging: Paging,
	next_cursor: Option<&'a code_moniker_query::QueryCursor>,
	result: &'a TreeChildrenResult,
}

fn render_daemon_explorer_lmnav(render: DaemonExplorerRender<'_>) -> String {
	let uri = normalize_workspace_uri(render.scheme, render.request_uri, DEFAULT_READ_URI);
	let mut output = String::new();
	output.push_str(&format!("uri: {uri}\n"));
	if let Some(next) = render.next_cursor {
		output.push_str(&format!(
			"completeness: partial (explorer rows {}-{} of {}, next cursor {})\n",
			render.paging.cursor,
			render.paging.cursor + render.result.rows.len(),
			render.result.total,
			next.offset
		));
	} else {
		output.push_str("completeness: full\n");
	}
	output.push_str(&format!("files: {}\n", render.result.scoped_files));
	output.push_str(&format!("files_total: {}\n", render.result.total_files));
	output.push_str(&format!("depth: {}\n\n", render.depth));
	output.push_str("scope:\n");
	for line in render.scope.describe() {
		output.push_str(&line);
		output.push('\n');
	}
	output.push('\n');
	output.push_str("summary:\n");
	output.push_str("  languages:\n");
	for language in &render.result.languages {
		output.push_str(&format!("    {}: {}\n", language.name, language.count));
	}
	output.push_str("  concentration:\n");
	for prefix in &render.result.prefixes {
		output.push_str(&format!("    {}: {} files\n", prefix.name, prefix.count));
	}
	output.push_str("  hints:\n");
	output.push_str("    start with code_moniker_symbols using path/lang/kind/shape filters before broad symbol reads\n\n");
	output.push_str("explorer:\n");
	if render.result.rows.is_empty() {
		output.push_str("  <empty>\n");
	} else {
		for row in &render.result.rows {
			output.push_str("  ");
			output.push_str(&explorer_row_label(row));
			output.push('\n');
		}
	}
	output.push_str("\nnext:\n");
	if let Some(next) = render.next_cursor {
		output.push_str(&format!(
			"  - code_moniker_read uri=\"{}workspace\"",
			render.scheme
		));
		render.scope.append_call_args(&mut output);
		append_call_number_arg(&mut output, "depth", render.depth);
		append_call_number_arg(&mut output, "limit", render.paging.limit);
		append_call_cursor_arg(&mut output, "cursor", next);
		output.push('\n');
	}
	append_read_next_call(
		&mut output,
		render.scheme,
		render.scope,
		(render.depth + 1).min(MAX_DEPTH),
		render.paging.limit,
		None,
	);
	append_symbols_call(&mut output, render.scheme, render.scope, 50);
	output
}

fn explorer_row_label(row: &code_moniker_query::TreeNode) -> String {
	match row.kind {
		code_moniker_query::TreeNodeKind::Directory => {
			format!("{}/ defs {} refs {}", row.path, row.defs, row.refs)
		}
		code_moniker_query::TreeNodeKind::File => {
			let language = row.language.as_deref().unwrap_or("?");
			format!(
				"{} [{}] defs {} refs {}",
				row.path, language, row.defs, row.refs
			)
		}
	}
}

pub(in crate::mcp) fn render_symbol_source_lmnav(
	scheme: &str,
	symbol: &SymbolRecord,
	source: &SourceFileRecord,
	source_text: &str,
	context_lines: usize,
) -> String {
	let total_lines = source_text.lines().count().max(1);
	let (raw_start, raw_end) = symbol
		.line_range
		.map(|(start, end)| (start.max(1) as usize, end.max(start).max(1) as usize))
		.unwrap_or((1, total_lines.min(80)));
	let target_start = raw_start.min(total_lines);
	let target_end = raw_end.min(total_lines).max(target_start);
	let slice_start = target_start.saturating_sub(context_lines).max(1);
	let slice_end = target_end.saturating_add(context_lines).min(total_lines);
	let mut output = String::new();
	output.push_str(&format!("uri: {}\n", symbol.identity));
	if symbol.line_range.is_some() {
		output.push_str("completeness: full\n");
	} else {
		output.push_str(
			"completeness: partial (symbol has no line range; showing first available lines)\n",
		);
	}
	output.push_str(&format!("file: {}\n", source.rel_path));
	output.push_str(&format!("language: {}\n", source.language));
	output.push_str(&format!("kind: {}\n", symbol.kind));
	output.push_str(&format!("name: {}\n", symbol.name));
	output.push_str(&format!("range: {target_start}-{target_end}\n"));
	output.push_str(&format!("slice: {slice_start}-{slice_end}\n\n"));
	output.push_str("code:\n");
	for (line_number, line) in source_text.lines().enumerate() {
		let line_number = line_number + 1;
		if line_number < slice_start || line_number > slice_end {
			continue;
		}
		output.push_str(&format!("  {line_number:>4} | {line}\n"));
	}
	output.push_str("\nnext:\n");
	output.push_str(&format!(
		"  - code_moniker_symbols uri=\"{scheme}workspace\""
	));
	append_call_string_arg(&mut output, "name", &symbol.name);
	append_call_number_arg(&mut output, "limit", 20);
	output.push('\n');
	output.push_str(&format!(
		"  - code_moniker_symbols uri=\"{scheme}workspace\""
	));
	append_call_string_arg(&mut output, "path", &source.rel_path);
	append_call_number_arg(&mut output, "limit", 50);
	output.push('\n');
	output
}

pub(in crate::mcp) fn render_explorer_lmnav(
	scheme: &str,
	request_uri: &str,
	depth: usize,
	catalog: &SourceCatalog,
	scope: &ScopeFilter,
	paging: Paging,
) -> String {
	let scoped_sources = catalog
		.sources
		.iter()
		.filter(|source| scope.matches_file(&source.display_name, source.language.as_deref()))
		.collect::<Vec<_>>();
	let mut tree = ExplorerNode::default();
	for source in &scoped_sources {
		tree.insert(source);
	}
	let uri = normalize_workspace_uri(scheme, request_uri, DEFAULT_READ_URI);
	let summary = WorkspaceSummary::from_sources(catalog.sources.len(), &scoped_sources);
	let mut lines = Vec::new();
	tree.render(depth, "", &mut lines);
	let (start, end, next) = paging.window(&lines);
	let mut output = String::new();
	output.push_str(&format!("uri: {uri}\n"));
	if let Some(next) = next {
		output.push_str(&format!(
			"completeness: partial (explorer rows {start}-{end} of {}, next cursor {next})\n",
			lines.len()
		));
	} else {
		output.push_str("completeness: full\n");
	}
	output.push_str(&format!("files: {}\n", summary.scoped_files));
	output.push_str(&format!("files_total: {}\n", summary.total_files));
	output.push_str(&format!("depth: {depth}\n\n"));
	output.push_str("scope:\n");
	for line in scope.describe() {
		output.push_str(&line);
		output.push('\n');
	}
	output.push('\n');
	summary.render(&mut output);
	output.push_str("explorer:\n");
	if lines.is_empty() {
		output.push_str("  <empty>\n");
	} else {
		for line in lines.iter().take(end).skip(start) {
			output.push_str(line);
			output.push('\n');
		}
	}
	output.push_str("\nnext:\n");
	if let Some(next) = next {
		append_read_next_call(&mut output, scheme, scope, depth, paging.limit, Some(next));
	}
	append_read_next_call(
		&mut output,
		scheme,
		scope,
		(depth + 1).min(MAX_DEPTH),
		paging.limit,
		None,
	);
	append_symbols_call(&mut output, scheme, scope, 50);
	output
}

fn append_read_next_call(
	output: &mut String,
	scheme: &str,
	scope: &ScopeFilter,
	depth: usize,
	limit: usize,
	cursor: Option<usize>,
) {
	output.push_str(&format!("  - code_moniker_read uri=\"{scheme}workspace\""));
	scope.append_call_args(output);
	append_call_number_arg(output, "depth", depth);
	append_call_number_arg(output, "limit", limit);
	if let Some(cursor) = cursor {
		append_call_number_arg(output, "cursor", cursor);
	}
	output.push('\n');
}

fn append_symbols_call(output: &mut String, scheme: &str, scope: &ScopeFilter, limit: usize) {
	output.push_str(&format!(
		"  - code_moniker_symbols uri=\"{scheme}workspace\""
	));
	scope.append_call_args(output);
	append_call_number_arg(output, "limit", limit);
	output.push('\n');
}

#[derive(Default)]
struct ExplorerNode {
	files: Vec<String>,
	children: BTreeMap<String, ExplorerNode>,
}

impl ExplorerNode {
	fn insert(&mut self, source: &SourceUnit) {
		let mut parts = source
			.display_name
			.split(['/', '\\'])
			.filter(|part| !part.is_empty())
			.peekable();
		let mut node = self;
		while let Some(part) = parts.next() {
			if parts.peek().is_some() {
				node = node.children.entry(part.to_string()).or_default();
			} else {
				node.files.push(match source.language.as_deref() {
					Some(language) if !language.is_empty() => format!("{part} [{language}]"),
					_ => part.to_string(),
				});
			}
		}
	}

	fn render(&self, depth: usize, prefix: &str, lines: &mut Vec<String>) {
		if depth == 0 {
			return;
		}
		for (name, child) in &self.children {
			let path = if prefix.is_empty() {
				format!("{name}/")
			} else {
				format!("{prefix}{name}/")
			};
			lines.push(format!("  {path}"));
			child.render(depth - 1, &path, lines);
		}
		for file in &self.files {
			let path = if prefix.is_empty() {
				file.to_string()
			} else {
				format!("{prefix}{file}")
			};
			lines.push(format!("  {path}"));
		}
	}
}

#[derive(Debug)]
struct WorkspaceSummary {
	total_files: usize,
	scoped_files: usize,
	languages: Vec<(String, usize)>,
	prefixes: Vec<(String, usize)>,
}

impl WorkspaceSummary {
	fn from_sources(total_files: usize, sources: &[&SourceUnit]) -> Self {
		let mut languages = BTreeMap::<String, usize>::new();
		let mut prefixes = BTreeMap::<String, usize>::new();
		for source in sources {
			if let Some(language) = source.language.as_deref() {
				*languages.entry(language.to_string()).or_default() += 1;
			}
			*prefixes
				.entry(path_prefix(&source.display_name))
				.or_default() += 1;
		}
		let mut languages = languages.into_iter().collect::<Vec<_>>();
		languages.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
		let mut prefixes = prefixes.into_iter().collect::<Vec<_>>();
		prefixes.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
		prefixes.truncate(8);
		Self {
			total_files,
			scoped_files: sources.len(),
			languages,
			prefixes,
		}
	}

	fn render(&self, output: &mut String) {
		output.push_str("summary:\n");
		self.render_languages(output);
		self.render_concentration(output);
		self.render_hints(output);
		output.push('\n');
	}

	fn render_languages(&self, output: &mut String) {
		output.push_str("  languages:\n");
		if self.languages.is_empty() {
			output.push_str("    <empty>\n");
		} else {
			for (language, count) in &self.languages {
				output.push_str(&format!("    {language}: {count}\n"));
			}
		}
	}

	fn render_concentration(&self, output: &mut String) {
		output.push_str("  concentration:\n");
		if self.prefixes.is_empty() {
			output.push_str("    <empty>\n");
		} else {
			for (prefix, count) in &self.prefixes {
				let percent = (count * 100).checked_div(self.scoped_files).unwrap_or(0);
				output.push_str(&format!("    {prefix}: {count} files ({percent}%)\n"));
			}
		}
	}

	fn render_hints(&self, output: &mut String) {
		output.push_str("  hints:\n");
		output.push_str("    start with code_moniker_symbols using path/lang/kind/shape filters before broad symbol reads\n");
		for (language, _) in self.languages.iter().take(4) {
			if let Some(lang) = code_moniker_core::lang::Lang::from_tag(language) {
				let kinds = language_kinds::known_kinds(std::iter::once(&lang))
					.into_iter()
					.take(18)
					.collect::<Vec<_>>();
				output.push_str(&format!("    {language} kinds: {}\n", kinds.join(", ")));
			}
		}
	}
}
