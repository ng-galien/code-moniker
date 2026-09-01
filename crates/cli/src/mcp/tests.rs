use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use code_moniker_daemon::WorkspaceDaemon;
use code_moniker_query::{
	Command, CommandRequest, DaemonWorkspaceConfig, Query, QueryRequest, QueryResult,
	SymbolSearchQuery, query_capability_specs,
};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::context::{DaemonRuntime, McpContext};
use super::tools;
use super::tools::scope::Paging;
use super::tools::{McpTool, ToolRegistry};
use crate::session::SessionOptions;

fn empty_context(paths: Vec<PathBuf>) -> McpContext {
	daemon_context(paths)
}

fn loaded_context(paths: Vec<PathBuf>) -> McpContext {
	daemon_context(paths)
}

fn preloading_context(paths: Vec<PathBuf>) -> McpContext {
	let opts = SessionOptions {
		paths: paths.clone(),
		project: None,
		cache_dir: None,
	};
	let config = DaemonWorkspaceConfig {
		roots: paths
			.iter()
			.map(|path| path.display().to_string())
			.collect(),
		project: None,
		cache_dir: None,
		live_refresh: None,
	};
	McpContext::new(
		opts,
		"code+moniker://".to_string(),
		DaemonRuntime::in_process_preload(config).expect("preloading daemon"),
	)
}

fn daemon_context(paths: Vec<PathBuf>) -> McpContext {
	let opts = SessionOptions {
		paths: paths.clone(),
		project: None,
		cache_dir: None,
	};
	let context = McpContext::new(
		opts,
		"code+moniker://".to_string(),
		DaemonRuntime::in_process(WorkspaceDaemon::new(paths.clone()).expect("daemon"), paths),
	);
	context
		.command(CommandRequest {
			command: Command::WorkspaceRefresh,
		})
		.expect("initial workspace refresh");
	context
}

fn generated_call_string_arg(text: &str, tool: &str, argument: &str) -> Option<String> {
	let call_prefix = format!("- {tool}");
	let inline_call_prefix = format!("- `{tool}");
	let argument_prefix = format!("{argument}=");
	text.lines().find_map(|line| {
		let call = line.trim_start();
		if !call.starts_with(&call_prefix) && !call.starts_with(&inline_call_prefix) {
			return None;
		}
		let (_, value) = call.split_once(&argument_prefix)?;
		serde_json::Deserializer::from_str(value)
			.into_iter::<String>()
			.next()?
			.ok()
	})
}

struct HttpTestServer {
	addr: SocketAddr,
	shutdown: CancellationToken,
	thread: Option<JoinHandle<()>>,
}

impl Drop for HttpTestServer {
	fn drop(&mut self) {
		self.shutdown.cancel();
		if let Some(thread) = self.thread.take() {
			let _ = thread.join();
		}
	}
}

fn start_http_test_server(opts: SessionOptions) -> HttpTestServer {
	let context = daemon_context(opts.paths);
	start_http_test_server_with_context(context)
}

fn start_http_test_server_with_context(context: McpContext) -> HttpTestServer {
	let shutdown = CancellationToken::new();
	let thread_shutdown = shutdown.child_token();
	let (ready_tx, ready_rx) = mpsc::channel();
	let thread = thread::spawn(move || {
		let runtime = tokio::runtime::Builder::new_multi_thread()
			.enable_all()
			.thread_name("code-moniker-mcp-test")
			.build()
			.expect("runtime");
		runtime.block_on(async move {
			let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
				.await
				.expect("bind");
			let addr = listener.local_addr().expect("addr");
			let router = super::router(context);
			ready_tx.send(addr).expect("ready");
			let _ = axum::serve(listener, router)
				.with_graceful_shutdown(async move { thread_shutdown.cancelled_owned().await })
				.await;
		});
	});
	let addr = ready_rx.recv().expect("server ready");
	HttpTestServer {
		addr,
		shutdown,
		thread: Some(thread),
	}
}

#[test]
fn read_description_matches_esac_style() {
	let descriptor = tools::read::ReadTool.descriptor();
	assert!(descriptor.description.starts_with("When to use:"));
	assert!(descriptor.description.contains("Read from code-moniker."));
	assert!(descriptor.description.contains("workspace"));
	assert!(descriptor.description.contains("limit/cursor"));
	assert!(descriptor.input_schema.get("required").is_none());
}

#[test]
fn tools_list_returns_mcp_shape() {
	let tools = ToolRegistry::new().descriptors();
	let names = tools
		.iter()
		.map(|tool| tool["name"].as_str().unwrap())
		.collect::<Vec<_>>();
	assert_eq!(
		names,
		vec![
			"code_moniker_read",
			"code_moniker_context",
			"code_moniker_query",
			"code_moniker_notes",
			"code_moniker_search",
			"code_moniker_symbols",
			"code_moniker_usages",
			"code_moniker_rules",
			"code_moniker_diff",
			"code_moniker_graph",
			"code_moniker_refresh",
		]
	);
	for capability in query_capability_specs() {
		assert!(
			names.contains(&capability.mcp_tool),
			"query {} declares missing MCP surface {}",
			capability.name,
			capability.mcp_tool
		);
	}
	assert!(
		tools[0]["description"]
			.as_str()
			.unwrap()
			.starts_with("When to use:")
	);
	for tool in &tools {
		assert_eq!(
			tool["inputSchema"]["properties"]["format"]["enum"],
			json!(["text", "json"]),
			"{} must publish the shared output representation contract",
			tool["name"]
		);
		assert_eq!(
			tool["inputSchema"]["properties"]["format"]["default"], "text",
			"{} must default to text-only output",
			tool["name"]
		);
	}
	for name in [
		"code_moniker_read",
		"code_moniker_context",
		"code_moniker_query",
		"code_moniker_notes",
		"code_moniker_search",
		"code_moniker_symbols",
		"code_moniker_usages",
		"code_moniker_rules",
		"code_moniker_diff",
		"code_moniker_graph",
	] {
		let tool = tools
			.iter()
			.find(|tool| tool["name"] == name)
			.expect("tool descriptor");
		assert_eq!(
			tool["inputSchema"]["properties"]["compact"]["type"],
			"boolean"
		);
		assert_eq!(
			tool["inputSchema"]["properties"]["compact"]["default"],
			true
		);
	}
	for tool in &tools[..tools.len() - 1] {
		assert_eq!(
			tool["inputSchema"]["properties"]["budget"]["default"],
			"small"
		);
	}
	let usages = tools
		.iter()
		.find(|tool| tool["name"] == "code_moniker_usages")
		.expect("usages descriptor");
	assert_eq!(
		usages["inputSchema"]["properties"]["include_descendants"]["default"],
		false
	);
	assert!(
		usages["description"]
			.as_str()
			.expect("usages description")
			.contains("include_descendants=true")
	);
	let graph = tools
		.iter()
		.find(|tool| tool["name"] == "code_moniker_graph")
		.expect("graph descriptor");
	assert!(
		graph["description"]
			.as_str()
			.expect("graph description")
			.contains("total facts before filters")
	);
}

#[test]
fn volume_profile_controls_and_caps_page_size_before_rendering() {
	let small = super::tools::common::AgentOutputOptions::from_arguments(&json!({})).unwrap();
	let medium = super::tools::common::AgentOutputOptions::from_arguments(&json!({
		"budget": "medium",
		"compact": false
	}))
	.unwrap();

	assert_eq!(
		Paging::from_arguments_for_volume(&json!({}), small)
			.unwrap()
			.limit,
		20
	);
	assert_eq!(
		Paging::from_arguments_for_volume(&json!({}), medium)
			.unwrap()
			.limit,
		80
	);
	assert_eq!(
		Paging::from_arguments_for_volume(&json!({"limit": 50}), small)
			.unwrap()
			.limit,
		20
	);
}

#[test]
fn refresh_tool_requests_daemon_refresh_and_reports_generation() {
	let temp = tempfile::tempdir().expect("tempdir");
	write_java_app_fixture(temp.path(), "class App {\n  void run() {}\n}\n");
	let context = empty_context(vec![temp.path().to_path_buf()]);

	let result = ToolRegistry::new()
		.call(&context, "code_moniker_refresh", &json!({}))
		.expect("refresh result");

	assert!(result.text.contains("refreshed: generation"));
	assert!(result.text.contains("workspace refreshed"));
}

#[test]
fn query_batch_keeps_partial_results_when_one_expression_fails() {
	let temp = tempfile::tempdir().expect("tempdir");
	write_java_app_fixture(temp.path(), "class App {\n  void run() {}\n}\n");
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let registry = ToolRegistry::new();

	let moniker = app_symbol_moniker(&context);
	let detail = format!("symbol.detail uri:\"{moniker}\"");
	let batch = registry
		.call(
			&context,
			"code_moniker_query",
			&json!({"queries": [detail, "identity.graph prefix:\"lang:nope\""]}),
		)
		.expect("a failing batch expression must not abort the batch");

	assert!(batch.text.contains("## Result 1"), "{}", batch.text);
	assert!(
		batch.text.contains("operation: `symbol.detail`"),
		"the healthy result must survive: {}",
		batch.text
	);
	assert!(
		batch.text.contains("## Result 2") && batch.text.contains("prefix_not_found"),
		"the failing expression must report inline: {}",
		batch.text
	);
	assert!(
		batch.text.contains("completeness: `partial (1 error(s))`"),
		"{}",
		batch.text
	);

	let single = registry.call(
		&context,
		"code_moniker_query",
		&json!({"query": "identity.graph prefix:\"lang:nope\""}),
	);
	assert!(
		single.is_err(),
		"a single failing query keeps the hard error contract"
	);
}

