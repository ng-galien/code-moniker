use std::path::PathBuf;

use code_moniker_query::{
	Page, Query, QueryRequest, QueryResult, SYNTAX_PARSE_MAX_SOURCE_BYTES,
	SYNTAX_TREE_DEFAULT_MAX_DEPTH, SYNTAX_TREE_DEFAULT_MAX_TEXT_CHARS, SYNTAX_TREE_MAX_TEXT_CHARS,
	SymbolDetailResult, SyntaxNodeDto, SyntaxParseQuery, SyntaxTreeQuery, SyntaxTreeResult,
	TreeChildrenQuery, TreeChildrenResult, ViewBoundaryDto, ViewDetailResult, ViewEvidenceDto,
	ViewGotchaDto, ViewListResult, ViewReadQuery, ViewReadResult, ViewRuleDto, ViewRuleRefDto,
};
use serde::Serialize;
use serde_json::{Value, json};

use super::common::{AgentOutputOptions, is_workspace_uri, normalize_workspace_uri};
use super::scope::{
	Paging, ScopeFilter, append_call_bool_arg, append_call_cursor_arg, append_call_number_arg,
	append_call_string_arg,
};
use super::{McpTool, OutputContract, OutputOptions, ToolDescriptor, ToolError, ToolResult};
use crate::language_kinds;
use crate::mcp::context::McpContext;
use crate::presentation::navigation as navigation_presentation;
use crate::views::{self, MonikerDisplay};

const DEFAULT_READ_URI: &str = "workspace";
const MAX_DEPTH: usize = 20;

pub(in crate::mcp) struct ReadTool;

impl ReadTool {
	pub(super) const NAME: &'static str = "code_moniker_read";

	const DESCRIPTION: &'static str = concat!(
		"When to use: default entry point to explore the current code-moniker workspace. ",
		"The same verb starts at the workspace root, expands an explorer tree, or reads code from a returned symbol moniker.\n",
		"\n",
		"Read from code-moniker.\n",
		"  workspace                — workspace summary, language vocabulary, concentration indicators, and explorer page; expected_roots is required\n",
		"  workspace/views          — project-defined contextual views for agents\n",
		"  code+moniker://workspace — same root with an explicit URI\n",
		"  <compact-or-canonical>   — moniker returned by code_moniker_symbols; reads the source slice around that symbol\n",
		"  <file-or-moniker> ast:true — parses the current source on demand and returns a bounded syntax tree; a moniker narrows the tree to its declaration\n",
		"  language:<tag> source:<text> — parses source text directly without indexing or workspace lookup\n",
		"Use path/lang to scope discovery, depth to expand the explorer, limit/cursor for paging, and moniker_format when a view should expose resolved monikers. AST reads are named-node-only by default and never populate the workspace index."
	);

	fn input_schema() -> Value {
		read_input_schema()
	}
}

fn read_input_schema() -> Value {
	json!({
		"type": "object",
		"properties": {
			"uri": {
				"type": "string",
				"description": "workspace | code+moniker://workspace | relative or absolute source path for ast:true (absolute disambiguates duplicate multi-root paths) | compact moniker, canonical URI, symbol id, unique bare name, or unambiguous lang:path.kind:name reference. Ambiguity returns candidates. With source+language, this is an optional parser filename hint such as snippet.tsx."
			},
			"source": {
				"type": "string",
				"maxLength": SYNTAX_PARSE_MAX_SOURCE_BYTES,
				"description": "Source text to parse directly. Requires language, implies syntax.parse, and is not indexed or persisted."
			},
			"language": {
				"type": "string",
				"description": "Canonical parser tag for direct source parsing: ts, rs, java, python, go, c, cs, sql, or plpgsql. Requires source."
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
			},
			"ast": {
				"type": "boolean",
				"description": "Return a bounded Tree-sitter syntax tree. Use uri for indexed source, or source+language for stateless parsing."
			},
			"max_depth": {
				"type": "integer",
				"minimum": 0,
				"description": "Client-selected maximum AST depth below the selected root. Defaults to 6."
			},
			"max_nodes": {
				"type": "integer",
				"minimum": 1,
				"description": "Client-selected maximum AST nodes to emit. Without an explicit value, the small, medium, or full output budget selects the node volume."
			},
			"named_only": {
				"type": "boolean",
				"description": "Return only named grammar nodes by default; false exposes the concrete syntax tree including punctuation."
			},
			"include_text": {
				"type": "boolean",
				"description": "Attach bounded normalized source text to leaf nodes. Defaults false."
			},
			"max_text_chars": {
				"type": "integer",
				"minimum": 0,
				"maximum": SYNTAX_TREE_MAX_TEXT_CHARS,
				"description": "Maximum source characters attached to each leaf when include_text is true. Defaults to 80."
			},
			"expected_roots": {
				"oneOf": [
					{ "type": "string" },
					{ "type": "array", "items": { "type": "string" }, "minItems": 1 }
				],
				"description": "Canonical workspace root(s) expected by the client. Workspace reads fail with workspace_mismatch unless the server is bound to exactly this set."
			}
		},
		"additionalProperties": false
	})
}

