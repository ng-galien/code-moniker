use std::collections::BTreeMap;

use code_moniker_query::{
	Query, QueryRequest, QueryResult, SymbolDto, SymbolInsightsResult, SymbolListResult,
	SymbolSearchQuery,
};
use code_moniker_workspace::snapshot::{ReferenceRecord, SourceFileRecord, SourceId, SymbolRecord};
use serde_json::{Value, json};

use super::common::{
	is_workspace_uri, line_range_suffix, normalize_workspace_uri, sorted_count_rows,
	symbol_line_suffix,
};
use super::scope::{
	Paging, SymbolMatch, SymbolScopeFilter, append_call_cursor_arg, append_call_number_arg,
	append_call_string_arg,
};
use super::{McpTool, ToolDescriptor, ToolError, ToolResult};
use crate::mcp::context::McpContext;

const DEFAULT_SYMBOL_URI: &str = "workspace";

pub(super) struct SymbolsTool;

impl SymbolsTool {
	pub(super) const NAME: &'static str = "code_moniker_symbols";

	const DESCRIPTION: &'static str = concat!(
		"When to use: list symbols after code_moniker_read has identified the relevant workspace, language, or subtree. ",
		"Use this instead of broad text search when you need named code structure or symbolic health signals.\n",
		"\n",
		"Query the code-moniker symbol index.\n",
		"  action=list     — list navigable symbols in the workspace\n",
		"  action=insights — summarize languages, kinds, shapes, refs, and concentrated files\n",
		"Filters are AND-combined: path/lang limit the files, kind/shape/name limit symbols. ",
		"Use limit and cursor for paging; the next section returns the follow-up call."
	);

	fn input_schema() -> Value {
		json!({
			"type": "object",
			"properties": {
				"action": {
					"type": "string",
					"enum": ["list", "insights"],
					"description": "list symbols, or insights for symbolic metrics."
				},
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
		let request = SymbolRequest::from_arguments(arguments).map_err(ToolError::failed)?;
		let text = read_symbols(context, &request).map_err(ToolError::failed)?;
		Ok(ToolResult {
			text,
			is_error: false,
		})
	}
}

struct SymbolRequest {
	action: SymbolAction,
	uri: String,
	scope: SymbolScopeFilter,
	paging: Paging,
}

impl SymbolRequest {
	fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		Ok(Self {
			action: SymbolAction::from_arguments(arguments)?,
			uri: arguments
				.get("uri")
				.and_then(Value::as_str)
				.unwrap_or(DEFAULT_SYMBOL_URI)
				.to_string(),
			scope: SymbolScopeFilter::from_arguments(arguments)?,
			paging: Paging::from_arguments(arguments)?,
		})
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mcp) enum SymbolAction {
	List,
	Insights,
}

impl SymbolAction {
	fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		match arguments
			.get("action")
			.and_then(Value::as_str)
			.unwrap_or("list")
		{
			"list" => Ok(Self::List),
			"insights" => Ok(Self::Insights),
			action => anyhow::bail!("unknown symbol action `{action}`"),
		}
	}
}

fn read_symbols(context: &McpContext, request: &SymbolRequest) -> anyhow::Result<String> {
	let uri = request.uri.as_str();
	if !is_workspace_uri(uri, context.scheme(), DEFAULT_SYMBOL_URI) {
		anyhow::bail!(
			"unsupported URI; use workspace or {}workspace",
			context.scheme()
		);
	}
	match request.action {
		SymbolAction::List => {
			let response = context.query(QueryRequest {
				query: Query::SymbolSearch(symbol_query(&request.scope)),
				consistency: code_moniker_query::Consistency::Current,
				page: request.paging.daemon_page(),
			})?;
			let QueryResult::SymbolList(result) = response.result else {
				anyhow::bail!("unexpected daemon response for symbols list");
			};
			Ok(render_daemon_symbol_list_lmnav(
				context.scheme(),
				uri,
				&request.scope,
				request.paging,
				response.next_cursor.as_ref(),
				&result,
			))
		}
		SymbolAction::Insights => {
			let response = context.query(QueryRequest::new(Query::SymbolInsights(
				symbol_query(&request.scope),
			)))?;
			let QueryResult::SymbolInsights(result) = response.result else {
				anyhow::bail!("unexpected daemon response for symbols insights");
			};
			Ok(render_daemon_symbol_insights_lmnav(
				context.scheme(),
				uri,
				&request.scope,
				request.paging,
				&result,
			))
		}
	}
}

fn render_daemon_symbol_list_lmnav(
	scheme: &str,
	request_uri: &str,
	scope: &SymbolScopeFilter,
	paging: Paging,
	next_cursor: Option<&code_moniker_query::QueryCursor>,
	result: &SymbolListResult,
) -> String {
	let uri = normalize_workspace_uri(scheme, request_uri, DEFAULT_SYMBOL_URI);
	let start = paging.cursor.min(result.total);
	let end = start.saturating_add(result.rows.len()).min(result.total);
	let mut output = String::new();
	output.push_str(&format!("uri: {uri}\n"));
	if let Some(next) = next_cursor {
		output.push_str(&format!(
			"completeness: partial (symbols {start}-{end} of {}, next cursor {})\n",
			result.total, next.offset
		));
	} else {
		output.push_str("completeness: full\n");
	}
	output.push_str(&format!("symbols: {}\n", result.total));
	output.push_str(&format!("limit: {}\n\n", paging.limit));
	output.push_str("scope:\n");
	for line in scope.describe() {
		output.push_str(&line);
		output.push('\n');
	}
	output.push('\n');
	output.push_str("results:\n");
	if result.rows.is_empty() {
		output.push_str("  <empty>\n");
	} else {
		for symbol in &result.rows {
			render_daemon_symbol_row(&mut output, symbol);
		}
	}
	output.push_str("\nnext:\n");
	if let Some(next) = next_cursor {
		output.push_str(&format!(
			"  - code_moniker_symbols uri=\"{scheme}workspace\""
		));
		append_call_string_arg(&mut output, "action", "list");
		scope.append_call_args(&mut output);
		append_call_number_arg(&mut output, "limit", paging.limit);
		append_call_cursor_arg(&mut output, "cursor", next);
		output.push('\n');
	}
	append_symbols_next_call(&mut output, scheme, scope, SymbolAction::Insights, 20, None);
	append_workspace_read_call(&mut output, scheme, scope, 2);
	output
}

fn render_daemon_symbol_row(output: &mut String, symbol: &SymbolDto) {
	output.push_str(&format!(
		"  - {} {} {}{}\n",
		symbol.kind,
		symbol.name,
		symbol.file,
		line_range_suffix(symbol.line_range)
	));
	output.push_str(&format!("    uri: {}\n", symbol.uri));
	output.push_str("    usages: code_moniker_usages");
	append_call_string_arg(output, "uri", &symbol.uri);
	append_call_number_arg(output, "limit", 50);
	output.push('\n');
}

fn render_daemon_symbol_insights_lmnav(
	scheme: &str,
	request_uri: &str,
	scope: &SymbolScopeFilter,
	paging: Paging,
	result: &SymbolInsightsResult,
) -> String {
	let uri = normalize_workspace_uri(scheme, request_uri, DEFAULT_SYMBOL_URI);
	let mut output = String::new();
	output.push_str(&format!("uri: {uri}\n"));
	output.push_str("completeness: full\n");
	output.push_str(&format!("files: {}\n", result.files));
	output.push_str(&format!("symbols: {}\n", result.symbols));
	output.push_str(&format!("refs: {}\n", result.references));
	output.push_str(&format!("limit: {}\n\n", paging.limit));
	output.push_str("scope:\n");
	for line in scope.describe() {
		output.push_str(&line);
		output.push('\n');
	}
	output.push('\n');
	output.push_str("insights:\n");
	output.push_str(&format!(
		"  navigable_symbols: {}\n",
		result.navigable_symbols
	));
	output.push_str(&format!(
		"  non_navigable_symbols: {}\n",
		result.non_navigable_symbols
	));
	render_daemon_counts(&mut output, "languages", &result.languages, paging.limit);
	render_daemon_counts(&mut output, "kinds", &result.kinds, paging.limit);
	render_daemon_counts(&mut output, "shapes", &result.shapes, paging.limit);
	render_daemon_counts(
		&mut output,
		"top_files_by_symbols",
		&result.top_files_by_symbols,
		paging.limit,
	);
	render_daemon_counts(
		&mut output,
		"top_files_by_refs",
		&result.top_files_by_refs,
		paging.limit,
	);
	output.push_str("\nnext:\n");
	append_symbols_next_call(&mut output, scheme, scope, SymbolAction::List, 50, None);
	append_workspace_read_call(&mut output, scheme, scope, 3);
	output
}

fn render_daemon_counts(
	output: &mut String,
	label: &str,
	rows: &[code_moniker_query::CountDto],
	limit: usize,
) {
	output.push_str(&format!("  {label}:\n"));
	for row in rows.iter().take(limit) {
		output.push_str(&format!("    {}: {}\n", row.name, row.count));
	}
}

fn symbol_query(scope: &SymbolScopeFilter) -> SymbolSearchQuery {
	SymbolSearchQuery {
		workspace: None,
		text: None,
		path: scope.files.paths.clone(),
		lang: scope.files.langs.clone(),
		kind: scope.kinds.clone(),
		shape: scope
			.shapes
			.iter()
			.map(|shape| shape.as_str().to_string())
			.collect(),
		name: scope.name.as_ref().map(|regex| regex.as_str().to_string()),
		include_non_navigable: scope.include_non_navigable,
		include_code: false,
		context_lines: 0,
		projection: Vec::new(),
	}
}

pub(in crate::mcp) struct SymbolIndexView<'a> {
	pub(in crate::mcp) sources: &'a [SourceFileRecord],
	pub(in crate::mcp) symbols: &'a [SymbolRecord],
	pub(in crate::mcp) references: &'a [ReferenceRecord],
}

