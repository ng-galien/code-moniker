use code_moniker_core::core::shape::Shape;
use code_moniker_query::{
	Query, QueryResult, SymbolDto, SymbolInsightsResult, SymbolListResult, SymbolSearchQuery,
};
use serde::Serialize;
use serde_json::{Value, json};

use super::common::{
	AgentOutputOptions, is_workspace_uri, line_range_suffix, normalize_workspace_uri,
};
use super::scope::{
	Paging, ScopeRowView, SymbolScopeFilter, append_call_bool_arg, append_call_cursor_arg,
	append_call_number_arg, append_call_string_arg, scope_rows,
};
use super::{McpTool, OutputContract, OutputOptions, ToolDescriptor, ToolError, ToolResult};
use crate::mcp::context::McpContext;
#[cfg(test)]
use crate::presentation::RenderOptions;
use crate::presentation::{TemplateOutput, symbols as symbols_presentation};

const DEFAULT_SYMBOL_URI: &str = "workspace";

pub(super) struct SymbolsTool;

impl SymbolsTool {
	pub(super) const NAME: &'static str = "code_moniker_symbols";

	const DESCRIPTION: &'static str = concat!(
		"When to use: list symbols after code_moniker_read has identified the relevant workspace, language, or subtree. ",
		"Use this instead of broad text search when you need named code structure or symbolic health signals.\n",
		"\n",
		"Query the code-moniker symbol index.\n",
		"  action=list     — list navigable symbols in the workspace\n",
		"  action=insights — summarize languages, kinds, shapes, refs, and concentrated files\n",
		"Filters are AND-combined: path/lang limit the files, kind/shape/name limit symbols. ",
		"Use limit and cursor for paging; compact output uses compact monikers by default."
	);

	fn input_schema() -> Value {
		json!({
			"type": "object",
			"properties": {
				"action": {
					"type": "string",
					"enum": ["list", "insights"],
					"description": "list symbols, or insights for symbolic metrics."
				},
				"uri": {
					"type": "string",
					"description": "workspace | code+moniker://workspace"
				},
				"path": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Relative file glob(s), OR-combined."
				},
				"lang": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Language tag(s), OR-combined."
				},
				"kind": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Concrete symbol kind(s), OR-combined. Example: class, interface, fn, method"
				},
				"shape": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Shape family, OR-combined. One of namespace,type,callable,value,annotation,ref"
				},
				"name": {
					"type": "string",
					"description": "Rust regex matched against the indexed symbol name. Callable names may include their parameter signature."
				},
				"include_non_navigable": {
					"type": "boolean",
					"description": "Include locals, params, and other non-navigation symbols."
				},
				"limit": {
					"type": "integer",
					"minimum": 1,
					"maximum": super::scope::MAX_LIMIT,
					"description": "Maximum symbols to emit."
				},
				"cursor": {
					"oneOf": [{ "type": "integer" }, { "type": "string" }],
					"description": "Opaque row offset returned in next calls."
				}
			},
			"additionalProperties": false
		})
	}
}

impl McpTool for SymbolsTool {
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
		let request = SymbolRequest::from_arguments(arguments, output.agent_options())
			.map_err(ToolError::failed)?;
		read_symbols(context, &request).map_err(ToolError::failed)
	}
}

struct SymbolRequest {
	action: SymbolAction,
	uri: String,
	scope: SymbolScopeFilter,
	paging: Paging,
	output: AgentOutputOptions,
}

impl SymbolRequest {
	fn from_arguments(arguments: &Value, output: AgentOutputOptions) -> anyhow::Result<Self> {
		Ok(Self {
			action: SymbolAction::from_arguments(arguments)?,
			uri: arguments
				.get("uri")
				.and_then(Value::as_str)
				.unwrap_or(DEFAULT_SYMBOL_URI)
				.to_string(),
			scope: SymbolScopeFilter::from_arguments(arguments)?,
			paging: Paging::from_arguments_for_volume(arguments, output)?,
			output,
		})
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mcp) enum SymbolAction {
	List,
	Insights,
}

impl SymbolAction {
	fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		match arguments
			.get("action")
			.and_then(Value::as_str)
			.unwrap_or("list")
		{
			"list" => Ok(Self::List),
			"insights" => Ok(Self::Insights),
			action => anyhow::bail!("unknown symbol action `{action}`"),
		}
	}
}