impl McpTool for ReadTool {
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
		let request = ReadRequest::from_arguments(arguments, output.agent_options())
			.map_err(ToolError::failed)?;
		read_resource(context, &request).map_err(ToolError::failed)
	}
}

struct ReadRequest {
	uri: String,
	depth: usize,
	context_lines: usize,
	include_code: bool,
	syntax: SyntaxReadOptions,
	moniker_display: MonikerDisplay,
	scope: ScopeFilter,
	paging: Paging,
	output: AgentOutputOptions,
	expected_roots: Option<Vec<PathBuf>>,
}

impl ReadRequest {
	fn from_arguments(arguments: &Value, output: AgentOutputOptions) -> anyhow::Result<Self> {
		Ok(Self {
			uri: read_string_argument(arguments, "uri")
				.unwrap_or(DEFAULT_READ_URI)
				.to_string(),
			depth: clamped_usize_argument(arguments, "depth", 2, MAX_DEPTH),
			context_lines: clamped_usize_argument(arguments, "context_lines", 2, 20),
			include_code: read_bool_argument(arguments, "include_code", false),
			syntax: read_syntax_options(arguments, output)?,
			moniker_display: MonikerDisplay::parse(read_string_argument(
				arguments,
				"moniker_format",
			))?,
			scope: ScopeFilter::from_arguments(arguments)?,
			paging: Paging::from_arguments_for_volume(arguments, output)?,
			output,
			expected_roots: read_path_list_argument(arguments, "expected_roots")?,
		})
	}
}

struct SyntaxReadOptions {
	enabled: bool,
	source: Option<String>,
	language: Option<String>,
	uri: Option<String>,
	max_depth: usize,
	max_nodes: usize,
	named_only: bool,
	include_text: bool,
	max_text_chars: usize,
}

fn read_syntax_options(
	arguments: &Value,
	output: AgentOutputOptions,
) -> anyhow::Result<SyntaxReadOptions> {
	let ast_requested = strict_bool_argument(arguments, "ast", false)?;
	let source = read_string_argument(arguments, "source").map(ToOwned::to_owned);
	let language = read_string_argument(arguments, "language").map(ToOwned::to_owned);
	if source.is_some() != language.is_some() {
		anyhow::bail!("source and language must be provided together");
	}
	Ok(SyntaxReadOptions {
		enabled: ast_requested || source.is_some(),
		source,
		language,
		uri: read_string_argument(arguments, "uri").map(ToOwned::to_owned),
		max_depth: strict_usize_argument(
			arguments,
			"max_depth",
			SYNTAX_TREE_DEFAULT_MAX_DEPTH,
			0,
			None,
		)?,
		max_nodes: strict_usize_argument(
			arguments,
			"max_nodes",
			output.default_page_limit(),
			1,
			None,
		)?
		.min(output.default_page_limit()),
		named_only: strict_bool_argument(arguments, "named_only", true)?,
		include_text: strict_bool_argument(arguments, "include_text", false)?,
		max_text_chars: strict_usize_argument(
			arguments,
			"max_text_chars",
			SYNTAX_TREE_DEFAULT_MAX_TEXT_CHARS,
			0,
			Some(SYNTAX_TREE_MAX_TEXT_CHARS),
		)?,
	})
}

fn strict_bool_argument(arguments: &Value, key: &str, default: bool) -> anyhow::Result<bool> {
	match arguments.get(key) {
		None => Ok(default),
		Some(Value::Bool(value)) => Ok(*value),
		Some(_) => anyhow::bail!("{key} must be a boolean"),
	}
}

fn strict_usize_argument(
	arguments: &Value,
	key: &str,
	default: usize,
	min: usize,
	max: Option<usize>,
) -> anyhow::Result<usize> {
	let Some(value) = arguments.get(key) else {
		return Ok(default);
	};
	let value = value
		.as_u64()
		.and_then(|value| usize::try_from(value).ok())
		.ok_or_else(|| anyhow::anyhow!("{key} must be an unsigned integer"))?;
	if value < min {
		anyhow::bail!("{key} must be at least {min}");
	}
	if let Some(max) = max
		&& value > max
	{
		anyhow::bail!("{key} must be between {min} and {max}");
	}
	Ok(value)
}

fn read_path_list_argument(arguments: &Value, key: &str) -> anyhow::Result<Option<Vec<PathBuf>>> {
	let Some(value) = arguments.get(key) else {
		return Ok(None);
	};
	let values = match value {
		Value::String(path) => vec![PathBuf::from(path)],
		Value::Array(paths) => paths
			.iter()
			.map(|path| {
				path.as_str()
					.map(PathBuf::from)
					.ok_or_else(|| anyhow::anyhow!("{key} entries must be strings"))
			})
			.collect::<anyhow::Result<Vec<_>>>()?,
		_ => anyhow::bail!("{key} must be a string or array of strings"),
	};
	if values.is_empty() {
		anyhow::bail!("{key} must contain at least one workspace root");
	}
	Ok(Some(values))
}

