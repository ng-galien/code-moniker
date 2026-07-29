use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use code_moniker_daemon::{WorkspaceCancellation, WorkspaceDaemon};
use rmcp::model::{
	CallToolRequestParams, CallToolResult, Content, Implementation, ListToolsResult,
	PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::streamable_http_server::{
	StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
};
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt};
use serde_json::Value;

use super::context::{InProcessPreloadParts, McpContext, PreloadStatus};
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

pub(crate) async fn serve_stdio(context: McpContext) -> anyhow::Result<()> {
	let _preload = context.in_process_preload_parts().map(start_preload);
	let service = CodeMonikerMcp::new(context)
		.serve(rmcp::transport::stdio())
		.await?;
	service.waiting().await?;
	Ok(())
}

struct InProcessPreload {
	cancellation: WorkspaceCancellation,
	worker: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl Drop for InProcessPreload {
	fn drop(&mut self) {
		self.cancellation.cancel();
		self.worker.abort();
	}
}

fn start_preload(parts: InProcessPreloadParts) -> InProcessPreload {
	let cancellation = WorkspaceCancellation::default();
	let worker_cancellation = cancellation.clone();
	let worker = tokio::task::spawn_blocking(move || run_preload(parts, worker_cancellation));
	InProcessPreload {
		cancellation,
		worker,
	}
}

fn run_preload(
	parts: InProcessPreloadParts,
	cancellation: WorkspaceCancellation,
) -> anyhow::Result<()> {
	let result = (|| {
		let mut daemon = WorkspaceDaemon::new_with_config(parts.config)?;
		daemon
			.refresh_cancellable(cancellation.clone())
			.map_err(|error| anyhow::anyhow!(error.to_string()))?;
		anyhow::ensure!(!cancellation.is_cancelled(), "workspace preload cancelled");
		*parts
			.daemon_slot
			.lock()
			.map_err(|_| anyhow::anyhow!("daemon lock poisoned during preload publish"))? = daemon;
		*parts
			.preload_status
			.lock()
			.map_err(|_| anyhow::anyhow!("preload status lock poisoned"))? = PreloadStatus::Ready;
		Ok(())
	})();
	if let Err(error) = &result {
		*parts
			.preload_status
			.lock()
			.map_err(|_| anyhow::anyhow!("preload status lock poisoned"))? =
			PreloadStatus::Failed(format!("{error:#}"));
	}
	result
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
		let workspace = self.context.workspace_label();
		ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
			.with_server_info(Implementation::new(
				"code-moniker",
				env!("CARGO_PKG_VERSION"),
			))
			.with_instructions(format!(
				concat!(
					"code-moniker serves a symbolic index of the workspace: every definition ",
					"has a stable moniker URI (scheme code+moniker://) and relations between ",
					"symbols (calls, uses_type, extends…) are counted facts. ",
					"This MCP is the complete agent surface: do not shell out to the daemon or ",
					"repeat an MCP exploration with direct queries. Start with ",
					"code_moniker_read uri:\"workspace\" and expected_roots set to the current ",
					"absolute workspace roots for a fail-closed overview, or ",
					"code_moniker_symbols to find a symbol and obtain its compact moniker. ",
					"Never guess a moniker. By default budget=small and compact=true: every tool ",
					"has a hard response ceiling, and canonical symbol URIs in descriptive data ",
					"are rendered in the existing compact moniker form, for example ",
					"rs:crates/cli/src/mcp.tools.fn:run(). Compact monikers returned by the ",
					"server can be passed directly to symbol tools; canonical URIs and symbol ",
					"ids remain accepted. Generated tool calls keep canonical URIs and can be ",
					"copied verbatim. Set compact=false for canonical verbose data and ",
					"additional guided follow-up calls. ",
					"Compact symbol rows omit duplicated per-row usages calls; pass the row's ",
					"compact moniker to code_moniker_usages when needed. ",
					"Prefer scoped filters and stop once the question is answered; paging, ",
					"budget=full, code, and broader scopes are opt-in. Use code_moniker_query ",
					"only for a read-only daemon capability not covered by an intent tool; ",
					"query.describe exposes the live grammar, and queries batches up to four ",
					"operations at one generation. ",
					"Then code_moniker_usages for callers/callees, code_moniker_graph for ",
					"coupling between scopes, or code_moniker_context once before a ",
					"structural edit to combine graph, notes, applicable rules, local ",
					"changes and canonical suggested checks. Use code_moniker_rules for architecture checks, ",
					"code_moniker_diff for structural change review. ",
					"Responses contain uri, completeness, and a body; next is optional and ",
					"appears only when a useful follow-up exists. ",
					"This server is bound to workspace roots: {workspace}. Start every session ",
					"with code_moniker_read uri:\"workspace\" and expected_roots set to the ",
					"current Codex workspace roots. Stop on workspace_mismatch."
				),
				workspace = workspace
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
		let result = match result {
			Err(error) if !error.is_unknown_tool() => {
				let uri = arguments
					.get("uri")
					.and_then(Value::as_str)
					.unwrap_or("workspace");
				registry
					.finalize_error(
						&name,
						&arguments,
						problem_lmnav(uri, &name, &error.to_string()),
					)
					.map(Ok)
					.unwrap_or(Err(error))
			}
			result => result,
		};
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
	let fix_hint = if message.starts_with("workspace_mismatch:") {
		"stop and connect to the project-owned code-moniker MCP server for the expected roots"
	} else if message.starts_with("workspace_identity_required:") {
		"retry the workspace read with expected_roots set to the current absolute workspace roots"
	} else {
		"retry with a supported URI and bounded arguments"
	};
	format!(
		"uri: {uri}\ncompleteness: partial (error)\n\nproblem: {message}\nwhere: {tool}\nfix_hint: {fix_hint}\n"
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn known_tool_errors_cross_the_agent_output_boundary() {
		let uri = "x".repeat(1_500);
		let arguments = serde_json::json!({
			"uri": uri,
			"max_chars": 1_000
		});
		let result = ToolRegistry::new()
			.finalize_error(
				"code_moniker_read",
				&arguments,
				problem_lmnav(&uri, "code_moniker_read", "symbol not found"),
			)
			.expect("known tool contract");
		let response = call_result("code_moniker_read", &arguments, Ok(result));
		let response = serde_json::to_value(response).expect("serialize call result");
		let text = response["content"][0]["text"].as_str().expect("error text");

		assert_eq!(response["isError"].as_bool(), Some(true));
		assert!(text.chars().count() <= 1_000, "{}", text.chars().count());
		assert!(text.contains("truncated_by: max_chars"), "{text}");
	}
}
