use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;

use code_moniker_workspace::registry::{LocalWorkspaceOptions, LocalWorkspaceRegistry};
use code_moniker_workspace::snapshot::{
	ReferenceRecord, ResourceGeneration, SourceCatalog, SourceFileRecord, SourceFileRecordFields,
	SourceId, SourceUnit, SymbolId, SymbolRecord, SymbolRecordFields, WorkspaceRequest,
	WorkspaceTransition,
};
use serde_json::json;

use super::context::McpContext;
use super::dispatch::handle_json_rpc;
use super::server::server_addr;
use super::tools::read::{render_explorer_lmnav, render_symbol_source_lmnav};
use super::tools::scope::{Paging, ScopeFilter, SymbolScopeFilter};
use super::tools::symbols::{SymbolAction, SymbolIndexView, render_symbols_lmnav};
use super::tools::{McpTool, ToolRegistry};
use super::{start, tools};
use crate::session::SessionOptions;
use crate::workspace_index::SharedWorkspaceIndex;

fn empty_context(paths: Vec<PathBuf>) -> McpContext {
	let opts = SessionOptions {
		paths,
		project: None,
		cache_dir: None,
	};
	McpContext::new(
		opts,
		"code+moniker://".to_string(),
		SharedWorkspaceIndex::new(None),
	)
}

fn loaded_context(paths: Vec<PathBuf>) -> McpContext {
	let opts = SessionOptions {
		paths,
		project: None,
		cache_dir: None,
	};
	let index = loaded_index(&opts);
	McpContext::new(opts, "code+moniker://".to_string(), index)
}

fn loaded_index(opts: &SessionOptions) -> SharedWorkspaceIndex {
	let mut workspace = LocalWorkspaceRegistry::local(
		LocalWorkspaceOptions::new(opts.paths.clone(), opts.project.clone())
			.with_cache_dir(opts.cache_dir.clone()),
	);
	match workspace
		.commands()
		.refresh(WorkspaceRequest::new("mcp-test"))
	{
		WorkspaceTransition::Ready { .. } => {
			SharedWorkspaceIndex::new(workspace.queries().snapshot_arc())
		}
		WorkspaceTransition::Failed { failure, .. } => {
			panic!("mcp test workspace failed: {}", failure.message)
		}
	}
}

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
		4,
		&catalog,
		&ScopeFilter::from_arguments(&json!({"path": "root/src/**", "lang": "java"})).unwrap(),
		Paging {
			cursor: 0,
			limit: 1,
		},
	);
	assert!(text.contains("uri: code+moniker://workspace"));
	assert!(text.contains("summary:"));
	assert!(text.contains("java: 2"));
	assert!(text.contains("concentration:"));
	assert!(text.contains("java kinds:"));
	assert!(text.contains("root/"));
	assert!(text.contains("src/"));
	assert!(text.contains("cursor=1"));
	assert!(text.contains("path=\"root/src/**\""));
	assert!(text.contains("lang=\"java\""));
	assert!(
		text.contains("code_moniker_symbols uri=\"code+moniker://workspace\" path=\"root/src/**\" lang=\"java\" limit=50")
	);
}

#[test]
fn tools_list_returns_mcp_shape() {
	let context = empty_context(vec![PathBuf::from(".")]);
	let response = handle_json_rpc(
		&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
		&context,
	);
	assert_eq!(response["result"]["tools"][0]["name"], "code_moniker_read");
	assert_eq!(
		response["result"]["tools"][1]["name"],
		"code_moniker_symbols"
	);
	assert_eq!(response["result"]["tools"][2]["name"], "code_moniker_rules");
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
	let context = empty_context(vec![PathBuf::from(".")]);
	let result = registry.call(&context, "not_a_tool", &json!({}));
	assert!(result.unwrap_err().is_unknown_tool());
}

#[test]
fn tool_limit_zero_is_rejected() {
	let registry = ToolRegistry::new();
	let context = empty_context(vec![PathBuf::from(".")]);
	let error = registry
		.call(
			&context,
			"code_moniker_read",
			&json!({"uri": "workspace", "limit": 0}),
		)
		.unwrap_err();
	assert!(error.to_string().contains("limit"));
	assert!(error.to_string().contains("greater than zero"));
}

#[test]
fn rules_tool_runs_check_on_workspace() {
	let temp = tempfile::tempdir().expect("tempdir");
	std::fs::create_dir_all(temp.path().join("src/main/java")).expect("mkdir");
	std::fs::write(temp.path().join("src/main/java/App.java"), "class App {}\n")
		.expect("write fixture");
	let registry = ToolRegistry::new();
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let result = registry
		.call(
			&context,
			"code_moniker_rules",
			&json!({
				"uri": "workspace",
				"action": "run",
				"limit": 5,
				"report": false
			}),
		)
		.expect("rules run");
	assert!(!result.is_error);
	assert!(result.text.contains("uri: code+moniker://workspace/rules"));
	assert!(result.text.contains("action: run"));
	assert!(result.text.contains("exit: match"));
	assert!(result.text.contains("report:"));
}