fn read_string_argument<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
	arguments.get(key).and_then(Value::as_str)
}

fn clamped_usize_argument(arguments: &Value, key: &str, default: usize, max: usize) -> usize {
	arguments
		.get(key)
		.and_then(Value::as_u64)
		.unwrap_or(default as u64)
		.min(max as u64) as usize
}

fn read_bool_argument(arguments: &Value, key: &str, default: bool) -> bool {
	arguments
		.get(key)
		.and_then(Value::as_bool)
		.unwrap_or(default)
}

fn read_resource(context: &McpContext, request: &ReadRequest) -> anyhow::Result<ToolResult> {
	if request.syntax.enabled {
		return read_syntax_tree(context, request);
	}
	if is_workspace_uri(&request.uri, context.scheme(), DEFAULT_READ_URI) {
		let expected_roots = request.expected_roots.as_deref().ok_or_else(|| {
			anyhow::anyhow!(
				"workspace_identity_required: workspace reads require expected_roots so the server can fail closed on incorrect MCP routing"
			)
		})?;
		context.verify_expected_roots(expected_roots)?;
		return read_workspace(context, request).map(ToolResult::templated);
	}
	if views::is_views_uri(&request.uri, context.scheme()) {
		return read_view(context, request).map(ToolResult::templated);
	}
	read_symbol(context, request)
}

fn read_syntax_tree(context: &McpContext, request: &ReadRequest) -> anyhow::Result<ToolResult> {
	if let (Some(source), Some(language)) = (
		request.syntax.source.as_ref(),
		request.syntax.language.as_ref(),
	) {
		let response = context.query(QueryRequest::new(Query::SyntaxParse(SyntaxParseQuery {
			language: language.clone(),
			source: source.clone(),
			uri: request.syntax.uri.as_deref().map(str::to_owned),
			max_depth: request.syntax.max_depth,
			max_nodes: request.syntax.max_nodes,
			named_only: request.syntax.named_only,
			include_text: request.syntax.include_text,
			max_text_chars: request.syntax.max_text_chars,
		})))?;
		let QueryResult::SyntaxTree(result) = response.result else {
			anyhow::bail!("daemon returned an unexpected result for syntax.parse");
		};
		return syntax_tree_tool_result(&result, "syntax.parse", request.output.budget.as_str());
	}
	let response = context.query_refreshed(
		Query::SyntaxTree(SyntaxTreeQuery {
			workspace: None,
			focus: request.uri.clone(),
			max_depth: request.syntax.max_depth,
			max_nodes: request.syntax.max_nodes,
			named_only: request.syntax.named_only,
			include_text: request.syntax.include_text,
			max_text_chars: request.syntax.max_text_chars,
		}),
		Page::default(),
	)?;
	let QueryResult::SyntaxTree(result) = response.result else {
		anyhow::bail!("daemon returned an unexpected result for syntax.tree");
	};
	Ok(ToolResult::templated(
		syntax_tree_output(&result, "syntax.tree", request.output.budget.as_str())?
			.with_monikers([result.focus.as_str()]),
	)
	.with_structured_content(serde_json::to_value(&result)?))
}

fn syntax_tree_tool_result(
	result: &SyntaxTreeResult,
	operation: &str,
	volume: &'static str,
) -> anyhow::Result<ToolResult> {
	Ok(
		ToolResult::templated(syntax_tree_output(result, operation, volume)?)
			.with_structured_content(serde_json::to_value(result)?),
	)
}

fn syntax_tree_output(
	result: &SyntaxTreeResult,
	operation: &str,
	volume: &'static str,
) -> anyhow::Result<crate::presentation::TemplateOutput> {
	let mut nodes = Vec::with_capacity(result.emitted_nodes);
	collect_syntax_nodes(&mut nodes, &result.root, 0);
	navigation_presentation::read_ast(&SyntaxTreeTemplate {
		uri: operation,
		volume,
		completeness: if result.truncated { "bounded" } else { "full" },
		file: &result.file,
		language: &result.language,
		focus: &result.focus,
		focus_lines: result
			.focus_line_range
			.map(|(start, end)| format!("{start}-{end}")),
		emitted_nodes: result.emitted_nodes,
		total_nodes: result.total_nodes,
		max_depth: result.max_depth,
		has_error: result.has_error,
		nodes,
	})
}

#[derive(Serialize)]
struct SyntaxTreeTemplate<'a> {
	uri: &'a str,
	volume: &'static str,
	completeness: &'static str,
	file: &'a str,
	language: &'a str,
	focus: &'a str,
	focus_lines: Option<String>,
	emitted_nodes: usize,
	total_nodes: usize,
	max_depth: usize,
	has_error: bool,
	nodes: Vec<SyntaxNodeTemplate<'a>>,
}

