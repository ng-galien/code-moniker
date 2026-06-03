use std::collections::{BTreeMap, BTreeSet};

use code_moniker_workspace::snapshot::{
	LinkageSnapshot, ReferenceId, ReferenceRecord, SourceFileRecord, SourceId, SymbolId,
	SymbolRecord,
};
use serde_json::{Value, json};

use super::scope::{
	Paging, ScopeFilter, append_call_number_arg, append_call_string_arg, path_prefix,
};
use super::{McpTool, ToolDescriptor, ToolError, ToolResult};
use crate::mcp::context::McpContext;

pub(in crate::mcp) struct UsagesTool;

impl UsagesTool {
	pub(super) const NAME: &'static str = "code_moniker_usages";

	const DESCRIPTION: &'static str = concat!(
		"When to use: inspect who uses a symbol returned by code_moniker_symbols. ",
		"Use this to decide whether a module/type/function behaves like a shared helper or is only locally consumed.\n",
		"\n",
		"Read symbolic usage edges.\n",
		"  direction=incoming — consumers of the target symbol\n",
		"  direction=outgoing — dependencies used by the target symbol\n",
		"  direction=both     — both sections\n",
		"Incoming usage diagnostics include file, context, prefix concentration, reference kinds, and a shared-helper signal."
	);

	fn input_schema() -> Value {
		json!({
			"type": "object",
			"properties": {
				"uri": {
					"type": "string",
					"description": "Exact symbol URI or symbol id returned by code_moniker_symbols."
				},
				"direction": {
					"type": "string",
					"enum": ["incoming", "outgoing", "both"],
					"description": "Usage direction to render."
				},
				"path": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Filter usage locations by relative file glob(s), OR-combined."
				},
				"lang": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Filter usage locations by language tag(s), OR-combined."
				},
				"limit": {
					"type": "integer",
					"minimum": 1,
					"maximum": super::scope::MAX_LIMIT,
					"description": "Maximum usage rows to emit."
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

impl McpTool for UsagesTool {
	fn descriptor(&self) -> ToolDescriptor {
		ToolDescriptor {
			name: Self::NAME,
			description: Self::DESCRIPTION,
			input_schema: Self::input_schema(),
		}
	}

	fn call(&self, context: &McpContext, arguments: &Value) -> Result<ToolResult, ToolError> {
		let request = UsageRequest::from_arguments(arguments).map_err(ToolError::failed)?;
		let text = read_usages(context, &request).map_err(ToolError::failed)?;
		Ok(ToolResult {
			text,
			is_error: false,
		})
	}
}

struct UsageRequest {
	uri: String,
	direction: UsageDirection,
	scope: ScopeFilter,
	paging: Paging,
}

impl UsageRequest {
	fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		Ok(Self {
			uri: arguments
				.get("uri")
				.and_then(Value::as_str)
				.ok_or_else(|| anyhow::anyhow!("`uri` is required"))?
				.to_string(),
			direction: UsageDirection::from_arguments(arguments)?,
			scope: ScopeFilter::from_arguments(arguments)?,
			paging: Paging::from_arguments(arguments)?,
		})
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mcp) enum UsageDirection {
	Incoming,
	Outgoing,
	Both,
}

impl UsageDirection {
	fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		match arguments
			.get("direction")
			.and_then(Value::as_str)
			.unwrap_or("incoming")
		{
			"incoming" => Ok(Self::Incoming),
			"outgoing" => Ok(Self::Outgoing),
			"both" => Ok(Self::Both),
			value => anyhow::bail!("unknown usage direction `{value}`"),
		}
	}

	fn as_str(self) -> &'static str {
		match self {
			Self::Incoming => "incoming",
			Self::Outgoing => "outgoing",
			Self::Both => "both",
		}
	}
}

fn read_usages(context: &McpContext, request: &UsageRequest) -> anyhow::Result<String> {
	if is_workspace_uri(&request.uri, context.scheme()) {
		anyhow::bail!("usage reads require an exact symbol URI returned by code_moniker_symbols");
	}
	let snapshot = context.index().index_snapshot()?;
	render_usages_lmnav(
		context.scheme(),
		UsageQuery {
			uri: &request.uri,
			direction: request.direction,
			scope: &request.scope,
			paging: request.paging,
		},
		UsageIndexView {
			sources: &snapshot.index.sources,
			symbols: &snapshot.index.symbols,
			references: &snapshot.index.references,
			linkage: &snapshot.linkage,
		},
	)
}

fn is_workspace_uri(uri: &str, scheme: &str) -> bool {
	let value = uri.trim();
	value.is_empty()
		|| value == "workspace"
		|| value == format!("{scheme}workspace")
		|| value == format!("{scheme}.")
		|| value == scheme.trim_end_matches('/')
}

pub(in crate::mcp) struct UsageQuery<'a> {
	pub(in crate::mcp) uri: &'a str,
	pub(in crate::mcp) direction: UsageDirection,
	pub(in crate::mcp) scope: &'a ScopeFilter,
	pub(in crate::mcp) paging: Paging,
}