#[test]
fn generic_query_tool_exposes_live_read_only_daemon_capabilities() {
	let temp = tempfile::tempdir().expect("tempdir");
	let literal = "code+moniker://./lang:rs/module:mcp/struct:Server";
	write_java_app_fixture(
		temp.path(),
		&format!("class App {{\n  String uri = \"{literal}\";\n  void run() {{}}\n}}\n"),
	);
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let registry = ToolRegistry::new();

	let described = registry
		.call(
			&context,
			"code_moniker_query",
			&json!({"query": "query.describe verb:\"identity.graph\""}),
		)
		.expect("query describe");
	assert!(!described.is_error);
	assert!(
		described
			.text
			.starts_with("# Query results\n\n- runtime: `stdio-worker`\n"),
		"MCP output must identify the in-process runtime: {}",
		described.text
	);
	assert!(
		described.text.contains("operation: `query.describe`"),
		"{}",
		described.text
	);
	assert!(
		described.text.contains("identity.graph"),
		"{}",
		described.text
	);
	assert!(
		described.text.contains("\"read_only\": true"),
		"{}",
		described.text
	);
	crate::presentation::tests::validate_agent_markdown(&described.text, "Query results", false)
		.expect("query Markdown");
	assert!(
		described.text.contains("\"name\": \"min_count\""),
		"{}",
		described.text
	);
	assert!(
		described.text.contains("\"default\": \"1\""),
		"{}",
		described.text
	);
	assert!(
		described.text.contains("\"name\": \"limit\""),
		"{}",
		described.text
	);
	assert!(
		described.text.contains("\"default\": \"80\""),
		"{}",
		described.text
	);
	let metrics = registry
		.call(
			&context,
			"code_moniker_query",
			&json!({"query": "query.describe verb:\"metrics.coupling\""}),
		)
		.expect("coupling metrics describe");
	assert!(!metrics.is_error);
	assert!(
		metrics.text.contains("metrics.coupling"),
		"{}",
		metrics.text
	);
	assert!(metrics.text.contains("from"), "{}", metrics.text);
	assert!(metrics.text.contains("to"), "{}", metrics.text);
	assert!(
		metrics.text.contains("\"name\": \"export\""),
		"{}",
		metrics.text
	);
	assert!(
		metrics.text.contains("\"default\": \"false\""),
		"{}",
		metrics.text
	);

	let bad_prefix = registry.call(
		&context,
		"code_moniker_query",
		&json!({"query": "identity.graph prefix:\"lang:java\" limit:10"}),
	);
	let error = bad_prefix.expect_err("a prefix matching no identity must fail loudly");
	assert!(
		error.to_string().contains("prefix_not_found") && error.to_string().contains("srcset:main"),
		"{error}"
	);

	let graph = registry
		.call(
			&context,
			"code_moniker_query",
			&json!({"query": "identity.graph prefix:\"srcset:main\" limit:10"}),
		)
		.expect("identity graph");
	assert!(!graph.is_error);
	assert!(
		graph.text.contains("operation: `identity.graph`"),
		"{}",
		graph.text
	);
	assert!(graph.text.contains("\"nodes\":"), "{}", graph.text);

	let moniker = app_symbol_moniker(&context);
	let compact = code_moniker_workspace::code::compact_identity(&moniker, "code+moniker://")
		.expect("compact moniker");
	let detail = format!("symbol.detail uri:\"{moniker}\"");
	let batch = registry
		.call(
			&context,
			"code_moniker_query",
			&json!({"queries": [detail.clone(), detail]}),
		)
		.expect("query batch");
	assert!(batch.text.contains("mode: `query.batch`"), "{}", batch.text);
	assert!(batch.text.contains(&compact), "{}", batch.text);
	assert!(!batch.text.contains(&moniker), "{}", batch.text);
	assert!(
		batch.text.contains(literal),
		"source URI literal was rewritten:\n{}",
		batch.text
	);

	let projected = "symbol.search name:\"App\" limit:1\nproject name uri";
	let projected_batch = registry
		.call(
			&context,
			"code_moniker_query",
			&json!({"queries": [projected, projected]}),
		)
		.expect("projected query batch");
	assert!(
		projected_batch.text.contains(&compact),
		"{}",
		projected_batch.text
	);

	let mutation = registry.call(
		&context,
		"code_moniker_query",
		&json!({"query": "notes action:create title:\"blocked\""}),
	);
	assert!(mutation.is_err(), "mixed query must use its dedicated tool");
}

#[test]
fn read_tool_returns_a_bounded_ast_only_when_requested() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src = temp.path().join("src");
	std::fs::create_dir_all(&src).expect("create src");
	std::fs::write(
		src.join("lib.rs"),
		"pub fn greet(name: &str) -> String {\n    format!(\"hello {name}\")\n}\n",
	)
	.expect("write fixture");
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let registry = ToolRegistry::new();

	let tree = registry
		.call(
			&context,
			"code_moniker_read",
			&json!({
				"uri": "src/lib.rs",
				"ast": true,
				"include_text": true,
				"max_depth": 8,
				"max_nodes": 100
			}),
		)
		.expect("AST read");
	crate::presentation::tests::validate_agent_markdown(&tree.text, "Syntax tree", false)
		.expect("syntax tree Markdown");
	assert!(!tree.is_error);
	assert!(tree.text.contains("uri: `syntax.tree`"), "{}", tree.text);
	assert!(tree.text.contains("file: `src/lib.rs`"), "{}", tree.text);
	assert!(tree.text.contains("- function_item "), "{}", tree.text);
	assert!(
		tree.text.contains("identifier") && tree.text.contains("text=\"greet\""),
		"{}",
		tree.text
	);

	let bounded = registry
		.call(
			&context,
			"code_moniker_read",
			&json!({"uri": "src/lib.rs", "ast": true, "max_nodes": 2}),
		)
		.expect("bounded AST read");
	assert!(
		bounded.text.contains("completeness: `bounded`"),
		"{}",
		bounded.text
	);
	assert!(bounded.text.contains("nodes: 2/"), "{}", bounded.text);

	let invalid = registry.call(
		&context,
		"code_moniker_read",
		&json!({"uri": "src/lib.rs", "ast": true, "max_nodes": 0}),
	);
	let error = invalid.expect_err("zero AST node limit must be rejected");
	assert!(
		error.to_string().contains("max_nodes must be at least 1"),
		"{error}"
	);
}

#[test]
fn read_tool_schema_and_parser_leave_syntax_limits_to_the_client() {
	let descriptor = tools::read::ReadTool.descriptor();
	let properties = descriptor.input_schema["properties"]
		.as_object()
		.expect("read tool properties");
	for (name, minimum) in [("max_depth", 0), ("max_nodes", 1)] {
		let property = properties[name]
			.as_object()
			.unwrap_or_else(|| panic!("missing {name} schema"));
		assert_eq!(property.get("minimum"), Some(&json!(minimum)));
		assert!(
			property.get("maximum").is_none(),
			"{name} must not expose an artificial maximum: {property:?}"
		);
	}
	let temp = tempfile::tempdir().expect("tempdir");
	let context = preloading_context(vec![temp.path().to_path_buf()]);
	let registry = ToolRegistry::new();
	for arguments in [
		json!({
			"language": "sql",
			"source": "SELECT account.id FROM public.account AS account;",
			"max_depth": 1_000
		}),
		json!({
			"language": "sql",
			"source": "SELECT account.id FROM public.account AS account;",
			"max_nodes": 20_000
		}),
	] {
		let result = registry
			.call(&context, "code_moniker_read", &arguments)
			.expect("client-selected MCP syntax limit");
		assert!(!result.is_error, "{}", result.text);
	}
	let error = registry
		.call(
			&context,
			"code_moniker_read",
			&json!({
				"language": "sql",
				"source": "SELECT 1;",
				"format": "yaml"
			}),
		)
		.expect_err("unknown output format must be rejected");
	assert!(
		error.to_string().contains("unknown output format"),
		"{error}"
	);
}

#[test]
fn read_tool_parses_source_text_without_indexing_it() {
	let temp = tempfile::tempdir().expect("tempdir");
	let context = preloading_context(vec![temp.path().to_path_buf()]);
	let registry = ToolRegistry::new();
	let tree = registry
		.call(
			&context,
			"code_moniker_read",
			&json!({
				"language": "plpgsql",
				"source": "DECLARE total numeric; BEGIN total := 1; RETURN total; END;",
				"include_text": true,
				"max_depth": 12,
				"max_nodes": 200,
				"budget": "full"
			}),
		)
		.expect("stateless PL/pgSQL AST");
	assert!(!tree.is_error);
	assert!(tree.text.contains("uri: `syntax.parse`"), "{}", tree.text);
	assert!(
		tree.text.contains("file: `snippet.plpgsql`"),
		"{}",
		tree.text
	);
	assert!(tree.text.contains("- decl_statement "), "{}", tree.text);
	assert!(tree.text.contains("- stmt_assign "), "{}", tree.text);
	assert!(tree.text.contains("- stmt_return "), "{}", tree.text);
	assert!(
		!temp.path().join("snippet.plpgsql").exists(),
		"stateless parse must not persist the source"
	);

	let rust = registry
		.call(
			&context,
			"code_moniker_read",
			&json!({
				"language": "rs",
				"source": "fn answer() -> u32 { 42 }",
				"uri": "virtual.rs",
				"include_text": true
			}),
		)
		.expect("stateless Rust AST");
	assert!(rust.text.contains("file: `virtual.rs`"), "{}", rust.text);
	assert!(rust.text.contains("- function_item "), "{}", rust.text);

	for arguments in [
		json!({"ast": true, "source": "fn main() {}"}),
		json!({"ast": true, "language": "rs"}),
	] {
		let error = registry
			.call(&context, "code_moniker_read", &arguments)
			.expect_err("invalid stateless AST must be rejected");
		assert!(error.to_string().contains("source and language"), "{error}");
	}
}