#[derive(Serialize)]
struct SyntaxNodeTemplate<'a> {
	indent: String,
	kind: &'a str,
	start_line: u32,
	start_column: u32,
	end_line: u32,
	end_column: u32,
	flags: Vec<String>,
	text: Option<String>,
}

fn collect_syntax_nodes<'a>(
	nodes: &mut Vec<SyntaxNodeTemplate<'a>>,
	node: &'a SyntaxNodeDto,
	depth: usize,
) {
	let mut flags = Vec::new();
	if let Some(language) = &node.language {
		flags.push(language.clone());
	}
	if let Some(entry_point) = &node.entry_point {
		flags.push(format!("entry:{entry_point}"));
	}
	if let Some(has_error) = node.has_error {
		flags.push(format!("injected-error:{has_error}"));
	}
	if !node.named {
		flags.push("anonymous".to_string());
	}
	if node.error {
		flags.push("error".to_string());
	}
	if node.missing {
		flags.push("missing".to_string());
	}
	let text = node.text.as_deref().map(|text| format!("{text:?}"));
	nodes.push(SyntaxNodeTemplate {
		indent: "  ".repeat(depth),
		kind: &node.kind,
		start_line: node.start.line,
		start_column: node.start.column,
		end_line: node.end.line,
		end_column: node.end.column,
		flags,
		text,
	});
	for child in &node.children {
		collect_syntax_nodes(nodes, child, depth + 1);
	}
}

fn read_workspace(
	context: &McpContext,
	request: &ReadRequest,
) -> anyhow::Result<crate::presentation::TemplateOutput> {
	let response = context.query_refreshed(
		Query::TreeChildren(TreeChildrenQuery {
			workspace: None,
			path: request.scope.paths.clone(),
			depth: request.depth,
			lang: request.scope.langs.clone(),
			projection: Vec::new(),
		}),
		request.paging.daemon_page(),
	)?;
	let QueryResult::TreeChildren(result) = response.result else {
		anyhow::bail!("unexpected daemon response for workspace read");
	};
	explorer_output(DaemonExplorerProjection {
		scheme: context.scheme(),
		request_uri: &request.uri,
		depth: request.depth,
		scope: &request.scope,
		paging: request.paging,
		next_cursor: response.next_cursor.as_ref(),
		result: &result,
		output: request.output,
		workspace_roots: context.workspace_roots(),
	})
}

fn read_symbol(context: &McpContext, request: &ReadRequest) -> anyhow::Result<ToolResult> {
	let response = context.query_refreshed(
		Query::SymbolDetail(code_moniker_query::SymbolDetailQuery {
			workspace: None,
			uri: request.uri.to_string(),
			context_lines: request.context_lines,
		}),
		code_moniker_query::Page::default(),
	)?;
	let QueryResult::SymbolDetail(result) = response.result else {
		anyhow::bail!("unexpected daemon response for symbol read");
	};
	Ok(ToolResult::templated(
		symbol_source_output(
			context.scheme(),
			&result,
			request.output,
			request.paging.limit,
		)?
		.with_monikers([result.symbol.uri.as_str()]),
	))
}

fn read_view(
	context: &McpContext,
	request: &ReadRequest,
) -> anyhow::Result<crate::presentation::TemplateOutput> {
	let response = context.query_refreshed(
		Query::ViewRead(ViewReadQuery {
			uri: request.uri.to_string(),
			scheme: Some(context.scheme().to_string()),
			context_lines: request.context_lines,
			include_code: request.include_code,
		}),
		code_moniker_query::Page::default(),
	)?;
	let QueryResult::ViewRead(result) = response.result else {
		anyhow::bail!("unexpected daemon response for view read");
	};
	view_output(
		context.scheme(),
		&result,
		ViewRenderOptions {
			moniker_display: request.moniker_display,
			output: request.output,
			next_limit: request.paging.limit,
		},
	)
}

const VIEWS_URI: &str = "workspace/views";

#[derive(Clone, Copy)]
struct ViewRenderOptions {
	moniker_display: MonikerDisplay,
	output: AgentOutputOptions,
	next_limit: usize,
}

fn view_output(
	scheme: &str,
	result: &ViewReadResult,
	options: ViewRenderOptions,
) -> anyhow::Result<crate::presentation::TemplateOutput> {
	match result {
		ViewReadResult::List(list) => view_list_output(scheme, list, options),
		ViewReadResult::Detail(detail) => view_detail_output(scheme, detail, options),
	}
}

#[derive(Serialize)]
struct ViewListTemplate<'a> {
	uri: String,
	volume: &'static str,
	views: Vec<ViewListItemTemplate<'a>>,
	next_calls: Vec<AgentCall>,
}

#[derive(Serialize)]
struct ViewListItemTemplate<'a> {
	id: &'a str,
	title: Option<&'a str>,
	fragment: &'a str,
	anchor: &'a str,
	scope: &'a str,
}

