use std::collections::BTreeMap;

use code_moniker_core::core::shape::Shape;
use code_moniker_query::{
	Query, QueryResult, SymbolDto, SymbolInsightsResult, SymbolListResult, SymbolSearchQuery,
	symbol_is_test_artifact,
};
use code_moniker_workspace::snapshot::{ReferenceRecord, SourceFileRecord, SourceId, SymbolRecord};
use serde_json::{Value, json};

use super::common::{
	compact_argument, is_workspace_uri, line_range_suffix, normalize_workspace_uri,
	sorted_count_rows, symbol_line_suffix,
};
use super::scope::{
	Paging, SymbolMatch, SymbolScopeFilter, append_call_bool_arg, append_call_cursor_arg,
	append_call_number_arg, append_call_string_arg,
};
use super::{McpTool, OutputContract, ToolDescriptor, ToolError, ToolResult};
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
		"Use limit and cursor for paging; compact output uses compact monikers by default."
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
					"description": "Rust regex matched against the indexed symbol name. Callable names may include their parameter signature."
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

	fn output_contract(&self) -> OutputContract {
		OutputContract::Agent
	}

	fn call(&self, context: &McpContext, arguments: &Value) -> Result<ToolResult, ToolError> {
		let request = SymbolRequest::from_arguments(arguments).map_err(ToolError::failed)?;
		read_symbols(context, &request).map_err(ToolError::failed)
	}
}

struct SymbolRequest {
	action: SymbolAction,
	uri: String,
	scope: SymbolScopeFilter,
	paging: Paging,
	compact: bool,
}