#[test]
fn read_tool_renders_embedded_plpgsql_from_the_language_sdk_document() {
	let temp = tempfile::tempdir().expect("tempdir");
	std::fs::write(
		temp.path().join("account.sql"),
		"CREATE FUNCTION account_balance(p_id bigint) RETURNS numeric\n\
		 LANGUAGE plpgsql AS $$\n\
		 DECLARE total numeric;\n\
		 BEGIN\n\
		   SELECT sum(amount) INTO total FROM ledger_entry WHERE account_id = p_id;\n\
		   IF total IS NULL THEN RETURN 0; END IF;\n\
		   RETURN total;\n\
		 END;\n\
		 $$;\n",
	)
	.expect("write PL/pgSQL fixture");
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let tree = ToolRegistry::new()
		.call(
			&context,
			"code_moniker_read",
			&json!({
				"uri": "account.sql",
			"ast": true,
			"max_depth": 20,
			"max_nodes": 500,
			"budget": "full"
			}),
		)
		.expect("PL/pgSQL AST read");
	assert!(!tree.is_error);
	assert!(
		tree.text.contains("- source_file") && tree.text.contains("[plpgsql,entry:block"),
		"{}",
		tree.text
	);
	assert!(tree.text.contains("entry:block"), "{}", tree.text);
	assert!(tree.text.contains("injected-error:false"), "{}", tree.text);
	assert!(tree.text.contains("- stmt_if "), "{}", tree.text);
	assert!(tree.text.contains("- sql_expression "), "{}", tree.text);
}

#[test]
fn context_tool_accepts_canonical_source_groups_and_returns_bounded_pre_change_facts() {
	let temp = tempfile::tempdir().expect("tempdir");
	std::fs::write(
		temp.path().join(".code-moniker.toml"),
		r#"
[[workspace.source_group]]
roots = [{ path = "src/main/java", srcset = "main" }]
"#,
	)
	.expect("source-group config");
	write_java_app_fixture(
		temp.path(),
		"class App {\n  void run() { helper(); }\n  void helper() {}\n}\n",
	);
	let git = |args: &[&str]| {
		let output = std::process::Command::new("git")
			.arg("-C")
			.arg(temp.path())
			.args(args)
			.output()
			.expect("run git");
		assert!(
			output.status.success(),
			"git {args:?}: {}",
			String::from_utf8_lossy(&output.stderr)
		);
	};
	git(&["init"]);
	git(&["config", "user.email", "cm@example.test"]);
	git(&["config", "user.name", "Code Moniker"]);
	git(&["add", "."]);
	git(&["commit", "-m", "initial"]);
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let moniker = app_symbol_moniker(&context);
	let compact = code_moniker_workspace::code::compact_identity(&moniker, "code+moniker://")
		.expect("compact moniker");
	let result = ToolRegistry::new()
		.call(
			&context,
			"code_moniker_context",
			&json!({"focus": moniker, "max_items": 1}),
		)
		.expect("change context");
	crate::presentation::tests::validate_agent_markdown(&result.text, "Change context", false)
		.expect("change context Markdown");
	assert!(!result.is_error);
	assert!(
		result.text.contains("mode: `change.context`"),
		"{}",
		result.text
	);
	assert!(result.text.contains("## Coverage"), "{}", result.text);
	assert!(result.text.contains("- members:"), "{}", result.text);
	assert!(result.text.contains(&compact), "{}", result.text);
	assert!(!result.text.contains(&moniker), "{}", result.text);
	assert!(
		result.text.contains("## Suggested checks"),
		"{}",
		result.text
	);
	assert!(
		result.text.contains("code_moniker_rules uri=\"workspace\""),
		"{}",
		result.text
	);
	assert!(
		!result.text.contains("code_moniker_rules uri=\"@"),
		"{}",
		result.text
	);
}

#[test]
fn compact_moniker_from_output_can_be_used_for_symbol_navigation() {
	let temp = tempfile::tempdir().expect("tempdir");
	write_java_app_fixture(temp.path(), "class App {\n  void run() {}\n}\n");
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let canonical = app_symbol_moniker(&context);
	let compact = code_moniker_workspace::code::compact_identity(&canonical, "code+moniker://")
		.expect("compact moniker");
	let result = ToolRegistry::new()
		.call(&context, "code_moniker_read", &json!({"uri": compact}))
		.expect("compact moniker navigation");
	crate::presentation::tests::validate_agent_markdown(&result.text, "Symbol source", false)
		.expect("symbol source Markdown");

	assert!(result.text.contains("name: `App`"), "{}", result.text);
	assert!(
		result.text.contains(&format!("- uri: `{compact}`")),
		"{}",
		result.text
	);
	assert!(!result.text.contains(&canonical), "{}", result.text);
}

#[test]
fn usages_tool_groups_repeated_contexts_and_attaches_bounded_source_evidence() {
	let temp = tempfile::tempdir().expect("tempdir");
	std::fs::create_dir_all(temp.path().join("src")).expect("mkdir rust source");
	std::fs::write(
		temp.path().join("Cargo.toml"),
		"[package]\nname = \"usage-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
	)
	.expect("write cargo manifest");
	std::fs::write(
		temp.path().join("src/lib.rs"),
		concat!(
			"pub fn target() {}\n",
			"pub fn caller() {\n",
			"\ttarget();\n",
			"\ttarget();\n",
			"}\n",
		),
	)
	.expect("write rust source");
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let target = symbol_moniker(&context, "target");
	let result = ToolRegistry::new()
		.call(
			&context,
			"code_moniker_usages",
			&json!({
				"uri": target,
				"direction": "incoming",
				"evidence": "representative",
				"max_evidence": 1,
				"context_lines": 0
			}),
		)
		.expect("symbol usages");

	assert!(!result.is_error, "{}", result.text);
	crate::presentation::tests::validate_agent_markdown(&result.text, "Symbol usages", false)
		.expect("valid usages Markdown");
	assert!(
		result.text.contains("- page references: 2"),
		"{}",
		result.text
	);
	assert!(result.text.contains("- groups: 1"), "{}", result.text);
	assert!(result.text.contains("- references: 2"), "{}", result.text);
	let compact = code_moniker_workspace::code::compact_identity(&target, "code+moniker://")
		.expect("compact target moniker");
	assert!(
		result.text.contains(&format!("- uri: `{compact}`")),
		"{}",
		result.text
	);
	assert!(!result.text.contains(&target), "{}", result.text);
	assert!(result.text.contains("#### Evidence"), "{}", result.text);
	assert!(result.text.contains("target();"), "{}", result.text);
}

#[test]
fn usages_tool_joins_external_crate_identities_to_workspace_definitions() {
	let temp = tempfile::tempdir().expect("tempdir");
	std::fs::write(
		temp.path().join("Cargo.toml"),
		concat!(
			"[workspace]\n",
			"members = [\"shared-model\", \"consumer-a\", \"consumer-b\"]\n",
			"resolver = \"3\"\n",
		),
	)
	.expect("write workspace manifest");
	for consumer in ["consumer-a", "consumer-b"] {
		std::fs::create_dir_all(temp.path().join(consumer).join("src"))
			.expect("mkdir consumer source");
		std::fs::write(
			temp.path().join(consumer).join("Cargo.toml"),
			format!(
				concat!(
					"[package]\n",
					"name = \"{}\"\n",
					"version = \"0.1.0\"\n",
					"edition = \"2024\"\n",
					"\n",
					"[dependencies]\n",
					"shared-model = {{ path = \"../shared-model\" }}\n",
				),
				consumer,
			),
		)
		.expect("write consumer manifest");
		std::fs::write(
			temp.path().join(consumer).join("src/lib.rs"),
			concat!(
				"use shared_model::SharedType;\n",
				"\n",
				"pub fn consume(value: SharedType) -> SharedType {\n",
				"\tvalue\n",
				"}\n",
			),
		)
		.expect("write consumer source");
	}
	std::fs::create_dir_all(temp.path().join("shared-model/src")).expect("mkdir shared source");
	std::fs::write(
		temp.path().join("shared-model/Cargo.toml"),
		concat!(
			"[package]\n",
			"name = \"shared-model\"\n",
			"version = \"0.1.0\"\n",
			"edition = \"2024\"\n",
		),
	)
	.expect("write shared manifest");
	std::fs::write(
		temp.path().join("shared-model/src/lib.rs"),
		concat!(
			"pub struct SharedType;\n",
			"\n",
			"pub fn build() -> SharedType {\n",
			"\tSharedType\n",
			"}\n",
		),
	)
	.expect("write shared source");

	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let target = symbol_moniker(&context, "SharedType");
	let result = ToolRegistry::new()
		.call(
			&context,
			"code_moniker_usages",
			&json!({
				"uri": target,
				"direction": "incoming",
				"limit": 50,
				"compact": false
			}),
		)
		.expect("symbol usages");

	assert!(!result.is_error, "{}", result.text);
	assert!(
		result.text.contains("## Incoming summary"),
		"{}",
		result.text
	);
	assert!(result.text.contains("- files: 3"), "{}", result.text);
	assert!(
		result
			.text
			.contains("- shared-helper signal: `shared_helper_candidate`"),
		"{}",
		result.text
	);
	assert!(
		result.text.contains("consumer-a/src/lib.rs"),
		"{}",
		result.text
	);
	assert!(
		result.text.contains("consumer-b/src/lib.rs"),
		"{}",
		result.text
	);
	assert!(
		result
			.text
			.contains("external_pkg:shared_model/path:SharedType"),
		"{}",
		result.text
	);
}