fn view_list_output(
	scheme: &str,
	list: &ViewListResult,
	options: ViewRenderOptions,
) -> anyhow::Result<crate::presentation::TemplateOutput> {
	let views = list
		.views
		.iter()
		.map(|view| ViewListItemTemplate {
			id: &view.id,
			title: view.title.as_deref(),
			fragment: &view.fragment,
			anchor: &view.anchor,
			scope: view_scope_label(&view.scope),
		})
		.collect::<Vec<_>>();
	let next_calls = list
		.views
		.iter()
		.take(options.next_limit)
		.map(|view| {
			let mut arguments = String::new();
			append_read_output_args(&mut arguments, options.output);
			AgentCall {
				tool: "code_moniker_read",
				uri: format!("{scheme}{VIEWS_URI}/{}", view.id),
				arguments,
			}
		})
		.collect::<Vec<_>>();
	navigation_presentation::read_view_list(&ViewListTemplate {
		uri: format!("{scheme}{VIEWS_URI}"),
		volume: options.output.budget.as_str(),
		views,
		next_calls,
	})
}

#[derive(Serialize)]
struct ViewDetailTemplate<'a> {
	uri: String,
	view: ViewMetadataTemplate<'a>,
	rules: Vec<ViewRuleTemplate<'a>>,
	boundaries: Vec<ViewBoundaryTemplate<'a>>,
	gotchas: Vec<ViewGotchaTemplate<'a>>,
	next_calls: Vec<AgentCall>,
}

#[derive(Serialize)]
struct ViewMetadataTemplate<'a> {
	id: &'a str,
	title: Option<&'a str>,
	fragment: &'a str,
	anchor: &'a str,
	scope: &'a str,
	intent: Option<&'a str>,
	summary: Option<&'a str>,
}

#[derive(Serialize)]
struct ViewRuleTemplate<'a> {
	id: &'a str,
	severity: &'a str,
	domain: &'a str,
	rationale: Option<&'a str>,
}

#[derive(Serialize)]
struct ViewBoundaryTemplate<'a> {
	id: &'a str,
	owns: &'a [String],
	forbids: &'a [String],
	forbids_status: &'static str,
	forbid_rules: &'a [String],
	rationale: Option<&'a str>,
	rules: Vec<ViewRuleRefTemplate<'a>>,
	evidence: Vec<ViewEvidenceTemplate<'a>>,
	missing: &'a [String],
}

#[derive(Serialize)]
struct ViewGotchaTemplate<'a> {
	id: &'a str,
	rationale: &'a str,
	check: Option<&'a str>,
	rules: Vec<ViewRuleRefTemplate<'a>>,
	evidence: Vec<ViewEvidenceTemplate<'a>>,
	missing: &'a [String],
}

#[derive(Serialize)]
struct ViewRuleRefTemplate<'a> {
	id: &'a str,
	present: bool,
}

#[derive(Serialize)]
struct ViewEvidenceTemplate<'a> {
	selector: &'a str,
	label: &'a str,
	moniker: Option<String>,
	compact_moniker: bool,
	file: &'a str,
	slice: Option<String>,
	code: Option<String>,
}

fn view_detail_output(
	scheme: &str,
	detail: &ViewDetailResult,
	options: ViewRenderOptions,
) -> anyhow::Result<crate::presentation::TemplateOutput> {
	let rules = detail.rules.iter().map(view_rule_template).collect();
	let boundaries = detail
		.boundaries
		.iter()
		.map(|boundary| view_boundary_template(boundary, options.moniker_display))
		.collect();
	let gotchas = detail
		.gotchas
		.iter()
		.map(|gotcha| view_gotcha_template(gotcha, options.moniker_display))
		.collect();
	let mut next_calls = Vec::new();
	let mut arguments = String::new();
	append_call_string_arg(
		&mut arguments,
		"path",
		&format!("{}**", view_next_scope_path(&detail.scope)),
	);
	append_call_number_arg(&mut arguments, "limit", options.next_limit);
	append_read_output_args(&mut arguments, options.output);
	next_calls.push(AgentCall {
		tool: "code_moniker_symbols",
		uri: format!("{scheme}workspace"),
		arguments,
	});
	if !options.output.compact {
		let mut arguments = String::new();
		append_call_string_arg(&mut arguments, "action", "list");
		append_call_number_arg(&mut arguments, "limit", 50);
		append_read_output_args(&mut arguments, options.output);
		next_calls.push(AgentCall {
			tool: "code_moniker_rules",
			uri: format!("{scheme}workspace"),
			arguments,
		});
	}
	navigation_presentation::read_view_detail(&ViewDetailTemplate {
		uri: format!("{scheme}{VIEWS_URI}/{}", detail.id),
		view: ViewMetadataTemplate {
			id: &detail.id,
			title: detail.title.as_deref(),
			fragment: &detail.fragment,
			anchor: &detail.anchor,
			scope: view_scope_label(&detail.scope),
			intent: detail.intent.as_deref(),
			summary: detail.summary.as_deref(),
		},
		rules,
		boundaries,
		gotchas,
		next_calls,
	})
}