fn read_symbols(context: &McpContext, request: &SymbolRequest) -> anyhow::Result<ToolResult> {
	let uri = request.uri.as_str();
	if !is_workspace_uri(uri, context.scheme(), DEFAULT_SYMBOL_URI) {
		anyhow::bail!(
			"unsupported URI; use workspace or {}workspace",
			context.scheme()
		);
	}
	match request.action {
		SymbolAction::List => {
			let response = context.query_refreshed(
				Query::SymbolSearch(symbol_query(&request.scope)),
				request.paging.daemon_page(),
			)?;
			let QueryResult::SymbolList(result) = response.result else {
				anyhow::bail!("unexpected daemon response for symbols list");
			};
			Ok(ToolResult::templated(daemon_symbol_list_template(
				context.scheme(),
				uri,
				&request.scope,
				SymbolListRender {
					paging: request.paging,
					output: request.output,
					cursor: response.next_cursor.as_ref(),
				},
				&result,
			)?))
		}
		SymbolAction::Insights => {
			let response = context.query_refreshed(
				Query::SymbolInsights(symbol_query(&request.scope)),
				code_moniker_query::Page::default(),
			)?;
			let QueryResult::SymbolInsights(result) = response.result else {
				anyhow::bail!("unexpected daemon response for symbols insights");
			};
			Ok(ToolResult::templated(daemon_symbol_insights_template(
				context.scheme(),
				uri,
				&request.scope,
				request.paging,
				&result,
				request.output,
			)?))
		}
	}
}

fn daemon_symbol_list_template(
	scheme: &str,
	request_uri: &str,
	scope: &SymbolScopeFilter,
	render: SymbolListRender<'_>,
	result: &SymbolListResult,
) -> anyhow::Result<TemplateOutput> {
	let page = SymbolListPage {
		rows: result.rows.iter().map(SymbolRowView::from).collect(),
		total: result.total,
		show_empty_hint: result.total == 0,
	};
	symbol_list_template(scheme, request_uri, scope, render, page)
}

fn daemon_symbol_insights_template(
	scheme: &str,
	request_uri: &str,
	scope: &SymbolScopeFilter,
	paging: Paging,
	result: &SymbolInsightsResult,
	output: AgentOutputOptions,
) -> anyhow::Result<TemplateOutput> {
	let groups = vec![
		count_group("Languages", &result.languages, paging.limit),
		count_group("Kinds", &result.kinds, paging.limit),
		count_group("Shapes", &result.shapes, paging.limit),
		count_group(
			"Top files by symbols",
			&result.top_files_by_symbols,
			paging.limit,
		),
		count_group("Top files by refs", &result.top_files_by_refs, paging.limit),
	];
	symbol_insights_template(
		scheme,
		request_uri,
		scope,
		paging,
		output,
		SymbolMetricsView {
			files: result.files,
			symbols: result.symbols,
			references: result.references,
			navigable_symbols: result.navigable_symbols,
			non_navigable_symbols: result.non_navigable_symbols,
			groups,
		},
	)
}

fn symbol_query(scope: &SymbolScopeFilter) -> SymbolSearchQuery {
	SymbolSearchQuery {
		workspace: None,
		text: None,
		path: scope.files.paths.clone(),
		lang: scope.files.langs.clone(),
		kind: scope.kinds.clone(),
		shape: scope
			.shapes
			.iter()
			.map(|shape| shape.as_str().to_string())
			.collect(),
		name: scope.name.as_ref().map(|regex| regex.as_str().to_string()),
		include_non_navigable: scope.include_non_navigable,
		include_code: false,
		context_lines: 0,
		projection: Vec::new(),
	}
}

fn signed_callable_name_hint(scope: &SymbolScopeFilter) -> Option<CallableHintView> {
	let callable_scope = (scope.kinds.is_empty()
		|| scope
			.kinds
			.iter()
			.any(|kind| Shape::for_kind(kind.as_bytes()) == Shape::Callable))
		&& (scope.shapes.is_empty() || scope.shapes.contains(&Shape::Callable));
	if !callable_scope {
		return None;
	}
	let name = scope.name.as_ref().map(regex::Regex::as_str)?;
	let bare_name = name
		.strip_prefix('^')
		.and_then(|name| name.strip_suffix('$'))
		.filter(|name| {
			!name.is_empty()
				&& name
					.chars()
					.all(|character| character.is_ascii_alphanumeric() || character == '_')
		})?;
	let mut argument = String::new();
	append_call_string_arg(&mut argument, "name", &format!("^{bare_name}\\("));
	Some(CallableHintView {
		name_argument: argument.trim().to_string(),
	})
}

