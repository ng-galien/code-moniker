use std::collections::BTreeMap;

use code_moniker_workspace::facade::{
	LocalWorkspaceOptions, WorkspaceFacade, local_workspace_ports,
};
use code_moniker_workspace::snapshot::{
	SourceCatalog, SourceUnit, WorkspaceRequest, WorkspaceTransition,
};
use code_moniker_workspace::source::LocalResourceCache;
use serde_json::{Value, json};

use super::scope::{Paging, ScopeFilter, path_prefix};
use super::{McpTool, ToolDescriptor, ToolError, ToolResult};
use crate::language_kinds;
use crate::mcp::context::McpContext;

const DEFAULT_READ_URI: &str = "workspace";
const MAX_DEPTH: usize = 20;

pub(in crate::mcp) struct ReadTool;

impl ReadTool {
	pub(super) const NAME: &'static str = "code_moniker_read";

	const DESCRIPTION: &'static str = concat!(
		"When to use: default entry point to explore the current code-moniker UI workspace. ",
		"The same verb starts at the workspace root and expands an explorer tree by depth.\n",
		"\n",
		"Read from code-moniker.\n",
		"  workspace                — workspace summary, language vocabulary, concentration indicators, and explorer page\n",
		"  code+moniker://workspace — same root with an explicit URI\n",
		"Use path/lang to scope discovery, depth to expand the explorer, and limit/cursor for paging. Pair with code_moniker_symbols when you need symbol rows."
	);

	fn input_schema() -> Value {
		json!({
			"type": "object",
			"properties": {
				"uri": {
					"type": "string",
					"description": "workspace | code+moniker://workspace"
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
		let uri = arguments
			.get("uri")
			.and_then(Value::as_str)
			.unwrap_or(DEFAULT_READ_URI);
		let depth = arguments.get("depth").and_then(Value::as_u64).unwrap_or(2) as usize;
		let scope = ScopeFilter::from_arguments(arguments).map_err(ToolError::failed)?;
		let paging = Paging::from_arguments(arguments).map_err(ToolError::failed)?;
		let text = read_workspace(context, uri, depth.min(MAX_DEPTH), &scope, paging)
			.map_err(ToolError::failed)?;
		Ok(ToolResult {
			text,
			is_error: false,
		})
	}
}

fn read_workspace(
	context: &McpContext,
	uri: &str,
	depth: usize,
	scope: &ScopeFilter,
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
	match workspace.load_catalog(WorkspaceRequest::new("mcp-read")) {
		WorkspaceTransition::Ready { .. } => {
			let Some(snapshot) = workspace.snapshot() else {
				anyhow::bail!("workspace catalog snapshot is unavailable");
			};
			Ok(render_explorer_lmnav(
				context.scheme(),
				uri,
				depth,
				&snapshot.catalog,
				scope,
				paging,
			))
		}
		WorkspaceTransition::Failed { failure, .. } => anyhow::bail!(failure.message),
	}
}

fn is_workspace_uri(uri: &str, scheme: &str) -> bool {
	let value = uri.trim();
	value.is_empty()
		|| value == DEFAULT_READ_URI
		|| value == format!("{scheme}workspace")
		|| value == format!("{scheme}.")
		|| value == scheme.trim_end_matches('/')
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
	let uri = normalize_workspace_uri(scheme, request_uri);
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
			output.push_str(&line);
			output.push('\n');
		}
	}
	output.push_str("\nnext:\n");
	if let Some(next) = next {
		output.push_str(&format!(
			"  - code_moniker_read uri=\"{scheme}workspace\" depth={depth} limit={} cursor={next}\n",
			paging.limit
		));
	}
	output.push_str(&format!(
		"  - code_moniker_read uri=\"{scheme}workspace\" depth={}\n",
		(depth + 1).min(MAX_DEPTH)
	));
	output.push_str(&format!(
		"  - code_moniker_symbols uri=\"{scheme}workspace\" limit=50\n"
	));
	output
}

fn normalize_workspace_uri(scheme: &str, request_uri: &str) -> String {
	let trimmed = request_uri.trim();
	if trimmed.is_empty() || trimmed == DEFAULT_READ_URI {
		format!("{scheme}workspace")
	} else {
		trimmed.to_string()
	}
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
		output.push_str("  languages:\n");
		if self.languages.is_empty() {
			output.push_str("    <empty>\n");
		} else {
			for (language, count) in &self.languages {
				output.push_str(&format!("    {language}: {count}\n"));
			}
		}
		output.push_str("  concentration:\n");
		if self.prefixes.is_empty() {
			output.push_str("    <empty>\n");
		} else {
			for (prefix, count) in &self.prefixes {
				let percent = if self.scoped_files == 0 {
					0
				} else {
					(count * 100) / self.scoped_files
				};
				output.push_str(&format!("    {prefix}: {count} files ({percent}%)\n"));
			}
		}
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
		output.push('\n');
	}
}