#[test]
fn usages_tool_pages_on_complete_symbolic_groups() {
	let temp = tempfile::tempdir().expect("tempdir");
	std::fs::create_dir_all(temp.path().join("src")).expect("mkdir rust source");
	std::fs::write(
		temp.path().join("Cargo.toml"),
		"[package]\nname = \"usage-pages\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
	)
	.expect("write cargo manifest");
	std::fs::write(
		temp.path().join("src/lib.rs"),
		concat!(
			"pub fn target() {}\n",
			"pub fn caller_a() {\n",
			"\ttarget();\n",
			"\ttarget();\n",
			"\ttarget();\n",
			"}\n",
			"pub fn caller_b() {\n",
			"\ttarget();\n",
			"\ttarget();\n",
			"}\n",
		),
	)
	.expect("write rust source");
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let target = symbol_moniker(&context, "target");
	let registry = ToolRegistry::new();
	let first = registry
		.call(
			&context,
			"code_moniker_usages",
			&json!({
				"uri": target,
				"direction": "incoming",
				"limit": 2,
				"evidence": "none"
			}),
		)
		.expect("first usage page");
	assert!(
		first
			.text
			.contains("partial (usages 0-3 of 5, next cursor 3)"),
		"{}",
		first.text
	);
	assert!(
		first.text.contains("- page references: 3"),
		"{}",
		first.text
	);
	assert!(first.text.contains("- groups: 1"), "{}", first.text);
	assert!(first.text.contains("- references: 3"), "{}", first.text);
	assert!(
		first
			.text
			.contains("### `in` · `production` · `calls` · `caller_a()`"),
		"{}",
		first.text
	);
	assert!(
		!first
			.text
			.contains("### `in` · `production` · `calls` · `caller_b()`"),
		"{}",
		first.text
	);
	let cursor = first
		.text
		.split("cursor=\"")
		.nth(1)
		.and_then(|value| value.split('"').next())
		.expect("generated cursor");

	let second = registry
		.call(
			&context,
			"code_moniker_usages",
			&json!({
				"uri": target,
				"direction": "incoming",
				"limit": 2,
				"cursor": cursor,
				"evidence": "none"
			}),
		)
		.expect("second usage page");
	assert!(
		second.text.contains("completeness: full"),
		"{}",
		second.text
	);
	assert!(
		second.text.contains("- page references: 2"),
		"{}",
		second.text
	);
	assert!(second.text.contains("- groups: 1"), "{}", second.text);
	assert!(second.text.contains("- references: 2"), "{}", second.text);
	assert!(
		second
			.text
			.contains("### `in` · `production` · `calls` · `caller_b()`"),
		"{}",
		second.text
	);
	assert!(
		!second
			.text
			.contains("### in · production · calls · caller_a"),
		"{}",
		second.text
	);
}

#[test]
fn invalid_output_volume_is_rejected_before_note_mutation() {
	let temp = tempfile::tempdir().expect("tempdir");
	write_java_app_fixture(temp.path(), "class App {}\n");
	let registry = ToolRegistry::new();
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let moniker = app_symbol_moniker(&context);

	let rejected = registry.call(
		&context,
		"code_moniker_notes",
		&json!({
			"action": "create",
			"id": "note_invalid_budget",
			"moniker": moniker,
			"kind": "todo",
			"title": "Must not persist",
			"body": "The response volume profile is invalid.",
			"created_by": "user",
			"budget": "tiny"
		}),
	);
	assert!(rejected.is_err(), "invalid budget must reject the call");
	assert!(
		!temp.path().join(".code-moniker/notes.toml").exists(),
		"note mutation happened before budget validation"
	);
}

#[test]
fn graph_tool_rejects_values_outside_its_schema() {
	let temp = tempfile::tempdir().expect("tempdir");
	write_java_app_fixture(temp.path(), "class App {}\n");
	let registry = ToolRegistry::new();
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let moniker = app_symbol_moniker(&context);

	for arguments in [
		json!({"focus": moniker.clone(), "max_items": 0}),
		json!({"focus": moniker.clone(), "min_count": 0}),
		json!({"focus": moniker.clone(), "include_internal": "yes"}),
		json!({"focus": moniker, "direction": 1}),
	] {
		assert!(
			registry
				.call(&context, "code_moniker_graph", &arguments)
				.is_err(),
			"invalid graph arguments were accepted: {arguments}"
		);
	}
}

#[test]
fn graph_tool_compacts_neighbor_monikers_and_verbose_mode_restores_uris() {
	let temp = tempfile::tempdir().expect("tempdir");
	write_java_app_fixture(
		temp.path(),
		"class App {\n  void run() { helper(); }\n  void helper() {}\n}\n",
	);
	let registry = ToolRegistry::new();
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let run = symbol_moniker(&context, "run()");

	let compact = registry
		.call(
			&context,
			"code_moniker_graph",
			&json!({"focus": run.clone(), "direction": "outgoing"}),
		)
		.expect("compact graph");
	crate::presentation::tests::validate_agent_markdown(&compact.text, "Symbol graph", false)
		.expect("valid graph Markdown");
	assert!(compact.text.contains("- uri: `java:"), "{}", compact.text);
	assert!(
		!compact.text.contains("code+moniker://./"),
		"{}",
		compact.text
	);

	let verbose = registry
		.call(
			&context,
			"code_moniker_graph",
			&json!({"focus": run, "direction": "outgoing", "compact": false}),
		)
		.expect("verbose graph");
	assert!(
		verbose.text.contains("- uri: `code+moniker://./"),
		"{}",
		verbose.text
	);
}

#[test]
fn usages_tool_rejects_values_outside_its_schema() {
	let context = empty_context(vec![PathBuf::from(".")]);
	let registry = ToolRegistry::new();
	for arguments in [
		json!({"uri": "not-used", "evidence": 1}),
		json!({"uri": "not-used", "technical": true}),
		json!({"uri": "not-used", "max_evidence": "4"}),
		json!({"uri": "not-used", "max_evidence": -1}),
		json!({"uri": "not-used", "max_evidence": 13}),
		json!({"uri": "not-used", "context_lines": "2"}),
		json!({"uri": "not-used", "context_lines": -1}),
		json!({"uri": "not-used", "context_lines": 9}),
	] {
		assert!(
			registry
				.call(&context, "code_moniker_usages", &arguments)
				.is_err(),
			"invalid usage arguments were accepted: {arguments}"
		);
	}
}

#[test]
fn notes_tool_manages_symbol_notes_with_controlled_transitions() {
	let temp = tempfile::tempdir().expect("tempdir");
	write_java_app_fixture(temp.path(), "class App {\n  void run() {}\n}\n");
	let registry = ToolRegistry::new();
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let moniker = app_symbol_moniker(&context);
	let compact = code_moniker_workspace::code::compact_identity(&moniker, "code+moniker://")
		.expect("compact moniker");

	let created = registry
		.call(
			&context,
			"code_moniker_notes",
			&json!({
				"action": "create",
				"id": "note_acceptance",
				"moniker": compact,
				"kind": "todo",
				"title": "Check App",
				"body": "Agent should inspect this symbol.",
				"created_by": "user"
			}),
		)
		.expect("create note");
	assert!(!created.is_error);
	assert!(
		created.text.contains("action: `create`"),
		"{}",
		created.text
	);
	assert!(
		created.text.contains("resolution: resolved"),
		"{}",
		created.text
	);
	assert!(created.text.contains("kind: `todo`"), "{}", created.text);
	assert!(created.text.contains(&compact), "{}", created.text);
	assert!(!created.text.contains(&moniker), "{}", created.text);
	crate::presentation::tests::validate_agent_markdown(&created.text, "Project notes", false)
		.expect("notes Markdown");
	assert!(
		temp.path().join(".code-moniker/notes.toml").is_file(),
		"notes file should be persisted"
	);
	let persisted =
		std::fs::read_to_string(temp.path().join(".code-moniker/notes.toml")).expect("read notes");
	assert!(persisted.contains(&moniker), "{persisted}");
	assert!(!persisted.contains(&compact), "{persisted}");

	let list = registry
		.call(
			&context,
			"code_moniker_notes",
			&json!({"action": "list", "moniker": compact}),
		)
		.expect("list notes");
	assert!(list.text.contains("notes: 1"), "{}", list.text);
	assert!(list.text.contains("note_acceptance"), "{}", list.text);
	assert!(list.text.contains("Check App"), "{}", list.text);

	let ongoing = registry
		.call(
			&context,
			"code_moniker_notes",
			&json!({
				"action": "transition",
				"id": "note_acceptance",
				"status": "ongoing"
			}),
		)
		.expect("transition ongoing");
	assert!(
		ongoing.text.contains("status: `ongoing`"),
		"{}",
		ongoing.text
	);

	let done = registry
		.call(
			&context,
			"code_moniker_notes",
			&json!({
				"action": "transition",
				"id": "note_acceptance",
				"status": "done"
			}),
		)
		.expect("transition done");
	assert!(done.text.contains("status: `done`"), "{}", done.text);

	let rejected = registry
		.call(
			&context,
			"code_moniker_notes",
			&json!({
				"action": "transition",
				"id": "note_acceptance",
				"status": "pending"
			}),
		)
		.unwrap_err();
	assert!(
		rejected
			.to_string()
			.contains("invalid note status transition"),
		"{rejected}"
	);

	let hidden_done = registry
		.call(&context, "code_moniker_notes", &json!({"action": "list"}))
		.expect("list active notes");
	assert!(
		hidden_done.text.contains("notes: 0"),
		"{}",
		hidden_done.text
	);

	let deleted = registry
		.call(
			&context,
			"code_moniker_notes",
			&json!({"action": "delete", "id": "note_acceptance"}),
		)
		.expect("delete note");
	assert!(
		deleted.text.contains("action: `delete`"),
		"{}",
		deleted.text
	);
	assert!(deleted.text.contains("note_acceptance"), "{}", deleted.text);
}