pub(in crate::mcp) struct UsageIndexView<'a> {
	pub(in crate::mcp) sources: &'a [SourceFileRecord],
	pub(in crate::mcp) symbols: &'a [SymbolRecord],
	pub(in crate::mcp) references: &'a [ReferenceRecord],
	pub(in crate::mcp) linkage: &'a LinkageSnapshot,
}

pub(in crate::mcp) fn render_usages_lmnav(
	scheme: &str,
	query: UsageQuery<'_>,
	index: UsageIndexView<'_>,
) -> anyhow::Result<String> {
	let lookup = UsageLookup::new(index);
	let target = lookup
		.find_symbol(query.uri)
		.ok_or_else(|| anyhow::anyhow!("symbol URI not found: {}", query.uri))?;
	let incoming = matches!(
		query.direction,
		UsageDirection::Incoming | UsageDirection::Both
	)
	.then(|| collect_incoming_rows(&lookup, target, query.scope))
	.unwrap_or_default();
	let outgoing = matches!(
		query.direction,
		UsageDirection::Outgoing | UsageDirection::Both
	)
	.then(|| collect_outgoing_rows(&lookup, target, query.scope))
	.unwrap_or_default();
	let mut rows = Vec::new();
	if matches!(
		query.direction,
		UsageDirection::Incoming | UsageDirection::Both
	) {
		rows.extend(incoming.iter().cloned());
	}
	if matches!(
		query.direction,
		UsageDirection::Outgoing | UsageDirection::Both
	) {
		rows.extend(outgoing.iter().cloned());
	}
	rows.sort_by(UsageRow::cmp_for_navigation);
	let (start, end, next) = query.paging.window(&rows);
	let mut output = String::new();
	output.push_str(&format!("uri: {}\n", target.identity));
	if let Some(next) = next {
		output.push_str(&format!(
			"completeness: partial (usages {start}-{end} of {}, next cursor {next})\n",
			rows.len()
		));
	} else {
		output.push_str("completeness: full\n");
	}
	output.push_str(&format!("direction: {}\n", query.direction.as_str()));
	output.push_str(&format!("limit: {}\n", query.paging.limit));
	render_target(&mut output, &lookup, target);
	output.push('\n');
	output.push_str("scope:\n");
	for line in query.scope.describe() {
		output.push_str(&line);
		output.push('\n');
	}
	output.push('\n');
	if matches!(
		query.direction,
		UsageDirection::Incoming | UsageDirection::Both
	) {
		render_incoming_summary(&mut output, &incoming);
		output.push('\n');
	}
	if matches!(
		query.direction,
		UsageDirection::Outgoing | UsageDirection::Both
	) {
		render_outgoing_summary(&mut output, &outgoing);
		output.push('\n');
	}
	render_usage_rows(&mut output, &rows[start..end]);
	output.push_str("\nnext:\n");
	if let Some(next) = next {
		append_usages_call(
			&mut output,
			UsageCall {
				scheme,
				target,
				direction: query.direction,
				scope: query.scope,
				limit: query.paging.limit,
				cursor: Some(next),
			},
		);
	}
	append_read_call(&mut output, target, 3);
	append_usages_call(
		&mut output,
		UsageCall {
			scheme,
			target,
			direction: UsageDirection::Incoming,
			scope: query.scope,
			limit: 50,
			cursor: None,
		},
	);
	append_usages_call(
		&mut output,
		UsageCall {
			scheme,
			target,
			direction: UsageDirection::Outgoing,
			scope: query.scope,
			limit: 50,
			cursor: None,
		},
	);
	Ok(output)
}