impl SymbolRequest {
	fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		let compact = compact_argument(arguments)?;
		Ok(Self {
			action: SymbolAction::from_arguments(arguments)?,
			uri: arguments
				.get("uri")
				.and_then(Value::as_str)
				.unwrap_or(DEFAULT_SYMBOL_URI)
				.to_string(),
			scope: SymbolScopeFilter::from_arguments(arguments)?,
			paging: Paging::from_arguments_for_output(arguments, compact)?,
			compact,
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

fn read_symbols(context: &McpContext, request: &SymbolRequest) -> anyhow::Result<ToolResult> {
	let uri = request.uri.as_str();
	if !is_workspace_uri(uri, context.scheme(), DEFAULT_SYMBOL_URI) {
		anyhow::bail!(
			"unsupported URI; use workspace or {}workspace",
			context.scheme()
		);
	}
	match request.action {
		SymbolAction::List => {
			let response = context.query_refreshed(
				Query::SymbolSearch(symbol_query(&request.scope)),
				request.paging.daemon_page(),
			)?;
			let QueryResult::SymbolList(result) = response.result else {
				anyhow::bail!("unexpected daemon response for symbols list");
			};
			Ok(ToolResult::success(render_daemon_symbol_list_lmnav(
				context.scheme(),
				uri,
				&request.scope,
				(request.paging, request.compact),
				response.next_cursor.as_ref(),
				&result,
			))
			.with_monikers(result.rows.iter().map(|symbol| symbol.uri.as_str())))
		}
		SymbolAction::Insights => {
			let response = context.query_refreshed(
				Query::SymbolInsights(symbol_query(&request.scope)),
				code_moniker_query::Page::default(),
			)?;
			let QueryResult::SymbolInsights(result) = response.result else {
				anyhow::bail!("unexpected daemon response for symbols insights");
			};
			Ok(ToolResult::success(render_daemon_symbol_insights_lmnav(
				context.scheme(),
				uri,
				&request.scope,
				request.paging,
				&result,
				request.compact,
			)))
		}
	}
}

pub(in crate::mcp) fn render_daemon_symbol_list_lmnav(
	scheme: &str,
	request_uri: &str,
	scope: &SymbolScopeFilter,
	render: (Paging, bool),
	next_cursor: Option<&code_moniker_query::QueryCursor>,
	result: &SymbolListResult,
) -> String {
	let (paging, compact) = render;
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
		if result.total == 0 {
			append_signed_callable_name_hint(&mut output, scope);
		}
	} else {
		for symbol in &result.rows {
			render_daemon_symbol_row(&mut output, symbol, compact);
		}
	}
	if next_cursor.is_some() || !compact {
		output.push_str("\nnext:\n");
	}
	if let Some(next) = next_cursor {
		output.push_str(&format!(
			"  - code_moniker_symbols uri=\"{scheme}workspace\""
		));
		append_call_string_arg(&mut output, "action", "list");
		scope.append_call_args(&mut output);
		append_call_number_arg(&mut output, "limit", paging.limit);
		append_call_cursor_arg(&mut output, "cursor", next);
		if !compact {
			append_call_bool_arg(&mut output, "compact", false);
		}
		output.push('\n');
	}
	if !compact {
		append_symbols_next_call(
			&mut output,
			scheme,
			scope,
			SymbolNextCall {
				action: SymbolAction::Insights,
				limit: 20,
				cursor: None,
				compact,
			},
		);
		append_workspace_read_call(&mut output, scheme, scope, 2, compact);
	}
	output
}

fn render_daemon_symbol_row(output: &mut String, symbol: &SymbolDto, compact: bool) {
	output.push_str(&format!(
		"  - {} {} {}{}\n",
		symbol.kind,
		symbol.name,
		symbol.file,
		line_range_suffix(symbol.line_range)
	));
	output.push_str(&format!("    uri: {}\n", symbol.uri));
	if !compact {
		output.push_str("    usages: code_moniker_usages");
		append_call_string_arg(output, "uri", &symbol.uri);
		append_call_number_arg(output, "limit", 50);
		output.push('\n');
	}
}

fn render_daemon_symbol_insights_lmnav(
	scheme: &str,
	request_uri: &str,
	scope: &SymbolScopeFilter,
	paging: Paging,
	result: &SymbolInsightsResult,
	compact: bool,
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
	append_symbols_next_call(
		&mut output,
		scheme,
		scope,
		SymbolNextCall {
			action: SymbolAction::List,
			limit: if compact { 20 } else { 50 },
			cursor: None,
			compact,
		},
	);
	if !compact {
		append_workspace_read_call(&mut output, scheme, scope, 3, compact);
	}
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
	render_symbols_lmnav_mode(scheme, request_uri, scope, paging, index, (action, true))
}

pub(in crate::mcp) fn render_symbols_lmnav_mode(
	scheme: &str,
	request_uri: &str,
	scope: &SymbolScopeFilter,
	paging: Paging,
	index: SymbolIndexView<'_>,
	mode: (SymbolAction, bool),
) -> String {
	let (action, compact) = mode;
	match action {
		SymbolAction::List => {
			render_symbol_list_lmnav(scheme, request_uri, scope, paging, index, compact)
		}
		SymbolAction::Insights => {
			render_symbol_insights_lmnav(scheme, request_uri, scope, paging, index, compact)
		}
	}
}

fn render_symbol_list_lmnav(
	scheme: &str,
	request_uri: &str,
	scope: &SymbolScopeFilter,
	paging: Paging,
	index: SymbolIndexView<'_>,
	compact: bool,
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
	rows.sort_by(symbol_navigation_cmp);
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
		append_signed_callable_name_hint(&mut output, scope);
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
			if !compact {
				output.push_str("    usages: code_moniker_usages");
				append_call_string_arg(&mut output, "uri", &symbol.identity);
				append_call_number_arg(&mut output, "limit", 50);
				output.push('\n');
			}
		}
	}
	if next.is_some() || !compact {
		output.push_str("\nnext:\n");
	}
	if let Some(next) = next {
		append_symbols_next_call(
			&mut output,
			scheme,
			scope,
			SymbolNextCall {
				action: SymbolAction::List,
				limit: paging.limit,
				cursor: Some(next),
				compact,
			},
		);
	}
	if !compact {
		append_symbols_next_call(
			&mut output,
			scheme,
			scope,
			SymbolNextCall {
				action: SymbolAction::Insights,
				limit: 20,
				cursor: None,
				compact,
			},
		);
		append_workspace_read_call(&mut output, scheme, scope, 2, compact);
	}
	output
}