#[test]
fn rules_tool_runs_check_on_multi_root_workspace() {
	let temp = tempfile::tempdir().expect("tempdir");
	let first = temp.path().join("first");
	let second = temp.path().join("second");
	std::fs::create_dir_all(first.join("src/main/java")).expect("mkdir first");
	std::fs::create_dir_all(second.join("src/main/java")).expect("mkdir second");
	std::fs::write(first.join("src/main/java/App.java"), "class App {}\n").expect("write first");
	std::fs::write(second.join("src/main/java/Other.java"), "class Other {}\n")
		.expect("write second");
	std::fs::write(
		temp.path().join(".code-moniker.toml"),
		r#"
		default_rules = false

		[[java.class.where]]
		id = "mcp-multiroot-class-rule"
		expr = "name =~ ^[A-Z]"
		message = "classes are pascal case"
		"#,
	)
	.expect("write rules");
	let registry = ToolRegistry::new();
	let context = loaded_context(vec![first.clone(), second.clone()]);
	let result = registry
		.call(
			&context,
			"code_moniker_rules",
			&json!({
				"uri": "workspace",
				"action": "run",
				"limit": 5,
				"report": false
			}),
		)
		.expect("rules run");
	assert!(!result.is_error);
	assert!(result.text.contains("exit: match"));
	assert!(result.text.contains(&format!("root: {}", first.display())));
	assert!(result.text.contains(&format!("root: {}", second.display())));
}

#[test]
fn rules_tool_lists_project_rules() {
	let temp = tempfile::tempdir().expect("tempdir");
	std::fs::create_dir_all(temp.path().join("src/main/java")).expect("mkdir");
	std::fs::write(temp.path().join("src/main/java/App.java"), "class App {}\n")
		.expect("write fixture");
	std::fs::write(
		temp.path().join(".code-moniker.toml"),
		r#"
		default_rules = false

		[[java.class.where]]
		id = "mcp-root-class-rule"
		expr = "name =~ ^App$"
		message = "loaded from workspace root"

		[[java.method.where]]
		id = "mcp-root-method-rule"
		expr = "name =~ ^[a-z]"
		message = "second rule for pagination"

		[[views]]
		id = "ignored-by-rules-loader"
		title = "Ignored by rules loader"
		"#,
	)
	.expect("write rules");
	let registry = ToolRegistry::new();
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let result = registry
		.call(
			&context,
			"code_moniker_rules",
			&json!({
				"uri": "workspace",
				"action": "list",
				"lang": "java",
				"severity": "error",
				"limit": 1
			}),
		)
		.expect("rules list");
	assert!(!result.is_error);
	assert!(result.text.contains("rules:"));
	assert!(result.text.contains("scope:"));
	assert!(result.text.contains("mcp-root-class-rule"));
	assert!(result.text.contains("loaded from workspace root"));
	assert!(result.text.contains("next:"));
	assert!(result.text.contains("lang=\"java\""));
	assert!(result.text.contains("severity=\"error\""));
	assert!(result.text.contains("cursor=1"));
}