fn view_rule_template(rule: &ViewRuleDto) -> ViewRuleTemplate<'_> {
	ViewRuleTemplate {
		id: &rule.id,
		severity: &rule.severity,
		domain: &rule.domain,
		rationale: rule.rationale.as_deref(),
	}
}

fn view_boundary_template<'a>(
	boundary: &'a ViewBoundaryDto,
	moniker_display: MonikerDisplay,
) -> ViewBoundaryTemplate<'a> {
	ViewBoundaryTemplate {
		id: &boundary.id,
		owns: &boundary.owns,
		forbids: &boundary.forbids,
		forbids_status: if boundary.forbid_rules.is_empty() {
			"advisory"
		} else {
			"enforced_by_rules"
		},
		forbid_rules: &boundary.forbid_rules,
		rationale: boundary.rationale.as_deref(),
		rules: view_rule_refs(&boundary.rule_refs),
		evidence: view_evidence(&boundary.evidence, moniker_display),
		missing: &boundary.missing,
	}
}

fn view_gotcha_template<'a>(
	gotcha: &'a ViewGotchaDto,
	moniker_display: MonikerDisplay,
) -> ViewGotchaTemplate<'a> {
	ViewGotchaTemplate {
		id: &gotcha.id,
		rationale: &gotcha.rationale,
		check: gotcha.check.as_deref(),
		rules: view_rule_refs(&gotcha.rule_refs),
		evidence: view_evidence(&gotcha.evidence, moniker_display),
		missing: &gotcha.missing,
	}
}

fn view_rule_refs(rule_refs: &[ViewRuleRefDto]) -> Vec<ViewRuleRefTemplate<'_>> {
	rule_refs
		.iter()
		.map(|rule| ViewRuleRefTemplate {
			id: &rule.id,
			present: rule.present,
		})
		.collect()
}

fn view_evidence(
	evidence: &[ViewEvidenceDto],
	moniker_display: MonikerDisplay,
) -> Vec<ViewEvidenceTemplate<'_>> {
	evidence
		.iter()
		.map(|item| ViewEvidenceTemplate {
			selector: &item.selector,
			label: &item.label,
			moniker: moniker_display.render(&item.moniker),
			compact_moniker: moniker_display == MonikerDisplay::Compact,
			file: &item.file,
			slice: item.slice.map(|(start, end)| format!("L{start}-L{end}")),
			code: (!item.code.is_empty()).then(|| {
				item.code
					.iter()
					.map(|line| {
						let marker = if item
							.active_slice
							.is_some_and(|(start, end)| start <= line.number && line.number <= end)
						{
							">"
						} else {
							" "
						};
						format!("{marker} {:>4} | {}", line.number, line.text)
					})
					.collect::<Vec<_>>()
					.join("\n")
			}),
		})
		.collect()
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

#[derive(Serialize)]
struct AgentCall {
	tool: &'static str,
	uri: String,
	arguments: String,
}

#[derive(Serialize)]
struct SymbolSourceTemplate<'a> {
	uri: &'a str,
	completeness: &'static str,
	file: &'a str,
	language: &'a str,
	kind: &'a str,
	name: &'a str,
	range: Option<String>,
	slice: Option<String>,
	code: Option<String>,
	next_calls: Vec<AgentCall>,
}

fn symbol_source_output(
	scheme: &str,
	result: &SymbolDetailResult,
	output: AgentOutputOptions,
	limit: usize,
) -> anyhow::Result<crate::presentation::TemplateOutput> {
	let symbol = &result.symbol;
	let mut next_calls = Vec::new();
	if !output.compact {
		let mut arguments = String::new();
		append_call_string_arg(&mut arguments, "name", &symbol.name);
		append_call_number_arg(&mut arguments, "limit", limit);
		append_read_output_args(&mut arguments, output);
		next_calls.push(AgentCall {
			tool: "code_moniker_symbols",
			uri: format!("{scheme}workspace"),
			arguments,
		});
	}
	let mut arguments = String::new();
	append_call_string_arg(&mut arguments, "path", &symbol.file);
	append_call_number_arg(&mut arguments, "limit", limit);
	append_read_output_args(&mut arguments, output);
	next_calls.push(AgentCall {
		tool: "code_moniker_symbols",
		uri: format!("{scheme}workspace"),
		arguments,
	});
	let (slice, code) = result.source.as_ref().map_or_else(
		|| (None, None),
		|source| {
			(
				Some(format!("{}-{}", source.first_line, source.last_line)),
				Some(
					source
						.lines
						.iter()
						.map(|line| format!("{:>4} | {}", line.number, line.text))
						.collect::<Vec<_>>()
						.join("\n"),
				),
			)
		},
	);
	navigation_presentation::read_symbol(&SymbolSourceTemplate {
		uri: &symbol.uri,
		completeness: if result.source.is_some() {
			"full"
		} else {
			"partial (symbol has no line range; showing first available lines)"
		},
		file: &symbol.file,
		language: &symbol.language,
		kind: &symbol.kind,
		name: &symbol.name,
		range: symbol
			.line_range
			.map(|(start, end)| format!("{start}-{end}")),
		slice,
		code,
		next_calls,
	})
}