#[derive(Clone, Copy)]
struct SymbolListRender<'a> {
	paging: Paging,
	output: AgentOutputOptions,
	cursor: Option<&'a code_moniker_query::QueryCursor>,
}

struct SymbolListPage {
	rows: Vec<SymbolRowView>,
	total: usize,
	show_empty_hint: bool,
}

struct SymbolNextCall<'a> {
	action: SymbolAction,
	limit: usize,
	cursor: Option<&'a code_moniker_query::QueryCursor>,
	output: AgentOutputOptions,
}

fn symbol_list_template(
	scheme: &str,
	request_uri: &str,
	scope: &SymbolScopeFilter,
	render: SymbolListRender<'_>,
	page: SymbolListPage,
) -> anyhow::Result<TemplateOutput> {
	let start = render.paging.cursor.min(page.total);
	let end = start.saturating_add(page.rows.len()).min(page.total);
	let mut next_calls = Vec::new();
	if let Some(cursor) = render.cursor {
		next_calls.push(symbols_next_call(
			scheme,
			scope,
			SymbolNextCall {
				action: SymbolAction::List,
				limit: render.paging.limit,
				cursor: Some(cursor),
				output: render.output,
			},
		));
	}
	if !render.output.compact {
		next_calls.push(symbols_next_call(
			scheme,
			scope,
			SymbolNextCall {
				action: SymbolAction::Insights,
				limit: render.output.default_page_limit(),
				cursor: None,
				output: render.output,
			},
		));
		next_calls.push(workspace_read_call(scheme, scope, 2, render.output));
	}
	let context = SymbolListView {
		uri: normalize_workspace_uri(scheme, request_uri, DEFAULT_SYMBOL_URI),
		partial: render.cursor.is_some(),
		start,
		end,
		total: page.total,
		next_cursor: render.cursor.map(|cursor| cursor.offset),
		limit: render.paging.limit,
		volume: render.output.budget.as_str(),
		compact: render.output.compact,
		scope: scope_rows(scope),
		rows: page.rows,
		hint: page
			.show_empty_hint
			.then(|| signed_callable_name_hint(scope))
			.flatten(),
		next_calls,
	};
	symbols_presentation::list(&context)
}

fn symbol_insights_template(
	scheme: &str,
	request_uri: &str,
	scope: &SymbolScopeFilter,
	paging: Paging,
	output: AgentOutputOptions,
	metrics: SymbolMetricsView,
) -> anyhow::Result<TemplateOutput> {
	let mut next_calls = vec![symbols_next_call(
		scheme,
		scope,
		SymbolNextCall {
			action: SymbolAction::List,
			limit: output.default_page_limit(),
			cursor: None,
			output,
		},
	)];
	if !output.compact {
		next_calls.push(workspace_read_call(scheme, scope, 3, output));
	}
	let context = SymbolInsightsView {
		uri: normalize_workspace_uri(scheme, request_uri, DEFAULT_SYMBOL_URI),
		files: metrics.files,
		symbols: metrics.symbols,
		references: metrics.references,
		limit: paging.limit,
		volume: output.budget.as_str(),
		scope: scope_rows(scope),
		navigable_symbols: metrics.navigable_symbols,
		non_navigable_symbols: metrics.non_navigable_symbols,
		groups: metrics.groups,
		next_calls,
	};
	symbols_presentation::insights(&context)
}

fn symbols_next_call(
	scheme: &str,
	scope: &SymbolScopeFilter,
	call: SymbolNextCall<'_>,
) -> ToolCallView {
	let mut arguments = String::new();
	append_call_string_arg(
		&mut arguments,
		"action",
		match call.action {
			SymbolAction::List => "list",
			SymbolAction::Insights => "insights",
		},
	);
	scope.append_call_args(&mut arguments);
	append_call_number_arg(&mut arguments, "limit", call.limit);
	if let Some(cursor) = call.cursor {
		append_call_cursor_arg(&mut arguments, "cursor", cursor);
	}
	append_call_string_arg(&mut arguments, "budget", call.output.budget.as_str());
	if !call.output.compact {
		append_call_bool_arg(&mut arguments, "compact", false);
	}
	ToolCallView {
		tool: "code_moniker_symbols",
		uri: format!("{scheme}workspace"),
		arguments,
	}
}

