use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;

use code_moniker_workspace::snapshot::{
	ResourceGeneration, SourceCatalog, SourceFileRecord, SourceFileRecordFields, SourceId,
	SourceUnit, SymbolId, SymbolRecord, SymbolRecordFields,
};
use serde_json::json;

use super::context::McpContext;
use super::dispatch::handle_json_rpc;
use super::server::server_addr;
use super::tools::read::render_explorer_lmnav;
use super::tools::scope::{Paging, ScopeFilter, SymbolScopeFilter};
use super::tools::symbols::render_symbols_lmnav;
use super::tools::{McpTool, ToolRegistry};
use super::{start, tools};
use crate::session::SessionOptions;

#[test]
fn read_description_matches_esac_style() {
	let descriptor = tools::read::ReadTool.descriptor();
	assert!(descriptor.description.starts_with("When to use:"));
	assert!(descriptor.description.contains("Read from code-moniker."));
	assert!(descriptor.description.contains("workspace"));
	assert!(descriptor.description.contains("limit/cursor"));
	assert_eq!(descriptor.input_schema["required"][0], "uri");
}

#[test]
fn read_root_summarizes_workspace_and_limits_explorer() {
	let catalog = SourceCatalog::new(
		ResourceGeneration::new(1),
		vec![
			SourceUnit::with_language("source:1", "root/src/main/java/App.java", "java"),
			SourceUnit::with_language("source:3", "root/src/main/java/Other.java", "java"),
			SourceUnit::with_language("source:2", "root/pom.xml", "xml"),
		],
	);
	let text = render_explorer_lmnav(
		"code+moniker://",
		"workspace",
		2,
		&catalog,
		&ScopeFilter::default(),
		Paging {
			cursor: 0,
			limit: 2,
		},
	);
	assert!(text.contains("uri: code+moniker://workspace"));
	assert!(text.contains("summary:"));
	assert!(text.contains("java: 2"));
	assert!(text.contains("concentration:"));
	assert!(text.contains("java kinds:"));
	assert!(text.contains("root/"));
	assert!(text.contains("src/"));
	assert!(!text.contains("App.java"));
	assert!(text.contains("cursor=2"));
}

#[test]
fn tools_list_returns_mcp_shape() {
	let context = McpContext::new(
		SessionOptions {
			paths: vec![PathBuf::from(".")],
			project: None,
			cache_dir: None,
		},
		"code+moniker://".to_string(),
	);
	let response = handle_json_rpc(
		&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
		&context,
	);
	assert_eq!(response["result"]["tools"][0]["name"], "code_moniker_read");
	assert_eq!(
		response["result"]["tools"][1]["name"],
		"code_moniker_symbols"
	);
	assert!(
		response["result"]["tools"][0]["description"]
			.as_str()
			.unwrap()
			.starts_with("When to use:")
	);
}

#[test]
fn registry_dispatches_read_tool() {
	let registry = ToolRegistry::new();
	let context = McpContext::new(
		SessionOptions {
			paths: vec![PathBuf::from(".")],
			project: None,
			cache_dir: None,
		},
		"code+moniker://".to_string(),
	);
	let result = registry.call(&context, "not_a_tool", &json!({}));
	assert!(result.unwrap_err().is_unknown_tool());
}

#[test]
fn symbols_tool_filters_and_pages_symbols() {
	let source_id = SourceId::new("source:1:src/App.java");
	let sources = vec![SourceFileRecord::from_fields(SourceFileRecordFields {
		id: source_id.clone(),
		uri: "code+moniker://./file:src/App.java".to_string(),
		source_root: 0,
		path: "src/App.java".to_string(),
		rel_path: "src/App.java".to_string(),
		anchor: "src/App.java".to_string(),
		language: "java".to_string(),
		text: String::new(),
	})];
	let symbols = vec![
		SymbolRecord::from_fields(SymbolRecordFields {
			id: SymbolId::new("symbol:1"),
			source: source_id.clone(),
			identity: "code+moniker://./lang:java/package:src/class:App".to_string(),
			name: "App".to_string(),
			kind: "class".to_string(),
			signature: String::new(),
			navigable: true,
			line_range: Some((1, 3)),
			parent: None,
		}),
		SymbolRecord::from_fields(SymbolRecordFields {
			id: SymbolId::new("symbol:2"),
			source: source_id,
			identity: "code+moniker://./lang:java/package:src/class:App/method:run()".to_string(),
			name: "run".to_string(),
			kind: "method".to_string(),
			signature: String::new(),
			navigable: true,
			line_range: Some((4, 5)),
			parent: None,
		}),
	];
	let scope =
		SymbolScopeFilter::from_arguments(&json!({"path": "src/**", "kind": "method"})).unwrap();
	let text = render_symbols_lmnav(
		"code+moniker://",
		"workspace",
		&scope,
		Paging {
			cursor: 0,
			limit: 1,
		},
		&sources,
		&symbols,
	);
	assert!(text.contains("symbols: 1"), "{text}");
	assert!(text.contains("method run src/App.java:4-5"), "{text}");
	assert!(!text.contains("class App"), "{text}");
}

#[test]
fn http_tool_call_reads_workspace_explorer() {
	let temp = tempfile::tempdir().expect("tempdir");
	std::fs::create_dir_all(temp.path().join("src/main/java")).expect("mkdir");
	std::fs::write(temp.path().join("src/main/java/App.java"), "class App {}\n")
		.expect("write fixture");
	let server = start(
		SessionOptions {
			paths: vec![temp.path().to_path_buf()],
			project: None,
			cache_dir: None,
		},
		"code+moniker://".to_string(),
		0,
	)
	.expect("server");
	let body = json!({
		"jsonrpc": "2.0",
		"id": 7,
		"method": "tools/call",
		"params": {
			"name": "code_moniker_read",
			"arguments": { "uri": "workspace", "depth": 4 }
		}
	})
	.to_string();
	let mut stream = TcpStream::connect(server_addr(&server)).expect("connect");
	write!(
		stream,
		"POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
		body.len(),
		body
	)
	.expect("request");
	let mut response = String::new();
	stream.read_to_string(&mut response).expect("response");
	assert!(response.contains("HTTP/1.1 200 OK"));
	assert!(response.contains("uri: code+moniker://workspace"));
	assert!(response.contains("App.java [java]"));
}