fn render_target(output: &mut String, lookup: &UsageLookup<'_>, target: &SymbolRecord) {
	output.push_str("target:\n");
	output.push_str(&format!("  kind: {}\n", target.kind));
	output.push_str(&format!("  name: {}\n", target.name));
	if let Some(source) = lookup.source(&target.source) {
		output.push_str(&format!(
			"  file: {}{}\n",
			source.rel_path,
			line_suffix(target)
		));
		output.push_str(&format!("  lang: {}\n", source.language));
	}
}

fn render_incoming_summary(output: &mut String, rows: &[UsageRow]) {
	let summary = UsageSummary::from_rows(rows);
	output.push_str("incoming_summary:\n");
	summary.render(output);
	output.push_str(&format!(
		"  shared_helper_signal: {}\n",
		summary.shared_helper_signal()
	));
}

fn render_outgoing_summary(output: &mut String, rows: &[UsageRow]) {
	let summary = UsageSummary::from_rows(rows);
	output.push_str("outgoing_summary:\n");
	summary.render(output);
}

fn render_usage_rows(output: &mut String, rows: &[UsageRow]) {
	output.push_str("usages:\n");
	if rows.is_empty() {
		output.push_str("  <empty>\n");
		return;
	}
	for row in rows {
		output.push_str(&format!(
			"  - {} {} {} {}\n",
			row.direction, row.kind, row.actor, row.location
		));
		output.push_str(&format!("    file: {}\n", row.file));
		output.push_str(&format!("    context: {}\n", row.context));
		output.push_str(&format!("    endpoint: {}\n", row.endpoint));
		output.push_str(&format!("    reference: {}\n", row.reference.as_str()));
		if let Some(via) = &row.via {
			output.push_str(&format!("    via: {via}\n"));
		}
	}
}

fn collect_incoming_rows(
	lookup: &UsageLookup<'_>,
	target: &SymbolRecord,
	scope: &ScopeFilter,
) -> Vec<UsageRow> {
	let mut rows = lookup
		.linkage
		.resolved
		.iter()
		.filter(|edge| edge.target == target.id)
		.filter_map(|edge| lookup.reference(&edge.reference))
		.filter_map(|reference| usage_row(lookup, reference, UsageDirection::Incoming, scope))
		.collect::<Vec<_>>();
	let mut seen = rows
		.iter()
		.map(|row| row.reference.clone())
		.collect::<BTreeSet<_>>();
	let mut visited = BTreeSet::from([target.id.clone()]);
	let mut collector = IndirectUsageCollector {
		lookup,
		scope,
		visited: &mut visited,
		seen: &mut seen,
		rows: &mut rows,
	};
	collector.collect(&target.id, 0);
	rows
}

struct IndirectUsageCollector<'a, 'b> {
	lookup: &'a UsageLookup<'a>,
	scope: &'a ScopeFilter,
	visited: &'b mut BTreeSet<SymbolId>,
	seen: &'b mut BTreeSet<ReferenceId>,
	rows: &'b mut Vec<UsageRow>,
}

impl IndirectUsageCollector<'_, '_> {
	fn collect(&mut self, target: &SymbolId, depth: usize) {
		const MAX_INDIRECT_USAGE_DEPTH: usize = 4;
		if depth >= MAX_INDIRECT_USAGE_DEPTH {
			return;
		}
		let aliases = self
			.lookup
			.linkage
			.resolved
			.iter()
			.filter(|edge| &edge.target == target)
			.filter_map(|edge| self.lookup.reference(&edge.reference))
			.filter(|reference| reference.kind == "uses_type")
			.filter_map(|reference| self.lookup.symbol(&reference.source_symbol))
			.filter(|symbol| is_indirect_usage_alias(symbol))
			.filter(|symbol| self.visited.insert(symbol.id.clone()))
			.cloned()
			.collect::<Vec<_>>();
		for alias in aliases {
			collect_direct_rows_via(self.lookup, &alias, self.scope, self.seen, self.rows);
			self.collect(&alias.id, depth + 1);
		}
	}
}