fn workspace_read_call(
	scheme: &str,
	scope: &SymbolScopeFilter,
	depth: usize,
	output: AgentOutputOptions,
) -> ToolCallView {
	let mut arguments = String::new();
	scope.files.append_call_args(&mut arguments);
	append_call_number_arg(&mut arguments, "depth", depth);
	append_call_string_arg(&mut arguments, "budget", output.budget.as_str());
	if !output.compact {
		append_call_bool_arg(&mut arguments, "compact", false);
	}
	ToolCallView {
		tool: "code_moniker_read",
		uri: format!("{scheme}workspace"),
		arguments,
	}
}

#[derive(Serialize)]
struct SymbolListView {
	uri: String,
	partial: bool,
	start: usize,
	end: usize,
	total: usize,
	next_cursor: Option<usize>,
	limit: usize,
	volume: &'static str,
	compact: bool,
	scope: Vec<ScopeRowView>,
	rows: Vec<SymbolRowView>,
	hint: Option<CallableHintView>,
	next_calls: Vec<ToolCallView>,
}

#[derive(Serialize)]
struct SymbolInsightsView {
	uri: String,
	files: usize,
	symbols: usize,
	references: usize,
	limit: usize,
	volume: &'static str,
	scope: Vec<ScopeRowView>,
	navigable_symbols: usize,
	non_navigable_symbols: usize,
	groups: Vec<CountGroupView>,
	next_calls: Vec<ToolCallView>,
}

struct SymbolMetricsView {
	files: usize,
	symbols: usize,
	references: usize,
	navigable_symbols: usize,
	non_navigable_symbols: usize,
	groups: Vec<CountGroupView>,
}

#[derive(Serialize)]
struct SymbolRowView {
	kind: String,
	name: String,
	location: String,
	uri: String,
}

impl From<&SymbolDto> for SymbolRowView {
	fn from(symbol: &SymbolDto) -> Self {
		Self {
			kind: symbol.kind.to_owned(),
			name: symbol.name.to_owned(),
			location: format!("{}{}", symbol.file, line_range_suffix(symbol.line_range)),
			uri: symbol.uri.to_owned(),
		}
	}
}

#[derive(Serialize)]
struct ToolCallView {
	tool: &'static str,
	uri: String,
	arguments: String,
}

#[derive(Serialize)]
struct CallableHintView {
	name_argument: String,
}

#[derive(Serialize)]
struct CountGroupView {
	title: &'static str,
	rows: Vec<CountRowView>,
}

#[derive(Serialize)]
struct CountRowView {
	name: String,
	count: usize,
}

