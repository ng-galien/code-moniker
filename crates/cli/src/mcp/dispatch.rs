use serde_json::{Value, json};

use super::context::McpContext;
use super::lmnav;
use super::tools::ToolRegistry;

pub(super) fn handle_json_rpc(request: &Value, context: &McpContext) -> Value {
	let id = request.get("id").cloned().unwrap_or(Value::Null);
	let method = request.get("method").and_then(Value::as_str).unwrap_or("");
	match method {
		"initialize" => initialize_response(id, request),
		"tools/list" => tools_list_response(id),
		"tools/call" => tool_call_response(id, request, context),
		"notifications/initialized" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
		_ => json_rpc_error(id, -32601, "method not found"),
	}
}

fn initialize_response(id: Value, request: &Value) -> Value {
	json!({
		"jsonrpc": "2.0",
		"id": id,
		"result": {
			"protocolVersion": protocol_version(request),
			"capabilities": { "tools": {} },
			"serverInfo": {
				"name": "code-moniker",
				"version": env!("CARGO_PKG_VERSION")
			},
			"instructions": "Use code_moniker_read with an LMNAV URI. Responses are compact text with uri, completeness, body, and next sections."
		}
	})
}

fn protocol_version(request: &Value) -> &str {
	request
		.pointer("/params/protocolVersion")
		.and_then(Value::as_str)
		.unwrap_or("2025-03-26")
}

fn tools_list_response(id: Value) -> Value {
	json!({
		"jsonrpc": "2.0",
		"id": id,
		"result": { "tools": ToolRegistry::new().descriptors() }
	})
}

fn tool_call_response(id: Value, request: &Value, context: &McpContext) -> Value {
	let Some(name) = request.pointer("/params/name").and_then(Value::as_str) else {
		return json_rpc_error(id, -32602, "missing tool name");
	};
	let arguments = request
		.pointer("/params/arguments")
		.cloned()
		.unwrap_or_else(|| json!({}));
	let uri = arguments
		.get("uri")
		.and_then(Value::as_str)
		.unwrap_or("workspace");
	let result = match ToolRegistry::new().call(context, name, &arguments) {
		Ok(result) => result,
		Err(error) if error.is_unknown_tool() => {
			return json_rpc_error(id, -32602, "unknown tool");
		}
		Err(error) => {
			let text = lmnav::problem(uri, name, &error.to_string());
			super::tools::ToolResult {
				text,
				is_error: true,
			}
		}
	};
	json!({
		"jsonrpc": "2.0",
		"id": id,
		"result": {
			"content": [{ "type": "text", "text": result.text }],
			"isError": result.is_error
		}
	})
}

fn json_rpc_error(id: Value, code: i64, message: &str) -> Value {
	json!({
		"jsonrpc": "2.0",
		"id": id,
		"error": {
			"code": code,
			"message": message
		}
	})
}