fn is_indirect_usage_alias(symbol: &SymbolRecord) -> bool {
	symbol.kind == "type"
}

fn collect_direct_rows_via(
	lookup: &UsageLookup<'_>,
	alias: &SymbolRecord,
	scope: &ScopeFilter,
	seen: &mut BTreeSet<ReferenceId>,
	rows: &mut Vec<UsageRow>,
) {
	for edge in lookup
		.linkage
		.resolved
		.iter()
		.filter(|edge| edge.target == alias.id)
	{
		let Some(reference) = lookup.reference(&edge.reference) else {
			continue;
		};
		if reference.source_symbol == alias.id || !seen.insert(reference.id.clone()) {
			continue;
		}
		let Some(mut row) = usage_row(lookup, reference, UsageDirection::Incoming, scope) else {
			continue;
		};
		row.via = Some(format!("{} ({})", alias.name, alias.identity));
		rows.push(row);
	}
}

fn collect_outgoing_rows(
	lookup: &UsageLookup<'_>,
	target: &SymbolRecord,
	scope: &ScopeFilter,
) -> Vec<UsageRow> {
	lookup
		.reference_rows
		.iter()
		.filter(|reference| reference.source_symbol == target.id)
		.filter_map(|reference| usage_row(lookup, reference, UsageDirection::Outgoing, scope))
		.collect()
}

fn usage_row(
	lookup: &UsageLookup<'_>,
	reference: &ReferenceRecord,
	direction: UsageDirection,
	scope: &ScopeFilter,
) -> Option<UsageRow> {
	let source = lookup.source(&reference.source)?;
	if !scope.matches_file(&source.rel_path, Some(&source.language)) {
		return None;
	}
	let actor = lookup
		.symbol(&reference.source_symbol)
		.map(|symbol| symbol.name.clone())
		.unwrap_or_else(|| reference.source_symbol.as_str().to_string());
	let context = lookup
		.navigable_context(&reference.source_symbol)
		.map(|symbol| symbol.identity.clone())
		.unwrap_or_else(|| reference.source_symbol.as_str().to_string());
	Some(UsageRow {
		direction: match direction {
			UsageDirection::Incoming => "incoming",
			UsageDirection::Outgoing => "outgoing",
			UsageDirection::Both => "both",
		},
		reference: reference.id.clone(),
		file: source.rel_path.clone(),
		prefix: path_prefix(&source.rel_path),
		actor,
		context,
		endpoint: endpoint_for_reference(lookup, reference),
		kind: reference.kind.clone(),
		line_range: reference.line_range,
		location: reference_location(source, reference),
		via: None,
	})
}

fn endpoint_for_reference(lookup: &UsageLookup<'_>, reference: &ReferenceRecord) -> String {
	lookup
		.resolved_target(&reference.id)
		.and_then(|target| lookup.symbol(&target))
		.map(|symbol| symbol.identity.clone())
		.or_else(|| lookup.external_target(&reference.id))
		.unwrap_or_else(|| reference.target_identity.to_string())
}

fn reference_location(source: &SourceFileRecord, reference: &ReferenceRecord) -> String {
	format!("{}{}", source.rel_path, reference_line_suffix(reference))
}

fn reference_line_suffix(reference: &ReferenceRecord) -> String {
	reference
		.line_range
		.map(|(start, end)| {
			if start == end {
				format!(":L{start}")
			} else {
				format!(":L{start}-L{end}")
			}
		})
		.unwrap_or_else(|| ":L?".to_string())
}

fn line_suffix(symbol: &SymbolRecord) -> String {
	symbol
		.line_range
		.map(|(start, end)| format!(":{start}-{end}"))
		.unwrap_or_default()
}

