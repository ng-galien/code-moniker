use std::collections::{BTreeMap, BTreeSet};

use code_moniker_query::{
	Query, QueryRequest, QueryResult, SymbolUsagesQuery, SymbolUsagesResult, UsageDto,
	UsageSummaryDto,
};
use code_moniker_workspace::snapshot::{
	LinkageSnapshot, ReferenceId, ReferenceRecord, SourceFileRecord, SourceId, SymbolId,
	SymbolRecord,
};
use serde_json::{Value, json};

use super::common::{
	apply_response_aliases, compact_argument, is_workspace_uri, line_range_suffix,
	sorted_count_rows, symbol_line_suffix,
};
use super::scope::{
	Paging, ScopeFilter, append_call_bool_arg, append_call_cursor_arg, append_call_number_arg,
	append_call_string_arg, path_prefix,
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
		"Incoming usage diagnostics include file, context, prefix concentration, reference kinds, and a shared-helper signal. ",
		"Compact output uses response-local aliases and one-line usage facts by default."
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
				"compact": {
					"type": "boolean",
					"default": true,
					"description": "Use response-local moniker aliases, one-line facts, and minimal next calls. Defaults true; false preserves canonical verbose output."
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
	compact: bool,
}

impl UsageRequest {
	fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		let compact = compact_argument(arguments)?;
		Ok(Self {
			uri: arguments
				.get("uri")
				.and_then(Value::as_str)
				.ok_or_else(|| anyhow::anyhow!("`uri` is required"))?
				.to_string(),
			direction: UsageDirection::from_arguments(arguments)?,
			scope: ScopeFilter::from_arguments(arguments)?,
			paging: Paging::from_arguments_for_output(arguments, compact)?,
			compact,
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
	if is_workspace_uri(&request.uri, context.scheme(), "workspace") {
		anyhow::bail!("usage reads require an exact symbol URI returned by code_moniker_symbols");
	}
	let response = context.query(QueryRequest {
		query: Query::SymbolUsages(SymbolUsagesQuery {
			workspace: None,
			uri: request.uri.clone(),
			direction: match request.direction {
				UsageDirection::Incoming => code_moniker_query::UsageDirection::Incoming,
				UsageDirection::Outgoing => code_moniker_query::UsageDirection::Outgoing,
				UsageDirection::Both => code_moniker_query::UsageDirection::Both,
			},
			path: request.scope.paths.clone(),
			lang: request.scope.langs.clone(),
			projection: Vec::new(),
		}),
		consistency: code_moniker_query::Consistency::Current,
		page: request.paging.daemon_page(),
	})?;
	let QueryResult::SymbolUsages(result) = response.result else {
		anyhow::bail!("unexpected daemon response for usages");
	};
	Ok(render_daemon_usages_lmnav(
		context.scheme(),
		request,
		response.next_cursor.as_ref(),
		&result,
	))
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

fn render_daemon_usages_lmnav(
	scheme: &str,
	request: &UsageRequest,
	next_cursor: Option<&code_moniker_query::QueryCursor>,
	result: &SymbolUsagesResult,
) -> String {
	let start = request.paging.cursor.min(result.total);
	let end = start.saturating_add(result.rows.len()).min(result.total);
	let mut output = String::new();
	output.push_str(&format!("uri: {}\n", result.target.uri));
	if let Some(next) = next_cursor {
		output.push_str(&format!(
			"completeness: partial (usages {start}-{end} of {}, next cursor {})\n",
			result.total, next.offset
		));
	} else {
		output.push_str("completeness: full\n");
	}
	output.push_str(&format!("direction: {}\n", request.direction.as_str()));
	output.push_str(&format!("limit: {}\n", request.paging.limit));
	output.push_str("target:\n");
	output.push_str(&format!("  kind: {}\n", result.target.kind));
	output.push_str(&format!("  name: {}\n", result.target.name));
	output.push_str(&format!(
		"  file: {}{}\n",
		result.target.file,
		line_range_suffix(result.target.line_range)
	));
	output.push_str(&format!("  lang: {}\n\n", result.target.language));
	output.push_str("scope:\n");
	for line in request.scope.describe() {
		output.push_str(&line);
		output.push('\n');
	}
	output.push('\n');
	if matches!(
		request.direction,
		UsageDirection::Incoming | UsageDirection::Both
	) {
		render_daemon_usage_summary(
			&mut output,
			"incoming_summary",
			result.incoming_summary.as_ref(),
			true,
		);
		output.push('\n');
	}
	if matches!(
		request.direction,
		UsageDirection::Outgoing | UsageDirection::Both
	) {
		render_daemon_usage_summary(
			&mut output,
			"outgoing_summary",
			result.outgoing_summary.as_ref(),
			false,
		);
		output.push('\n');
	}
	render_daemon_usage_rows(&mut output, &result.rows, request.compact);
	output.push_str("\nnext:\n");
	if let Some(next) = next_cursor {
		append_daemon_usages_call(
			&mut output,
			DaemonUsageCall {
				target_uri: &result.target.uri,
				direction: request.direction,
				scope: &request.scope,
				limit: request.paging.limit,
				cursor: Some(next),
				compact: request.compact,
			},
		);
	}
	output.push_str("  - code_moniker_read");
	append_call_string_arg(&mut output, "uri", &result.target.uri);
	append_call_number_arg(&mut output, "context_lines", 3);
	if !request.compact {
		append_call_bool_arg(&mut output, "compact", false);
	}
	output.push('\n');
	if !request.compact {
		append_daemon_usages_call(
			&mut output,
			DaemonUsageCall {
				target_uri: &result.target.uri,
				direction: UsageDirection::Incoming,
				scope: &request.scope,
				limit: 50,
				cursor: None,
				compact: request.compact,
			},
		);
		append_daemon_usages_call(
			&mut output,
			DaemonUsageCall {
				target_uri: &result.target.uri,
				direction: UsageDirection::Outgoing,
				scope: &request.scope,
				limit: 50,
				cursor: None,
				compact: request.compact,
			},
		);
		output.push_str(&format!(
			"  - code_moniker_symbols uri=\"{scheme}workspace\""
		));
		request.scope.append_call_args(&mut output);
		append_call_string_arg(&mut output, "name", &result.target.name);
		append_call_number_arg(&mut output, "limit", 20);
		append_call_bool_arg(&mut output, "compact", false);
		output.push('\n');
	}
	let candidates = usage_dto_alias_candidates(&result.target.uri, &result.rows);
	apply_response_aliases(output, request.compact, candidates)
}

fn render_daemon_usage_summary(
	output: &mut String,
	label: &str,
	summary: Option<&UsageSummaryDto>,
	shared_signal: bool,
) {
	output.push_str(&format!("{label}:\n"));
	let Some(summary) = summary else {
		output.push_str("  refs: 0\n");
		return;
	};
	render_usage_summary_dto(output, summary);
	if shared_signal {
		output.push_str(&format!(
			"  shared_helper_signal: {}\n",
			summary.shared_helper_signal
		));
	}
}

fn render_usage_summary_dto(output: &mut String, summary: &UsageSummaryDto) {
	output.push_str(&format!("  refs: {}\n", summary.refs));
	output.push_str(&format!("  files: {}\n", summary.files));
	output.push_str(&format!("  contexts: {}\n", summary.contexts));
	output.push_str(&format!("  prefixes: {}\n", summary.prefixes));
	if !summary.dominant_prefix.is_empty() {
		output.push_str(&format!("  dominant_prefix: {}\n", summary.dominant_prefix));
	}
	render_count_dtos(output, "kinds", &summary.kinds, 8);
	render_count_dtos(output, "top_actors", &summary.top_actors, 8);
	render_count_dtos(output, "top_prefixes", &summary.top_prefixes, 8);
}

fn render_count_dtos(
	output: &mut String,
	label: &str,
	rows: &[code_moniker_query::CountDto],
	limit: usize,
) {
	if rows.is_empty() {
		output.push_str(&format!("  {label}: <empty>\n"));
		return;
	}
	output.push_str(&format!("  {label}:\n"));
	for row in rows.iter().take(limit) {
		output.push_str(&format!("    - {}: {}\n", row.name, row.count));
	}
}

fn render_daemon_usage_rows(output: &mut String, rows: &[UsageDto], compact: bool) {
	output.push_str("usages:\n");
	if rows.is_empty() {
		output.push_str("  <empty>\n");
		return;
	}
	for row in rows {
		if compact {
			render_compact_daemon_usage_row(output, row);
			continue;
		}
		output.push_str(&format!(
			"  - {} {} {} {}\n",
			row.direction.as_str(),
			row.kind,
			row.actor,
			row.location
		));
		output.push_str(&format!("    file: {}\n", row.file));
		output.push_str(&format!("    context: {}\n", row.context));
		output.push_str(&format!("    endpoint: {}\n", row.endpoint));
		output.push_str(&format!("    reference: {}\n", row.reference));
		if let Some(via) = &row.via {
			output.push_str(&format!("    via: {via}\n"));
		}
	}
}

fn render_compact_daemon_usage_row(output: &mut String, row: &UsageDto) {
	match row.direction {
		code_moniker_query::UsageDirection::Incoming => {
			output.push_str(&format!(
				"  - in {} {} {} context={} ref={} ",
				row.kind, row.actor, row.location, row.context, row.reference
			));
		}
		code_moniker_query::UsageDirection::Outgoing => {
			output.push_str(&format!(
				"  - out {} {} {} ref={} ",
				row.kind, row.endpoint, row.location, row.reference
			));
		}
		code_moniker_query::UsageDirection::Both => {
			output.push_str(&format!(
				"  - both {} {} {} context={} endpoint={} ref={} ",
				row.kind, row.actor, row.location, row.context, row.endpoint, row.reference
			));
		}
	}
	if let Some(via) = &row.via {
		output.push_str(&format!("via={via}"));
	}
	while output.ends_with(' ') {
		output.pop();
	}
	output.push('\n');
}

fn usage_dto_alias_candidates<'a>(target: &'a str, rows: &'a [UsageDto]) -> Vec<&'a str> {
	let mut candidates = vec![target];
	for row in rows {
		candidates.push(&row.context);
		candidates.push(&row.endpoint);
		if let Some(via) = row.via.as_deref().and_then(via_moniker) {
			candidates.push(via);
		}
	}
	candidates
}

fn via_moniker(via: &str) -> Option<&str> {
	let start = via.rfind("(code+moniker://")?.saturating_add(1);
	via.get(start..via.len().checked_sub(1)?)
}

struct DaemonUsageCall<'a> {
	target_uri: &'a str,
	direction: UsageDirection,
	scope: &'a ScopeFilter,
	limit: usize,
	cursor: Option<&'a code_moniker_query::QueryCursor>,
	compact: bool,
}

fn append_daemon_usages_call(output: &mut String, call: DaemonUsageCall<'_>) {
	output.push_str("  - code_moniker_usages");
	append_call_string_arg(output, "uri", call.target_uri);
	append_call_string_arg(output, "direction", call.direction.as_str());
	call.scope.append_call_args(output);
	append_call_number_arg(output, "limit", call.limit);
	if let Some(cursor) = call.cursor {
		append_call_cursor_arg(output, "cursor", cursor);
	}
	if !call.compact {
		append_call_bool_arg(output, "compact", false);
	}
	output.push('\n');
}

pub(in crate::mcp) fn render_usages_lmnav(
	scheme: &str,
	query: UsageQuery<'_>,
	index: UsageIndexView<'_>,
) -> anyhow::Result<String> {
	render_usages_lmnav_mode(scheme, query, index, true)
}

pub(in crate::mcp) fn render_usages_lmnav_mode(
	scheme: &str,
	query: UsageQuery<'_>,
	index: UsageIndexView<'_>,
	compact: bool,
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
	render_usage_rows(&mut output, &rows[start..end], compact);
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
				compact,
			},
		);
	}
	append_symbol_read_call(&mut output, target, 3, compact);
	if !compact {
		append_usages_call(
			&mut output,
			UsageCall {
				scheme,
				target,
				direction: UsageDirection::Incoming,
				scope: query.scope,
				limit: 50,
				cursor: None,
				compact,
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
				compact,
			},
		);
	}
	let candidates = usage_row_alias_candidates(target.identity.as_ref(), &rows[start..end]);
	Ok(apply_response_aliases(output, compact, candidates))
}