pub(in crate::mcp) fn render_symbols_lmnav(
	scheme: &str,
	request_uri: &str,
	scope: &SymbolScopeFilter,
	paging: Paging,
	index: SymbolIndexView<'_>,
	action: SymbolAction,
) -> String {
	match action {
		SymbolAction::List => render_symbol_list_lmnav(scheme, request_uri, scope, paging, index),
		SymbolAction::Insights => {
			render_symbol_insights_lmnav(scheme, request_uri, scope, paging, index)
		}
	}
}

fn render_symbol_list_lmnav(
	scheme: &str,
	request_uri: &str,
	scope: &SymbolScopeFilter,
	paging: Paging,
	index: SymbolIndexView<'_>,
) -> String {
	let source_by_id = index
		.sources
		.iter()
		.map(|source| (source.id, source))
		.collect::<BTreeMap<_, _>>();
	let mut rows = index
		.symbols
		.iter()
		.filter_map(|symbol| {
			let source = source_by_id.get(&symbol.source)?;
			scope
				.files
				.matches_file(&source.rel_path, Some(&source.language))
				.then_some((symbol, *source))
		})
		.filter(|(symbol, _)| {
			scope.matches_symbol(SymbolMatch {
				name: &symbol.name,
				kind: &symbol.kind,
				navigable: symbol.navigable,
			})
		})
		.collect::<Vec<_>>();
	rows.sort_by(|a, b| {
		a.1.rel_path
			.cmp(&b.1.rel_path)
			.then_with(|| a.0.line_range.cmp(&b.0.line_range))
			.then_with(|| a.0.identity.cmp(&b.0.identity))
	});
	let (start, end, next) = paging.window(&rows);
	let uri = normalize_workspace_uri(scheme, request_uri, DEFAULT_SYMBOL_URI);
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
				symbol_line_suffix(symbol)
			));
			output.push_str(&format!("    uri: {}\n", symbol.identity));
			output.push_str("    usages: code_moniker_usages");
			append_call_string_arg(&mut output, "uri", &symbol.identity);
			append_call_number_arg(&mut output, "limit", 50);
			output.push('\n');
		}
	}
	output.push_str("\nnext:\n");
	if let Some(next) = next {
		append_symbols_next_call(
			&mut output,
			scheme,
			scope,
			SymbolAction::List,
			paging.limit,
			Some(next),
		);
	}
	append_symbols_next_call(&mut output, scheme, scope, SymbolAction::Insights, 20, None);
	append_workspace_read_call(&mut output, scheme, scope, 2);
	output
}