#[test]
fn notes_tool_flags_orphan_notes() {
	let temp = tempfile::tempdir().expect("tempdir");
	write_java_app_fixture(temp.path(), "class App {}\n");
	let registry = ToolRegistry::new();
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	registry
		.call(
			&context,
			"code_moniker_notes",
			&json!({
				"action": "create",
				"id": "note_orphan",
				"moniker": "code+moniker://./lang:java/class:Missing",
				"title": "Missing target"
			}),
		)
		.expect("create orphan note");

	let orphans = registry
		.call(
			&context,
			"code_moniker_notes",
			&json!({"action": "list", "orphan": true}),
		)
		.expect("list orphans");

	assert!(orphans.text.contains("notes: 1"), "{}", orphans.text);
	assert!(orphans.text.contains("note_orphan"), "{}", orphans.text);
	assert!(
		orphans.text.contains("resolution: orphan"),
		"{}",
		orphans.text
	);
}

#[test]
fn notes_tool_reads_workspace_notes_refreshed_after_context_load() {
	let temp = tempfile::tempdir().expect("tempdir");
	write_java_app_fixture(temp.path(), "class App {}\n");
	let registry = ToolRegistry::new();
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let moniker = app_symbol_moniker(&context);
	write_notes_toml(
		temp.path(),
		&format!(
			r#"
			[[notes]]
			id = "note_external"
			moniker = "{moniker}"
			kind = "todo"
			status = "pending"
			title = "External note"
			body = "This note was written after the MCP context loaded."
			created_by = "user"
			created_at = "2026-06-02T00:00:00Z"
			updated_at = "2026-06-02T00:00:00Z"
			"#
		),
	);

	let list = registry
		.call(&context, "code_moniker_notes", &json!({"action": "list"}))
		.expect("list refreshed notes");

	assert!(list.text.contains("notes: 1"), "{}", list.text);
	assert!(list.text.contains("note_external"), "{}", list.text);
	assert!(list.text.contains("External note"), "{}", list.text);
	assert!(list.text.contains("resolution: resolved"), "{}", list.text);
}

#[test]
fn notes_tool_rejects_status_update_without_persisting() {
	let temp = tempfile::tempdir().expect("tempdir");
	write_java_app_fixture(temp.path(), "class App {}\n");
	let registry = ToolRegistry::new();
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let moniker = app_symbol_moniker(&context);
	registry
		.call(
			&context,
			"code_moniker_notes",
			&json!({
				"action": "create",
				"id": "note_update_status",
				"moniker": moniker,
				"title": "Status contract"
			}),
		)
		.expect("create note");

	let error = registry
		.call(
			&context,
			"code_moniker_notes",
			&json!({
				"action": "update",
				"id": "note_update_status",
				"status": "done",
				"title": "Ignored status"
			}),
		)
		.unwrap_err();
	assert!(
		error
			.to_string()
			.contains("status changes require action=transition"),
		"{error}"
	);

	let note = registry
		.call(
			&context,
			"code_moniker_notes",
			&json!({"action": "get", "id": "note_update_status"}),
		)
		.expect("get note");
	assert!(note.text.contains("status: `pending`"), "{}", note.text);
	assert!(note.text.contains("Status contract"), "{}", note.text);
	assert!(!note.text.contains("Ignored status"), "{}", note.text);
}

#[test]
fn notes_tool_persists_create_after_daemon_refresh() {
	let temp = tempfile::tempdir().expect("tempdir");
	let registry = ToolRegistry::new();
	let context = empty_context(vec![temp.path().to_path_buf()]);

	let created = registry
		.call(
			&context,
			"code_moniker_notes",
			&json!({
				"action": "create",
				"id": "note_no_index",
				"moniker": "code+moniker://./file:src/App.java",
				"title": "No index"
			}),
		)
		.expect("create note after daemon refresh");

	assert!(
		created.text.contains("action: `create`"),
		"{}",
		created.text
	);
	assert!(
		temp.path().join(".code-moniker/notes.toml").exists(),
		"daemon-backed create must persist notes"
	);
}

#[test]
fn notes_tool_resolves_file_module_monikers() {
	let temp = tempfile::tempdir().expect("tempdir");
	write_java_app_fixture(temp.path(), "class App {}\n");
	let registry = ToolRegistry::new();
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let file_moniker = "code+moniker://./srcset:main/lang:java/module:App";

	let created = registry
		.call(
			&context,
			"code_moniker_notes",
			&json!({
					"action": "create",
					"id": "note_file",
					"moniker": file_moniker,
				"title": "File target"
			}),
		)
		.expect("create file note");

	assert!(
		created.text.contains("target: `module App`"),
		"{}",
		created.text
	);
	assert!(
		created.text.contains("file: `src/main/java/App.java`"),
		"{}",
		created.text
	);
}

fn write_java_app_fixture(root: &std::path::Path, source: &str) {
	std::fs::create_dir_all(root.join("src/main/java")).expect("mkdir");
	std::fs::write(root.join("src/main/java/App.java"), source).expect("write fixture");
}

fn write_notes_toml(root: &std::path::Path, contents: &str) {
	std::fs::create_dir_all(root.join(".code-moniker")).expect("mkdir notes");
	std::fs::write(root.join(".code-moniker/notes.toml"), contents).expect("write notes");
}

fn app_symbol_moniker(context: &McpContext) -> String {
	let response = context
		.query(QueryRequest::new(Query::SymbolSearch(SymbolSearchQuery {
			name: Some("^App$".to_string()),
			..Default::default()
		})))
		.expect("symbol search");
	let QueryResult::SymbolList(result) = response.result else {
		panic!("unexpected symbol query response");
	};
	result
		.rows
		.iter()
		.find(|symbol| symbol.name == "App")
		.expect("app symbol")
		.uri
		.clone()
}

fn symbol_moniker(context: &McpContext, name: &str) -> String {
	let response = context
		.query(QueryRequest::new(Query::SymbolSearch(SymbolSearchQuery {
			name: Some(format!("^{name}")),
			include_non_navigable: true,
			..Default::default()
		})))
		.expect("symbol search");
	let QueryResult::SymbolList(result) = response.result else {
		panic!("unexpected symbol query response");
	};
	result
		.rows
		.iter()
		.find(|symbol| symbol.name == name || symbol.name.starts_with(&format!("{name}(")))
		.unwrap_or_else(|| {
			panic!(
				"{name} symbol; candidates: {:?}",
				result
					.rows
					.iter()
					.map(|symbol| symbol.name.as_str())
					.collect::<Vec<_>>()
			)
		})
		.uri
		.clone()
}

#[test]
fn registry_dispatches_read_tool() {
	let registry = ToolRegistry::new();
	let context = empty_context(vec![PathBuf::from(".")]);
	let result = registry.call(&context, "not_a_tool", &json!({}));
	assert!(result.unwrap_err().is_unknown_tool());
}

