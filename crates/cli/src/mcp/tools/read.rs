use std::collections::BTreeMap;

use code_moniker_workspace::snapshot::{SourceCatalog, SourceFileRecord, SourceUnit, SymbolRecord};
use serde_json::{Value, json};

use super::common::{is_workspace_uri, normalize_workspace_uri};
use super::scope::{
	Paging, ScopeFilter, append_call_number_arg, append_call_string_arg, path_prefix,
};
use super::{McpTool, ToolDescriptor, ToolError, ToolResult};
use crate::language_kinds;
use crate::mcp::context::McpContext;
use crate::views::{self, MonikerDisplay, RenderOptions};

const DEFAULT_READ_URI: &str = "workspace";
const MAX_DEPTH: usize = 20;

pub(in crate::mcp) struct ReadTool;

impl ReadTool {
	pub(super) const NAME: &'static str = "code_moniker_read";

	const DESCRIPTION: &'static str = concat!(
		"When to use: default entry point to explore the current code-moniker UI workspace. ",
		"The same verb starts at the workspace root, expands an explorer tree, or reads code from an exact symbol URI.\n",
		"\n",
		"Read from code-moniker.\n",
		"  workspace                — workspace summary, language vocabulary, concentration indicators, and explorer page\n",
		"  workspace/views          — project-defined contextual views for agents\n",
		"  code+moniker://workspace — same root with an explicit URI\n",
		"  code+moniker://...       — symbol URI returned by code_moniker_symbols; reads the source slice around that symbol\n",
		"Use path/lang to scope discovery, depth to expand the explorer, limit/cursor for paging, and moniker_format when a view should expose resolved monikers. Pair with code_moniker_symbols when you need symbol rows."
	);

	fn input_schema() -> Value {
		json!({
			"type": "object",
			"properties": {
				"uri": {
					"type": "string",
					"description": "workspace | code+moniker://workspace | exact symbol URI returned by code_moniker_symbols"
				},
				"depth": {
					"type": "integer",
					"minimum": 0,
					"maximum": MAX_DEPTH,
					"description": "Explorer depth to render."
				},
				"path": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Relative file glob(s), OR-combined. Example: crates/cli/src/mcp/**"
				},
				"lang": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Language tag(s), OR-combined. Example: rs, java"
				},
				"limit": {
					"type": "integer",
					"minimum": 1,
					"maximum": super::scope::MAX_LIMIT,
					"description": "Maximum explorer rows to emit."
				},
				"cursor": {
					"oneOf": [{ "type": "integer" }, { "type": "string" }],
					"description": "Opaque row offset returned in next calls."
				},
				"context_lines": {
					"type": "integer",
					"minimum": 0,
					"maximum": 20,
					"description": "Extra lines around a symbol source slice."
				},
				"moniker_format": {
					"type": "string",
					"enum": ["none", "compact", "uri"],
					"description": "For workspace/views reads, optionally display resolved evidence monikers."
				},
				"include_code": {
					"type": "boolean",
					"description": "For workspace/views reads, include source snippets for resolved evidence."
				}
			},
			"required": ["uri"],
			"additionalProperties": false
		})
	}
}

impl McpTool for ReadTool {
	fn descriptor(&self) -> ToolDescriptor {
		ToolDescriptor {
			name: Self::NAME,
			description: Self::DESCRIPTION,
			input_schema: Self::input_schema(),
		}
	}

	fn call(&self, context: &McpContext, arguments: &Value) -> Result<ToolResult, ToolError> {
		let request = ReadRequest::from_arguments(arguments).map_err(ToolError::failed)?;
		let text = read_resource(context, &request).map_err(ToolError::failed)?;
		Ok(ToolResult {
			text,
			is_error: false,
		})
	}
}

struct ReadRequest {
	uri: String,
	depth: usize,
	context_lines: usize,
	include_code: bool,
	moniker_display: MonikerDisplay,
	scope: ScopeFilter,
	paging: Paging,
}

impl ReadRequest {
	fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		Ok(Self {
			uri: arguments
				.get("uri")
				.and_then(Value::as_str)
				.unwrap_or(DEFAULT_READ_URI)
				.to_string(),
			depth: arguments
				.get("depth")
				.and_then(Value::as_u64)
				.unwrap_or(2)
				.min(MAX_DEPTH as u64) as usize,
			context_lines: arguments
				.get("context_lines")
				.and_then(Value::as_u64)
				.unwrap_or(2)
				.min(20) as usize,
			include_code: arguments
				.get("include_code")
				.and_then(Value::as_bool)
				.unwrap_or(false),
			moniker_display: MonikerDisplay::parse(
				arguments.get("moniker_format").and_then(Value::as_str),
			)?,
			scope: ScopeFilter::from_arguments(arguments)?,
			paging: Paging::from_arguments(arguments)?,
		})
	}
}

fn read_resource(context: &McpContext, request: &ReadRequest) -> anyhow::Result<String> {
	if is_workspace_uri(&request.uri, context.scheme(), DEFAULT_READ_URI) {
		return read_workspace(
			context,
			&request.uri,
			request.depth,
			&request.scope,
			request.paging,
		);
	}
	if views::is_views_uri(&request.uri, context.scheme()) {
		return read_view(
			context,
			&request.uri,
			request.context_lines,
			request.include_code,
			request.moniker_display,
		);
	}
	read_symbol(context, &request.uri, request.context_lines)
}

fn read_workspace(
	context: &McpContext,
	uri: &str,
	depth: usize,
	scope: &ScopeFilter,
	paging: Paging,
) -> anyhow::Result<String> {
	let snapshot = context.index().catalog_snapshot()?;
	Ok(render_explorer_lmnav(
		context.scheme(),
		uri,
		depth,
		&snapshot.catalog,
		scope,
		paging,
	))
}

fn read_symbol(context: &McpContext, uri: &str, context_lines: usize) -> anyhow::Result<String> {
	let snapshot = context.index().index_snapshot()?;
	let symbol = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity == uri || symbol.id.as_str() == uri)
		.ok_or_else(|| anyhow::anyhow!("symbol URI not found: {uri}"))?;
	let source = snapshot
		.index
		.sources
		.iter()
		.find(|source| source.id == symbol.source)
		.ok_or_else(|| anyhow::anyhow!("source not found for symbol: {uri}"))?;
	let source_text = std::fs::read_to_string(&source.path)
		.map_err(|err| anyhow::anyhow!("cannot read {}: {err}", source.path))?;
	Ok(render_symbol_source_lmnav(
		context.scheme(),
		symbol,
		source,
		&source_text,
		context_lines,
	))
}

fn read_view(
	context: &McpContext,
	uri: &str,
	context_lines: usize,
	include_code: bool,
	moniker_display: MonikerDisplay,
) -> anyhow::Result<String> {
	let snapshot = context.index().index_snapshot()?;
	views::render_lmnav(
		uri,
		&context.opts().paths,
		context.scheme(),
		&snapshot,
		RenderOptions {
			moniker_display,
			context_lines,
			include_code,
		},
	)
}

pub(in crate::mcp) fn render_symbol_source_lmnav(
	scheme: &str,
	symbol: &SymbolRecord,
	source: &SourceFileRecord,
	source_text: &str,
	context_lines: usize,
) -> String {
	let total_lines = source_text.lines().count().max(1);
	let (raw_start, raw_end) = symbol
		.line_range
		.map(|(start, end)| (start.max(1) as usize, end.max(start).max(1) as usize))
		.unwrap_or((1, total_lines.min(80)));
	let target_start = raw_start.min(total_lines);
	let target_end = raw_end.min(total_lines).max(target_start);
	let slice_start = target_start.saturating_sub(context_lines).max(1);
	let slice_end = target_end.saturating_add(context_lines).min(total_lines);
	let mut output = String::new();
	output.push_str(&format!("uri: {}\n", symbol.identity));
	if symbol.line_range.is_some() {
		output.push_str("completeness: full\n");
	} else {
		output.push_str(
			"completeness: partial (symbol has no line range; showing first available lines)\n",
		);
	}
	output.push_str(&format!("file: {}\n", source.rel_path));
	output.push_str(&format!("language: {}\n", source.language));
	output.push_str(&format!("kind: {}\n", symbol.kind));
	output.push_str(&format!("name: {}\n", symbol.name));
	output.push_str(&format!("range: {target_start}-{target_end}\n"));
	output.push_str(&format!("slice: {slice_start}-{slice_end}\n\n"));
	output.push_str("code:\n");
	for (line_number, line) in source_text.lines().enumerate() {
		let line_number = line_number + 1;
		if line_number < slice_start || line_number > slice_end {
			continue;
		}
		output.push_str(&format!("  {line_number:>4} | {line}\n"));
	}
	output.push_str("\nnext:\n");
	output.push_str(&format!(
		"  - code_moniker_symbols uri=\"{scheme}workspace\""
	));
	append_call_string_arg(&mut output, "name", &symbol.name);
	append_call_number_arg(&mut output, "limit", 20);
	output.push('\n');
	output.push_str(&format!(
		"  - code_moniker_symbols uri=\"{scheme}workspace\""
	));
	append_call_string_arg(&mut output, "path", &source.rel_path);
	append_call_number_arg(&mut output, "limit", 50);
	output.push('\n');
	output
}

pub(in crate::mcp) fn render_explorer_lmnav(
	scheme: &str,
	request_uri: &str,
	depth: usize,
	catalog: &SourceCatalog,
	scope: &ScopeFilter,
	paging: Paging,
) -> String {
	let scoped_sources = catalog
		.sources
		.iter()
		.filter(|source| scope.matches_file(&source.display_name, source.language.as_deref()))
		.collect::<Vec<_>>();
	let mut tree = ExplorerNode::default();
	for source in &scoped_sources {
		tree.insert(source);
	}
	let uri = normalize_workspace_uri(scheme, request_uri, DEFAULT_READ_URI);
	let summary = WorkspaceSummary::from_sources(catalog.sources.len(), &scoped_sources);
	let mut lines = Vec::new();
	tree.render(depth, "", &mut lines);
	let (start, end, next) = paging.window(&lines);
	let mut output = String::new();
	output.push_str(&format!("uri: {uri}\n"));
	if let Some(next) = next {
		output.push_str(&format!(
			"completeness: partial (explorer rows {start}-{end} of {}, next cursor {next})\n",
			lines.len()
		));
	} else {
		output.push_str("completeness: full\n");
	}
	output.push_str(&format!("files: {}\n", summary.scoped_files));
	output.push_str(&format!("files_total: {}\n", summary.total_files));
	output.push_str(&format!("depth: {depth}\n\n"));
	output.push_str("scope:\n");
	for line in scope.describe() {
		output.push_str(&line);
		output.push('\n');
	}
	output.push('\n');
	summary.render(&mut output);
	output.push_str("explorer:\n");
	if lines.is_empty() {
		output.push_str("  <empty>\n");
	} else {
		for line in lines.iter().take(end).skip(start) {
			output.push_str(line);
			output.push('\n');
		}
	}
	output.push_str("\nnext:\n");
	if let Some(next) = next {
		append_read_next_call(&mut output, scheme, scope, depth, paging.limit, Some(next));
	}
	append_read_next_call(
		&mut output,
		scheme,
		scope,
		(depth + 1).min(MAX_DEPTH),
		paging.limit,
		None,
	);
	append_symbols_call(&mut output, scheme, scope, 50);
	output
}

fn append_read_next_call(
	output: &mut String,
	scheme: &str,
	scope: &ScopeFilter,
	depth: usize,
	limit: usize,
	cursor: Option<usize>,
) {
	output.push_str(&format!("  - code_moniker_read uri=\"{scheme}workspace\""));
	scope.append_call_args(output);
	append_call_number_arg(output, "depth", depth);
	append_call_number_arg(output, "limit", limit);
	if let Some(cursor) = cursor {
		append_call_number_arg(output, "cursor", cursor);
	}
	output.push('\n');
}

fn append_symbols_call(output: &mut String, scheme: &str, scope: &ScopeFilter, limit: usize) {
	output.push_str(&format!(
		"  - code_moniker_symbols uri=\"{scheme}workspace\""
	));
	scope.append_call_args(output);
	append_call_number_arg(output, "limit", limit);
	output.push('\n');
}

#[derive(Default)]
struct ExplorerNode {
	files: Vec<String>,
	children: BTreeMap<String, ExplorerNode>,
}

impl ExplorerNode {
	fn insert(&mut self, source: &SourceUnit) {
		let mut parts = source
			.display_name
			.split(['/', '\\'])
			.filter(|part| !part.is_empty())
			.peekable();
		let mut node = self;
		while let Some(part) = parts.next() {
			if parts.peek().is_some() {
				node = node.children.entry(part.to_string()).or_default();
			} else {
				node.files.push(match source.language.as_deref() {
					Some(language) if !language.is_empty() => format!("{part} [{language}]"),
					_ => part.to_string(),
				});
			}
		}
	}

	fn render(&self, depth: usize, prefix: &str, lines: &mut Vec<String>) {
		if depth == 0 {
			return;
		}
		for (name, child) in &self.children {
			let path = if prefix.is_empty() {
				format!("{name}/")
			} else {
				format!("{prefix}{name}/")
			};
			lines.push(format!("  {path}"));
			child.render(depth - 1, &path, lines);
		}
		for file in &self.files {
			let path = if prefix.is_empty() {
				file.to_string()
			} else {
				format!("{prefix}{file}")
			};
			lines.push(format!("  {path}"));
		}
	}
}

#[derive(Debug)]
struct WorkspaceSummary {
	total_files: usize,
	scoped_files: usize,
	languages: Vec<(String, usize)>,
	prefixes: Vec<(String, usize)>,
}

impl WorkspaceSummary {
	fn from_sources(total_files: usize, sources: &[&SourceUnit]) -> Self {
		let mut languages = BTreeMap::<String, usize>::new();
		let mut prefixes = BTreeMap::<String, usize>::new();
		for source in sources {
			if let Some(language) = source.language.as_deref() {
				*languages.entry(language.to_string()).or_default() += 1;
			}
			*prefixes
				.entry(path_prefix(&source.display_name))
				.or_default() += 1;
		}
		let mut languages = languages.into_iter().collect::<Vec<_>>();
		languages.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
		let mut prefixes = prefixes.into_iter().collect::<Vec<_>>();
		prefixes.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
		prefixes.truncate(8);
		Self {
			total_files,
			scoped_files: sources.len(),
			languages,
			prefixes,
		}
	}

	fn render(&self, output: &mut String) {
		output.push_str("summary:\n");
		self.render_languages(output);
		self.render_concentration(output);
		self.render_hints(output);
		output.push('\n');
	}

	fn render_languages(&self, output: &mut String) {
		output.push_str("  languages:\n");
		if self.languages.is_empty() {
			output.push_str("    <empty>\n");
		} else {
			for (language, count) in &self.languages {
				output.push_str(&format!("    {language}: {count}\n"));
			}
		}
	}

	fn render_concentration(&self, output: &mut String) {
		output.push_str("  concentration:\n");
		if self.prefixes.is_empty() {
			output.push_str("    <empty>\n");
		} else {
			for (prefix, count) in &self.prefixes {
				let percent = (count * 100).checked_div(self.scoped_files).unwrap_or(0);
				output.push_str(&format!("    {prefix}: {count} files ({percent}%)\n"));
			}
		}
	}

	fn render_hints(&self, output: &mut String) {
		output.push_str("  hints:\n");
		output.push_str("    start with code_moniker_symbols using path/lang/kind/shape filters before broad symbol reads\n");
		for (language, _) in self.languages.iter().take(4) {
			if let Some(lang) = code_moniker_core::lang::Lang::from_tag(language) {
				let kinds = language_kinds::known_kinds(std::iter::once(&lang))
					.into_iter()
					.take(18)
					.collect::<Vec<_>>();
				output.push_str(&format!("    {language} kinds: {}\n", kinds.join(", ")));
			}
		}
	}
}