fn render_symbol_insights_lmnav(
	scheme: &str,
	request_uri: &str,
	scope: &SymbolScopeFilter,
	paging: Paging,
	index: SymbolIndexView<'_>,
) -> String {
	let scoped_sources = index
		.sources
		.iter()
		.filter(|source| {
			scope
				.files
				.matches_file(&source.rel_path, Some(&source.language))
		})
		.collect::<Vec<_>>();
	let scoped_source_ids = scoped_sources
		.iter()
		.map(|source| source.id.clone())
		.collect::<std::collections::BTreeSet<_>>();
	let scoped_symbols = index
		.symbols
		.iter()
		.filter(|symbol| scoped_source_ids.contains(&symbol.source))
		.filter(|symbol| {
			scope.matches_symbol(SymbolMatch {
				name: &symbol.name,
				kind: &symbol.kind,
				navigable: symbol.navigable,
			})
		})
		.collect::<Vec<_>>();
	let scoped_references = index
		.references
		.iter()
		.filter(|reference| scoped_source_ids.contains(&reference.source))
		.collect::<Vec<_>>();
	let metrics = collect_symbol_insights(&scoped_sources, &scoped_symbols, &scoped_references);
	let uri = normalize_workspace_uri(scheme, request_uri, DEFAULT_SYMBOL_URI);
	let mut output = String::new();
	output.push_str(&format!("uri: {uri}\n"));
	output.push_str("completeness: full\n");
	output.push_str(&format!("files: {}\n", scoped_sources.len()));
	output.push_str(&format!("symbols: {}\n", scoped_symbols.len()));
	output.push_str(&format!("refs: {}\n", scoped_references.len()));
	output.push_str(&format!("limit: {}\n\n", paging.limit));
	output.push_str("scope:\n");
	for line in scope.describe() {
		output.push_str(&line);
		output.push('\n');
	}
	output.push('\n');
	metrics.render(&mut output, paging.limit);
	output.push_str("next:\n");
	append_symbols_next_call(&mut output, scheme, scope, SymbolAction::List, 50, None);
	append_workspace_read_call(&mut output, scheme, scope, 3);
	output
}