#[test]
fn workspace_read_reports_roots_and_rejects_a_mismatched_expectation() {
	let workspace = tempfile::tempdir().expect("workspace");
	std::fs::write(workspace.path().join("App.java"), "class App {}\n").expect("fixture");
	let other = tempfile::tempdir().expect("other workspace");
	let workspace_root = workspace.path().canonicalize().expect("workspace root");
	let other_root = other.path().canonicalize().expect("other root");
	let registry = ToolRegistry::new();
	let context = loaded_context(vec![workspace_root.clone()]);
	let missing_identity = registry
		.call(&context, "code_moniker_read", &json!({"uri": "workspace"}))
		.expect_err("workspace identity must fail closed");
	assert!(
		missing_identity
			.to_string()
			.contains("workspace_identity_required"),
		"{missing_identity}"
	);

	let result = registry
		.call(
			&context,
			"code_moniker_read",
			&json!({
				"uri": "workspace",
				"expected_roots": [workspace_root.display().to_string()]
			}),
		)
		.expect("matching workspace read");
	crate::presentation::tests::validate_agent_markdown(&result.text, "Workspace map", false)
		.expect("workspace map Markdown");
	assert!(
		result.text.contains("## Workspace\n\n### Roots"),
		"{}",
		result.text
	);
	assert!(
		result.text.contains(&workspace_root.display().to_string()),
		"{}",
		result.text
	);
	assert!(
		result.text.contains(&format!(
			"expected_roots=[{}]",
			serde_json::to_string(&workspace_root.display().to_string()).expect("root JSON")
		)),
		"generated workspace follow-ups must preserve identity: {}",
		result.text
	);
	assert!(
		result.text.contains("code_moniker_read uri=\"workspace\""),
		"generated workspace follow-ups must use a navigable compact URI: {}",
		result.text
	);
	assert!(
		!result
			.text
			.contains("code_moniker_read uri=\"code+moniker://workspace\""),
		"compact follow-ups must not retain the canonical workspace URI: {}",
		result.text
	);
	let generated_uri = generated_call_string_arg(&result.text, "code_moniker_read", "uri")
		.expect("workspace response must expose a read follow-up URI");
	assert_eq!(generated_uri, "workspace");
	let replayed = registry
		.call(
			&context,
			"code_moniker_read",
			&json!({
				"uri": generated_uri,
				"expected_roots": [workspace_root.display().to_string()],
				"depth": 3,
				"limit": 20
			}),
		)
		.expect("generated compact workspace follow-up must be navigable");
	assert!(
		replayed.text.contains("## Workspace\n\n### Roots"),
		"{}",
		replayed.text
	);
	for (budget, limit) in [("medium", 80), ("full", 500)] {
		let result = registry
			.call(
				&context,
				"code_moniker_read",
				&json!({
					"uri": "workspace",
					"expected_roots": [workspace_root.display().to_string()],
					"budget": budget
				}),
			)
			.expect("volume-aware workspace read");
		let generated_budget =
			generated_call_string_arg(&result.text, "code_moniker_read", "budget")
				.expect("workspace continuation must preserve its budget");
		assert_eq!(generated_budget, budget, "{}", result.text);
		let generated_uri = generated_call_string_arg(&result.text, "code_moniker_read", "uri")
			.expect("workspace continuation URI");
		let replayed = registry
			.call(
				&context,
				"code_moniker_read",
				&json!({
					"uri": generated_uri,
					"expected_roots": [workspace_root.display().to_string()],
					"depth": 3,
					"limit": limit,
					"budget": generated_budget
				}),
			)
			.expect("generated volume-aware continuation must be replayable");
		assert!(
			replayed.text.contains(&format!("- volume: `{budget}`")),
			"{}",
			replayed.text
		);
	}

	let error = registry
		.call(
			&context,
			"code_moniker_read",
			&json!({
				"uri": "workspace",
				"expected_roots": other_root.display().to_string()
			}),
		)
		.expect_err("workspace mismatch must fail closed");
	let message = error.to_string();
	assert!(message.contains("workspace_mismatch"), "{message}");
	assert!(
		message.contains(&other_root.display().to_string()),
		"{message}"
	);
	assert!(
		message.contains(&workspace_root.display().to_string()),
		"{message}"
	);
}

#[test]
fn search_tool_uses_fuzzy_symbol_search_with_existing_scope_filters() {
	let temp = tempfile::tempdir().expect("tempdir");
	std::fs::create_dir_all(temp.path().join("src/main/java")).expect("mkdir java");
	std::fs::create_dir_all(temp.path().join("src/test/java")).expect("mkdir test");
	std::fs::write(
		temp.path().join("src/main/java/App.java"),
		"class App {\n  void run() {\n    work();\n  }\n}\n",
	)
	.expect("write app");
	std::fs::write(
		temp.path().join("src/main/java/Other.java"),
		"class Other {\n  void retry() {\n    work();\n  }\n}\n",
	)
	.expect("write other");
	std::fs::write(
		temp.path().join("src/test/java/AppTest.java"),
		"class AppTest {\n  void run() {\n    work();\n  }\n}\n",
	)
	.expect("write test");
	let registry = ToolRegistry::new();
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let result = registry
		.call(
			&context,
			"code_moniker_search",
			&json!({
				"query": "r",
				"path": "src/main",
				"lang": "java",
				"kind": "interface",
				"shape": "callable",
				"limit": 1
			}),
		)
		.expect("search");
	assert!(!result.is_error);
	crate::presentation::tests::validate_agent_markdown(&result.text, "Symbol search", false)
		.expect("search CommonMark");
	assert!(
		result
			.text
			.contains("uri: `code+moniker://workspace/search`")
	);
	assert!(result.text.contains("hits: 2"), "{}", result.text);
	assert!(
		result
			.text
			.contains("`method` `run()` `src/main/java/App.java:2-4`"),
		"{}",
		result.text
	);
	assert!(result.text.contains("reason: name"));
	assert!(result.text.contains("uri: `java:"), "{}", result.text);
	assert!(
		!result.text.contains("code+moniker://./"),
		"{}",
		result.text
	);
	assert!(!result.text.contains("code:"));
	assert!(!result.text.contains("   2 |   void run() {"));
	assert!(!result.text.contains("src/test/java/AppTest.java"));
	assert!(result.text.contains("path=\"src/main\""));
	assert!(result.text.contains("lang=\"java\""));
	assert!(result.text.contains("kind=\"interface\""));
	assert!(result.text.contains("shape=\"callable\""));
	assert!(result.text.contains("cursor="));
	assert!(result.text.contains("budget=\"small\""));

	let detail = registry
		.call(
			&context,
			"code_moniker_search",
			&json!({
				"query": "run",
				"path": "src/main",
				"lang": "java",
				"kind": "method",
				"include_code": true,
				"context_lines": 0,
				"limit": 1,
				"compact": false
			}),
		)
		.expect("search with code");
	crate::presentation::tests::validate_agent_markdown(&detail.text, "Symbol search", false)
		.expect("detailed search CommonMark");
	assert!(detail.text.contains("#### Code"), "{}", detail.text);
	assert!(detail.text.contains("   2 |   void run() {"));
	assert!(detail.text.contains("include_code=true"));
	assert!(detail.text.contains("context_lines=0"));
	assert!(detail.text.contains("budget=\"small\""));
	assert!(detail.text.contains("compact=false"));
	assert!(
		detail.text.contains("uri: `code+moniker://./"),
		"{}",
		detail.text
	);
}

#[test]
fn search_tool_rejects_invalid_regex() {
	let registry = ToolRegistry::new();
	let context = empty_context(vec![PathBuf::from(".")]);
	let error = registry
		.call(
			&context,
			"code_moniker_search",
			&json!({"query": "run", "name": "(unclosed"}),
		)
		.unwrap_err();
	assert!(error.to_string().contains("invalid name regex"));
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
fn diff_tool_reports_symbol_level_change_facts() {
	let temp = tempfile::tempdir().expect("tempdir");
	let git = |args: &[&str]| {
		let output = std::process::Command::new("git")
			.arg("-C")
			.arg(temp.path())
			.args(args)
			.output()
			.expect("run git");
		assert!(
			output.status.success(),
			"git {args:?}: {}",
			String::from_utf8_lossy(&output.stderr)
		);
	};
	git(&["init"]);
	git(&["config", "user.email", "cm@example.test"]);
	git(&["config", "user.name", "Code Moniker"]);
	std::fs::create_dir_all(temp.path().join("src")).expect("mkdir");
	std::fs::write(
		temp.path().join("src/util.rs"),
		"pub fn assist() { work(); }\n",
	)
	.expect("write fixture");
	git(&["add", "."]);
	git(&["commit", "-m", "initial"]);
	git(&["mv", "src/util.rs", "src/support.rs"]);
	let registry = ToolRegistry::new();
	let context = loaded_context(vec![temp.path().to_path_buf()]);

	let result = registry
		.call(&context, "code_moniker_diff", &json!({}))
		.expect("diff call");

	assert!(!result.is_error);
	crate::presentation::tests::validate_agent_markdown(&result.text, "Semantic diff", false)
		.expect("valid diff Markdown");
	assert!(
		result
			.text
			.contains("`src/util.rs -> src/support.rs` · `moved`"),
		"{}",
		result.text
	);
	assert!(
		result.text.contains("`moved` `fn`") && result.text.contains("fn:assist()"),
		"symbol facts must carry the side identity: {}",
		result.text
	);
	assert!(result.text.contains("[`certain`]"), "{}", result.text);
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
	assert!(
		result
			.text
			.contains("uri: `code+moniker://workspace/rules`")
	);
	assert!(result.text.contains("action: run"));
	assert!(result.text.contains("corpus: daemon_index"));
	assert!(result.text.contains("generation: 1"));
	assert!(result.text.contains("exit: `match`"));
	assert!(result.text.contains("## Report"));
	crate::presentation::tests::validate_agent_markdown(&result.text, "Project rules check", false)
		.expect("rules check Markdown");
	let project = temp.path().canonicalize().expect("canonical project");
	let normalized = result
		.text
		.replace(project.to_str().expect("utf-8 project"), "<PROJECT>");
	insta::assert_snapshot!("mcp_rules_run_markdown", normalized);
}

#[test]
fn rules_tool_runs_group_line_statistics_on_the_indexed_corpus() {
	let temp = tempfile::tempdir().expect("tempdir");
	std::fs::create_dir_all(temp.path().join("src/main/java")).expect("mkdir");
	std::fs::write(
		temp.path().join("src/main/java/App.java"),
		"class Small {}\nclass Large {\n\tint value;\n}\nclass Other {}\n",
	)
	.expect("write fixture");
	let rules = temp.path().join("scratch-rules.toml");
	std::fs::write(
		&rules,
		r#"
		default_rules = false

		[[workspace.group.where]]
		id = "balanced-types"
		severity = "warn"
		members = "shape = 'type'"
		group_by = ["lang"]
		expr = "count(member) >= 3 => gini(member, lines) = 0"
		"#,
	)
	.expect("write statistic rules");
	let registry = ToolRegistry::new();
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let result = registry
		.call(
			&context,
			"code_moniker_rules",
			&json!({
				"uri": "workspace",
				"action": "run",
				"rules": rules,
				"limit": 5,
				"report": true
			}),
		)
		.expect("statistic rules run");

	assert!(!result.is_error, "{}", result.text);
	assert!(result.text.contains("corpus: daemon_index"));
	assert!(result.text.contains("workspace.group.balanced-types"));
	assert!(result.text.contains("gini(member, lines)="));
	assert!(result.text.contains("3/3 line ranges"));
}

#[test]
fn rules_tool_reports_workspace_path_verdict_coverage_and_witness() {
	let temp = tempfile::tempdir().expect("tempdir");
	std::fs::create_dir_all(temp.path().join("src")).expect("mkdir");
	std::fs::write(
		temp.path().join("src/lib.rs"),
		"pub fn entry() { middle(); }\nfn middle() { sink(); }\npub fn sink() {}\n",
	)
	.expect("write fixture");
	std::fs::write(
		temp.path().join(".code-moniker.toml"),
		r#"
		default_rules = false

		[[workspace.path]]
		id = "entry-must-not-reach-sink"
		severity = "warn"
		from = "shape = 'callable' AND name =~ ^entry"
		to = "shape = 'callable' AND name =~ ^sink"
		expect = "no_path"
		relation = ["calls"]
		max_depth = 4
		max_symbols = 100
		max_edges = 100
		max_pairs = 10
		min_coverage = 100
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
				"action": "run",
				"limit": 5,
				"report": true
			}),
		)
		.expect("rules run");

	assert!(!result.is_error, "{}", result.text);
	assert!(
		result
			.text
			.contains("`workspace.path.entry-must-not-reach-sink`: verdict=`fail`"),
		"{}",
		result.text
	);
	assert!(result.text.contains("coverage: 100%"), "{}", result.text);
	assert!(result.text.contains("decided "), "{}", result.text);
	assert!(
		!result
			.text
			.contains("coverage: 100% (minimum 100%, resolved "),
		"{}",
		result.text
	);
	assert!(result.text.contains("witness:"), "{}", result.text);
	assert!(result.text.contains("-[`calls`]->"), "{}", result.text);
	assert!(
		!result.text.contains("code+moniker://./"),
		"{}",
		result.text
	);
}

