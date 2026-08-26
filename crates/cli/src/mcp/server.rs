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
use tracing::Instrument as _;

use code_moniker_query::bounded_debug;

use super::context::{InProcessPreloadParts, McpContext};
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
	let preload_span = detached_preload_span();
	let worker = tokio::task::spawn_blocking(move || {
		preload_span.in_scope(|| run_preload(parts, worker_cancellation))
	});
	InProcessPreload {
		cancellation,
		worker,
	}
}

fn detached_preload_span() -> tracing::Span {
	let span = tracing::info_span!(
		parent: None,
		"workspace.background_operation",
		operation.name = "mcp.initial_preload",
		operation.async = true,
	);
	#[cfg(feature = "telemetry")]
	{
		use opentelemetry::KeyValue;
		use opentelemetry::trace::TraceContextExt as _;
		use tracing_opentelemetry::OpenTelemetrySpanExt as _;

		let context = tracing::Span::current().context();
		let span_context = context.span().span_context().clone();
		span.add_link_with_attributes(
			span_context,
			vec![KeyValue::new("operation.name", "mcp.initial_preload")],
		);
	}
	span
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
			.lifecycle
			.lock()
			.map_err(|_| anyhow::anyhow!("workspace lifecycle lock poisoned"))? =
			code_moniker_query::WorkspaceLifecycle::ready();
		Ok(())
	})();
	if let Err(error) = &result {
		*parts
			.lifecycle
			.lock()
			.map_err(|_| anyhow::anyhow!("workspace lifecycle lock poisoned"))? =
			code_moniker_query::WorkspaceLifecycle::failed(format!("{error:#}"));
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
		ServerInfo::new(
			ServerCapabilities::builder()
				.enable_tools()
				.enable_tool_list_changed()
				.build(),
		)
		.with_server_info(Implementation::new(
			"code-moniker",
			env!("CARGO_PKG_VERSION"),
		))
		.with_instructions(server_instructions(&workspace))
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

fn server_instructions(workspace: &str) -> String {
	format!(
		concat!(
			"Code Moniker exposes indexed symbols and relationships for roots: ",
			"{workspace}. Use it for targeted structural exploration when callers, ",
			"callees, coupling, ownership or change impact add value; ordinary known-file ",
			"and exact-string work does not require it. Verify expected_roots once before ",
			"the first workspace-wide exploration, after reconnect or when roots change, ",
			"not before every call. Prefer narrow compact calls with small budgets, stop ",
			"when the question is answered, and never guess a moniker. Do not repeat an ",
			"MCP exploration through the CLI or raw daemon."
		),
		workspace = workspace
	)
}

async fn dispatch_tool_call(
	registry: Arc<ToolRegistry>,
	context: McpContext,
	request: CallToolRequestParams,
) -> Result<CallToolResult, McpError> {
	let started = Instant::now();
	let name = request.name.to_string();
	let arguments = Value::Object(request.arguments.unwrap_or_default());
	let span = tool_call_span(&name, &arguments);
	let result_span = span.clone();
	async move {
		tracing::info!(event = "tool_call_started", tool = %name, "mcp tool call started");
		let blocking_span = tracing::Span::current();
		let joined = tokio::task::spawn_blocking(move || {
			let _entered = blocking_span.enter();
			let result = registry.call(&context, &name, &arguments);
			let result = match result {
				Err(error) if !error.is_unknown_tool() => registry
					.render_problem(
						&name,
						&arguments,
						context.scheme(),
						Some(context.runtime_label()),
						&error.to_string(),
					)
					.map(Ok)
					.unwrap_or(Err(error)),
				result => result,
			};
			(name, arguments, result)
		})
		.await;
		let (name, _arguments, result) = match joined {
			Ok(result) => result,
			Err(join_error) => {
				result_span.record("mcp.tool.status", "join_error");
				return Err(McpError::internal_error(join_error.to_string(), None));
			}
		};
		let status = tool_result_status(&result);
		result_span.record("mcp.tool.status", status);
		let response = call_result(&name, result);
		result_span.record("mcp.response.content_count", response.content.len());
		result_span.record("mcp.response.is_error", response.is_error.unwrap_or(false));
		tracing::info!(
			event = "tool_call_finished",
			tool = %name,
			status,
			elapsed_ms = started.elapsed().as_millis(),
			"mcp tool call finished"
		);
		Ok(response)
	}
	.instrument(span)
	.await
}

fn tool_call_span(name: &str, arguments: &Value) -> tracing::Span {
	tracing::info_span!(
		parent: None,
		"mcp.tool.call",
		mcp.tool.name = %name,
		mcp.tool.arguments = %bounded_debug(&arguments, 4_096),
		mcp.tool.status = tracing::field::Empty,
		mcp.response.content_count = tracing::field::Empty,
		mcp.response.is_error = tracing::field::Empty,
	)
}

fn tool_result_status(result: &Result<ToolResult, super::tools::ToolError>) -> &'static str {
	match result {
		Ok(result) if result.is_error => "tool_error",
		Ok(_) => "ok",
		Err(error) if error.is_unknown_tool() => "unknown_tool",
		Err(_) => "failed",
	}
}