struct DaemonExplorerProjection<'a> {
	scheme: &'a str,
	request_uri: &'a str,
	depth: usize,
	scope: &'a ScopeFilter,
	paging: Paging,
	next_cursor: Option<&'a code_moniker_query::QueryCursor>,
	result: &'a TreeChildrenResult,
	output: AgentOutputOptions,
	workspace_roots: &'a [PathBuf],
}

#[derive(Serialize)]
struct CountTemplate<'a> {
	name: &'a str,
	count: usize,
	percent: Option<usize>,
}

#[derive(Serialize)]
struct LanguageHintTemplate<'a> {
	language: &'a str,
	kinds: Vec<String>,
}

#[derive(Serialize)]
struct ExplorerTemplate<'a> {
	uri: String,
	completeness: String,
	roots: Vec<String>,
	scoped_files: usize,
	total_files: usize,
	depth: usize,
	volume: &'static str,
	paths: &'a [String],
	langs: &'a [String],
	languages: Vec<CountTemplate<'a>>,
	concentrations: Vec<CountTemplate<'a>>,
	language_hints: Vec<LanguageHintTemplate<'a>>,
	rows: Vec<String>,
	next_calls: Vec<AgentCall>,
}

fn explorer_output(
	render: DaemonExplorerProjection<'_>,
) -> anyhow::Result<crate::presentation::TemplateOutput> {
	let uri = normalize_workspace_uri(render.scheme, render.request_uri, DEFAULT_READ_URI);
	let completeness = if let Some(next) = render.next_cursor {
		format!(
			"partial (explorer rows {}-{} of {}, next cursor {})",
			render.paging.cursor,
			render.paging.cursor + render.result.rows.len(),
			render.result.total,
			next.offset
		)
	} else {
		"full".to_string()
	};
	let mut next_calls = Vec::new();
	if let Some(next) = render.next_cursor {
		let mut arguments = String::new();
		render.scope.append_call_args(&mut arguments);
		append_expected_roots_arg(&mut arguments, render.workspace_roots);
		append_call_number_arg(&mut arguments, "depth", render.depth);
		append_call_number_arg(&mut arguments, "limit", render.paging.limit);
		append_call_cursor_arg(&mut arguments, "cursor", next);
		append_read_output_args(&mut arguments, render.output);
		next_calls.push(AgentCall {
			tool: "code_moniker_read",
			uri: format!("{}workspace", render.scheme),
			arguments,
		});
	}
	next_calls.push(read_next_call(
		render.scheme,
		render.scope,
		ReadNextCall {
			depth: (render.depth + 1).min(MAX_DEPTH),
			limit: render.paging.limit,
			cursor: None,
			output: render.output,
			expected_roots: Some(render.workspace_roots),
		},
	));
	next_calls.push(symbols_call(
		render.scheme,
		render.scope,
		render.paging.limit,
		render.output,
	));
	let languages = render
		.result
		.languages
		.iter()
		.map(|language| CountTemplate {
			name: &language.name,
			count: language.count,
			percent: None,
		})
		.collect();
	let concentrations = render
		.result
		.prefixes
		.iter()
		.map(|prefix| CountTemplate {
			name: &prefix.name,
			count: prefix.count,
			percent: None,
		})
		.collect();
	let language_hints = language_hints(
		render
			.result
			.languages
			.iter()
			.map(|language| language.name.as_str()),
	);
	navigation_presentation::read_explorer(&ExplorerTemplate {
		uri,
		completeness,
		roots: render
			.workspace_roots
			.iter()
			.map(|root| root.display().to_string())
			.collect(),
		scoped_files: render.result.scoped_files,
		total_files: render.result.total_files,
		depth: render.depth,
		volume: render.output.budget.as_str(),
		paths: &render.scope.paths,
		langs: &render.scope.langs,
		languages,
		concentrations,
		language_hints,
		rows: render.result.rows.iter().map(explorer_row_label).collect(),
		next_calls,
	})
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

struct ReadNextCall<'a> {
	depth: usize,
	limit: usize,
	cursor: Option<usize>,
	output: AgentOutputOptions,
	expected_roots: Option<&'a [PathBuf]>,
}

fn read_next_call(scheme: &str, scope: &ScopeFilter, call: ReadNextCall<'_>) -> AgentCall {
	let mut arguments = String::new();
	scope.append_call_args(&mut arguments);
	if let Some(expected_roots) = call.expected_roots {
		append_expected_roots_arg(&mut arguments, expected_roots);
	}
	append_call_number_arg(&mut arguments, "depth", call.depth);
	append_call_number_arg(&mut arguments, "limit", call.limit);
	if let Some(cursor) = call.cursor {
		append_call_number_arg(&mut arguments, "cursor", cursor);
	}
	append_read_output_args(&mut arguments, call.output);
	AgentCall {
		tool: "code_moniker_read",
		uri: format!("{scheme}workspace"),
		arguments,
	}
}