#[test]
fn rules_tool_reports_violations_by_srcset() {
	let temp = tempfile::tempdir().expect("tempdir");
	std::fs::create_dir_all(temp.path().join("src/main/java/com/acme")).expect("mkdir main");
	std::fs::create_dir_all(temp.path().join("src/test/java/com/acme")).expect("mkdir test");
	std::fs::write(
		temp.path().join("src/main/java/com/acme/MainType.java"),
		"package com.acme;\npublic class MainType {}\n",
	)
	.expect("write main fixture");
	std::fs::write(
		temp.path().join("src/test/java/com/acme/TestType.java"),
		"package com.acme;\npublic class TestType {}\n",
	)
	.expect("write test fixture");
	std::fs::write(
		temp.path().join(".code-moniker.toml"),
		r#"
		default_rules = false

		[[java.class.where]]
		id = "must-be-main"
		expr = "srcset = 'main'"

		[[java.class.where]]
		id = "must-be-test"
		expr = "srcset = 'test'"
		"#,
	)
	.expect("write rules");
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let result = ToolRegistry::new()
		.call(
			&context,
			"code_moniker_rules",
			&json!({
				"uri": "workspace",
				"action": "run",
				"limit": 5,
				"report": true
			}),
		)
		.expect("rules run");

	assert!(!result.is_error, "{}", result.text);
	assert!(
		result
			.text
			.contains("violations_by_srcset: `main`=1, `test`=1"),
		"{}",
		result.text
	);
}

#[test]
fn rules_tool_distinguishes_rule_errors_from_scan_errors() {
	let temp = tempfile::tempdir().expect("tempdir");
	std::fs::create_dir_all(temp.path().join("src/main/java")).expect("mkdir");
	std::fs::write(temp.path().join("src/main/java/App.java"), "class App {}\n")
		.expect("write fixture");
	std::fs::write(
		temp.path().join(".code-moniker.toml"),
		r#"
		default_rules = false

		[[java.class.where]]
		id = "error-rule"
		expr = "name = 'Expected'"
		severity = "error"

		[[java.class.where]]
		id = "warning-rule"
		expr = "name = 'Expected'"
		severity = "warn"
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
				"action": "run",
				"limit": 5,
				"report": false
			}),
		)
		.expect("rules run");

	assert!(!result.is_error);
	assert!(result.text.contains("verdict: `fail`"), "{}", result.text);
	assert!(result.text.contains("exit: `no_match`"), "{}", result.text);
	assert!(
		result
			.text
			.contains("2 violation(s): 1 warning(s), 1 rule error(s); 0 scan error(s)")
	);
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
	assert!(result.text.contains("exit: `match`"));
	assert!(result.text.contains(&format!(
		"root: `{}`",
		first.canonicalize().expect("canonical first").display()
	)));
	assert!(result.text.contains(&format!(
		"root: `{}`",
		second.canonicalize().expect("canonical second").display()
	)));
}

#[test]
fn generic_query_identity_graph_next_cursor_is_replayable() {
	let temp = tempfile::tempdir().expect("tempdir");
	std::fs::create_dir_all(temp.path().join("src/main/java/a")).expect("mkdir a");
	std::fs::create_dir_all(temp.path().join("src/main/java/b")).expect("mkdir b");
	std::fs::write(
		temp.path().join("src/main/java/a/App.java"),
		"package a; import b.Other; class App { void run() { new Other().work(); } }\n",
	)
	.expect("write App");
	std::fs::write(
		temp.path().join("src/main/java/b/Other.java"),
		"package b; public class Other { public void work() {} }\n",
	)
	.expect("write Other");
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let registry = ToolRegistry::new();
	let query =
		"identity.graph prefix:\"srcset:main/lang:java\" path:\"src/main/**\" min_count:1 limit:1";

	let first = registry
		.call(&context, "code_moniker_query", &json!({"query": query}))
		.expect("first identity graph page");
	assert!(first.text.contains("## Next"), "{}", first.text);
	assert!(
		first.text.contains("code_moniker_query query="),
		"{}",
		first.text
	);
	assert!(first.text.contains("path:"), "{}", first.text);
	assert!(first.text.contains("src/main/"), "{}", first.text);
	assert!(first.text.contains("min_count:1"), "{}", first.text);
	assert!(first.text.contains("limit:1"), "{}", first.text);
	let cursor = first
		.text
		.split(" cursor=\"")
		.nth(1)
		.and_then(|suffix| suffix.split('"').next())
		.expect("generation-aware cursor");
	assert!(cursor.contains(':'), "{cursor}");

	let second = registry
		.call(
			&context,
			"code_moniker_query",
			&json!({"query": query, "cursor": cursor}),
		)
		.expect("replayed identity graph page");
	assert!(!second.is_error, "{}", second.text);
	assert!(
		second.text.contains("operation: `identity.graph`"),
		"{}",
		second.text
	);
	assert_ne!(
		first.text, second.text,
		"cursor replay must advance the page"
	);
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
	assert!(result.text.contains("## Scope"));
	assert!(result.text.contains("mcp-root-class-rule"));
	assert!(result.text.contains("loaded from workspace root"));
	assert!(result.text.contains("## Next"));
	assert!(result.text.contains("lang=\"java\""));
	assert!(result.text.contains("severity=\"error\""));
	assert!(result.text.contains("cursor="));
	crate::presentation::tests::validate_agent_markdown(
		&result.text,
		"Active project rules",
		false,
	)
	.expect("rules list Markdown");
	insta::assert_snapshot!("mcp_rules_list_markdown", result.text);
}

