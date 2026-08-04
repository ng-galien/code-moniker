use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use code_moniker_daemon::WorkspaceDaemon;
use code_moniker_query::{
	Command, CommandRequest, DaemonWorkspaceConfig, Query, QueryRequest, QueryResult,
	SymbolListResult, SymbolSearchQuery, query_capability_specs,
};
use code_moniker_workspace::snapshot::{
	LinkageEdge, LinkageSnapshot, ReferenceId, ReferenceRecord, ResourceGeneration, SourceCatalog,
	SourceFileRecord, SourceId, SourceUnit, SymbolId, SymbolRecord,
};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::context::{DaemonRuntime, McpContext};
use super::tools;
use super::tools::read::{render_explorer_lmnav, render_symbol_source_lmnav};
use super::tools::scope::{Paging, ScopeFilter, SymbolScopeFilter};
use super::tools::symbols::{
	SymbolAction, SymbolIndexView, render_daemon_symbol_list_lmnav, render_symbols_lmnav,
	render_symbols_lmnav_mode,
};
use super::tools::usages::{UsageDirection, UsageIndexView, UsageQuery, render_usages_lmnav};
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
		DaemonRuntime::in_process(WorkspaceDaemon::new(paths).expect("daemon")),
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
	let argument_prefix = format!("{argument}=");
	text.lines().find_map(|line| {
		let call = line.trim_start();
		if !call.starts_with(&call_prefix) {
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

fn source_file(id: SourceId, rel_path: &str, language: &str) -> SourceFileRecord {
	SourceFileRecord {
		id,
		uri: format!("code+moniker://./file:{rel_path}"),
		source_root: 0,
		path: rel_path.to_string(),
		rel_path: rel_path.to_string(),
		anchor: rel_path.to_string(),
		language: language.to_string(),
		text: String::new(),
	}
}

fn symbol_record(
	id: SymbolId,
	source: SourceId,
	identity: &str,
	name: &str,
	kind: &str,
	line_range: Option<(u32, u32)>,
) -> SymbolRecord {
	SymbolRecord {
		id,
		source,
		identity: std::sync::Arc::from(identity),
		name: name.to_string(),
		kind: kind.to_string(),
		visibility: "public".to_string(),
		signature: String::new(),
		call_name: None,
		call_arity: None,
		navigable: true,
		line_range,
		parent: None,
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
fn read_root_summarizes_workspace_and_limits_explorer() {
	let catalog = SourceCatalog::new(
		ResourceGeneration::new(1),
		vec![
			SourceUnit::with_language(SourceId::at(1), "root/src/main/java/App.java", "java"),
			SourceUnit::with_language(SourceId::at(3), "root/src/main/java/Other.java", "java"),
			SourceUnit::with_language(SourceId::at(2), "root/pom.xml", "xml"),
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
			generation: None,
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
		text.contains("code_moniker_symbols uri=\"code+moniker://workspace\" path=\"root/src/**\" lang=\"java\" limit=20")
	);
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
		assert_eq!(
			tool["inputSchema"]["properties"]["max_chars"]["type"],
			"integer"
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
fn compact_mode_uses_a_smaller_default_page_without_changing_explicit_limits() {
	assert_eq!(
		Paging::from_arguments_for_output(&json!({}), true)
			.unwrap()
			.limit,
		20
	);
	assert_eq!(
		Paging::from_arguments_for_output(&json!({}), false)
			.unwrap()
			.limit,
		80
	);
	assert_eq!(
		Paging::from_arguments_for_output(&json!({"limit": 7}), true)
			.unwrap()
			.limit,
		7
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

	assert!(batch.text.contains("result: 1"), "{}", batch.text);
	assert!(
		batch.text.contains("operation: symbol.detail"),
		"the healthy result must survive: {}",
		batch.text
	);
	assert!(
		batch.text.contains("result: 2") && batch.text.contains("prefix_not_found"),
		"the failing expression must report inline: {}",
		batch.text
	);
	assert!(
		batch.text.contains("completeness: partial (1 error(s))"),
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
		described.text.contains("operation: query.describe"),
		"{}",
		described.text
	);
	assert!(
		described.text.contains("identity.graph"),
		"{}",
		described.text
	);
	assert!(
		described.text.contains("read_only=true"),
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
	assert!(metrics.text.contains("export"), "{}", metrics.text);

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
		graph.text.contains("operation: identity.graph"),
		"{}",
		graph.text
	);
	assert!(graph.text.contains("nodes:"), "{}", graph.text);

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
	assert!(batch.text.contains("mode: query.batch"), "{}", batch.text);
	assert!(batch.text.contains(&compact), "{}", batch.text);
	assert!(!batch.text.contains(&moniker), "{}", batch.text);
	assert!(
		batch.text.contains(&format!("\"{literal}\"")),
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
	assert!(!tree.is_error);
	assert!(tree.text.contains("uri: syntax.tree"), "{}", tree.text);
	assert!(tree.text.contains("file: src/lib.rs"), "{}", tree.text);
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
		bounded.text.contains("completeness: bounded"),
		"{}",
		bounded.text
	);
	assert!(bounded.text.contains("nodes: 2/"), "{}", bounded.text);

	let invalid = registry.call(
		&context,
		"code_moniker_read",
		&json!({"uri": "src/lib.rs", "ast": true, "max_nodes": 0}),
	);
	let error = invalid.expect_err("zero AST node limit must fail");
	assert!(
		error.to_string().contains("max_nodes must be between 1"),
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
				"max_nodes": 200
			}),
		)
		.expect("stateless PL/pgSQL AST");
	assert!(!tree.is_error);
	assert!(tree.text.contains("uri: syntax.parse"), "{}", tree.text);
	assert!(tree.text.contains("file: snippet.plpgsql"), "{}", tree.text);
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
	assert!(rust.text.contains("file: virtual.rs"), "{}", rust.text);
	assert!(rust.text.contains("- function_item "), "{}", rust.text);

	for arguments in [
		json!({"ast": true, "source": "fn main() {}"}),
		json!({"ast": true, "language": "rs"}),
	] {
		let error = registry
			.call(&context, "code_moniker_read", &arguments)
			.expect_err("invalid stateless AST contract must fail");
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
				"max_nodes": 500
			}),
		)
		.expect("PL/pgSQL AST read");
	assert!(!tree.is_error);
	assert!(
		tree.text.contains("- source_file") && tree.text.contains("[plpgsql]"),
		"{}",
		tree.text
	);
	assert!(tree.text.contains("- stmt_if "), "{}", tree.text);
	assert!(tree.text.contains("- sql_expression "), "{}", tree.text);
}

#[test]
fn context_tool_returns_bounded_pre_change_facts_and_canonical_checks() {
	let temp = tempfile::tempdir().expect("tempdir");
	write_java_app_fixture(
		temp.path(),
		"class App {\n  void run() { helper(); }\n  void helper() {}\n}\n",
	);
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
	assert!(!result.is_error);
	assert!(
		result.text.contains("mode: change.context"),
		"{}",
		result.text
	);
	assert!(result.text.contains("coverage:"), "{}", result.text);
	assert!(result.text.contains("members:"), "{}", result.text);
	assert!(result.text.contains(&compact), "{}", result.text);
	assert!(!result.text.contains(&moniker), "{}", result.text);
	assert!(result.text.contains("suggested_checks:"), "{}", result.text);
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

	assert!(result.text.contains("name: App"), "{}", result.text);
	assert!(result.text.contains("uri: java:"), "{}", result.text);
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
	assert!(result.text.contains("page_refs: 2"), "{}", result.text);
	assert!(result.text.contains("groups: 1"), "{}", result.text);
	assert!(result.text.contains("[2 refs]"), "{}", result.text);
	assert!(result.text.contains("evidence:"), "{}", result.text);
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
	assert!(result.text.contains("incoming_summary:"), "{}", result.text);
	assert!(result.text.contains("files: 3"), "{}", result.text);
	assert!(
		result
			.text
			.contains("shared_helper_signal: shared_helper_candidate"),
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
	assert!(first.text.contains("page_refs: 3"), "{}", first.text);
	assert!(first.text.contains("groups: 1"), "{}", first.text);
	assert!(first.text.contains("[3 refs]"), "{}", first.text);
	assert!(
		first.text.contains("- in production calls caller_a"),
		"{}",
		first.text
	);
	assert!(
		!first.text.contains("- in production calls caller_b"),
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
	assert!(second.text.contains("page_refs: 2"), "{}", second.text);
	assert!(second.text.contains("groups: 1"), "{}", second.text);
	assert!(second.text.contains("[2 refs]"), "{}", second.text);
	assert!(
		second.text.contains("- in production calls caller_b"),
		"{}",
		second.text
	);
	assert!(
		!second.text.contains("- in production calls caller_a"),
		"{}",
		second.text
	);
}

#[test]
fn invalid_output_budget_is_rejected_before_note_mutation() {
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
			"body": "The response budget is invalid.",
			"created_by": "user",
			"max_chars": 99
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
	assert!(compact.text.contains("uri: java:"), "{}", compact.text);
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
		verbose.text.contains("uri: code+moniker://./"),
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
	assert!(created.text.contains("action: create"), "{}", created.text);
	assert!(
		created.text.contains("resolution: resolved"),
		"{}",
		created.text
	);
	assert!(created.text.contains("kind: todo"), "{}", created.text);
	assert!(created.text.contains(&compact), "{}", created.text);
	assert!(!created.text.contains(&moniker), "{}", created.text);
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
	assert!(ongoing.text.contains("status: ongoing"), "{}", ongoing.text);

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
	assert!(done.text.contains("status: done"), "{}", done.text);

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
	assert!(deleted.text.contains("action: delete"), "{}", deleted.text);
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
	assert!(note.text.contains("status: pending"), "{}", note.text);
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

	assert!(created.text.contains("action: create"), "{}", created.text);
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
		created.text.contains("target: module App"),
		"{}",
		created.text
	);
	assert!(
		created.text.contains("file: src/main/java/App.java"),
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
		.expect_err("workspace read without expected roots must fail");
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
	assert!(
		result.text.contains("workspace:\n  roots:"),
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
		replayed.text.contains("workspace:\n  roots:"),
		"{}",
		replayed.text
	);

	let error = registry
		.call(
			&context,
			"code_moniker_read",
			&json!({
				"uri": "workspace",
				"expected_roots": other_root.display().to_string()
			}),
		)
		.expect_err("mismatched workspace must fail");
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
fn search_tool_uses_tui_symbol_search_with_existing_scope_filters() {
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
	assert!(result.text.contains("uri: code+moniker://workspace/search"));
	assert!(result.text.contains("hits: 2"), "{}", result.text);
	assert!(
		result
			.text
			.contains("method run() src/main/java/App.java:2-4"),
		"{}",
		result.text
	);
	assert!(result.text.contains("reason: name"));
	assert!(result.text.contains("uri: java:"), "{}", result.text);
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
	assert!(detail.text.contains("code:"), "{}", detail.text);
	assert!(detail.text.contains("   2 |   void run() {"));
	assert!(detail.text.contains("include_code=true"));
	assert!(detail.text.contains("context_lines=0"));
	assert!(detail.text.contains("compact=false"));
	assert!(
		detail.text.contains("uri: code+moniker://./"),
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
	assert!(
		result.text.contains("src/util.rs -> src/support.rs moved"),
		"{}",
		result.text
	);
	assert!(
		result.text.contains("moved fn") && result.text.contains("fn:assist()"),
		"symbol facts must carry the side identity: {}",
		result.text
	);
	assert!(result.text.contains("[certain]"), "{}", result.text);
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
	assert!(result.text.contains("corpus: daemon_index"));
	assert!(result.text.contains("generation: 1"));
	assert!(result.text.contains("exit: match"));
	assert!(result.text.contains("report:"));
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
			.contains("workspace.path.entry-must-not-reach-sink: verdict=fail"),
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
	assert!(result.text.contains("-[calls]->"), "{}", result.text);
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
		result.text.contains("violations_by_srcset: main=1, test=1"),
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
	assert!(result.text.contains("verdict: fail"), "{}", result.text);
	assert!(result.text.contains("exit: no_match"), "{}", result.text);
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
	assert!(result.text.contains("exit: match"));
	assert!(result.text.contains(&format!(
		"root: {}",
		first.canonicalize().expect("canonical first").display()
	)));
	assert!(result.text.contains(&format!(
		"root: {}",
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
	assert!(first.text.contains("next:"), "{}", first.text);
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
		second.text.contains("operation: identity.graph"),
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
	assert!(result.text.contains("scope:"));
	assert!(result.text.contains("mcp-root-class-rule"));
	assert!(result.text.contains("loaded from workspace root"));
	assert!(result.text.contains("next:"));
	assert!(result.text.contains("lang=\"java\""));
	assert!(result.text.contains("severity=\"error\""));
	assert!(result.text.contains("cursor="));
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
	assert!(!list.is_error);
	assert!(list.text.contains("uri: code+moniker://workspace/views"));
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
	assert!(!detail.is_error);
	assert!(detail.text.contains("view: java-app"), "{}", detail.text);
	assert!(detail.text.contains("rules:"));
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
	assert!(
		detail
			.text
			.contains("- missing-view-rule [missing] domain=unresolved"),
		"{}",
		detail.text
	);
	assert!(detail.text.contains("boundaries:"));
	assert!(
		detail.text.contains("forbids_status: enforced_by_rules"),
		"{}",
		detail.text
	);
	assert!(detail.text.contains("forbid_rules:"), "{}", detail.text);
	assert!(detail.text.contains("gotchas:"));
	assert!(detail.text.contains("moniker:"));
	assert!(detail.text.contains("class:App"), "{}", detail.text);
	assert!(detail.text.contains("method:run"), "{}", detail.text);
	assert!(detail.text.contains("selector: count"), "{}", detail.text);
	assert!(detail.text.contains("status: missing"), "{}", detail.text);
	assert!(!detail.text.contains("code:"), "{}", detail.text);
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
	assert!(detail_with_code.text.contains("code:"));
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
		rules = ["view-boundary-rule", "missing-view-rule"]

		[[views.gotchas]]
		id = "method-slice"
		rationale = "The run method should render a source slice as evidence."
		symbols = ["method:run"]
		"#,
	)
	.expect("write fragment view");
}

fn app_symbols_fixture() -> (Vec<SourceFileRecord>, Vec<SymbolRecord>, SymbolScopeFilter) {
	let source_id = SourceId::at(1);
	let sources = vec![SourceFileRecord {
		id: source_id.clone(),
		uri: "code+moniker://./file:src/App.java".to_string(),
		source_root: 0,
		path: "src/App.java".to_string(),
		rel_path: "src/App.java".to_string(),
		anchor: "src/App.java".to_string(),
		language: "java".to_string(),
		text: String::new(),
	}];
	let symbols = vec![
		SymbolRecord {
			id: SymbolId::at(0, 1),
			source: source_id.clone(),
			identity: std::sync::Arc::from("code+moniker://./lang:java/package:src/class:App"),
			name: "App".to_string(),
			kind: "class".to_string(),
			visibility: "public".to_string(),
			signature: String::new(),
			call_name: None,
			call_arity: None,
			navigable: true,
			line_range: Some((1, 3)),
			parent: None,
		},
		SymbolRecord {
			id: SymbolId::at(0, 2),
			source: source_id.clone(),
			identity: std::sync::Arc::from(
				"code+moniker://./lang:java/package:src/class:App/method:run()",
			),
			name: "run".to_string(),
			kind: "method".to_string(),
			visibility: "public".to_string(),
			signature: String::new(),
			call_name: None,
			call_arity: None,
			navigable: true,
			line_range: Some((4, 5)),
			parent: None,
		},
		SymbolRecord {
			id: SymbolId::at(0, 3),
			source: source_id,
			identity: std::sync::Arc::from(
				"code+moniker://./lang:java/package:src/class:App/method:retry()",
			),
			name: "retry".to_string(),
			kind: "method".to_string(),
			visibility: "private".to_string(),
			signature: String::new(),
			call_name: None,
			call_arity: None,
			navigable: true,
			line_range: Some((6, 7)),
			parent: None,
		},
	];
	let scope = SymbolScopeFilter::from_arguments(&json!({
		"path": "src/**",
		"lang": "java",
		"kind": "method",
		"name": "^r"
	}))
	.unwrap();
	(sources, symbols, scope)
}

#[test]
fn symbols_tool_verbose_mode_keeps_canonical_uris_and_next_calls() {
	let (sources, symbols, scope) = app_symbols_fixture();
	let verbose = render_symbols_lmnav_mode(
		"code+moniker://",
		"workspace",
		&scope,
		Paging {
			cursor: 0,
			generation: None,
			limit: 1,
		},
		SymbolIndexView {
			sources: &sources,
			symbols: &symbols,
			references: &[],
		},
		(SymbolAction::List, false),
	);
	assert!(
		verbose.contains("uri: code+moniker://./lang:java/package:src/class:App/method:run()"),
		"{verbose}"
	);
	assert!(verbose.contains("limit=50"), "{verbose}");
	assert!(verbose.contains("compact=false"), "{verbose}");
	assert!(verbose.contains("usages: code_moniker_usages"), "{verbose}");
	assert!(verbose.contains("code_moniker_read"), "{verbose}");
}

#[test]
fn symbols_tool_filters_and_pages_symbols() {
	let (sources, symbols, scope) = app_symbols_fixture();
	let text = render_symbols_lmnav(
		"code+moniker://",
		"workspace",
		&scope,
		Paging {
			cursor: 0,
			generation: None,
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
	assert!(!text.contains("usages: code_moniker_usages"), "{text}");
	assert!(
		text.contains("uri: code+moniker://./lang:java/package:src/class:App/method:run()"),
		"{text}"
	);
	assert!(!text.contains("class App"), "{text}");
	assert!(text.contains("name=\"^r\""), "{text}");
	assert!(text.contains("cursor=1"), "{text}");
	assert!(!text.contains("code_moniker_read"), "{text}");
}

#[test]
fn symbols_tool_lists_production_before_tests() {
	let bench_source_id = SourceId::at(1);
	let production_source_id = SourceId::at(2);
	let sources = vec![
		source_file(bench_source_id.clone(), "benches/speed.rs", "rs"),
		source_file(production_source_id.clone(), "src/lib.rs", "rs"),
	];
	let symbols = vec![
		symbol_record(
			SymbolId::at(1, 1),
			bench_source_id,
			"code+moniker://./lang:rs/dir:benches/module:speed/fn:benchmark_helper()",
			"benchmark_helper()",
			"fn",
			Some((2, 3)),
		),
		symbol_record(
			SymbolId::at(2, 1),
			production_source_id.clone(),
			"code+moniker://./lang:rs/module:tests/fn:helper()",
			"helper()",
			"fn",
			Some((2, 3)),
		),
		symbol_record(
			SymbolId::at(2, 2),
			production_source_id,
			"code+moniker://./lang:rs/module:lib/fn:production_entry()",
			"production_entry()",
			"fn",
			Some((8, 9)),
		),
	];
	let text = render_symbols_lmnav(
		"code+moniker://",
		"workspace",
		&SymbolScopeFilter::from_arguments(&json!({"shape": "callable"})).unwrap(),
		Paging {
			cursor: 0,
			generation: None,
			limit: 1,
		},
		SymbolIndexView {
			sources: &sources,
			symbols: &symbols,
			references: &[],
		},
		SymbolAction::List,
	);

	assert!(text.contains("production_entry()"), "{text}");
	assert!(!text.contains("benchmark_helper()"), "{text}");
	assert!(!text.contains("helper()"), "{text}");
}

#[test]
fn symbols_tool_explains_signed_callable_names_after_an_exact_bare_name_miss() {
	let source_id = SourceId::at(1);
	let sources = vec![source_file(source_id.clone(), "src/lib.rs", "rs")];
	let symbols = vec![symbol_record(
		SymbolId::at(0, 1),
		source_id,
		"code+moniker://./lang:rs/module:lib/fn:call(context:&Context)",
		"call(context:&Context)",
		"fn",
		Some((1, 3)),
	)];
	let scope = SymbolScopeFilter::from_arguments(&json!({
		"path": "src/**",
		"lang": "rs",
		"shape": "callable",
		"name": "^call$"
	}))
	.unwrap();
	let text = render_symbols_lmnav(
		"code+moniker://",
		"workspace",
		&scope,
		Paging {
			cursor: 0,
			generation: None,
			limit: 20,
		},
		SymbolIndexView {
			sources: &sources,
			symbols: &symbols,
			references: &[],
		},
		SymbolAction::List,
	);

	assert!(text.contains("symbols: 0"), "{text}");
	assert!(
		text.contains("callable names may include their parameter signature"),
		"{text}"
	);
	assert!(text.contains(r#"try name="^call\\(""#), "{text}");
}

#[test]
fn symbols_tool_does_not_suggest_callable_signatures_for_explicit_type_searches() {
	let scope = SymbolScopeFilter::from_arguments(&json!({
		"kind": "fn",
		"shape": "type",
		"name": "^Missing$"
	}))
	.unwrap();
	let text = render_symbols_lmnav(
		"code+moniker://",
		"workspace",
		&scope,
		Paging {
			cursor: 0,
			generation: None,
			limit: 20,
		},
		SymbolIndexView {
			sources: &[],
			symbols: &[],
			references: &[],
		},
		SymbolAction::List,
	);

	assert!(!text.contains("callable names may include"), "{text}");
}

#[test]
fn symbols_tool_does_not_explain_a_daemon_page_past_existing_matches() {
	let scope = SymbolScopeFilter::from_arguments(&json!({
		"name": "^call$"
	}))
	.unwrap();
	let text = render_daemon_symbol_list_lmnav(
		"code+moniker://",
		"workspace",
		&scope,
		(
			Paging {
				cursor: 1,
				generation: None,
				limit: 20,
			},
			true,
		),
		None,
		&SymbolListResult {
			rows: Vec::new(),
			total: 1,
		},
	);

	assert!(text.contains("symbols: 1"), "{text}");
	assert!(!text.contains("callable names may include"), "{text}");
}

#[test]
fn usages_render_shared_helper_signal_from_cross_prefix_consumers() {
	let helper_source = SourceId::at(0);
	let app_source = SourceId::at(1);
	let batch_source = SourceId::at(2);
	let api_source = SourceId::at(3);
	let sources = vec![
		source_file(helper_source.clone(), "src/shared/Helper.java", "java"),
		source_file(app_source.clone(), "src/app/App.java", "java"),
		source_file(batch_source.clone(), "src/batch/Job.java", "java"),
		source_file(api_source.clone(), "src/api/Controller.java", "java"),
	];
	let helper = symbol_record(
		SymbolId::at(0, 20),
		helper_source,
		"code+moniker://./lang:java/package:shared/class:Helper",
		"Helper",
		"class",
		Some((1, 12)),
	);
	let app = symbol_record(
		SymbolId::at(1, 0),
		app_source.clone(),
		"code+moniker://./lang:java/package:app/class:App/method:run()",
		"run",
		"method",
		Some((3, 5)),
	);
	let batch = symbol_record(
		SymbolId::at(2, 0),
		batch_source.clone(),
		"code+moniker://./lang:java/package:batch/class:Job/method:run()",
		"run",
		"method",
		Some((4, 6)),
	);
	let api = symbol_record(
		SymbolId::at(3, 0),
		api_source.clone(),
		"code+moniker://./lang:java/package:api/class:Controller/method:handle()",
		"handle",
		"method",
		Some((5, 8)),
	);
	let references = vec![
		ReferenceRecord::new(
			ReferenceId::at(1, 0),
			app_source,
			SymbolId::at(1, 0),
			helper.identity.as_ref(),
			"uses_type",
			Some((4, 4)),
		),
		ReferenceRecord::new(
			ReferenceId::at(2, 0),
			batch_source,
			SymbolId::at(2, 0),
			helper.identity.as_ref(),
			"calls",
			Some((5, 5)),
		),
		ReferenceRecord::new(
			ReferenceId::at(3, 0),
			api_source,
			SymbolId::at(3, 0),
			helper.identity.as_ref(),
			"method_call",
			Some((7, 7)),
		),
	];
	let linkage = LinkageSnapshot::with_refs(
		ResourceGeneration::new(2),
		ResourceGeneration::new(1),
		vec![
			LinkageEdge::new(ReferenceId::at(1, 0), helper.id.clone()),
			LinkageEdge::new(ReferenceId::at(2, 0), helper.id.clone()),
			LinkageEdge::new(ReferenceId::at(3, 0), helper.id.clone()),
		],
		Vec::new(),
	);
	let helper_identity = helper.identity.clone();
	let text = render_usages_lmnav(
		"code+moniker://",
		UsageQuery {
			uri: &helper_identity,
			direction: UsageDirection::Incoming,
			scope: &ScopeFilter::from_arguments(&json!({"lang": "java"})).unwrap(),
			paging: Paging {
				cursor: 0,
				generation: None,
				limit: 10,
			},
		},
		UsageIndexView {
			sources: &sources,
			symbols: &[helper, app, batch, api],
			references: &references,
			linkage: &linkage,
		},
	)
	.expect("usage render");
	assert!(text.contains("incoming_summary:"), "{text}");
	assert!(text.contains("refs: 3"), "{text}");
	assert!(text.contains("files: 3"), "{text}");
	assert!(
		text.contains("shared_helper_signal: shared_helper_candidate"),
		"{text}"
	);
	assert!(text.contains("src/app/App.java:L4"), "{text}");
	assert!(
		text.contains(&format!("code_moniker_read uri=\"{helper_identity}\"")),
		"{text}"
	);
}

#[test]
fn usages_roll_up_indirect_type_alias_consumers() {
	let shared_source = SourceId::at(0);
	let app_source = SourceId::at(1);
	let sources = vec![
		source_file(shared_source.clone(), "packages/shared/src/ws.ts", "ts"),
		source_file(app_source.clone(), "packages/client/src/store/ws.ts", "ts"),
	];
	let member = symbol_record(
		SymbolId::at(0, 21),
		shared_source.clone(),
		"code+moniker://./lang:ts/dir:packages/dir:shared/dir:src/module:ws/interface:WsStateMessage",
		"WsStateMessage",
		"interface",
		Some((27, 36)),
	);
	let union = symbol_record(
		SymbolId::at(0, 22),
		shared_source.clone(),
		"code+moniker://./lang:ts/dir:packages/dir:shared/dir:src/module:ws/type:WsServerMessage",
		"WsServerMessage",
		"type",
		Some((97, 108)),
	);
	let handler = symbol_record(
		SymbolId::at(0, 23),
		app_source.clone(),
		"code+moniker://./lang:ts/dir:packages/dir:client/dir:src/module:ws/function:connect()",
		"connect()",
		"function",
		Some((280, 320)),
	);
	let caller = symbol_record(
		SymbolId::at(0, 24),
		app_source.clone(),
		"code+moniker://./lang:ts/dir:packages/dir:client/dir:src/module:ws/function:start()",
		"start()",
		"function",
		Some((340, 360)),
	);
	let references = vec![
		ReferenceRecord::new(
			ReferenceId::at(0, 0),
			shared_source,
			union.id.clone(),
			member.identity.as_ref(),
			"uses_type",
			Some((98, 98)),
		),
		ReferenceRecord::new(
			ReferenceId::at(0, 1),
			app_source.clone(),
			handler.id.clone(),
			union.identity.as_ref(),
			"uses_type",
			Some((287, 287)),
		),
		ReferenceRecord::new(
			ReferenceId::at(0, 2),
			app_source,
			caller.id.clone(),
			handler.identity.as_ref(),
			"calls",
			Some((345, 345)),
		),
	];
	let linkage = LinkageSnapshot::with_refs(
		ResourceGeneration::new(2),
		ResourceGeneration::new(1),
		vec![
			LinkageEdge::new(ReferenceId::at(0, 0), member.id.clone()),
			LinkageEdge::new(ReferenceId::at(0, 1), union.id.clone()),
			LinkageEdge::new(ReferenceId::at(0, 2), handler.id.clone()),
		],
		Vec::new(),
	);
	let member_identity = member.identity.clone();
	let text = render_usages_lmnav(
		"code+moniker://",
		UsageQuery {
			uri: &member_identity,
			direction: UsageDirection::Incoming,
			scope: &ScopeFilter::from_arguments(&json!({"lang": "ts"})).unwrap(),
			paging: Paging {
				cursor: 0,
				generation: None,
				limit: 20,
			},
		},
		UsageIndexView {
			sources: &sources,
			symbols: &[member, union, handler, caller],
			references: &references,
			linkage: &linkage,
		},
	)
	.expect("usage render");
	assert!(text.contains("refs: 2"), "{text}");
	assert!(text.contains("packages/shared/src/ws.ts:L98"), "{text}");
	assert!(
		text.contains("packages/client/src/store/ws.ts:L287"),
		"{text}"
	);
	assert!(text.contains("via=WsServerMessage"), "{text}");
	assert!(!text.contains("ref:caller"), "{text}");
	assert!(!text.contains("start()"), "{text}");
}

#[test]
fn read_symbol_source_renders_source_slice() {
	let source_id = SourceId::at(1);
	let source = SourceFileRecord {
		id: source_id.clone(),
		uri: "code+moniker://./file:src/App.java".to_string(),
		source_root: 0,
		path: "src/App.java".to_string(),
		rel_path: "src/App.java".to_string(),
		anchor: "src/App.java".to_string(),
		language: "java".to_string(),
		text: String::new(),
	};
	let symbol = SymbolRecord {
		id: SymbolId::at(0, 1),
		source: source_id,
		identity: std::sync::Arc::from(
			"code+moniker://./lang:java/package:src/class:App/method:run()",
		),
		name: "run".to_string(),
		kind: "method".to_string(),
		visibility: "public".to_string(),
		signature: String::new(),
		call_name: None,
		call_arity: None,
		navigable: true,
		line_range: Some((3, 5)),
		parent: None,
	};
	let text = render_symbol_source_lmnav(
		"code+moniker://",
		&symbol,
		&source,
		"class App {\n  void before() {}\n  void run() {\n    work();\n  }\n}\n",
		1,
	);
	assert!(
		text.contains("uri: code+moniker://./lang:java/package:src/class:App/method:run()"),
		"{text}"
	);
	assert!(text.contains("file: src/App.java"));
	assert!(text.contains("slice: 2-6"));
	assert!(text.contains("   3 |   void run() {"));
	assert!(text.contains("code_moniker_symbols"));
}

#[test]
fn symbols_insights_summarize_index() {
	let source_id = SourceId::at(1);
	let sources = vec![SourceFileRecord {
		id: source_id.clone(),
		uri: "code+moniker://./file:src/App.java".to_string(),
		source_root: 0,
		path: "src/App.java".to_string(),
		rel_path: "src/App.java".to_string(),
		anchor: "src/App.java".to_string(),
		language: "java".to_string(),
		text: String::new(),
	}];
	let class = SymbolRecord {
		id: SymbolId::at(0, 10),
		source: source_id.clone(),
		identity: std::sync::Arc::from("code+moniker://./lang:java/package:src/class:App"),
		name: "App".to_string(),
		kind: "class".to_string(),
		visibility: "public".to_string(),
		signature: String::new(),
		call_name: None,
		call_arity: None,
		navigable: true,
		line_range: Some((1, 6)),
		parent: None,
	};
	let method = SymbolRecord {
		id: SymbolId::at(0, 11),
		source: source_id.clone(),
		identity: std::sync::Arc::from(
			"code+moniker://./lang:java/package:src/class:App/method:run()",
		),
		name: "run".to_string(),
		kind: "method".to_string(),
		visibility: "public".to_string(),
		signature: String::new(),
		call_name: None,
		call_arity: None,
		navigable: true,
		line_range: Some((3, 5)),
		parent: Some(SymbolId::at(0, 10)),
	};
	let references = vec![ReferenceRecord::new(
		ReferenceId::at(0, 0),
		source_id,
		SymbolId::at(0, 11),
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
			generation: None,
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
	assert!(response.contains("uri: code+moniker://workspace"));
	assert!(response.contains("next:"), "{response}");
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
					"max_nodes": 200
				}
			}
		}),
	);
	assert!(response.contains("HTTP/1.1 200 OK"), "{response}");
	assert!(response.contains("uri: syntax.parse"), "{response}");
	assert!(response.contains("file: snippet.plpgsql"), "{response}");
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