fn count_group(
	title: &'static str,
	rows: &[code_moniker_query::CountDto],
	limit: usize,
) -> CountGroupView {
	CountGroupView {
		title,
		rows: rows
			.iter()
			.take(limit)
			.map(|row| CountRowView {
				name: row.name.to_owned(),
				count: row.count,
			})
			.collect(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn symbol(name: &str) -> SymbolDto {
		SymbolDto {
			root: "/workspace".to_string(),
			uri: format!("code+moniker://./lang:rs/module:lib/fn:{name}"),
			id: "symbol:1:1".to_string(),
			name: name.to_string(),
			kind: "fn".to_string(),
			visibility: "public".to_string(),
			signature: String::new(),
			file: "src/lib.rs".to_string(),
			language: "rs".to_string(),
			line_range: Some((10, 12)),
			navigable: true,
			score: None,
			match_reason: None,
			source: None,
		}
	}

	#[test]
	fn volume_profiles_shape_symbol_pages_before_rendering() {
		for (budget, expected) in [("small", 20), ("medium", 80), ("full", 500)] {
			let arguments = serde_json::json!({"budget": budget});
			let output = AgentOutputOptions::from_arguments(&arguments).expect("output options");
			let request =
				SymbolRequest::from_arguments(&arguments, output).expect("symbol request");
			assert_eq!(request.paging.limit, expected, "{budget}");
		}

		let arguments = serde_json::json!({"budget": "medium", "compact": false});
		let output = AgentOutputOptions::from_arguments(&arguments).expect("output options");
		let request = SymbolRequest::from_arguments(&arguments, output).expect("symbol request");
		let result = SymbolListResult {
			rows: Vec::new(),
			total: 0,
			hint: None,
		};
		let template = daemon_symbol_list_template(
			"code+moniker://",
			"workspace",
			&request.scope,
			SymbolListRender {
				paging: request.paging,
				output,
				cursor: None,
			},
			&result,
		)
		.expect("symbol list template");
		let rendered = template
			.render(RenderOptions {
				compact: output.compact,
				scheme: "code+moniker://",
				runtime: None,
			})
			.expect("symbol list");
		crate::presentation::tests::validate_agent_markdown(&rendered, "Workspace symbols", false)
			.expect("symbols CommonMark");
		assert!(rendered.contains("page-size: 80"), "{rendered}");
		assert!(rendered.contains("budget=\"medium\""), "{rendered}");
		assert!(rendered.contains("compact=false"), "{rendered}");
	}

	#[test]
	fn symbol_list_template_renders_filtered_page_and_replayable_cursor() {
		let arguments = serde_json::json!({
			"path": "src/**",
			"lang": "rs",
			"name": "^run",
			"budget": "small"
		});
		let output = AgentOutputOptions::from_arguments(&arguments).expect("output options");
		let request = SymbolRequest::from_arguments(&arguments, output).expect("symbol request");
		let cursor = code_moniker_query::QueryCursor::new(1, None);
		let result = SymbolListResult {
			rows: vec![symbol("run()")],
			total: 2,
			hint: None,
		};
		let template = daemon_symbol_list_template(
			"code+moniker://",
			"workspace",
			&request.scope,
			SymbolListRender {
				paging: request.paging,
				output,
				cursor: Some(&cursor),
			},
			&result,
		)
		.expect("symbol list template");
		let rendered = template
			.render(RenderOptions {
				compact: true,
				scheme: "code+moniker://",
				runtime: None,
			})
			.expect("symbol list");
		crate::presentation::tests::validate_agent_markdown(&rendered, "Workspace symbols", false)
			.expect("symbols CommonMark");
		assert!(
			rendered.contains("`fn` `run()` `src/lib.rs:10-12`"),
			"{rendered}"
		);
		assert!(rendered.contains("uri: `rs:"), "{rendered}");
		assert!(rendered.contains("path=\"src/**\""), "{rendered}");
		assert!(rendered.contains("cursor=1"), "{rendered}");
		assert!(rendered.contains("budget=\"small\""), "{rendered}");
	}

	#[test]
	fn symbol_templates_explain_empty_callable_searches_and_render_insights() {
		let arguments = serde_json::json!({"shape": "callable", "name": "^call$"});
		let output = AgentOutputOptions::from_arguments(&arguments).expect("output options");
		let request = SymbolRequest::from_arguments(&arguments, output).expect("symbol request");
		let empty = SymbolListResult {
			rows: Vec::new(),
			total: 0,
			hint: None,
		};
		let empty_text = daemon_symbol_list_template(
			"code+moniker://",
			"workspace",
			&request.scope,
			SymbolListRender {
				paging: request.paging,
				output,
				cursor: None,
			},
			&empty,
		)
		.expect("empty symbol template")
		.render(RenderOptions {
			compact: true,
			scheme: "code+moniker://",
			runtime: None,
		})
		.expect("empty symbol list");
		assert!(
			empty_text.contains("callable names may include their parameter signature"),
			"{empty_text}"
		);

		let count = |name: &str, count| code_moniker_query::CountDto {
			name: name.to_string(),
			count,
		};
		let insights = SymbolInsightsResult {
			files: 1,
			symbols: 2,
			references: 3,
			navigable_symbols: 2,
			non_navigable_symbols: 0,
			languages: vec![count("rs", 1)],
			kinds: vec![count("fn", 2)],
			shapes: vec![count("callable", 2)],
			top_files_by_symbols: vec![count("src/lib.rs", 2)],
			top_files_by_refs: vec![count("src/lib.rs", 3)],
		};
		let insights_text = daemon_symbol_insights_template(
			"code+moniker://",
			"workspace",
			&request.scope,
			request.paging,
			&insights,
			output,
		)
		.expect("insights template")
		.render(RenderOptions {
			compact: true,
			scheme: "code+moniker://",
			runtime: None,
		})
		.expect("insights");
		crate::presentation::tests::validate_agent_markdown(
			&insights_text,
			"Symbol insights",
			true,
		)
		.expect("insights CommonMark");
		assert!(insights_text.contains("| rs | 1 |"), "{insights_text}");
		assert!(
			insights_text.contains("| src/lib.rs | 3 |"),
			"{insights_text}"
		);
	}
}