#[test]
fn read_views_lists_and_renders_fragment_view() {
	let temp = tempfile::tempdir().expect("tempdir");
	let source_dir = temp.path().join("src/main/java");
	std::fs::create_dir_all(&source_dir).expect("mkdir");
	std::fs::write(
		source_dir.join("App.java"),
		"class App {\n  void before() {}\n  void run() {\n    work();\n  }\n}\n",
	)
	.expect("write fixture");
	std::fs::write(
		temp.path().join(".code-moniker.toml"),
		r#"
		default_rules = false

		[[views]]
		id = "root-map"
		title = "Root map"
		"#,
	)
	.expect("write root config");
	std::fs::write(
		source_dir.join("code-moniker.fragment.toml"),
		r#"
		fragment = "java-app"

		[[views]]
		id = "java-app"
		title = "Java app"
		scope = "."
		intent = "Understand the fixture application."
		summary = """
		The fixture view is anchored to the Java source fragment and resolves evidence from
		the indexed symbols instead of storing code excerpts in TOML.
		"""

		[[views.boundaries]]
		id = "entry"
		owns = ["fixture entry class"]
		forbids = ["workspace runtime concerns"]
		rationale = """
		The entry boundary highlights the class and method an agent should inspect first.
		"""
		symbols = ["class:App", "method:run"]

		[[views.gotchas]]
		id = "method-slice"
		rationale = "The run method should render a source slice as evidence."
		symbols = ["method:run"]
		"#,
	)
	.expect("write fragment view");
	let registry = ToolRegistry::new();
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let list = registry
		.call(
			&context,
			"code_moniker_read",
			&json!({"uri": "workspace/views"}),
		)
		.expect("view list");
	assert!(!list.is_error);
	assert!(list.text.contains("uri: code+moniker://workspace/views"));
	assert!(list.text.contains("java-app"));
	assert!(list.text.contains("root-map"));
	assert!(
		list.text
			.contains("code_moniker_read uri=\"code+moniker://workspace/views/java-app\"")
	);

	let detail = registry
		.call(
			&context,
			"code_moniker_read",
			&json!({
				"uri": "workspace/views/java-app",
				"context_lines": 0,
				"moniker_format": "compact"
			}),
		)
		.expect("view detail");
	assert!(!detail.is_error);
	assert!(detail.text.contains("view: java-app"), "{}", detail.text);
	assert!(detail.text.contains("boundaries:"));
	assert!(detail.text.contains("gotchas:"));
	assert!(detail.text.contains("moniker:"));
	assert!(detail.text.contains("class:App"), "{}", detail.text);
	assert!(detail.text.contains("method:run"), "{}", detail.text);
	assert!(detail.text.contains("void run()"), "{}", detail.text);
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
			source: source_id.clone(),
			identity: "code+moniker://./lang:java/package:src/class:App/method:run()".to_string(),
			name: "run".to_string(),
			kind: "method".to_string(),
			signature: String::new(),
			navigable: true,
			line_range: Some((4, 5)),
			parent: None,
		}),
		SymbolRecord::from_fields(SymbolRecordFields {
			id: SymbolId::new("symbol:3"),
			source: source_id,
			identity: "code+moniker://./lang:java/package:src/class:App/method:retry()".to_string(),
			name: "retry".to_string(),
			kind: "method".to_string(),
			signature: String::new(),
			navigable: true,
			line_range: Some((6, 7)),
			parent: None,
		}),
	];
	let scope = SymbolScopeFilter::from_arguments(&json!({
		"path": "src/**",
		"lang": "java",
		"kind": "method",
		"name": "^r"
	}))
	.unwrap();
	let text = render_symbols_lmnav(
		"code+moniker://",
		"workspace",
		&scope,
		Paging {
			cursor: 0,
			limit: 1,
		},
		SymbolIndexView {
			sources: &sources,
			symbols: &symbols,
			references: &[],
		},
		SymbolAction::List,
	);
	assert!(text.contains("symbols: 2"), "{text}");
	assert!(text.contains("method run src/App.java:4-5"), "{text}");
	assert!(!text.contains("class App"), "{text}");
	assert!(text.contains("path=\"src/**\""), "{text}");
	assert!(text.contains("lang=\"java\""), "{text}");
	assert!(text.contains("kind=\"method\""), "{text}");
	assert!(text.contains("name=\"^r\""), "{text}");
	assert!(text.contains("cursor=1"), "{text}");
	assert!(
		text.contains(
			"code_moniker_read uri=\"code+moniker://workspace\" path=\"src/**\" lang=\"java\" depth=2"
		),
		"{text}"
	);
}

#[test]
fn read_symbol_source_renders_source_slice() {
	let source_id = SourceId::new("source:1:src/App.java");
	let source = SourceFileRecord::from_fields(SourceFileRecordFields {
		id: source_id.clone(),
		uri: "code+moniker://./file:src/App.java".to_string(),
		source_root: 0,
		path: "src/App.java".to_string(),
		rel_path: "src/App.java".to_string(),
		anchor: "src/App.java".to_string(),
		language: "java".to_string(),
		text: String::new(),
	});
	let symbol = SymbolRecord::from_fields(SymbolRecordFields {
		id: SymbolId::new("symbol:1"),
		source: source_id,
		identity: "code+moniker://./lang:java/package:src/class:App/method:run()".to_string(),
		name: "run".to_string(),
		kind: "method".to_string(),
		signature: String::new(),
		navigable: true,
		line_range: Some((3, 5)),
		parent: None,
	});
	let text = render_symbol_source_lmnav(
		"code+moniker://",
		&symbol,
		&source,
		"class App {\n  void before() {}\n  void run() {\n    work();\n  }\n}\n",
		1,
	);
	assert!(text.contains("uri: code+moniker://./lang:java/package:src/class:App/method:run()"));
	assert!(text.contains("file: src/App.java"));
	assert!(text.contains("slice: 2-6"));
	assert!(text.contains("   3 |   void run() {"));
	assert!(text.contains("code_moniker_symbols"));
}

