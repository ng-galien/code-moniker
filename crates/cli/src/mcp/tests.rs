use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use code_moniker_daemon::WorkspaceDaemon;
use code_moniker_query::{
	Command, CommandRequest, Query, QueryRequest, QueryResult, SymbolSearchQuery,
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
use super::tools::symbols::{SymbolAction, SymbolIndexView, render_symbols_lmnav};
use super::tools::usages::{UsageDirection, UsageIndexView, UsageQuery, render_usages_lmnav};
use super::tools::{McpTool, ToolRegistry};
use crate::session::SessionOptions;

fn empty_context(paths: Vec<PathBuf>) -> McpContext {
	daemon_context(paths)
}

fn loaded_context(paths: Vec<PathBuf>) -> McpContext {
	daemon_context(paths)
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
	assert_eq!(descriptor.input_schema["required"][0], "uri");
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
		text.contains("code_moniker_symbols uri=\"code+moniker://workspace\" path=\"root/src/**\" lang=\"java\" limit=50")
	);
}

#[test]
fn tools_list_returns_mcp_shape() {
	let tools = ToolRegistry::new().descriptors();
	assert_eq!(tools[0]["name"], "code_moniker_read");
	assert_eq!(tools[1]["name"], "code_moniker_notes");
	assert_eq!(tools[2]["name"], "code_moniker_search");
	assert_eq!(tools[3]["name"], "code_moniker_symbols");
	assert_eq!(tools[4]["name"], "code_moniker_usages");
	assert_eq!(tools[5]["name"], "code_moniker_rules");
	assert_eq!(tools[6]["name"], "code_moniker_diff");
	assert_eq!(tools[7]["name"], "code_moniker_graph");
	assert_eq!(tools[8]["name"], "code_moniker_refresh");
	assert!(
		tools[0]["description"]
			.as_str()
			.unwrap()
			.starts_with("When to use:")
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
fn notes_tool_manages_symbol_notes_with_controlled_transitions() {
	let temp = tempfile::tempdir().expect("tempdir");
	write_java_app_fixture(temp.path(), "class App {\n  void run() {}\n}\n");
	let registry = ToolRegistry::new();
	let context = loaded_context(vec![temp.path().to_path_buf()]);
	let moniker = app_symbol_moniker(&context);

	let created = registry
		.call(
			&context,
			"code_moniker_notes",
			&json!({
				"action": "create",
				"id": "note_acceptance",
				"moniker": moniker,
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
	assert!(
		temp.path().join(".code-moniker/notes.toml").is_file(),
		"notes file should be persisted"
	);

	let list = registry
		.call(
			&context,
			"code_moniker_notes",
			&json!({"action": "list", "moniker": moniker}),
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

#[test]
fn registry_dispatches_read_tool() {
	let registry = ToolRegistry::new();
	let context = empty_context(vec![PathBuf::from(".")]);
	let result = registry.call(&context, "not_a_tool", &json!({}));
	assert!(result.unwrap_err().is_unknown_tool());
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
				"limit": 1
			}),
		)
		.expect("search with code");
	assert!(detail.text.contains("code:"), "{}", detail.text);
	assert!(detail.text.contains("   2 |   void run() {"));
	assert!(detail.text.contains("include_code=true"));
	assert!(detail.text.contains("context_lines=0"));
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
	assert!(result.text.contains("moved fn assist()"), "{}", result.text);
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

#[test]
fn symbols_tool_filters_and_pages_symbols() {
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
	assert!(text.contains("usages: code_moniker_usages"), "{text}");
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
		text.contains(
			"code_moniker_read uri=\"code+moniker://./lang:java/package:shared/class:Helper\""
		),
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
	assert!(text.contains("via: WsServerMessage"), "{text}");
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
	assert!(text.contains("uri: code+moniker://./lang:java/package:src/class:App/method:run()"));
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
	std::fs::write(temp.path().join("src/main/java/App.java"), "class App {}\n")
		.expect("write fixture");
	let opts = SessionOptions {
		paths: vec![temp.path().to_path_buf()],
		project: None,
		cache_dir: None,
	};
	let server = start_http_test_server(opts.clone());
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
	let mut stream = TcpStream::connect(server.addr).expect("connect");
	write!(
		stream,
		"POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
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