#[test]
fn read_views_lists_and_renders_fragment_view() {
	let temp = tempfile::tempdir().expect("tempdir");
	let source_dir = temp.path().join("src/main/java");
	write_fragment_view_fixture(temp.path(), &source_dir);
	let registry = ToolRegistry::new();
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let list = registry
		.call(
			&context,
			"code_moniker_read",
			&json!({"uri": "workspace/views"}),
		)
		.expect("view list");
	crate::presentation::tests::validate_agent_markdown(&list.text, "Project views", false)
		.expect("view list Markdown");
	assert!(!list.is_error);
	assert!(list.text.contains("uri: `code+moniker://workspace/views`"));
	assert!(list.text.contains("java-app"));
	assert!(list.text.contains("root-map"));
	assert!(
		list.text
			.contains("code_moniker_read uri=\"workspace/views/java-app\"")
	);
	let generated_uri = generated_call_string_arg(&list.text, "code_moniker_read", "uri")
		.expect("view list must expose a read follow-up URI");
	assert_eq!(generated_uri, "workspace/views/java-app");

	let detail = registry
		.call(
			&context,
			"code_moniker_read",
			&json!({
				"uri": generated_uri,
				"context_lines": 0,
				"moniker_format": "compact"
			}),
		)
		.expect("compact view next must be navigable");
	crate::presentation::tests::validate_agent_markdown(
		&detail.text,
		"Project view: java-app",
		false,
	)
	.expect("view detail Markdown");
	assert!(!detail.is_error);
	assert!(detail.text.contains("view: java-app"), "{}", detail.text);
	assert!(detail.text.contains("## Rules"));
	assert_eq!(
		detail.text.matches("Boundary rule rationale.").count(),
		1,
		"{}",
		detail.text
	);
	assert!(
		detail.text.contains("view-boundary-rule"),
		"{}",
		detail.text
	);
	assert!(detail.text.contains("## Boundaries"));
	assert!(
		detail.text.contains("- status: `enforced_by_rules`"),
		"{}",
		detail.text
	);
	assert!(detail.text.contains("- enforced by:"), "{}", detail.text);
	assert!(detail.text.contains("## Gotchas"));
	assert!(detail.text.contains("moniker:"));
	assert!(
		detail
			.text
			.lines()
			.any(|line| line.trim_start().starts_with("- file:")),
		"view evidence must render file metadata as its own item: {}",
		detail.text
	);
	assert!(
		!detail
			.text
			.lines()
			.any(|line| line.contains("moniker:") && line.contains("- file:")),
		"view evidence metadata must not be fused: {}",
		detail.text
	);
	assert!(detail.text.contains("class:App"), "{}", detail.text);
	assert!(detail.text.contains("method:run"), "{}", detail.text);
	assert!(detail.text.contains("selector: `count`"), "{}", detail.text);
	assert!(detail.text.contains("status: missing"), "{}", detail.text);
	assert!(!detail.text.contains("```text"), "{}", detail.text);
	assert!(
		!detail.text.contains("void run(int count)"),
		"{}",
		detail.text
	);

	let detail_with_code = registry
		.call(
			&context,
			"code_moniker_read",
			&json!({
				"uri": "workspace/views/java-app",
				"context_lines": 0,
				"include_code": true
			}),
		)
		.expect("view detail with code");
	assert!(!detail_with_code.is_error);
	assert!(detail_with_code.text.contains("```text"));
	assert!(
		detail_with_code.text.contains("void run(int count)"),
		"{}",
		detail_with_code.text
	);
}

fn write_fragment_view_fixture(root: &std::path::Path, source_dir: &std::path::Path) {
	std::fs::create_dir_all(source_dir).expect("mkdir");
	std::fs::write(
		source_dir.join("App.java"),
		"class App {\n  void before() {}\n  void run(int count) {\n    work();\n  }\n}\n",
	)
	.expect("write fixture");
	std::fs::write(
		root.join(".code-moniker.toml"),
		r#"
		default_rules = false

		[[java.class.where]]
		id = "view-boundary-rule"
		expr = "name =~ ^App$"
		message = "boundary rule"
		rationale = """
		Boundary rule rationale.
		"""

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
		forbid_rules = ["view-boundary-rule"]
		rationale = """
		The entry boundary highlights the class and method an agent should inspect first.
		"""
		symbols = ["class:App", "method:run", "count"]
		rules = ["view-boundary-rule"]

		[[views.gotchas]]
		id = "method-slice"
		rationale = "The run method should render a source slice as evidence."
		symbols = ["method:run"]
		"#,
	)
	.expect("write fragment view");
}

#[test]
fn http_tool_call_reads_workspace_explorer() {
	let temp = tempfile::tempdir().expect("tempdir");
	std::fs::create_dir_all(temp.path().join("src/main/java")).expect("mkdir");
	std::fs::create_dir_all(temp.path().join("tests")).expect("mkdir tests");
	std::fs::write(temp.path().join("src/main/java/App.java"), "class App {}\n")
		.expect("write fixture");
	std::fs::write(temp.path().join("tests/AppTest.java"), "class AppTest {}\n")
		.expect("write test fixture");
	let opts = SessionOptions {
		paths: vec![temp.path().to_path_buf()],
		project: None,
		cache_dir: None,
	};
	let server = start_http_test_server(opts.clone());
	let response = post_rpc(
		server.addr,
		&json!({
			"jsonrpc": "2.0",
			"id": 7,
			"method": "tools/call",
			"params": {
				"name": "code_moniker_read",
				"arguments": {
					"uri": "workspace",
					"expected_roots": [temp.path().display().to_string()],
					"depth": 4,
					"limit": 1
				}
			}
		}),
	);
	assert!(response.contains("HTTP/1.1 200 OK"));
	assert!(response.contains("uri: `code+moniker://workspace`"));
	assert!(response.contains("## Next"), "{response}");
	assert!(
		response.contains("code_moniker_read uri=\\\"workspace\\\""),
		"{response}"
	);
	assert!(
		!response.contains("code_moniker_read uri=\\\"code+moniker://workspace\\\""),
		"{response}"
	);
	let cursor = escaped_call_argument(&response, "cursor").expect("generated next cursor");
	let next = post_rpc(
		server.addr,
		&json!({
			"jsonrpc": "2.0",
			"id": 8,
			"method": "tools/call",
			"params": {
				"name": "code_moniker_read",
				"arguments": {
					"uri": "workspace",
					"expected_roots": [temp.path().display().to_string()],
					"depth": 4,
					"limit": 1,
					"cursor": cursor
				}
			}
		}),
	);
	assert!(next.contains("HTTP/1.1 200 OK"), "{next}");
	assert!(
		next.contains("tests/") || next.contains("AppTest.java"),
		"{next}"
	);
}

#[test]
fn http_tool_call_parses_stateless_source_text() {
	let temp = tempfile::tempdir().expect("tempdir");
	let server =
		start_http_test_server_with_context(preloading_context(vec![temp.path().to_path_buf()]));
	let response = post_rpc(
		server.addr,
		&json!({
			"jsonrpc": "2.0",
			"id": 9,
			"method": "tools/call",
			"params": {
				"name": "code_moniker_read",
				"arguments": {
					"language": "plpgsql",
					"source": "DECLARE total numeric; BEGIN total := 1; RETURN total; END;",
					"include_text": true,
					"max_depth": 12,
					"max_nodes": 200,
					"budget": "full"
				}
			}
		}),
	);
	assert!(response.contains("HTTP/1.1 200 OK"), "{response}");
	assert!(response.contains("uri: `syntax.parse`"), "{response}");
	assert!(response.contains("file: `snippet.plpgsql`"), "{response}");
	assert!(response.contains("decl_statement"), "{response}");
	assert!(response.contains("stmt_assign"), "{response}");
	assert!(response.contains("stmt_return"), "{response}");
	assert!(
		!temp.path().join("snippet.plpgsql").exists(),
		"HTTP stateless parse must not persist source"
	);
}

#[test]
fn http_workspace_identity_failures_fail_closed_with_routing_guidance() {
	let workspace = tempfile::tempdir().expect("workspace");
	let other = tempfile::tempdir().expect("other workspace");
	std::fs::write(workspace.path().join("App.java"), "class App {}\n").expect("fixture");
	let server = start_http_test_server(SessionOptions {
		paths: vec![workspace.path().to_path_buf()],
		project: None,
		cache_dir: None,
	});
	let missing = post_rpc(
		server.addr,
		&json!({
			"jsonrpc": "2.0",
			"id": 7,
			"method": "tools/call",
			"params": {
				"name": "code_moniker_read",
				"arguments": { "uri": "workspace" }
			}
		}),
	);
	assert!(missing.contains("workspace_identity_required"), "{missing}");
	assert!(missing.contains("expected_roots"), "{missing}");
	let response = post_rpc(
		server.addr,
		&json!({
			"jsonrpc": "2.0",
			"id": 8,
			"method": "tools/call",
			"params": {
				"name": "code_moniker_read",
				"arguments": {
					"uri": "workspace",
					"expected_roots": [other.path().display().to_string()]
				}
			}
		}),
	);
	assert!(response.contains("workspace_mismatch"), "{response}");
	assert!(response.contains("project-owned"), "{response}");
}

#[test]
fn http_initialized_notification_is_accepted_without_json_response() {
	let temp = tempfile::tempdir().expect("tempdir");
	std::fs::write(temp.path().join("App.java"), "class App {}\n").expect("write fixture");
	let opts = SessionOptions {
		paths: vec![temp.path().to_path_buf()],
		project: None,
		cache_dir: None,
	};
	let server = start_http_test_server(opts.clone());
	let response = post_rpc(
		server.addr,
		&json!({
			"jsonrpc": "2.0",
			"method": "notifications/initialized"
		}),
	);
	assert!(response.contains("HTTP/1.1 202 Accepted"));
}

fn post_rpc(addr: SocketAddr, body: &serde_json::Value) -> String {
	let body = body.to_string();
	let mut stream = TcpStream::connect(addr).expect("connect");
	write!(
		stream,
		"POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nConnection: close\r\n"
	)
	.expect("request head");
	write!(stream, "Content-Length: {}\r\n\r\n{}", body.len(), body).expect("request body");
	let mut response = String::new();
	stream.read_to_string(&mut response).expect("response");
	response
}

fn escaped_call_argument(response: &str, name: &str) -> Option<String> {
	let marker = format!("{name}=\\\"");
	let value = response.split_once(&marker)?.1;
	Some(value.split_once("\\\"")?.0.to_string())
}