fn append_symbols_next_call(
	output: &mut String,
	scheme: &str,
	scope: &SymbolScopeFilter,
	action: SymbolAction,
	limit: usize,
	cursor: Option<usize>,
) {
	output.push_str(&format!(
		"  - code_moniker_symbols uri=\"{scheme}workspace\""
	));
	append_call_string_arg(
		output,
		"action",
		match action {
			SymbolAction::List => "list",
			SymbolAction::Insights => "insights",
		},
	);
	scope.append_call_args(output);
	append_call_number_arg(output, "limit", limit);
	if let Some(cursor) = cursor {
		append_call_number_arg(output, "cursor", cursor);
	}
	output.push('\n');
}

fn append_workspace_read_call(
	output: &mut String,
	scheme: &str,
	scope: &SymbolScopeFilter,
	depth: usize,
) {
	output.push_str(&format!("  - code_moniker_read uri=\"{scheme}workspace\""));
	scope.files.append_call_args(output);
	append_call_number_arg(output, "depth", depth);
	output.push('\n');
}

#[derive(Default)]
struct SymbolInsights {
	languages: BTreeMap<String, usize>,
	kinds: BTreeMap<String, usize>,
	shapes: BTreeMap<&'static str, usize>,
	symbols_by_file: BTreeMap<SourceId, usize>,
	refs_by_file: BTreeMap<SourceId, usize>,
	files_by_id: BTreeMap<SourceId, String>,
	navigable_symbols: usize,
	non_navigable_symbols: usize,
}