#[test]
fn symbols_insights_summarize_index() {
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
	let class = SymbolRecord::from_fields(SymbolRecordFields {
		id: SymbolId::new("symbol:class"),
		source: source_id.clone(),
		identity: "code+moniker://./lang:java/package:src/class:App".to_string(),
		name: "App".to_string(),
		kind: "class".to_string(),
		signature: String::new(),
		navigable: true,
		line_range: Some((1, 6)),
		parent: None,
	});
	let method = SymbolRecord::from_fields(SymbolRecordFields {
		id: SymbolId::new("symbol:method"),
		source: source_id.clone(),
		identity: "code+moniker://./lang:java/package:src/class:App/method:run()".to_string(),
		name: "run".to_string(),
		kind: "method".to_string(),
		signature: String::new(),
		navigable: true,
		line_range: Some((3, 5)),
		parent: Some(SymbolId::new("symbol:class")),
	});
	let references = vec![ReferenceRecord::new(
		"ref:1",
		source_id,
		SymbolId::new("symbol:method"),
		"class:Other",
		"calls",
		Some((4, 4)),
	)];
	let text = render_symbols_lmnav(
		"code+moniker://",
		"workspace",
		&SymbolScopeFilter::from_arguments(&json!({"lang": "java"})).unwrap(),
		Paging {
			cursor: 0,
			limit: 5,
		},
		SymbolIndexView {
			sources: &sources,
			symbols: &[class, method],
			references: &references,
		},
		SymbolAction::Insights,
	);
	assert!(text.contains("insights:"));
	assert!(text.contains("java: 1"));
	assert!(text.contains("class: 1"));
	assert!(text.contains("method: 1"));
	assert!(text.contains("top_files_by_refs:"));
	assert!(text.contains("src/App.java: 1"));
}

#[test]
fn http_tool_call_reads_workspace_explorer() {
	let temp = tempfile::tempdir().expect("tempdir");
	std::fs::create_dir_all(temp.path().join("src/main/java")).expect("mkdir");
	std::fs::write(temp.path().join("src/main/java/App.java"), "class App {}\n")
		.expect("write fixture");
	let opts = SessionOptions {
		paths: vec![temp.path().to_path_buf()],
		project: None,
		cache_dir: None,
	};
	let server = start(
		opts.clone(),
		"code+moniker://".to_string(),
		0,
		loaded_index(&opts),
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

#[test]
fn http_cors_allows_only_loopback_origins() {
	let temp = tempfile::tempdir().expect("tempdir");
	std::fs::create_dir_all(temp.path().join("src/main/java")).expect("mkdir");
	std::fs::write(temp.path().join("src/main/java/App.java"), "class App {}\n")
		.expect("write fixture");
	let opts = SessionOptions {
		paths: vec![temp.path().to_path_buf()],
		project: None,
		cache_dir: None,
	};
	let server = start(
		opts.clone(),
		"code+moniker://".to_string(),
		0,
		loaded_index(&opts),
	)
	.expect("server");
	let allowed = post_rpc(
		server_addr(&server),
		Some("http://localhost:3000"),
		&json!({
			"jsonrpc": "2.0",
			"id": 8,
			"method": "tools/list"
		}),
	);
	assert!(allowed.contains("HTTP/1.1 200 OK"));
	assert!(allowed.contains("Access-Control-Allow-Origin: http://localhost:3000"));
	let forbidden = post_rpc(
		server_addr(&server),
		Some("https://evil.example"),
		&json!({
			"jsonrpc": "2.0",
			"id": 9,
			"method": "tools/list"
		}),
	);
	assert!(forbidden.contains("HTTP/1.1 403 Forbidden"));
	assert!(!forbidden.contains("Access-Control-Allow-Origin"));
	let forged_loopback = post_rpc(
		server_addr(&server),
		Some("http://localhost:3000.evil.example"),
		&json!({
			"jsonrpc": "2.0",
			"id": 10,
			"method": "tools/list"
		}),
	);
	assert!(forged_loopback.contains("HTTP/1.1 403 Forbidden"));
	assert!(!forged_loopback.contains("Access-Control-Allow-Origin"));
}

fn post_rpc(addr: std::net::SocketAddr, origin: Option<&str>, body: &serde_json::Value) -> String {
	let body = body.to_string();
	let mut stream = TcpStream::connect(addr).expect("connect");
	write!(
		stream,
		"POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n"
	)
	.expect("request head");
	if let Some(origin) = origin {
		write!(stream, "Origin: {origin}\r\n").expect("origin");
	}
	write!(stream, "Content-Length: {}\r\n\r\n{}", body.len(), body).expect("request body");
	let mut response = String::new();
	stream.read_to_string(&mut response).expect("response");
	response
}
