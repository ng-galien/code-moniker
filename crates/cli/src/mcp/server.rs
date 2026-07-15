use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use rmcp::model::{
	CallToolRequestParams, CallToolResult, Content, Implementation, ListToolsResult,
	PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::streamable_http_server::{
	StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
};
use rmcp::{ErrorData as McpError, ServerHandler};
use serde_json::Value;

use super::context::McpContext;
use super::tools::{ToolRegistry, ToolResult};

pub(crate) fn router(context: McpContext) -> axum::Router<()> {
	let service: StreamableHttpService<CodeMonikerMcp, NeverSessionManager> =
		StreamableHttpService::new(
			move || Ok(CodeMonikerMcp::new(context.clone())),
			Default::default(),
			StreamableHttpServerConfig::default()
				.with_stateful_mode(false)
				.with_json_response(true)
				.with_sse_keep_alive(None)
				.with_allowed_hosts(["localhost".to_string(), "127.0.0.1".to_string()]),
		);
	axum::Router::new().nest_service("/mcp", service)
}

#[derive(Clone)]
struct CodeMonikerMcp {
	context: McpContext,
	registry: Arc<ToolRegistry>,
}

impl CodeMonikerMcp {
	fn new(context: McpContext) -> Self {
		Self {
			context,
			registry: Arc::new(ToolRegistry::new()),
		}
	}
}

impl ServerHandler for CodeMonikerMcp {
	fn get_info(&self) -> ServerInfo {
		tracing::info!(event = "initialize_info", "mcp server info requested");
		ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
			.with_server_info(Implementation::new(
				"code-moniker",
				env!("CARGO_PKG_VERSION"),
			))
			.with_instructions(concat!(
				"code-moniker serves a symbolic index of the workspace: every definition ",
				"has a stable moniker URI (scheme code+moniker://) and relations between ",
				"symbols (calls, uses_type, extends…) are counted facts. ",
				"Start with code_moniker_read uri:\"workspace\" for an overview, or ",
				"code_moniker_symbols to find a symbol and obtain its exact URI. ",
				"Never guess a URI. By default compact=true: repeated canonical monikers ",
				"in descriptive data may be declared once under aliases and referenced ",
				"as @N. Each alias is local to that single response: it is not stored by ",
				"the server, is invalid as a tool argument, and must never be reused in a ",
				"later response. Generated tool calls always keep canonical URIs and can ",
				"be copied verbatim. When composing a call from aliased data, resolve @N ",
				"through that response's aliases section first. Set compact=false for ",
				"canonical verbose data and additional guided follow-up calls. ",
				"Compact symbol rows omit duplicated per-row usages calls; pass the row's ",
				"canonical URI to code_moniker_usages when needed. ",
				"Then code_moniker_usages for callers/callees, code_moniker_graph for ",
				"coupling between scopes, code_moniker_rules for architecture checks, ",
				"code_moniker_diff for structural change review. ",
				"Responses contain uri, completeness, and a body; next is optional and ",
				"appears only when a useful follow-up exists."
			))
	}

	fn list_tools(
		&self,
		_request: Option<PaginatedRequestParams>,
		_context: RequestContext<RoleServer>,
	) -> impl Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
		let started = Instant::now();
		let tools = self.registry.tools();
		tracing::info!(
			event = "tools_list",
			tools = tools.len(),
			elapsed_ms = started.elapsed().as_millis(),
			"mcp tools listed"
		);
		std::future::ready(Ok(ListToolsResult::with_all_items(tools)))
	}

	fn call_tool(
		&self,
		request: CallToolRequestParams,
		_context: RequestContext<RoleServer>,
	) -> impl Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
		dispatch_tool_call(self.registry.clone(), self.context.clone(), request)
	}

	fn get_tool(&self, name: &str) -> Option<Tool> {
		self.registry
			.tools()
			.into_iter()
			.find(|tool| tool.name == name)
	}
}

async fn dispatch_tool_call(
	registry: Arc<ToolRegistry>,
	context: McpContext,
	request: CallToolRequestParams,
) -> Result<CallToolResult, McpError> {
	let started = Instant::now();
	let name = request.name.to_string();
	let arguments = Value::Object(request.arguments.unwrap_or_default());
	tracing::info!(event = "tool_call_started", tool = %name, "mcp tool call started");
	let (name, arguments, result) = tokio::task::spawn_blocking(move || {
		let result = registry.call(&context, &name, &arguments);
		(name, arguments, result)
	})
	.await
	.map_err(|join_error| McpError::internal_error(join_error.to_string(), None))?;
	let status = tool_result_status(&result);
	let response = call_result(&name, &arguments, result);
	tracing::info!(
		event = "tool_call_finished",
		tool = %name,
		status,
		elapsed_ms = started.elapsed().as_millis(),
		"mcp tool call finished"
	);
	Ok(response)
}

fn tool_result_status(result: &Result<ToolResult, super::tools::ToolError>) -> &'static str {
	match result {
		Ok(result) if result.is_error => "tool_error",
		Ok(_) => "ok",
		Err(error) if error.is_unknown_tool() => "unknown_tool",
		Err(_) => "failed",
	}
}

fn call_result(
	name: &str,
	arguments: &Value,
	result: Result<ToolResult, super::tools::ToolError>,
) -> CallToolResult {
	match result {
		Ok(result) if result.is_error => CallToolResult::error(vec![Content::text(result.text)]),
		Ok(result) => CallToolResult::success(vec![Content::text(result.text)]),
		Err(error) if error.is_unknown_tool() => {
			CallToolResult::error(vec![Content::text(format!("unknown tool: {name}"))])
		}
		Err(error) => {
			let uri = arguments
				.get("uri")
				.and_then(Value::as_str)
				.unwrap_or("workspace");
			CallToolResult::error(vec![Content::text(problem_lmnav(
				uri,
				name,
				&error.to_string(),
			))])
		}
	}
}

fn problem_lmnav(uri: &str, tool: &str, message: &str) -> String {
	format!(
		"uri: {uri}\ncompleteness: partial (error)\n\nproblem: {message}\nwhere: {tool}\nfix_hint: retry with a supported URI and bounded arguments\n"
	)
}
