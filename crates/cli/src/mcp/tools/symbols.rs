use std::collections::BTreeMap;

use code_moniker_workspace::facade::{
	LocalWorkspaceOptions, WorkspaceFacade, local_workspace_ports,
};
use code_moniker_workspace::snapshot::{
	SourceFileRecord, SymbolRecord, WorkspaceRequest, WorkspaceTransition,
};
use code_moniker_workspace::source::LocalResourceCache;
use serde_json::{Value, json};

use super::scope::{Paging, SymbolScopeFilter};
use super::{McpTool, ToolDescriptor, ToolError, ToolResult};
use crate::mcp::context::McpContext;

const DEFAULT_SYMBOL_URI: &str = "workspace";

pub(super) struct SymbolsTool;

impl SymbolsTool {
	pub(super) const NAME: &'static str = "code_moniker_symbols";

	const DESCRIPTION: &'static str = concat!(
		"When to use: list symbols after code_moniker_read has identified the relevant workspace, language, or subtree. ",
		"Use this instead of broad text search when you need named code structure.\n",
		"\n",
		"Query the code-moniker symbol index.\n",
		"  workspace                — list navigable symbols in the workspace\n",
		"  code+moniker://workspace — same root with an explicit URI\n",
		"Filters are AND-combined: path/lang limit the files, kind/shape/name limit symbols. ",
		"Use limit and cursor for paging; the next section returns the follow-up call."
	);

	fn input_schema() -> Value {
		json!({
			"type": "object",
			"properties": {
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
					"description": "Rust regex matched against symbol name."
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
			"required": ["uri"],
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

	fn call(&self, context: &McpContext, arguments: &Value) -> Result<ToolResult, ToolError> {
		let uri = arguments
			.get("uri")
			.and_then(Value::as_str)
			.unwrap_or(DEFAULT_SYMBOL_URI);
		let scope = SymbolScopeFilter::from_arguments(arguments).map_err(ToolError::failed)?;
		let paging = Paging::from_arguments(arguments).map_err(ToolError::failed)?;
		let text = read_symbols(context, uri, &scope, paging).map_err(ToolError::failed)?;
		Ok(ToolResult {
			text,
			is_error: false,
		})
	}
}

fn read_symbols(
	context: &McpContext,
	uri: &str,
	scope: &SymbolScopeFilter,
	paging: Paging,
) -> anyhow::Result<String> {
	if !is_workspace_uri(uri, context.scheme()) {
		anyhow::bail!(
			"unsupported URI; use workspace or {}workspace",
			context.scheme()
		);
	}
	let opts = context.opts();
	let mut workspace = WorkspaceFacade::new(local_workspace_ports(
		LocalWorkspaceOptions::new(opts.paths.clone(), opts.project.clone())
			.with_cache_dir(opts.cache_dir.clone()),
		LocalResourceCache::default(),
	));
	match workspace.load_index(WorkspaceRequest::new("mcp-symbols")) {
		WorkspaceTransition::Ready { .. } => {
			let Some(snapshot) = workspace.snapshot() else {
				anyhow::bail!("workspace index snapshot is unavailable");
			};
			Ok(render_symbols_lmnav(
				context.scheme(),
				uri,
				scope,
				paging,
				&snapshot.index.sources,
				&snapshot.index.symbols,
			))
		}
		WorkspaceTransition::Failed { failure, .. } => anyhow::bail!(failure.message),
	}
}

fn is_workspace_uri(uri: &str, scheme: &str) -> bool {
	let value = uri.trim();
	value.is_empty()
		|| value == DEFAULT_SYMBOL_URI
		|| value == format!("{scheme}workspace")
		|| value == format!("{scheme}.")
		|| value == scheme.trim_end_matches('/')
}

pub(in crate::mcp) fn render_symbols_lmnav(
	scheme: &str,
	request_uri: &str,
	scope: &SymbolScopeFilter,
	paging: Paging,
	sources: &[SourceFileRecord],
	symbols: &[SymbolRecord],
) -> String {
	let source_by_id = sources
		.iter()
		.map(|source| (source.id.as_str(), source))
		.collect::<BTreeMap<_, _>>();
	let mut rows = symbols
		.iter()
		.filter_map(|symbol| {
			let source = source_by_id.get(symbol.source.as_str())?;
			scope
				.files
				.matches_file(&source.rel_path, Some(&source.language))
				.then_some((symbol, *source))
		})
		.filter(|(symbol, _)| scope.matches_symbol(&symbol.name, &symbol.kind, symbol.navigable))
		.collect::<Vec<_>>();
	rows.sort_by(|a, b| {
		a.1.rel_path
			.cmp(&b.1.rel_path)
			.then_with(|| a.0.line_range.cmp(&b.0.line_range))
			.then_with(|| a.0.identity.cmp(&b.0.identity))
	});
	let (start, end, next) = paging.window(&rows);
	let uri = normalize_workspace_uri(scheme, request_uri);
	let mut output = String::new();
	output.push_str(&format!("uri: {uri}\n"));
	if let Some(next) = next {
		output.push_str(&format!(
			"completeness: partial (symbols {start}-{end} of {}, next cursor {next})\n",
			rows.len()
		));
	} else {
		output.push_str("completeness: full\n");
	}
	output.push_str(&format!("symbols: {}\n", rows.len()));
	output.push_str(&format!("limit: {}\n", paging.limit));
	output.push('\n');
	output.push_str("scope:\n");
	for line in scope.describe() {
		output.push_str(&line);
		output.push('\n');
	}
	output.push('\n');
	output.push_str("results:\n");
	if rows.is_empty() {
		output.push_str("  <empty>\n");
	} else {
		for (symbol, source) in rows.iter().take(end).skip(start) {
			output.push_str(&format!(
				"  - {} {} {}{}\n",
				symbol.kind,
				symbol.name,
				source.rel_path,
				line_suffix(symbol)
			));
			output.push_str(&format!("    uri: {}\n", symbol.identity));
		}
	}
	output.push_str("\nnext:\n");
	if let Some(next) = next {
		output.push_str(&format!(
			"  - code_moniker_symbols uri=\"{scheme}workspace\" limit={} cursor={next}\n",
			paging.limit
		));
	}
	output.push_str(&format!(
		"  - code_moniker_read uri=\"{scheme}workspace\" depth=2\n"
	));
	output
}

fn line_suffix(symbol: &SymbolRecord) -> String {
	symbol
		.line_range
		.map(|(start, end)| format!(":{start}-{end}"))
		.unwrap_or_default()
}

fn normalize_workspace_uri(scheme: &str, request_uri: &str) -> String {
	let trimmed = request_uri.trim();
	if trimmed.is_empty() || trimmed == DEFAULT_SYMBOL_URI {
		format!("{scheme}workspace")
	} else {
		trimmed.to_string()
	}
}