fn append_expected_roots_arg(output: &mut String, roots: &[PathBuf]) {
	let roots = roots
		.iter()
		.map(|root| Value::String(root.display().to_string()))
		.collect::<Vec<_>>();
	output.push_str(&format!(" expected_roots={}", Value::Array(roots)));
}

fn symbols_call(
	scheme: &str,
	scope: &ScopeFilter,
	limit: usize,
	output: AgentOutputOptions,
) -> AgentCall {
	let mut arguments = String::new();
	scope.append_call_args(&mut arguments);
	append_call_number_arg(&mut arguments, "limit", limit);
	append_read_output_args(&mut arguments, output);
	AgentCall {
		tool: "code_moniker_symbols",
		uri: format!("{scheme}workspace"),
		arguments,
	}
}

fn append_read_output_args(arguments: &mut String, output: AgentOutputOptions) {
	append_call_string_arg(arguments, "budget", output.budget.as_str());
	if !output.compact {
		append_call_bool_arg(arguments, "compact", false);
	}
}

fn language_hints<'a>(
	languages: impl IntoIterator<Item = &'a str>,
) -> Vec<LanguageHintTemplate<'a>> {
	languages
		.into_iter()
		.take(4)
		.filter_map(|language| {
			let lang = code_moniker_core::lang::Lang::from_tag(language)?;
			Some(LanguageHintTemplate {
				language,
				kinds: language_kinds::known_kinds(std::iter::once(&lang))
					.into_iter()
					.take(18)
					.map(str::to_owned)
					.collect(),
			})
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::{
		AgentOutputOptions, ViewBoundaryTemplate, ViewDetailTemplate, ViewEvidenceTemplate,
		ViewGotchaTemplate, ViewMetadataTemplate, ViewRuleRefTemplate, read_syntax_options,
	};
	use crate::mcp::tools::common::OutputBudget;

	#[test]
	fn syntax_volume_profile_caps_explicit_node_count() {
		let options = read_syntax_options(
			&serde_json::json!({"ast": true, "max_nodes": 20_000}),
			AgentOutputOptions {
				compact: true,
				budget: OutputBudget::Small,
			},
		)
		.expect("small syntax request");

		assert_eq!(options.max_nodes, 20);
	}

	#[test]
	fn view_detail_keeps_conditional_evidence_and_rule_fields_on_separate_lines() {
		let canonical = "code+moniker://./lang:rs/module:app/fn:run()";
		let evidence = ViewEvidenceTemplate {
			selector: "fn:run",
			label: "Run function",
			moniker: Some(canonical.to_string()),
			compact_moniker: true,
			file: "src/app.rs",
			slice: Some("L1-L4".to_string()),
			code: None,
		};
		let view = ViewDetailTemplate {
			uri: "code+moniker://workspace/views/app".to_string(),
			view: ViewMetadataTemplate {
				id: "app",
				title: None,
				fragment: "app",
				anchor: ".",
				scope: ".",
				intent: None,
				summary: None,
			},
			rules: Vec::new(),
			boundaries: vec![ViewBoundaryTemplate {
				id: "entry",
				owns: &[],
				forbids: &[],
				forbids_status: "advisory",
				forbid_rules: &[],
				rationale: None,
				rules: Vec::new(),
				evidence: vec![evidence],
				missing: &[],
			}],
			gotchas: vec![ViewGotchaTemplate {
				id: "careful",
				rationale: "Keep the boundary visible.",
				check: None,
				rules: vec![
					ViewRuleRefTemplate {
						id: "rule-a",
						present: true,
					},
					ViewRuleRefTemplate {
						id: "rule-b",
						present: false,
					},
				],
				evidence: Vec::new(),
				missing: &[],
			}],
			next_calls: Vec::new(),
		};
		let rendered = crate::presentation::navigation::read_view_detail(&view)
			.expect("view template")
			.render(crate::presentation::RenderOptions {
				compact: true,
				scheme: "code+moniker://",
				runtime: None,
			})
			.expect("rendered view");
		let lines = rendered.lines().collect::<Vec<_>>();

		assert!(
			lines.contains(&"  - moniker: `rs:app.fn:run()`"),
			"{rendered}"
		);
		assert!(
			lines.contains(&"  - file: `src/app.rs`, slice: `L1-L4`"),
			"{rendered}"
		);
		assert!(
			lines.contains(&"- rules: `rule-a`, `rule-b` [missing]"),
			"{rendered}"
		);
		crate::presentation::tests::validate_agent_markdown(&rendered, "Project view: app", false)
			.expect("view CommonMark");
	}
}