fn append_read_call(output: &mut String, target: &SymbolRecord, context_lines: usize) {
	output.push_str("  - code_moniker_read");
	append_call_string_arg(output, "uri", &target.identity);
	append_call_number_arg(output, "context_lines", context_lines);
	output.push('\n');
}

struct UsageCall<'a> {
	scheme: &'a str,
	target: &'a SymbolRecord,
	direction: UsageDirection,
	scope: &'a ScopeFilter,
	limit: usize,
	cursor: Option<usize>,
}

fn append_usages_call(output: &mut String, call: UsageCall<'_>) {
	output.push_str("  - code_moniker_usages");
	append_call_string_arg(output, "uri", &call.target.identity);
	append_call_string_arg(output, "direction", call.direction.as_str());
	call.scope.append_call_args(output);
	append_call_number_arg(output, "limit", call.limit);
	if let Some(cursor) = call.cursor {
		append_call_number_arg(output, "cursor", cursor);
	}
	output.push('\n');
	if matches!(call.direction, UsageDirection::Incoming) {
		output.push_str(&format!(
			"  - code_moniker_symbols uri=\"{}workspace\"",
			call.scheme
		));
		call.scope.append_call_args(output);
		append_call_string_arg(output, "name", &call.target.name);
		append_call_number_arg(output, "limit", 20);
		output.push('\n');
	}
}

#[derive(Clone, Debug)]
struct UsageRow {
	direction: &'static str,
	reference: ReferenceId,
	file: String,
	prefix: String,
	actor: String,
	context: String,
	endpoint: String,
	kind: String,
	line_range: Option<(u32, u32)>,
	location: String,
	via: Option<String>,
}

impl UsageRow {
	fn cmp_for_navigation(left: &Self, right: &Self) -> std::cmp::Ordering {
		usage_kind_priority(&left.kind)
			.cmp(&usage_kind_priority(&right.kind))
			.then_with(|| left.file.cmp(&right.file))
			.then_with(|| left.line_range.cmp(&right.line_range))
			.then_with(|| left.actor.cmp(&right.actor))
			.then_with(|| left.reference.cmp(&right.reference))
	}
}

fn usage_kind_priority(kind: &str) -> u8 {
	match kind {
		"implements" | "extends" => 0,
		"method_call" | "calls" => 10,
		"instantiates" => 20,
		"reads" | "uses_type" | "returns_type" | "annotates" => 30,
		"imports_symbol" | "imports_module" => 40,
		_ => 50,
	}
}

#[derive(Default)]
struct UsageSummary {
	refs: usize,
	files: BTreeSet<String>,
	contexts: BTreeSet<String>,
	prefixes: BTreeMap<String, usize>,
	kinds: BTreeMap<String, usize>,
	actors: BTreeMap<String, usize>,
}

impl UsageSummary {
	fn from_rows(rows: &[UsageRow]) -> Self {
		let mut summary = Self::default();
		for row in rows {
			summary.refs += 1;
			summary.files.insert(row.file.clone());
			summary.contexts.insert(row.context.clone());
			*summary.prefixes.entry(row.prefix.clone()).or_default() += 1;
			*summary.kinds.entry(row.kind.clone()).or_default() += 1;
			*summary.actors.entry(row.actor.clone()).or_default() += 1;
		}
		summary
	}

	fn render(&self, output: &mut String) {
		output.push_str(&format!("  refs: {}\n", self.refs));
		output.push_str(&format!("  files: {}\n", self.files.len()));
		output.push_str(&format!("  contexts: {}\n", self.contexts.len()));
		output.push_str(&format!("  prefixes: {}\n", self.prefixes.len()));
		if let Some((prefix, count)) = sorted_counts(&self.prefixes).first() {
			output.push_str(&format!(
				"  dominant_prefix: {prefix} ({count} refs, {}%)\n",
				percent(*count, self.refs)
			));
		}
		render_top_counts(output, "kinds", &self.kinds, 8);
		render_top_counts(output, "top_actors", &self.actors, 8);
		render_top_counts(output, "top_prefixes", &self.prefixes, 8);
	}