fn symbol_navigation_cmp(
	left: &(&SymbolRecord, &SourceFileRecord),
	right: &(&SymbolRecord, &SourceFileRecord),
) -> std::cmp::Ordering {
	symbol_is_test_artifact(&left.0.kind, &left.1.rel_path, left.0.identity.as_ref())
		.cmp(&symbol_is_test_artifact(
			&right.0.kind,
			&right.1.rel_path,
			right.0.identity.as_ref(),
		))
		.then_with(|| left.1.rel_path.cmp(&right.1.rel_path))
		.then_with(|| left.0.line_range.cmp(&right.0.line_range))
		.then_with(|| left.0.identity.cmp(&right.0.identity))
}

fn append_signed_callable_name_hint(output: &mut String, scope: &SymbolScopeFilter) {
	let callable_scope = (scope.kinds.is_empty()
		|| scope
			.kinds
			.iter()
			.any(|kind| Shape::for_kind(kind.as_bytes()) == Shape::Callable))
		&& (scope.shapes.is_empty() || scope.shapes.contains(&Shape::Callable));
	if !callable_scope {
		return;
	}
	let Some(name) = scope.name.as_ref().map(regex::Regex::as_str) else {
		return;
	};
	let Some(bare_name) = name
		.strip_prefix('^')
		.and_then(|name| name.strip_suffix('$'))
		.filter(|name| {
			!name.is_empty()
				&& name
					.chars()
					.all(|character| character.is_ascii_alphanumeric() || character == '_')
		})
	else {
		return;
	};
	output.push_str("\nhint:\n");
	output.push_str("  callable names may include their parameter signature; try");
	append_call_string_arg(output, "name", &format!("^{bare_name}\\("));
	output.push_str(" or omit the trailing `$`.\n");
}

fn render_symbol_insights_lmnav(
	scheme: &str,
	request_uri: &str,
	scope: &SymbolScopeFilter,
	paging: Paging,
	index: SymbolIndexView<'_>,
	compact: bool,
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
		.map(|source| source.id)
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
	append_symbols_next_call(
		&mut output,
		scheme,
		scope,
		SymbolNextCall {
			action: SymbolAction::List,
			limit: if compact { 20 } else { 50 },
			cursor: None,
			compact,
		},
	);
	if !compact {
		append_workspace_read_call(&mut output, scheme, scope, 3, compact);
	}
	output
}

struct SymbolNextCall {
	action: SymbolAction,
	limit: usize,
	cursor: Option<usize>,
	compact: bool,
}

fn append_symbols_next_call(
	output: &mut String,
	scheme: &str,
	scope: &SymbolScopeFilter,
	call: SymbolNextCall,
) {
	output.push_str(&format!(
		"  - code_moniker_symbols uri=\"{scheme}workspace\""
	));
	append_call_string_arg(
		output,
		"action",
		match call.action {
			SymbolAction::List => "list",
			SymbolAction::Insights => "insights",
		},
	);
	scope.append_call_args(output);
	append_call_number_arg(output, "limit", call.limit);
	if let Some(cursor) = call.cursor {
		append_call_number_arg(output, "cursor", cursor);
	}
	if !call.compact {
		append_call_bool_arg(output, "compact", false);
	}
	output.push('\n');
}

fn append_workspace_read_call(
	output: &mut String,
	scheme: &str,
	scope: &SymbolScopeFilter,
	depth: usize,
	compact: bool,
) {
	output.push_str(&format!("  - code_moniker_read uri=\"{scheme}workspace\""));
	scope.files.append_call_args(output);
	append_call_number_arg(output, "depth", depth);
	if !compact {
		append_call_bool_arg(output, "compact", false);
	}
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
		self.files_by_id.insert(source.id, source.rel_path.clone());
	}

	fn add_symbol(&mut self, symbol: &SymbolRecord) {
		*self.kinds.entry(symbol.kind.clone()).or_default() += 1;
		*self
			.shapes
			.entry(code_moniker_core::core::shape::Shape::for_kind(symbol.kind.as_bytes()).as_str())
			.or_default() += 1;
		*self.symbols_by_file.entry(symbol.source).or_default() += 1;
		if symbol.navigable {
			self.navigable_symbols += 1;
		} else {
			self.non_navigable_symbols += 1;
		}
	}

	fn add_reference(&mut self, reference: &ReferenceRecord) {
		*self.refs_by_file.entry(reference.source).or_default() += 1;
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