fn call_result(name: &str, result: Result<ToolResult, super::tools::ToolError>) -> CallToolResult {
	match result {
		Ok(result) => {
			let (is_error, text, structured_content) = result.into_response_parts();
			let content = if structured_content.is_some() {
				Vec::new()
			} else {
				vec![Content::text(text)]
			};
			let mut response = if is_error {
				CallToolResult::error(content)
			} else {
				CallToolResult::success(content)
			};
			response.structured_content = structured_content;
			response
		}
		Err(error) if error.is_unknown_tool() => {
			CallToolResult::error(vec![Content::text(format!("unknown tool: {name}"))])
		}
		Err(error) => CallToolResult::error(vec![Content::text(error.to_string())]),
	}
}

#[cfg(test)]
mod tests {
	use std::sync::{Arc, Mutex};

	use super::*;
	use tracing::Subscriber;
	use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

	#[derive(Clone)]
	struct RootCapture(Arc<Mutex<Option<bool>>>);

	impl<S: Subscriber> Layer<S> for RootCapture {
		fn on_new_span(
			&self,
			attributes: &tracing::span::Attributes<'_>,
			_id: &tracing::Id,
			_context: Context<'_, S>,
		) {
			if attributes.metadata().name() == "mcp.tool.call" {
				*self.0.lock().unwrap() = Some(attributes.is_root());
			}
		}
	}

	#[test]
	fn server_instructions_are_bounded_and_non_prescriptive() {
		let instructions = server_instructions("/workspace");

		assert!(instructions.len() < 800, "{}", instructions.len());
		assert!(instructions.contains("Verify expected_roots once"));
		assert!(!instructions.contains("Start every session"));
		assert!(instructions.contains("known-file"));
	}

	#[test]
	fn tool_call_span_is_root_even_inside_a_process_span() {
		let captured = Arc::new(Mutex::new(None));
		let subscriber = tracing_subscriber::registry().with(RootCapture(captured.clone()));
		tracing::subscriber::with_default(subscriber, || {
			let process = tracing::info_span!("cli.command");
			let _entered = process.enter();
			let span = tool_call_span("code_moniker_query", &serde_json::json!({}));
			let _entered = span.enter();
		});

		assert_eq!(*captured.lock().unwrap(), Some(true));
	}

	#[test]
	fn structured_tool_errors_do_not_also_emit_text_content() {
		let result = ToolResult::error("refresh failed")
			.with_structured_content(serde_json::json!({"problem": "refresh failed"}));
		let response = call_result("code_moniker_refresh", Ok(result));

		assert_eq!(response.is_error, Some(true));
		assert!(response.content.is_empty());
		assert_eq!(
			response.structured_content.unwrap()["problem"],
			"refresh failed"
		);
	}
}