	fn shared_helper_signal(&self) -> &'static str {
		if self.refs == 0 {
			return "unused_or_unresolved";
		}
		let dominant = sorted_counts(&self.prefixes)
			.first()
			.map(|(_, count)| percent(*count, self.refs))
			.unwrap_or(0);
		if self.files.len() >= 3 && self.contexts.len() >= 3 && self.prefixes.len() >= 2 {
			"shared_helper_candidate"
		} else if self.files.len() <= 1 || dominant >= 80 {
			"localized_not_shared"
		} else {
			"mixed_review_needed"
		}
	}
}

fn render_top_counts(
	output: &mut String,
	label: &str,
	counts: &BTreeMap<String, usize>,
	limit: usize,
) {
	output.push_str(&format!("  {label}:\n"));
	let rows = sorted_counts(counts);
	if rows.is_empty() {
		output.push_str("    <empty>\n");
		return;
	}
	for (name, count) in rows.iter().take(limit) {
		output.push_str(&format!("    {name}: {count}\n"));
	}
}

fn sorted_counts(counts: &BTreeMap<String, usize>) -> Vec<(String, usize)> {
	let mut rows = counts
		.iter()
		.map(|(name, count)| (name.clone(), *count))
		.collect::<Vec<_>>();
	rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
	rows
}

fn percent(count: usize, total: usize) -> usize {
	count.saturating_mul(100).checked_div(total).unwrap_or(0)
}

struct UsageLookup<'a> {
	sources: BTreeMap<&'a str, &'a SourceFileRecord>,
	symbols: BTreeMap<&'a str, &'a SymbolRecord>,
	symbols_by_identity: BTreeMap<&'a str, &'a SymbolRecord>,
	references: BTreeMap<&'a str, &'a ReferenceRecord>,
	reference_rows: &'a [ReferenceRecord],
	linkage: &'a LinkageSnapshot,
}

impl<'a> UsageLookup<'a> {
	fn new(index: UsageIndexView<'a>) -> Self {
		Self {
			sources: index
				.sources
				.iter()
				.map(|source| (source.id.as_str(), source))
				.collect(),
			symbols: index
				.symbols
				.iter()
				.map(|symbol| (symbol.id.as_str(), symbol))
				.collect(),
			symbols_by_identity: index
				.symbols
				.iter()
				.map(|symbol| (symbol.identity.as_str(), symbol))
				.collect(),
			references: index
				.references
				.iter()
				.map(|reference| (reference.id.as_str(), reference))
				.collect(),
			reference_rows: index.references,
			linkage: index.linkage,
		}
	}

	fn find_symbol(&self, uri: &str) -> Option<&'a SymbolRecord> {
		self.symbols_by_identity
			.get(uri)
			.or_else(|| self.symbols.get(uri))
			.copied()
	}

	fn source(&self, id: &SourceId) -> Option<&'a SourceFileRecord> {
		self.sources.get(id.as_str()).copied()
	}

	fn symbol(&self, id: &SymbolId) -> Option<&'a SymbolRecord> {
		self.symbols.get(id.as_str()).copied()
	}

	fn reference(&self, id: &ReferenceId) -> Option<&'a ReferenceRecord> {
		self.references.get(id.as_str()).copied()
	}

	fn resolved_target(&self, reference: &ReferenceId) -> Option<SymbolId> {
		self.linkage
			.resolved
			.iter()
			.find(|edge| &edge.reference == reference)
			.map(|edge| edge.target.clone())
	}

	fn external_target(&self, reference: &ReferenceId) -> Option<String> {
		self.linkage
			.external
			.iter()
			.find(|external| &external.reference == reference)
			.map(|external| external.target_identity.to_string())
	}

	fn navigable_context(&self, symbol: &SymbolId) -> Option<&'a SymbolRecord> {
		let mut current = self.symbol(symbol)?;
		loop {
			if current.navigable {
				return Some(current);
			}
			let parent = current.parent.as_ref()?;
			current = self.symbol(parent)?;
		}
	}
}