fn render_target(output: &mut String, lookup: &UsageLookup<'_>, target: &SymbolRecord) {
	output.push_str("target:\n");
	output.push_str(&format!("  kind: {}\n", target.kind));
	output.push_str(&format!("  name: {}\n", target.name));
	if let Some(source) = lookup.source(&target.source) {
		output.push_str(&format!(
			"  file: {}{}\n",
			source.rel_path,
			symbol_line_suffix(target)
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

fn render_usage_rows(output: &mut String, rows: &[UsageRow], compact: bool) {
	output.push_str("usages:\n");
	if rows.is_empty() {
		output.push_str("  <empty>\n");
		return;
	}
	for row in rows {
		if compact {
			render_compact_usage_row(output, row);
			continue;
		}
		output.push_str(&format!(
			"  - {} {} {} {}\n",
			row.direction, row.kind, row.actor, row.location
		));
		output.push_str(&format!("    file: {}\n", row.file));
		output.push_str(&format!("    context: {}\n", row.context));
		output.push_str(&format!("    endpoint: {}\n", row.endpoint));
		output.push_str(&format!("    reference: {}\n", row.reference));
		if let Some(via) = &row.via {
			output.push_str(&format!("    via: {via}\n"));
		}
	}
}

fn render_compact_usage_row(output: &mut String, row: &UsageRow) {
	match row.direction {
		"incoming" => output.push_str(&format!(
			"  - in {} {} {} context={} ref={} ",
			row.kind, row.actor, row.location, row.context, row.reference
		)),
		"outgoing" => output.push_str(&format!(
			"  - out {} {} {} ref={} ",
			row.kind, row.endpoint, row.location, row.reference
		)),
		_ => output.push_str(&format!(
			"  - {} {} {} {} context={} endpoint={} ref={} ",
			row.direction,
			row.kind,
			row.actor,
			row.location,
			row.context,
			row.endpoint,
			row.reference
		)),
	}
	if let Some(via) = &row.via {
		output.push_str(&format!("via={via}"));
	}
	while output.ends_with(' ') {
		output.pop();
	}
	output.push('\n');
}

fn usage_row_alias_candidates<'a>(target: &'a str, rows: &'a [UsageRow]) -> Vec<&'a str> {
	let mut candidates = vec![target];
	for row in rows {
		candidates.push(&row.context);
		candidates.push(&row.endpoint);
		if let Some(via) = row.via.as_deref().and_then(via_moniker) {
			candidates.push(via);
		}
	}
	candidates
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
		.map(|row| row.reference)
		.collect::<BTreeSet<_>>();
	let mut visited = BTreeSet::from([target.id]);
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
			.filter(|symbol| self.visited.insert(symbol.id))
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
		if reference.source_symbol == alias.id || !seen.insert(reference.id) {
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
		.map(|symbol| symbol.name.to_string())
		.unwrap_or_else(|| reference.source_symbol.to_string());
	let context = lookup
		.navigable_context(&reference.source_symbol)
		.map(|symbol| symbol.identity.to_string())
		.unwrap_or_else(|| reference.source_symbol.to_string());
	Some(UsageRow {
		direction: match direction {
			UsageDirection::Incoming => "incoming",
			UsageDirection::Outgoing => "outgoing",
			UsageDirection::Both => "both",
		},
		reference: reference.id,
		file: source.rel_path.to_string(),
		prefix: path_prefix(&source.rel_path),
		actor,
		context,
		endpoint: endpoint_for_reference(lookup, reference),
		kind: reference.kind.to_string(),
		line_range: reference.line_range,
		location: reference_location(source, reference),
		via: None,
	})
}

fn endpoint_for_reference(lookup: &UsageLookup<'_>, reference: &ReferenceRecord) -> String {
	lookup
		.resolved_target(&reference.id)
		.and_then(|target| lookup.symbol(&target))
		.map(|symbol| symbol.identity.to_string())
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

fn append_symbol_read_call(
	output: &mut String,
	target: &SymbolRecord,
	context_lines: usize,
	compact: bool,
) {
	output.push_str("  - code_moniker_read");
	append_call_string_arg(output, "uri", &target.identity);
	append_call_number_arg(output, "context_lines", context_lines);
	if !compact {
		append_call_bool_arg(output, "compact", false);
	}
	output.push('\n');
}

struct UsageCall<'a> {
	scheme: &'a str,
	target: &'a SymbolRecord,
	direction: UsageDirection,
	scope: &'a ScopeFilter,
	limit: usize,
	cursor: Option<usize>,
	compact: bool,
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
	if !call.compact {
		append_call_bool_arg(output, "compact", false);
	}
	output.push('\n');
	if matches!(call.direction, UsageDirection::Incoming) && !call.compact {
		output.push_str(&format!(
			"  - code_moniker_symbols uri=\"{}workspace\"",
			call.scheme
		));
		call.scope.append_call_args(output);
		append_call_string_arg(output, "name", &call.target.name);
		append_call_number_arg(output, "limit", 20);
		append_call_bool_arg(output, "compact", false);
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
			summary.files.insert(row.file.to_string());
			summary.contexts.insert(row.context.to_string());
			*summary.prefixes.entry(row.prefix.to_string()).or_default() += 1;
			*summary.kinds.entry(row.kind.to_string()).or_default() += 1;
			*summary.actors.entry(row.actor.to_string()).or_default() += 1;
		}
		summary
	}

	fn render(&self, output: &mut String) {
		output.push_str(&format!("  refs: {}\n", self.refs));
		output.push_str(&format!("  files: {}\n", self.files.len()));
		output.push_str(&format!("  contexts: {}\n", self.contexts.len()));
		output.push_str(&format!("  prefixes: {}\n", self.prefixes.len()));
		if let Some((prefix, count)) = sorted_count_rows(&self.prefixes).first() {
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
		let dominant = sorted_count_rows(&self.prefixes)
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
	let rows = sorted_count_rows(counts);
	if rows.is_empty() {
		output.push_str("    <empty>\n");
		return;
	}
	for (name, count) in rows.iter().take(limit) {
		output.push_str(&format!("    {name}: {count}\n"));
	}
}

fn percent(count: usize, total: usize) -> usize {
	count.saturating_mul(100).checked_div(total).unwrap_or(0)
}

struct UsageLookup<'a> {
	sources: BTreeMap<SourceId, &'a SourceFileRecord>,
	symbols: BTreeMap<SymbolId, &'a SymbolRecord>,
	symbols_by_identity: BTreeMap<&'a str, &'a SymbolRecord>,
	references: BTreeMap<ReferenceId, &'a ReferenceRecord>,
	reference_rows: &'a [ReferenceRecord],
	linkage: &'a LinkageSnapshot,
}

impl<'a> UsageLookup<'a> {
	fn new(index: UsageIndexView<'a>) -> Self {
		Self {
			sources: index
				.sources
				.iter()
				.map(|source| (source.id, source))
				.collect(),
			symbols: index
				.symbols
				.iter()
				.map(|symbol| (symbol.id, symbol))
				.collect(),
			symbols_by_identity: index
				.symbols
				.iter()
				.map(|symbol| (symbol.identity.as_ref(), symbol))
				.collect(),
			references: index
				.references
				.iter()
				.map(|reference| (reference.id, reference))
				.collect(),
			reference_rows: index.references,
			linkage: index.linkage,
		}
	}

	fn find_symbol(&self, uri: &str) -> Option<&'a SymbolRecord> {
		self.symbols_by_identity.get(uri).copied().or_else(|| {
			let id = SymbolId::parse(uri)?;
			self.symbols.get(&id).copied()
		})
	}

	fn source(&self, id: &SourceId) -> Option<&'a SourceFileRecord> {
		self.sources.get(id).copied()
	}

	fn reference(&self, id: &ReferenceId) -> Option<&'a ReferenceRecord> {
		self.references.get(id).copied()
	}

	fn resolved_target(&self, reference: &ReferenceId) -> Option<SymbolId> {
		self.linkage
			.resolved
			.iter()
			.find(|edge| &edge.reference == reference)
			.map(|edge| edge.target)
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

	fn symbol(&self, id: &SymbolId) -> Option<&'a SymbolRecord> {
		self.symbols.get(id).copied()
	}
}