impl SymbolInsights {
	fn add_source(&mut self, source: &SourceFileRecord) {
		*self.languages.entry(source.language.clone()).or_default() += 1;
		self.files_by_id
			.insert(source.id.clone(), source.rel_path.clone());
	}

	fn add_symbol(&mut self, symbol: &SymbolRecord) {
		*self.kinds.entry(symbol.kind.clone()).or_default() += 1;
		*self
			.shapes
			.entry(code_moniker_core::core::shape::Shape::for_kind(symbol.kind.as_bytes()).as_str())
			.or_default() += 1;
		*self
			.symbols_by_file
			.entry(symbol.source.clone())
			.or_default() += 1;
		if symbol.navigable {
			self.navigable_symbols += 1;
		} else {
			self.non_navigable_symbols += 1;
		}
	}

	fn add_reference(&mut self, reference: &ReferenceRecord) {
		*self
			.refs_by_file
			.entry(reference.source.clone())
			.or_default() += 1;
	}

	fn render(&self, output: &mut String, limit: usize) {
		output.push_str("insights:\n");
		output.push_str(&format!(
			"  navigable_symbols: {}\n",
			self.navigable_symbols
		));
		output.push_str(&format!(
			"  non_navigable_symbols: {}\n",
			self.non_navigable_symbols
		));
		render_counts(
			output,
			"languages",
			&sorted_count_rows(&self.languages),
			limit,
		);
		render_counts(output, "kinds", &sorted_count_rows(&self.kinds), limit);
		render_counts(output, "shapes", &sorted_count_rows(&self.shapes), limit);
		render_source_counts(
			output,
			"top_files_by_symbols",
			&self.files_by_id,
			&self.symbols_by_file,
			limit,
		);
		render_source_counts(
			output,
			"top_files_by_refs",
			&self.files_by_id,
			&self.refs_by_file,
			limit,
		);
		output.push('\n');
	}
}

fn collect_symbol_insights(
	sources: &[&SourceFileRecord],
	symbols: &[&SymbolRecord],
	references: &[&ReferenceRecord],
) -> SymbolInsights {
	let mut insights = SymbolInsights::default();
	for source in sources {
		insights.add_source(source);
	}
	for symbol in symbols {
		insights.add_symbol(symbol);
	}
	for reference in references {
		insights.add_reference(reference);
	}
	insights
}

fn render_counts(output: &mut String, label: &str, counts: &[(String, usize)], limit: usize) {
	output.push_str(&format!("  {label}:\n"));
	if counts.is_empty() {
		output.push_str("    <empty>\n");
		return;
	}
	for (name, count) in counts.iter().take(limit) {
		output.push_str(&format!("    {name}: {count}\n"));
	}
}

fn render_source_counts(
	output: &mut String,
	label: &str,
	files_by_id: &BTreeMap<SourceId, String>,
	counts_by_file: &BTreeMap<SourceId, usize>,
	limit: usize,
) {
	output.push_str(&format!("  {label}:\n"));
	let counts = sorted_source_counts(files_by_id, counts_by_file);
	if counts.is_empty() {
		output.push_str("    <empty>\n");
		return;
	}
	for (path, count) in counts.iter().take(limit) {
		output.push_str(&format!("    {path}: {count}\n"));
	}
}

fn sorted_source_counts(
	files_by_id: &BTreeMap<SourceId, String>,
	counts_by_file: &BTreeMap<SourceId, usize>,
) -> Vec<(String, usize)> {
	let mut rows = counts_by_file
		.iter()
		.filter_map(|(source_id, count)| {
			files_by_id
				.get(source_id)
				.map(|path| (path.clone(), *count))
		})
		.collect::<Vec<_>>();
	rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
	rows
}
