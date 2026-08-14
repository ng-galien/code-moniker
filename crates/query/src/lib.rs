use std::collections::BTreeMap;
use std::fmt::{self, Write};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

mod discovery;
pub use discovery::*;
mod bounded;
pub use bounded::*;

#[cfg(feature = "rpc")]
pub mod rpc {
	use jsonrpsee::core::SubscriptionResult;
	use jsonrpsee::proc_macros::rpc;
	use jsonrpsee::types::ErrorObjectOwned;

	use crate::{
		CommandRequest, CommandResponse, HandshakeResponse, QueryRequest, QueryResponse,
		WorkspaceEventDto,
	};

	pub const RPC_NAMESPACE: &str = "moniker";

	#[rpc(server, client, namespace = "moniker")]
	pub trait DaemonRpc {
		#[method(name = "handshake")]
		async fn handshake(&self, client: String) -> Result<HandshakeResponse, ErrorObjectOwned>;

		#[method(name = "query")]
		async fn query(&self, request: QueryRequest) -> Result<QueryResponse, ErrorObjectOwned>;

		#[method(name = "command")]
		async fn command(
			&self,
			request: CommandRequest,
		) -> Result<CommandResponse, ErrorObjectOwned>;

		#[method(name = "shutdown")]
		async fn shutdown(&self) -> Result<(), ErrorObjectOwned>;

		#[subscription(name = "subscribeEvents" => "events", unsubscribe = "unsubscribeEvents", item = WorkspaceEventDto)]
		async fn subscribe_events(&self) -> SubscriptionResult;
	}
}

#[cfg(feature = "rpc")]
pub use rpc::*;

pub const PROTOCOL_VERSION: u32 = 17;
pub const SYNTAX_TREE_DEFAULT_MAX_DEPTH: usize = 6;
pub const SYNTAX_TREE_DEFAULT_MAX_NODES: usize = 100;
pub const SYNTAX_TREE_DEFAULT_MAX_TEXT_CHARS: usize = 80;
pub const SYNTAX_TREE_MAX_DEPTH: usize = 32;
pub const SYNTAX_TREE_MAX_NODES: usize = 2_000;
pub const SYNTAX_TREE_MAX_TEXT_CHARS: usize = 1_000;
pub const SYNTAX_PARSE_MAX_SOURCE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProtocolRequest {
	Query(Box<QueryRequest>),
	Command(CommandRequest),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProtocolResponse {
	Query(Box<QueryResponse>),
	Command(CommandResponse),
	Error(QueryError),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HandshakeResponse {
	pub protocol_version: u32,
	pub daemon_version: String,
	#[serde(default)]
	pub build: BuildIdentity,
	pub workspace_root: String,
	pub workspace_roots: Vec<String>,
	pub capabilities: CapabilitySet,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DaemonWorkspaceConfig {
	pub roots: Vec<String>,
	pub project: Option<String>,
	pub cache_dir: Option<String>,
	pub live_refresh: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CapabilitySet {
	pub queries: Vec<String>,
	#[serde(default)]
	pub query_mcp_tools: BTreeMap<String, String>,
	pub commands: Vec<String>,
	pub events: Vec<String>,
}

impl Default for CapabilitySet {
	fn default() -> Self {
		let mut queries: Vec<String> = query_capability_specs()
			.iter()
			.map(|spec| spec.name.to_string())
			.collect();
		queries.push("diff-impact.compare".to_string());
		Self {
			queries,
			query_mcp_tools: query_capability_specs()
				.iter()
				.map(|spec| (spec.name.to_string(), spec.mcp_tool.to_string()))
				.collect(),
			commands: [
				"workspace.refresh",
				"workspace.source_set.replace",
				"workspace.source_set.remove",
			]
			.into_iter()
			.map(str::to_string)
			.collect(),
			events: Vec::new(),
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueryCapabilitySpec {
	pub name: &'static str,
	pub category: &'static str,
	pub read_only: bool,
	pub mcp_tool: &'static str,
	pub fields: &'static [&'static str],
	pub required_fields: &'static [&'static str],
	pub positionals: usize,
	pub projection: bool,
	pub paginated: bool,
	pub example: &'static str,
}

const COMMON_FIELDS: &[&str] = &["limit", "cursor", "consistency"];
const BRACKET_LIST_FIELDS: &[&str] = &["lang", "kind", "shape", "severity", "relation"];
const MULTI_VALUE_FIELDS: &[&str] = &[
	"path", "lang", "kind", "shape", "severity", "file", "relation",
];

const QUERY_CAPABILITY_SPECS: &[QueryCapabilitySpec] = &[
	QueryCapabilitySpec {
		name: "query.describe",
		category: "discovery",
		read_only: true,
		mcp_tool: "code_moniker_query",
		fields: &["verb"],
		required_fields: &[],
		positionals: 1,
		projection: false,
		paginated: false,
		example: "query.describe verb:\"symbol.usages\"",
	},
	QueryCapabilitySpec {
		name: "workspace.status",
		category: "workspace",
		read_only: true,
		mcp_tool: "code_moniker_read",
		fields: &[],
		required_fields: &[],
		positionals: 0,
		projection: false,
		paginated: false,
		example: "workspace.status",
	},
	QueryCapabilitySpec {
		name: "tree.children",
		category: "navigation",
		read_only: true,
		mcp_tool: "code_moniker_read",
		fields: &["workspace", "path", "depth", "lang"],
		required_fields: &[],
		positionals: 0,
		projection: true,
		paginated: true,
		example: "tree.children path:\"src/**\" depth:2 limit:20",
	},
	QueryCapabilitySpec {
		name: "symbol.search",
		category: "symbol",
		read_only: true,
		mcp_tool: "code_moniker_symbols",
		fields: &[
			"workspace",
			"path",
			"lang",
			"kind",
			"shape",
			"name",
			"include_non_navigable",
			"include_code",
			"context_lines",
		],
		required_fields: &[],
		positionals: 1,
		projection: true,
		paginated: true,
		example: "symbol.search name:\"PaymentService\" shape:type limit:10",
	},
	QueryCapabilitySpec {
		name: "symbol.insights",
		category: "symbol",
		read_only: true,
		mcp_tool: "code_moniker_symbols",
		fields: &[
			"workspace",
			"path",
			"lang",
			"kind",
			"shape",
			"name",
			"include_non_navigable",
		],
		required_fields: &[],
		positionals: 0,
		projection: true,
		paginated: false,
		example: "symbol.insights path:\"src/**\"",
	},
	QueryCapabilitySpec {
		name: "symbol.detail",
		category: "symbol",
		read_only: true,
		mcp_tool: "code_moniker_read",
		fields: &["workspace", "uri", "context_lines"],
		required_fields: &["uri"],
		positionals: 1,
		projection: false,
		paginated: false,
		example: "symbol.detail uri:\"code+moniker://...\" context_lines:2",
	},
	QueryCapabilitySpec {
		name: "syntax.tree",
		category: "syntax",
		read_only: true,
		mcp_tool: "code_moniker_read",
		fields: &[
			"workspace",
			"focus",
			"max_depth",
			"max_nodes",
			"named_only",
			"include_text",
			"max_text_chars",
		],
		required_fields: &["focus"],
		positionals: 1,
		projection: false,
		paginated: false,
		example: "syntax.tree focus:\"src/service.ts\" max_depth:6 max_nodes:100",
	},
	QueryCapabilitySpec {
		name: "syntax.parse",
		category: "syntax",
		read_only: true,
		mcp_tool: "code_moniker_read",
		fields: &[
			"language",
			"source",
			"uri",
			"max_depth",
			"max_nodes",
			"named_only",
			"include_text",
			"max_text_chars",
		],
		required_fields: &["language", "source"],
		positionals: 0,
		projection: false,
		paginated: false,
		example: "syntax.parse language:\"rs\" source:\"fn main() {}\"",
	},
	QueryCapabilitySpec {
		name: "symbol.usages",
		category: "symbol",
		read_only: true,
		mcp_tool: "code_moniker_usages",
		fields: &[
			"workspace",
			"uri",
			"direction",
			"path",
			"lang",
			"include_descendants",
		],
		required_fields: &["uri"],
		positionals: 1,
		projection: true,
		paginated: true,
		example: "symbol.usages uri:\"code+moniker://...\" direction:incoming limit:20",
	},
	QueryCapabilitySpec {
		name: "view.read",
		category: "context",
		read_only: true,
		mcp_tool: "code_moniker_read",
		fields: &["uri", "scheme", "context_lines", "include_code"],
		required_fields: &["uri"],
		positionals: 1,
		projection: false,
		paginated: false,
		example: "view.read uri:\"workspace/views\"",
	},
	QueryCapabilitySpec {
		name: "rules.list",
		category: "rules",
		read_only: true,
		mcp_tool: "code_moniker_rules",
		fields: &["workspace", "profile", "rules", "lang", "severity"],
		required_fields: &[],
		positionals: 0,
		projection: false,
		paginated: true,
		example: "rules.list profile:agent limit:20",
	},
	QueryCapabilitySpec {
		name: "rules.check",
		category: "rules",
		read_only: true,
		mcp_tool: "code_moniker_rules",
		fields: &["workspace", "profile", "rules", "file", "report"],
		required_fields: &[],
		positionals: 0,
		projection: false,
		paginated: true,
		example: "rules.check profile:agent file:\"src/**\" limit:20",
	},
	QueryCapabilitySpec {
		name: "rules.applicable",
		category: "rules",
		read_only: true,
		mcp_tool: "code_moniker_query",
		fields: &["workspace", "focus", "profile", "rules"],
		required_fields: &["focus"],
		positionals: 1,
		projection: false,
		paginated: true,
		example: "rules.applicable focus:\"code+moniker://...\" profile:agent limit:20",
	},
	QueryCapabilitySpec {
		name: "change.review",
		category: "change",
		read_only: true,
		mcp_tool: "code_moniker_diff",
		fields: &["workspace"],
		required_fields: &[],
		positionals: 0,
		projection: false,
		paginated: false,
		example: "change.review",
	},
	QueryCapabilitySpec {
		name: "change.context",
		category: "change",
		read_only: true,
		mcp_tool: "code_moniker_context",
		fields: &["workspace", "focus", "profile", "max_items"],
		required_fields: &["focus"],
		positionals: 1,
		projection: false,
		paginated: false,
		example: "change.context focus:\"code+moniker://...\" profile:agent max_items:20",
	},
	QueryCapabilitySpec {
		name: "symbol.graph",
		category: "graph",
		read_only: true,
		mcp_tool: "code_moniker_graph",
		fields: &[
			"workspace",
			"focus",
			"direction",
			"relation",
			"min_count",
			"include_internal",
		],
		required_fields: &["focus"],
		positionals: 1,
		projection: false,
		paginated: false,
		example: "symbol.graph focus:\"src/service.ts\"",
	},
	QueryCapabilitySpec {
		name: "graph.path",
		category: "graph",
		read_only: true,
		mcp_tool: "code_moniker_query",
		fields: &[
			"workspace",
			"from",
			"to",
			"expect",
			"relation",
			"max_depth",
			"max_symbols",
			"max_edges",
			"min_coverage",
		],
		required_fields: &["from", "to"],
		positionals: 0,
		projection: false,
		paginated: false,
		example: "graph.path from:\"code+moniker://...\" to:\"code+moniker://...\" expect:no_path",
	},
	QueryCapabilitySpec {
		name: "identity.children",
		category: "graph",
		read_only: true,
		mcp_tool: "code_moniker_query",
		fields: &["workspace", "prefix"],
		required_fields: &[],
		positionals: 1,
		projection: false,
		paginated: false,
		example: "identity.children prefix:\"lang:rs/dir:crates\"",
	},
	QueryCapabilitySpec {
		name: "identity.graph",
		category: "graph",
		read_only: true,
		mcp_tool: "code_moniker_query",
		fields: &["workspace", "prefix", "path", "min_count"],
		required_fields: &[],
		positionals: 1,
		projection: false,
		paginated: true,
		example: "identity.graph prefix:\"lang:rs/dir:crates\"",
	},
	QueryCapabilitySpec {
		name: "metrics.coupling",
		category: "metrics",
		read_only: true,
		mcp_tool: "code_moniker_query",
		fields: &["workspace", "from", "to", "relation", "snapshot", "export"],
		required_fields: &["from", "to"],
		positionals: 0,
		projection: false,
		paginated: false,
		example: "metrics.coupling from:\"lang:rs/dir:crates/dir:check\" to:\"lang:rs/dir:crates/dir:workspace\"",
	},
	QueryCapabilitySpec {
		name: "resolution.audit",
		category: "diagnostic",
		read_only: true,
		mcp_tool: "code_moniker_query",
		fields: &["workspace", "prefix", "cluster"],
		required_fields: &[],
		positionals: 1,
		projection: false,
		paginated: true,
		example: "resolution.audit prefix:\"lang:java\" limit:20",
	},
	QueryCapabilitySpec {
		name: "notes",
		category: "notes",
		read_only: false,
		mcp_tool: "code_moniker_notes",
		fields: &[
			"action",
			"id",
			"moniker",
			"kind",
			"status",
			"title",
			"body",
			"created_by",
			"orphan",
			"include_done",
		],
		required_fields: &[],
		positionals: 0,
		projection: false,
		paginated: true,
		example: "notes action:list limit:20",
	},
];

pub fn query_capability_specs() -> &'static [QueryCapabilitySpec] {
	QUERY_CAPABILITY_SPECS
}

pub fn query_capability_spec(name: &str) -> Option<&'static QueryCapabilitySpec> {
	QUERY_CAPABILITY_SPECS.iter().find(|spec| spec.name == name)
}

pub fn query_projection_fields(name: &str) -> &'static [&'static str] {
	match name {
		"tree.children" => &[
			"root",
			"path",
			"kind",
			"language",
			"defs",
			"refs",
			"change_count",
		],
		"symbol.search" => &[
			"root",
			"uri",
			"id",
			"name",
			"kind",
			"visibility",
			"signature",
			"file",
			"language",
			"line_range",
			"navigable",
			"score",
			"match_reason",
			"source",
		],
		"symbol.insights" => &[
			"files",
			"symbols",
			"references",
			"navigable_symbols",
			"non_navigable_symbols",
			"languages",
			"kinds",
			"shapes",
			"top_files_by_symbols",
			"top_files_by_refs",
		],
		"symbol.usages" => &[
			"root",
			"direction",
			"reference",
			"kind",
			"actor",
			"context",
			"endpoint",
			"file",
			"prefix",
			"location",
			"line_range",
			"via",
		],
		_ => &[],
	}
}

pub fn describe_query_capabilities(verb: Option<&str>) -> Option<QueryDescribeResult> {
	let specs: Vec<&QueryCapabilitySpec> = match verb {
		Some(name) => vec![query_capability_spec(name)?],
		None => QUERY_CAPABILITY_SPECS.iter().collect(),
	};
	Some(QueryDescribeResult {
		capabilities: specs.into_iter().map(query_capability_dto).collect(),
	})
}

fn query_capability_dto(spec: &QueryCapabilitySpec) -> QueryCapabilityDto {
	let fields = spec
		.fields
		.iter()
		.chain(COMMON_FIELDS)
		.map(|name| QueryFieldDto {
			name: (*name).to_string(),
			value_type: query_field_type(name).to_string(),
			multiple: MULTI_VALUE_FIELDS.contains(name),
			required: spec.required_fields.contains(name),
			default: query_field_default(spec.name, name).map(ToOwned::to_owned),
		})
		.collect();
	QueryCapabilityDto {
		name: spec.name.to_string(),
		category: spec.category.to_string(),
		read_only: spec.read_only,
		mcp_tool: spec.mcp_tool.to_string(),
		projection: spec.projection,
		projection_fields: query_projection_fields(spec.name)
			.iter()
			.map(|field| (*field).to_string())
			.collect(),
		paginated: spec.paginated,
		positionals: spec.positionals,
		fields,
		example: spec.example.to_string(),
	}
}

fn query_field_type(name: &str) -> &'static str {
	match name {
		"limit" | "depth" | "context_lines" | "max_items" | "min_count" | "max_depth"
		| "max_nodes" | "max_text_chars" | "max_symbols" | "max_edges" | "min_coverage" => {
			"unsigned_integer"
		}
		"include_non_navigable"
		| "include_code"
		| "include_descendants"
		| "include_internal"
		| "include_text"
		| "named_only"
		| "report"
		| "orphan"
		| "include_done" => "boolean",
		"direction" => "enum:incoming|outgoing|both",
		"expect" => "enum:reachable|no_path",
		"consistency" => "enum:current|refresh-if-stale|stale-ok",
		"action" => "enum:list|get|create|update|transition|delete",
		"cursor" => "cursor",
		name if MULTI_VALUE_FIELDS.contains(&name) => "string_list",
		_ => "string",
	}
}

fn query_field_default(verb: &str, name: &str) -> Option<&'static str> {
	match (verb, name) {
		("resolution.audit", "limit") => Some("20"),
		(_, "limit") => Some("80"),
		(_, "consistency") => Some("current"),
		("tree.children", "depth") => Some("1"),
		("symbol.detail" | "view.read", "context_lines") => Some("2"),
		("syntax.tree" | "syntax.parse", "max_depth") => Some("6"),
		("syntax.tree" | "syntax.parse", "max_nodes") => Some("100"),
		("syntax.tree" | "syntax.parse", "named_only") => Some("true"),
		("syntax.tree" | "syntax.parse", "include_text") => Some("false"),
		("syntax.tree" | "syntax.parse", "max_text_chars") => Some("80"),
		("symbol.search", "context_lines") => Some("0"),
		("symbol.usages", "direction") => Some("incoming"),
		("symbol.graph", "direction") => Some("both"),
		("symbol.graph", "min_count") => Some("1"),
		("symbol.graph", "include_internal") => Some("true"),
		("graph.path", "expect") => Some("reachable"),
		("graph.path", "relation") => Some("calls,method_call"),
		("graph.path", "max_depth") => Some("12"),
		("graph.path", "max_symbols") => Some("10000"),
		("graph.path", "max_edges") => Some("50000"),
		("graph.path", "min_coverage") => Some("100"),
		("rules.check", "report") => Some("true"),
		("change.context", "max_items") => Some("20"),
		("notes", "action") => Some("list"),
		(_, "include_non_navigable" | "include_code" | "include_descendants" | "include_done") => {
			Some("false")
		}
		_ => None,
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QueryRequest {
	pub query: Query,
	pub consistency: Consistency,
	pub page: Page,
}

impl QueryRequest {
	pub fn new(query: Query) -> Self {
		Self {
			query,
			consistency: Consistency::Current,
			page: Page::default(),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Query {
	QueryDescribe(QueryDescribeQuery),
	WorkspaceStatus,
	TreeChildren(TreeChildrenQuery),
	SymbolSearch(SymbolSearchQuery),
	SymbolInsights(SymbolSearchQuery),
	SymbolDetail(SymbolDetailQuery),
	SyntaxTree(SyntaxTreeQuery),
	SyntaxParse(SyntaxParseQuery),
	SymbolUsages(SymbolUsagesQuery),
	ViewRead(ViewReadQuery),
	RulesList(RulesListQuery),
	RulesCheck(RulesCheckQuery),
	RulesApplicable(RulesApplicableQuery),
	ChangeReview(ChangeReviewQuery),
	DiffImpactCompare(DiffImpactCompareQuery),
	ChangeContext(ChangeContextQuery),
	SymbolGraph(SymbolGraphQuery),
	GraphPath(GraphPathQuery),
	IdentityChildren(IdentityChildrenQuery),
	IdentityGraph(IdentityGraphQuery),
	MetricsCoupling(MetricsCouplingQuery),
	ResolutionAudit(ResolutionAuditQuery),
	Notes(NotesQuery),
}

impl Query {
	pub fn capability(&self) -> &'static str {
		match self {
			Self::QueryDescribe(_) => "query.describe",
			Self::WorkspaceStatus => "workspace.status",
			Self::TreeChildren(_) => "tree.children",
			Self::SymbolSearch(_) => "symbol.search",
			Self::SymbolInsights(_) => "symbol.insights",
			Self::SymbolDetail(_) => "symbol.detail",
			Self::SyntaxTree(_) => "syntax.tree",
			Self::SyntaxParse(_) => "syntax.parse",
			Self::SymbolUsages(_) => "symbol.usages",
			Self::ViewRead(_) => "view.read",
			Self::RulesList(_) => "rules.list",
			Self::RulesCheck(_) => "rules.check",
			Self::RulesApplicable(_) => "rules.applicable",
			Self::ChangeReview(_) => "change.review",
			Self::DiffImpactCompare(_) => "diff-impact.compare",
			Self::ChangeContext(_) => "change.context",
			Self::SymbolGraph(_) => "symbol.graph",
			Self::GraphPath(_) => "graph.path",
			Self::IdentityChildren(_) => "identity.children",
			Self::IdentityGraph(_) => "identity.graph",
			Self::MetricsCoupling(_) => "metrics.coupling",
			Self::ResolutionAudit(_) => "resolution.audit",
			Self::Notes(_) => "notes",
		}
	}

	pub fn requires_workspace_snapshot(&self) -> bool {
		!matches!(
			self,
			Self::QueryDescribe(_)
				| Self::WorkspaceStatus
				| Self::SyntaxParse(_)
				| Self::DiffImpactCompare(_)
		)
	}
}

pub fn query_projection(query: &Query) -> &[String] {
	match query {
		Query::TreeChildren(query) => &query.projection,
		Query::SymbolSearch(query) | Query::SymbolInsights(query) => &query.projection,
		Query::SymbolUsages(query) => &query.projection,
		_ => &[],
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QueryDescribeQuery {
	pub verb: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QueryCapabilityDto {
	pub name: String,
	pub category: String,
	pub read_only: bool,
	pub mcp_tool: String,
	pub projection: bool,
	pub projection_fields: Vec<String>,
	pub paginated: bool,
	pub positionals: usize,
	pub fields: Vec<QueryFieldDto>,
	pub example: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QueryFieldDto {
	pub name: String,
	pub value_type: String,
	pub multiple: bool,
	pub required: bool,
	pub default: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QueryDescribeResult {
	pub capabilities: Vec<QueryCapabilityDto>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TreeChildrenQuery {
	pub workspace: Option<String>,
	pub path: Vec<String>,
	pub depth: usize,
	pub lang: Vec<String>,
	pub projection: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolSearchQuery {
	pub workspace: Option<String>,
	pub text: Option<String>,
	pub path: Vec<String>,
	pub lang: Vec<String>,
	pub kind: Vec<String>,
	pub shape: Vec<String>,
	pub name: Option<String>,
	pub include_non_navigable: bool,
	pub include_code: bool,
	pub context_lines: usize,
	pub projection: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolDetailQuery {
	pub workspace: Option<String>,
	pub uri: String,
	pub context_lines: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SyntaxTreeQuery {
	pub workspace: Option<String>,
	pub focus: String,
	#[cfg_attr(feature = "schema", schemars(range(min = 0, max = 32)))]
	pub max_depth: usize,
	#[cfg_attr(feature = "schema", schemars(range(min = 1, max = 2000)))]
	pub max_nodes: usize,
	pub named_only: bool,
	pub include_text: bool,
	#[cfg_attr(feature = "schema", schemars(range(min = 0, max = 1000)))]
	pub max_text_chars: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SyntaxParseQuery {
	pub language: String,
	pub source: String,
	pub uri: Option<String>,
	#[cfg_attr(feature = "schema", schemars(range(min = 0, max = 32)))]
	pub max_depth: usize,
	#[cfg_attr(feature = "schema", schemars(range(min = 1, max = 2000)))]
	pub max_nodes: usize,
	pub named_only: bool,
	pub include_text: bool,
	#[cfg_attr(feature = "schema", schemars(range(min = 0, max = 1000)))]
	pub max_text_chars: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolUsagesQuery {
	pub workspace: Option<String>,
	pub uri: String,
	pub direction: UsageDirection,
	pub path: Vec<String>,
	pub lang: Vec<String>,
	pub include_descendants: bool,
	pub projection: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ViewReadQuery {
	pub uri: String,
	pub scheme: Option<String>,
	pub context_lines: usize,
	pub include_code: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ResolutionAuditQuery {
	pub workspace: Option<String>,
	pub prefix: String,
	pub limit: usize,
	pub cluster: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum UsageDirection {
	#[default]
	Incoming,
	Outgoing,
	Both,
}

impl UsageDirection {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Incoming => "incoming",
			Self::Outgoing => "outgoing",
			Self::Both => "both",
		}
	}
}

impl FromStr for UsageDirection {
	type Err = QueryParseError;

	fn from_str(value: &str) -> Result<Self, Self::Err> {
		match value {
			"incoming" => Ok(Self::Incoming),
			"outgoing" => Ok(Self::Outgoing),
			"both" => Ok(Self::Both),
			_ => Err(QueryParseError::InvalidValue {
				key: "direction".to_string(),
				value: value.to_string(),
			}),
		}
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RulesListQuery {
	pub workspace: Option<String>,
	pub profile: Option<String>,
	pub rules: Option<String>,
	pub lang: Vec<String>,
	pub severity: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RulesCheckQuery {
	pub workspace: Option<String>,
	pub profile: Option<String>,
	pub rules: Option<String>,
	pub file: Vec<String>,
	pub report: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RulesApplicableQuery {
	pub workspace: Option<String>,
	pub focus: String,
	pub profile: Option<String>,
	pub rules: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ChangeReviewQuery {
	pub workspace: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DiffImpactCompareQuery {
	pub scope: String,
	pub project: Option<String>,
	pub base: WorkspaceSourceSetDto,
	pub head: WorkspaceSourceSetDto,
	pub files: Vec<DiffImpactCompareFile>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DiffImpactCompareFile {
	pub status: DiffImpactFileStatus,
	pub old_uri: Option<String>,
	pub new_uri: Option<String>,
	#[serde(default)]
	pub old_hunks: Vec<DiffImpactLineSpan>,
	#[serde(default)]
	pub new_hunks: Vec<DiffImpactLineSpan>,
	pub rename_score: Option<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DiffImpactFileStatus {
	Added,
	Modified,
	Deleted,
	Renamed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DiffImpactLineSpan {
	pub start: u32,
	pub end: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ChangeContextQuery {
	pub workspace: Option<String>,
	pub focus: String,
	pub profile: Option<String>,
	pub max_items: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolGraphQuery {
	pub workspace: Option<String>,
	pub focus: String,
	pub direction: UsageDirection,
	pub relation: Vec<String>,
	pub min_count: usize,
	pub include_internal: bool,
}

impl Default for SymbolGraphQuery {
	fn default() -> Self {
		Self {
			workspace: None,
			focus: String::new(),
			direction: UsageDirection::Both,
			relation: Vec::new(),
			min_count: 1,
			include_internal: true,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GraphPathQuery {
	pub workspace: Option<String>,
	pub from: String,
	pub to: String,
	pub expect: GraphPathExpectation,
	pub relation: Vec<String>,
	pub max_depth: usize,
	pub max_symbols: usize,
	pub max_edges: usize,
	pub min_coverage: usize,
}

impl Default for GraphPathQuery {
	fn default() -> Self {
		Self {
			workspace: None,
			from: String::new(),
			to: String::new(),
			expect: GraphPathExpectation::Reachable,
			relation: vec!["calls".to_string(), "method_call".to_string()],
			max_depth: 12,
			max_symbols: 10_000,
			max_edges: 50_000,
			min_coverage: 100,
		}
	}
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum GraphPathExpectation {
	#[default]
	Reachable,
	NoPath,
}

impl GraphPathExpectation {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Reachable => "reachable",
			Self::NoPath => "no_path",
		}
	}
}

impl FromStr for GraphPathExpectation {
	type Err = QueryParseError;

	fn from_str(value: &str) -> Result<Self, Self::Err> {
		match value {
			"reachable" => Ok(Self::Reachable),
			"no_path" => Ok(Self::NoPath),
			_ => Err(QueryParseError::InvalidValue {
				key: "expect".to_string(),
				value: value.to_string(),
			}),
		}
	}
}

// One level of the identity tree: children of a moniker identity prefix
// (`""` = the workspace root). The symbolic navigation surface - no
// filesystem involved.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct IdentityChildrenQuery {
	pub workspace: Option<String>,
	pub prefix: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct IdentityGraphQuery {
	pub workspace: Option<String>,
	pub prefix: String,
	pub path: Vec<String>,
	pub min_count: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MetricsCouplingQuery {
	pub workspace: Option<String>,
	pub from: String,
	pub to: String,
	pub relation: Vec<String>,
	pub snapshot: Option<String>,
	pub export: bool,
}

impl Default for IdentityGraphQuery {
	fn default() -> Self {
		Self {
			workspace: None,
			prefix: String::new(),
			path: Vec::new(),
			min_count: 1,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NotesQuery {
	pub action: NotesAction,
	pub id: Option<String>,
	pub moniker: Option<String>,
	pub kind: Option<String>,
	pub status: Option<String>,
	pub title: Option<String>,
	pub body: Option<String>,
	pub created_by: Option<String>,
	pub orphan: Option<bool>,
	pub include_done: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum NotesAction {
	#[default]
	List,
	Get,
	Create,
	Update,
	Transition,
	Delete,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CommandRequest {
	pub command: Command,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorkspaceSourceSetDto {
	pub srcset: String,
	pub revision: Option<String>,
	pub documents: Vec<WorkspaceSourceDocumentDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorkspaceSourceDocumentDto {
	pub uri: String,
	pub language: String,
	pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Command {
	WorkspaceRefresh,
	WorkspaceSourceSetReplace { source_set: WorkspaceSourceSetDto },
	WorkspaceSourceSetRemove { srcset: String },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Consistency {
	#[default]
	Current,
	RefreshIfStale,
	StaleOk,
}

impl FromStr for Consistency {
	type Err = QueryParseError;

	fn from_str(value: &str) -> Result<Self, Self::Err> {
		match value {
			"current" => Ok(Self::Current),
			"refresh-if-stale" => Ok(Self::RefreshIfStale),
			"stale-ok" => Ok(Self::StaleOk),
			_ => Err(QueryParseError::InvalidValue {
				key: "consistency".to_string(),
				value: value.to_string(),
			}),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Page {
	pub cursor: Option<QueryCursor>,
	pub limit: usize,
}

impl Default for Page {
	fn default() -> Self {
		Self {
			cursor: None,
			limit: 80,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QueryCursor {
	pub offset: usize,
	pub generation: Option<WorkspaceGeneration>,
}

impl QueryCursor {
	pub fn new(offset: usize, generation: Option<WorkspaceGeneration>) -> Self {
		Self { offset, generation }
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorkspaceGeneration(pub u64);

/// A workspace change pushed to attached clients over a daemon subscription.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorkspaceEventDto {
	pub kind: WorkspaceEventKind,
	pub generation: Option<WorkspaceGeneration>,
	pub stale_summary: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceEventKind {
	Stale,
	Refreshed,
	Failed,
	Notes,
	GitBase,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QueryResponse {
	pub generation: Option<WorkspaceGeneration>,
	pub result: QueryResult,
	pub next_cursor: Option<QueryCursor>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum QueryResult {
	QueryDescribe(QueryDescribeResult),
	WorkspaceStatus(WorkspaceStatus),
	TreeChildren(TreeChildrenResult),
	SymbolList(SymbolListResult),
	SymbolInsights(SymbolInsightsResult),
	SymbolDetail(SymbolDetailResult),
	SyntaxTree(SyntaxTreeResult),
	SymbolUsages(Box<SymbolUsagesResult>),
	ViewRead(ViewReadResult),
	RulesList(RulesListResult),
	RulesCheck(RulesCheckResult),
	RulesApplicable(Box<RulesApplicableResult>),
	ChangeReview(Box<ChangeReviewResult>),
	DiffImpact(Box<DiffImpactResult>),
	ChangeContext(Box<ChangeContextResult>),
	SymbolGraph(Box<SymbolGraphResult>),
	GraphPath(Box<GraphPathResult>),
	IdentityChildren(IdentityChildrenResult),
	IdentityGraph(Box<IdentityGraphResult>),
	MetricsCoupling(Box<MetricsCouplingResult>),
	ResolutionAudit(Box<ResolutionAuditResult>),
	Notes(NotesResult),
}

// Refs without a unique in-workspace target, decomposed so explained decisions
// never masquerade as resolution gaps. Candidate and dynamic references remain
// outside the graph while preserving their honest classification.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnlinkedRefsDto {
	pub external: usize,
	pub sdk: usize,
	pub dependency: usize,
	pub injected_external: usize,
	pub unknown_external: usize,
	pub candidate: usize,
	pub dynamic: usize,
	pub manifest_blocked: usize,
	pub unresolved: usize,
	pub unresolved_reasons: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolGraphResult {
	pub focus: SymbolGraphFocus,
	pub coverage: SymbolGraphCoverage,
	pub members: Vec<SymbolDto>,
	pub internal_edges: Vec<SymbolGraphEdge>,
	pub callers: Vec<SymbolGraphNeighbor>,
	pub callees: Vec<SymbolGraphNeighbor>,
	pub unlinked: UnlinkedRefsDto,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolGraphCoverage {
	pub members: GraphSectionCoverage,
	pub internal_edges: GraphSectionCoverage,
	pub callers: GraphSectionCoverage,
	pub callees: GraphSectionCoverage,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GraphSectionCoverage {
	pub total: usize,
	pub matching: usize,
	pub returned: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SymbolGraphFocus {
	Symbol { symbol: Box<SymbolDto> },
	File { path: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolGraphNeighbor {
	pub symbol: SymbolDto,
	pub kinds: Vec<String>,
	pub count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolGraphEdge {
	pub source: String,
	pub target: String,
	pub kinds: Vec<String>,
	pub count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GraphPathResult {
	pub from: SymbolDto,
	pub to: SymbolDto,
	pub expectation: GraphPathExpectation,
	pub verdict: GraphPathVerdict,
	pub reachable: Option<bool>,
	pub no_path: Option<bool>,
	pub path: Vec<GraphPathStep>,
	pub coverage: GraphPathCoverage,
	pub search: GraphPathSearchStats,
	pub reasons: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum GraphPathVerdict {
	Pass,
	Fail,
	Inconclusive,
}

impl GraphPathVerdict {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Pass => "pass",
			Self::Fail => "fail",
			Self::Inconclusive => "inconclusive",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GraphPathStep {
	pub source: SymbolDto,
	pub target: SymbolDto,
	pub relation: String,
	pub reference: String,
	pub file: String,
	pub line_range: Option<(u32, u32)>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GraphPathCoverage {
	pub total: usize,
	pub decided: usize,
	pub resolved: usize,
	pub external: usize,
	pub candidate: usize,
	pub dynamic: usize,
	pub manifest_blocked: usize,
	pub unresolved: usize,
	pub percent: usize,
	pub gap_reasons: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GraphPathSearchStats {
	pub max_depth: usize,
	pub depth_reached: usize,
	pub explored_symbols: usize,
	pub explored_edges: usize,
	pub depth_limit_reached: bool,
	pub symbol_limit_reached: bool,
	pub edge_limit_reached: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct IdentityChildrenResult {
	pub prefix: String,
	pub children: Vec<IdentitySegmentDto>,
}

// One child segment under the requested prefix. `symbol` is attached when the
// segment itself is a navigable definition; organizational segments (package,
// dir, srcset, lang, module wrappers) only aggregate what lives below.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct IdentitySegmentDto {
	pub segment: String,
	pub kind: String,
	pub name: String,
	pub identity: String,
	pub defs: usize,
	pub has_children: bool,
	pub symbol: Option<Box<SymbolDto>>,
}

// The embedded resolution audit partitions every reference by decision class
// and clusters candidates, dynamic references, and unresolved references under
// mechanical pattern keys. Stable cluster ids support paginated drill-downs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ResolutionAuditResult {
	pub prefix: String,
	pub totals: AuditTotalsDto,
	pub clusters: Vec<AuditClusterDto>,
	pub zones: Vec<AuditZoneDto>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AuditTotalsDto {
	pub references: usize,
	pub resolved: usize,
	pub unique: usize,
	pub candidate: usize,
	pub external: usize,
	pub sdk: usize,
	pub dependency: usize,
	pub injected_external: usize,
	pub unknown_external: usize,
	pub dynamic: usize,
	pub blocked: usize,
	pub unresolved: usize,
	pub explained: usize,
	pub weak_or_unexplained: usize,
	pub name_match_resolved: usize,
	pub name_match_candidate: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AuditClusterDto {
	pub id: String,
	pub pattern: String,
	pub count: usize,
	pub samples: Vec<AuditSampleDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AuditSampleDto {
	pub file: String,
	pub line_range: Option<(u32, u32)>,
	pub snippet: String,
	pub source: String,
	pub call_name: String,
	pub receiver: String,
	pub target: String,
	pub evidence: String,
	pub constraints: Vec<String>,
	pub candidates: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AuditZoneDto {
	pub zone: String,
	pub unresolved: usize,
	pub dominant_pattern: String,
}

// The scoped exploration graph: one level of the identity tree projected as
// a graph. Nodes are the prefix's children; edges are resolved references
// rolled up to the pair of child segments they connect; ports aggregate what
// crosses the scope boundary.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct IdentityGraphResult {
	pub prefix: String,
	pub path: Vec<String>,
	pub min_count: usize,
	pub coverage: IdentityGraphCoverage,
	pub nodes: Vec<IdentitySegmentDto>,
	pub edges: Vec<IdentityGraphEdge>,
	pub ports_in: Vec<IdentityGraphPort>,
	pub ports_out: Vec<IdentityGraphPort>,
	pub unlinked: UnlinkedRefsDto,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct IdentityGraphCoverage {
	pub rows_total: usize,
	pub rows_matching: usize,
	pub rows_emitted: usize,
	pub nodes_total: usize,
	pub nodes_emitted: usize,
	pub edges_total: usize,
	pub edges_matching: usize,
	pub edges_emitted: usize,
	pub ports_in_total: usize,
	pub ports_in_matching: usize,
	pub ports_in_emitted: usize,
	pub ports_out_total: usize,
	pub ports_out_matching: usize,
	pub ports_out_emitted: usize,
}

// source/target are child segment identities of the requested prefix.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct IdentityGraphEdge {
	pub source: String,
	pub target: String,
	pub kinds: Vec<String>,
	pub count: usize,
}

// Aggregated boundary crossing: `identity` is the nearest out-of-scope
// segment (rolled up to the scope's own depth in the identity tree).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct IdentityGraphPort {
	pub identity: String,
	pub kinds: Vec<String>,
	pub count: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MetricsCouplingResult {
	pub from: String,
	pub to: String,
	pub relation: Vec<String>,
	pub snapshot: String,
	pub git: Option<GitRevisionDto>,
	pub export_requested: bool,
	pub export_recorded: bool,
	pub references: usize,
	pub connections: usize,
	pub source_symbols: usize,
	pub target_symbols: usize,
	pub same_symbol_references: usize,
	pub coverage: MetricsCouplingCoverage,
	pub by_kind: Vec<CountDto>,
	pub by_target: Vec<MetricsCouplingTargetUsage>,
	pub unlinked: UnlinkedRefsDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MetricsCouplingTargetUsage {
	pub moniker: String,
	pub references: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GitRevisionDto {
	pub branch: String,
	pub commit: String,
	pub dirty: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MetricsCouplingCoverage {
	pub source_references: usize,
	pub resolved_source_references: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ChangeReviewResult {
	pub scope: String,
	pub summary: ChangeReviewSummary,
	pub files: Vec<ChangeReviewFile>,
	pub symbol_changes: Vec<ChangeReviewSymbol>,
	pub ref_changes: Vec<ChangeReviewRef>,
	pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ChangeReviewSummary {
	pub files: usize,
	pub analyzable_files: usize,
	pub symbol_changes: usize,
	pub ref_changes: usize,
	pub retargeted_refs: usize,
	pub residual_files: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ChangeReviewFile {
	pub old_path: Option<String>,
	pub new_path: Option<String>,
	pub disposition: String,
	pub analyzable: bool,
	pub symbol_changes: usize,
	pub moved_symbols: usize,
	pub coverage_explained: bool,
	pub old_residual: Vec<(u32, u32)>,
	pub new_residual: Vec<(u32, u32)>,
	pub test_artifact: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ChangeReviewSymbol {
	pub kind: String,
	pub confidence: String,
	pub body_changed: bool,
	pub signature_changed: bool,
	pub visibility_changed: bool,
	pub header_changed: bool,
	pub file_moved: bool,
	pub old: Option<ChangeReviewSide>,
	pub new: Option<ChangeReviewSide>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ChangeReviewSide {
	pub identity: String,
	pub file: String,
	pub kind: String,
	pub name: String,
	pub visibility: String,
	pub lines: Option<(u32, u32)>,
	pub test_artifact: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ChangeReviewRef {
	pub kind: String,
	pub file: String,
	pub ref_kind: String,
	pub old_target: Option<String>,
	pub new_target: Option<String>,
	pub old_lines: Option<(u32, u32)>,
	pub new_lines: Option<(u32, u32)>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DiffImpactResult {
	pub scope: String,
	pub summary: DiffImpactSummary,
	pub files: Vec<DiffImpactFile>,
	pub symbol_changes: Vec<DiffImpactSymbol>,
	pub ref_changes: Vec<DiffImpactRef>,
	pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DiffImpactSummary {
	pub files: usize,
	pub analyzable_files: usize,
	pub symbol_changes: usize,
	pub ref_changes: usize,
	pub retargeted_refs: usize,
	pub residual_files: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DiffImpactFile {
	pub old_path: Option<String>,
	pub new_path: Option<String>,
	pub disposition: String,
	pub analyzable: bool,
	pub symbol_changes: usize,
	pub moved_symbols: usize,
	pub coverage_explained: bool,
	pub old_residual: Vec<(u32, u32)>,
	pub new_residual: Vec<(u32, u32)>,
	pub test_artifact: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DiffImpactSymbol {
	pub kind: String,
	pub confidence: String,
	pub body_changed: bool,
	pub signature_changed: bool,
	pub visibility_changed: bool,
	pub header_changed: bool,
	pub file_moved: bool,
	pub old: Option<DiffImpactSide>,
	pub new: Option<DiffImpactSide>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DiffImpactSide {
	pub identity: String,
	pub compact_identity: String,
	pub file: String,
	pub kind: String,
	pub name: String,
	pub visibility: String,
	pub lines: Option<(u32, u32)>,
	pub test_artifact: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DiffImpactRef {
	pub kind: String,
	pub file: String,
	pub ref_kind: String,
	pub old_target: Option<String>,
	pub new_target: Option<String>,
	pub old_target_compact: Option<String>,
	pub new_target_compact: Option<String>,
	pub old_lines: Option<(u32, u32)>,
	pub new_lines: Option<(u32, u32)>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CommandResponse {
	pub generation: Option<WorkspaceGeneration>,
	pub message: String,
	pub status: Option<Box<WorkspaceStatus>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViewReadResult {
	List(ViewListResult),
	Detail(Box<ViewDetailResult>),
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ViewListResult {
	pub views: Vec<ViewSummaryDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ViewSummaryDto {
	pub id: String,
	pub title: Option<String>,
	pub fragment: String,
	pub anchor: String,
	pub scope: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ViewDetailResult {
	pub id: String,
	pub title: Option<String>,
	pub fragment: String,
	pub anchor: String,
	pub scope: String,
	pub intent: Option<String>,
	pub summary: Option<String>,
	pub rules: Vec<ViewRuleDto>,
	pub boundaries: Vec<ViewBoundaryDto>,
	pub gotchas: Vec<ViewGotchaDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ViewRuleDto {
	pub id: String,
	pub severity: String,
	pub domain: String,
	pub rationale: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ViewRuleRefDto {
	pub id: String,
	pub present: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ViewBoundaryDto {
	pub id: String,
	pub owns: Vec<String>,
	pub forbids: Vec<String>,
	pub forbid_rules: Vec<String>,
	pub rationale: Option<String>,
	pub rule_refs: Vec<ViewRuleRefDto>,
	pub evidence: Vec<ViewEvidenceDto>,
	pub missing: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ViewGotchaDto {
	pub id: String,
	pub rationale: String,
	pub check: Option<String>,
	pub rule_refs: Vec<ViewRuleRefDto>,
	pub evidence: Vec<ViewEvidenceDto>,
	pub missing: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ViewEvidenceDto {
	pub selector: String,
	pub label: String,
	pub moniker: String,
	pub file: String,
	pub slice: Option<(u32, u32)>,
	pub active_slice: Option<(u32, u32)>,
	pub code: Vec<SourceLine>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorkspaceStatus {
	#[serde(default)]
	pub producer: BuildIdentity,
	pub root: String,
	pub phase: WorkspacePhase,
	#[serde(default)]
	pub failure: Option<WorkspaceFailureDto>,
	pub roots: Vec<WorkspaceRootStatus>,
	pub generation: Option<WorkspaceGeneration>,
	pub files: usize,
	pub symbols: usize,
	pub references: usize,
	pub stale: bool,
	pub stale_summary: String,
	#[serde(default)]
	pub timings: WorkspaceTimingsDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorkspaceLifecycle {
	pub phase: WorkspacePhase,
	#[serde(default)]
	pub failure: Option<WorkspaceFailureDto>,
}

impl WorkspaceLifecycle {
	pub fn loading() -> Self {
		Self {
			phase: WorkspacePhase::Loading,
			failure: None,
		}
	}

	pub fn ready() -> Self {
		Self {
			phase: WorkspacePhase::Ready,
			failure: None,
		}
	}

	pub fn failed(message: impl Into<String>) -> Self {
		Self {
			phase: WorkspacePhase::Failed,
			failure: Some(WorkspaceFailureDto {
				resource: None,
				message: message.into(),
			}),
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePhase {
	Loading,
	Ready,
	Refreshing,
	Failed,
}

impl std::fmt::Display for WorkspacePhase {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::Loading => "loading",
			Self::Ready => "ready",
			Self::Refreshing => "refreshing",
			Self::Failed => "failed",
		})
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorkspaceFailureDto {
	pub resource: Option<String>,
	pub message: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorkspaceTimingsDto {
	pub source_catalog_ms: u64,
	pub extract_sources_ms: u64,
	pub semantic_index_ms: u64,
	pub code_index_ms: u64,
	pub linkage_ms: u64,
	pub change_overlay_ms: u64,
	pub total_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorkspaceRootStatus {
	pub root: String,
	pub generation: Option<WorkspaceGeneration>,
	pub files: usize,
	pub symbols: usize,
	pub references: usize,
	pub stale: bool,
	pub stale_summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TreeChildrenResult {
	pub root: String,
	pub roots: Vec<String>,
	pub rows: Vec<TreeNode>,
	pub total: usize,
	pub total_files: usize,
	pub scoped_files: usize,
	pub languages: Vec<CountDto>,
	pub prefixes: Vec<CountDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TreeNode {
	pub root: String,
	pub path: String,
	pub kind: TreeNodeKind,
	pub language: Option<String>,
	pub defs: usize,
	pub refs: usize,
	pub change_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TreeNodeKind {
	File,
	Directory,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolListResult {
	pub rows: Vec<SymbolDto>,
	pub total: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolDto {
	pub root: String,
	pub uri: String,
	pub id: String,
	pub name: String,
	pub kind: String,
	pub visibility: String,
	pub signature: String,
	pub file: String,
	pub language: String,
	pub line_range: Option<(u32, u32)>,
	pub navigable: bool,
	pub score: Option<u32>,
	pub match_reason: Option<String>,
	pub source: Option<SourceSnippet>,
}

pub fn symbol_is_test_artifact(kind: &str, file: &str, uri: &str) -> bool {
	kind == "test"
		|| uri
			.split('/')
			.any(|segment| matches!(segment, "module:test" | "module:tests"))
		|| file.split(['/', '\\']).any(|component| {
			matches!(
				component,
				"test"
					| "tests" | "bench"
					| "benches" | "fixture"
					| "fixtures" | "testdata"
					| "__tests__"
			)
		})
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolInsightsResult {
	pub files: usize,
	pub symbols: usize,
	pub references: usize,
	pub navigable_symbols: usize,
	pub non_navigable_symbols: usize,
	pub languages: Vec<CountDto>,
	pub kinds: Vec<CountDto>,
	pub shapes: Vec<CountDto>,
	pub top_files_by_symbols: Vec<CountDto>,
	pub top_files_by_refs: Vec<CountDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolDetailResult {
	pub symbol: SymbolDto,
	pub source: Option<SourceSnippet>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SyntaxTreeResult {
	pub file: String,
	pub language: String,
	pub focus: String,
	pub focus_line_range: Option<(u32, u32)>,
	pub root: SyntaxNodeDto,
	pub emitted_nodes: usize,
	pub total_nodes: usize,
	pub max_depth: usize,
	pub truncated: bool,
	pub has_error: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SyntaxNodeDto {
	pub kind: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub language: Option<String>,
	pub named: bool,
	pub error: bool,
	pub missing: bool,
	pub byte_range: (usize, usize),
	pub start: SyntaxPointDto,
	pub end: SyntaxPointDto,
	pub text: Option<String>,
	pub children: Vec<SyntaxNodeDto>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SyntaxPointDto {
	/// One-based line number.
	pub line: u32,
	/// Zero-based UTF-8 byte column, matching Tree-sitter.
	pub column: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SourceSnippet {
	pub file: String,
	pub first_line: u32,
	pub last_line: u32,
	pub lines: Vec<SourceLine>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SourceLine {
	pub number: u32,
	pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolUsagesResult {
	pub target: SymbolDto,
	pub direction: UsageDirection,
	pub include_descendants: bool,
	pub targets: usize,
	pub rows: Vec<UsageDto>,
	pub total: usize,
	pub incoming_summary: Option<UsageSummaryDto>,
	pub outgoing_summary: Option<UsageSummaryDto>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UsageSummaryDto {
	pub refs: usize,
	pub files: usize,
	pub contexts: usize,
	pub prefixes: usize,
	pub dominant_prefix: String,
	pub kinds: Vec<CountDto>,
	pub top_actors: Vec<CountDto>,
	pub top_prefixes: Vec<CountDto>,
	pub shared_helper_signal: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UsageDto {
	pub root: String,
	pub direction: UsageDirection,
	pub reference: String,
	pub kind: String,
	pub actor: String,
	pub context: String,
	pub endpoint: String,
	pub file: String,
	pub prefix: String,
	pub location: String,
	pub line_range: Option<(u32, u32)>,
	pub via: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RulesListResult {
	pub roots: Vec<String>,
	pub rows: Vec<RuleDto>,
	pub total: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuleDto {
	pub root: String,
	pub id: String,
	pub severity: String,
	pub lang: String,
	pub rule_root: String,
	pub subject: String,
	pub plan: String,
	pub capabilities: Vec<String>,
	#[serde(default)]
	pub group_by: Vec<String>,
	pub domain: String,
	pub kind: Option<String>,
	pub expr: String,
	pub expanded_expr: String,
	pub message: Option<String>,
	pub rationale: Option<String>,
	pub require_doc_comment: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RulesApplicableResult {
	pub focus: SymbolGraphFocus,
	pub file: String,
	pub language: String,
	pub symbol_kind: Option<String>,
	pub total: usize,
	pub rows: Vec<RuleApplicabilityDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuleApplicabilityDto {
	pub rule: RuleDto,
	pub status: String,
	pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ChangeContextResult {
	pub focus: SymbolGraphFocus,
	pub source: Option<SourceSnippet>,
	pub graph: Box<SymbolGraphResult>,
	pub notes: Vec<NoteDto>,
	pub rules: Vec<RuleApplicabilityDto>,
	pub changed_files: Vec<ChangeReviewFile>,
	pub changed_symbols: Vec<ChangeReviewSymbol>,
	pub suggested_checks: Vec<String>,
	pub coverage: ChangeContextCoverageDto,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ChangeContextCoverageDto {
	pub members_total: usize,
	pub members_emitted: usize,
	pub internal_edges_total: usize,
	pub internal_edges_emitted: usize,
	pub callers_total: usize,
	pub callers_emitted: usize,
	pub callees_total: usize,
	pub callees_emitted: usize,
	pub notes_total: usize,
	pub notes_emitted: usize,
	pub rules_total: usize,
	pub rules_emitted: usize,
	pub changes_total: usize,
	pub changes_emitted: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RulesCheckResult {
	pub verdict: RulesCheckVerdict,
	pub exit: String,
	pub summary: CheckSummaryDto,
	pub roots: Vec<RulesCheckRootResult>,
	pub violations: Vec<ViolationDto>,
	pub errors: Vec<FileErrorDto>,
	pub rule_reports: Vec<RuleReportDto>,
	pub skip_reasons: Vec<CheckSkipReasonDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RulesCheckRootResult {
	pub root: String,
	pub verdict: RulesCheckVerdict,
	pub exit: String,
	pub summary: CheckSummaryDto,
	pub violations: Vec<ViolationDto>,
	pub errors: Vec<FileErrorDto>,
	pub rule_reports: Vec<RuleReportDto>,
	pub skip_reason: Option<CheckSkipReasonDto>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RulesCheckVerdict {
	Pass,
	Fail,
	Error,
}

impl RulesCheckVerdict {
	pub fn from_exit(exit: &str) -> Self {
		match exit {
			"match" => Self::Pass,
			"no_match" => Self::Fail,
			_ => Self::Error,
		}
	}

	pub fn as_str(self) -> &'static str {
		match self {
			Self::Pass => "pass",
			Self::Fail => "fail",
			Self::Error => "error",
		}
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CheckSummaryDto {
	pub files_scanned: usize,
	pub files_with_violations: usize,
	pub total_violations: usize,
	pub total_rule_errors: usize,
	pub total_warnings: usize,
	pub files_with_errors: usize,
	pub total_errors: usize,
	pub elapsed_ms: u64,
	pub failed_rules: Vec<FailedRuleDto>,
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub violations_by_srcset: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FailedRuleDto {
	pub rule_id: String,
	pub severity: String,
	pub violations: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ViolationDto {
	pub root: String,
	pub path: String,
	pub rule_id: String,
	pub severity: String,
	pub moniker: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub srcset: Option<String>,
	pub kind: String,
	pub lines: (u32, u32),
	pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FileErrorDto {
	pub root: String,
	pub path: String,
	pub error: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuleReportDto {
	pub root: String,
	pub path: Option<String>,
	pub rule_id: String,
	pub severity: String,
	pub domain: String,
	pub evaluated: usize,
	pub matches: usize,
	pub violations: usize,
	pub antecedent_matches: Option<usize>,
	pub warning: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub inconclusive: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub verdict: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub coverage: Option<RuleCoverageDto>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub path_analysis: Option<RulePathReportDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuleCoverageDto {
	pub total: usize,
	pub decided: usize,
	pub resolved: usize,
	pub external: usize,
	pub candidate: usize,
	pub dynamic: usize,
	pub blocked: usize,
	pub unresolved: usize,
	pub percent: usize,
	pub min_percent: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RulePathStepDto {
	pub source: String,
	pub target: String,
	pub relation: String,
	pub reference: String,
	pub file: String,
	pub line_range: Option<(u32, u32)>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RulePathReportDto {
	pub expectation: String,
	pub relation: Vec<String>,
	pub max_depth: usize,
	pub max_symbols: usize,
	pub max_edges: usize,
	pub max_pairs: usize,
	pub min_coverage: usize,
	pub source_symbols: usize,
	pub target_symbols: usize,
	pub via_symbols: usize,
	pub evaluated_pairs: usize,
	pub explored_symbols: usize,
	pub explored_edges: usize,
	pub depth_limit_reached: bool,
	pub symbol_limit_reached: bool,
	pub edge_limit_reached: bool,
	pub pair_limit_reached: bool,
	pub reasons: Vec<String>,
	pub witness: Vec<RulePathStepDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CheckSkipReasonDto {
	pub root: String,
	pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NotesResult {
	pub action: String,
	pub total: usize,
	pub rows: Vec<NoteDto>,
	pub deleted: Option<NoteDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NoteDto {
	pub id: String,
	pub moniker: String,
	pub kind: String,
	pub status: String,
	pub title: String,
	pub body: String,
	pub created_by: String,
	pub updated_at: String,
	pub resolution: NoteResolutionDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum NoteResolutionDto {
	Resolved {
		target: String,
		file: String,
		slice: Option<(u32, u32)>,
	},
	Orphan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CountDto {
	pub name: String,
	pub count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum DaemonEvent {
	WorkspaceStale {
		generation: Option<WorkspaceGeneration>,
		summary: String,
	},
	WorkspaceRefreshed {
		generation: WorkspaceGeneration,
	},
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QueryError {
	pub code: String,
	pub message: String,
}

impl QueryError {
	pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
		Self {
			code: code.into(),
			message: message.into(),
		}
	}
}

impl fmt::Display for QueryError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}: {}", self.code, self.message)
	}
}

impl std::error::Error for QueryError {}

#[derive(Debug, thiserror::Error)]
pub enum QueryParseError {
	#[error("empty query")]
	Empty,
	#[error("unknown query operation `{0}`")]
	UnknownOperation(String),
	#[error("invalid token `{0}`")]
	InvalidToken(String),
	#[error("invalid value for `{key}`: `{value}`")]
	InvalidValue { key: String, value: String },
	#[error("missing required `{0}`")]
	MissingRequired(&'static str),
	#[error("unknown field `{key}` for `{op}`{hint}")]
	UnknownField {
		op: String,
		key: String,
		hint: String,
	},
	#[error("unexpected argument `{value}` for `{op}`")]
	UnexpectedArgument { op: String, value: String },
	#[error("`project` is not supported by `{op}`")]
	UnsupportedProjection { op: String },
	#[error("unknown projection field `{field}` for `{op}`{hint}")]
	UnknownProjectionField {
		op: String,
		field: String,
		hint: String,
	},
}

pub fn parse_query(input: &str) -> Result<QueryRequest, QueryParseError> {
	let mut lines = input.lines().map(str::trim).filter(|line| !line.is_empty());
	let first = lines.next().ok_or(QueryParseError::Empty)?;
	let mut tokens = tokenize(first)?;
	let op = tokens
		.first()
		.map(|token| token.text.clone())
		.ok_or(QueryParseError::Empty)?;
	tokens.remove(0);
	let mut fields = FieldBag::default();
	let mut positional = Vec::new();
	collect_tokens(&tokens, &mut fields, &mut positional)?;
	for line in lines {
		collect_line(line, &mut fields, &mut positional)?;
	}
	fields.positional = positional;
	let spec = verb_spec(&op).ok_or_else(|| QueryParseError::UnknownOperation(op.clone()))?;
	validate_fields(&op, spec, &fields)?;
	if let Some(value) = fields.one("consistency") {
		fields.consistency = value.parse()?;
	}
	let page = fields.page()?;
	let consistency = fields.consistency;
	let query = build_query(&op, fields)?;
	Ok(QueryRequest {
		query,
		consistency,
		page,
	})
}

fn collect_line(
	line: &str,
	fields: &mut FieldBag,
	positional: &mut Vec<String>,
) -> Result<(), QueryParseError> {
	let mut tokens = tokenize(line)?;
	if tokens.is_empty() {
		return Ok(());
	}
	let section = tokens.remove(0);
	if section.text.contains(':') {
		let mut all = vec![section];
		all.extend(tokens);
		return collect_tokens(&all, fields, positional);
	}
	match section.text.as_str() {
		"filter" | "page" => collect_tokens(&tokens, fields, positional)?,
		"project" => {
			fields.projection.extend(
				tokens
					.into_iter()
					.map(|token| token.text.trim_end_matches(',').to_string())
					.filter(|token| !token.is_empty()),
			);
		}
		"consistency" => {
			let value = tokens
				.first()
				.ok_or(QueryParseError::MissingRequired("consistency value"))?;
			fields.consistency = value.text.parse()?;
		}
		"direction" => {
			let value = tokens
				.first()
				.ok_or(QueryParseError::MissingRequired("direction value"))?;
			fields.values.push((
				"direction".to_string(),
				FieldValue {
					text: value.text.clone(),
					quoted: value.quoted,
				},
			));
		}
		_ => return Err(QueryParseError::InvalidToken(section.text)),
	}
	Ok(())
}

fn build_query(op: &str, fields: FieldBag) -> Result<Query, QueryParseError> {
	let query = match op {
		"query.describe" => Query::QueryDescribe(QueryDescribeQuery {
			verb: fields
				.one("verb")
				.or_else(|| fields.positional.first().cloned()),
		}),
		"workspace.status" => Query::WorkspaceStatus,
		"tree.children" => Query::TreeChildren(TreeChildrenQuery {
			workspace: fields.one("workspace"),
			path: fields.many("path"),
			depth: fields.usize("depth")?.unwrap_or(1),
			lang: fields.many("lang"),
			projection: fields.projection,
		}),
		"symbol.search" => Query::SymbolSearch(symbol_search_query(&fields)?),
		"symbol.insights" => Query::SymbolInsights(symbol_insights_query(&fields)?),
		"symbol.detail" => Query::SymbolDetail(SymbolDetailQuery {
			workspace: fields.one("workspace"),
			uri: fields
				.one("uri")
				.or_else(|| fields.positional.first().cloned())
				.ok_or(QueryParseError::MissingRequired("uri"))?,
			context_lines: fields.usize("context_lines")?.unwrap_or(2),
		}),
		"syntax.tree" => Query::SyntaxTree(SyntaxTreeQuery {
			workspace: fields.one("workspace"),
			focus: fields
				.one("focus")
				.or_else(|| fields.positional.first().cloned())
				.ok_or(QueryParseError::MissingRequired("focus"))?,
			max_depth: fields
				.usize("max_depth")?
				.unwrap_or(SYNTAX_TREE_DEFAULT_MAX_DEPTH),
			max_nodes: fields
				.usize("max_nodes")?
				.unwrap_or(SYNTAX_TREE_DEFAULT_MAX_NODES),
			named_only: fields.bool("named_only")?.unwrap_or(true),
			include_text: fields.bool("include_text")?.unwrap_or(false),
			max_text_chars: fields
				.usize("max_text_chars")?
				.unwrap_or(SYNTAX_TREE_DEFAULT_MAX_TEXT_CHARS),
		}),
		"syntax.parse" => Query::SyntaxParse(syntax_parse_query(&fields)?),
		"symbol.usages" => Query::SymbolUsages(symbol_usages_query(&fields)?),
		"view.read" => Query::ViewRead(ViewReadQuery {
			uri: fields
				.one("uri")
				.or_else(|| fields.positional.first().cloned())
				.ok_or(QueryParseError::MissingRequired("uri"))?,
			scheme: fields.one("scheme"),
			context_lines: fields.usize("context_lines")?.unwrap_or(2),
			include_code: fields.bool("include_code")?.unwrap_or(false),
		}),
		"rules.list" => Query::RulesList(RulesListQuery {
			workspace: fields.one("workspace"),
			profile: fields.one("profile"),
			rules: fields.one("rules"),
			lang: fields.many("lang"),
			severity: fields.many("severity"),
		}),
		"rules.check" => Query::RulesCheck(RulesCheckQuery {
			workspace: fields.one("workspace"),
			profile: fields.one("profile"),
			rules: fields.one("rules"),
			file: fields.many("file"),
			report: fields.bool("report")?.unwrap_or(true),
		}),
		"rules.applicable" => Query::RulesApplicable(RulesApplicableQuery {
			workspace: fields.one("workspace"),
			focus: fields
				.one("focus")
				.or_else(|| fields.positional.first().cloned())
				.ok_or(QueryParseError::MissingRequired("focus"))?,
			profile: fields.one("profile"),
			rules: fields.one("rules"),
		}),
		"change.review" => Query::ChangeReview(ChangeReviewQuery {
			workspace: fields.one("workspace"),
		}),
		"change.context" => Query::ChangeContext(ChangeContextQuery {
			workspace: fields.one("workspace"),
			focus: fields
				.one("focus")
				.or_else(|| fields.positional.first().cloned())
				.ok_or(QueryParseError::MissingRequired("focus"))?,
			profile: fields.one("profile"),
			max_items: fields.usize("max_items")?.unwrap_or(20),
		}),
		"symbol.graph" => Query::SymbolGraph(symbol_graph_query(&fields)?),
		"graph.path" => Query::GraphPath(graph_path_query(&fields)?),
		"identity.children" => Query::IdentityChildren(identity_children_query(&fields)),
		"identity.graph" => Query::IdentityGraph(identity_graph_query(&fields)?),
		"metrics.coupling" => Query::MetricsCoupling(MetricsCouplingQuery {
			workspace: fields.one("workspace"),
			from: fields
				.one("from")
				.ok_or(QueryParseError::MissingRequired("from"))?,
			to: fields
				.one("to")
				.ok_or(QueryParseError::MissingRequired("to"))?,
			relation: fields.many("relation"),
			snapshot: fields.one("snapshot"),
			export: fields.bool("export")?.unwrap_or(false),
		}),
		"resolution.audit" => Query::ResolutionAudit(ResolutionAuditQuery {
			workspace: fields.one("workspace"),
			prefix: fields
				.one("prefix")
				.or_else(|| fields.positional.first().cloned())
				.unwrap_or_default(),
			limit: fields.usize("limit")?.unwrap_or(20),
			cluster: fields.one("cluster"),
		}),
		"notes" => Query::Notes(notes_query(&fields)?),
		_ => return Err(QueryParseError::UnknownOperation(op.to_string())),
	};
	Ok(query)
}

fn verb_spec(op: &str) -> Option<&'static QueryCapabilitySpec> {
	query_capability_spec(op)
}

fn validate_fields(
	op: &str,
	spec: &QueryCapabilitySpec,
	fields: &FieldBag,
) -> Result<(), QueryParseError> {
	for (key, value) in &fields.values {
		let key = key.as_str();
		if !COMMON_FIELDS.contains(&key) && !spec.fields.contains(&key) {
			return Err(QueryParseError::UnknownField {
				op: op.to_string(),
				key: key.to_string(),
				hint: field_hint(key, spec.fields),
			});
		}
		if !value.quoted
			&& BRACKET_LIST_FIELDS.contains(&key)
			&& value.text.starts_with('[') != value.text.ends_with(']')
		{
			return Err(QueryParseError::InvalidValue {
				key: key.to_string(),
				value: value.text.clone(),
			});
		}
	}
	if let Some(extra) = fields.positional.get(spec.positionals) {
		return Err(QueryParseError::UnexpectedArgument {
			op: op.to_string(),
			value: extra.clone(),
		});
	}
	if !spec.projection && !fields.projection.is_empty() {
		return Err(QueryParseError::UnsupportedProjection { op: op.to_string() });
	}
	let projection_fields = query_projection_fields(op);
	for field in &fields.projection {
		if !projection_fields.contains(&field.as_str()) {
			return Err(QueryParseError::UnknownProjectionField {
				op: op.to_string(),
				field: field.clone(),
				hint: projection_field_hint(field, projection_fields),
			});
		}
	}
	Ok(())
}

fn projection_field_hint(field: &str, allowed: &'static [&'static str]) -> String {
	if let Some(suggestion) = allowed
		.iter()
		.copied()
		.map(|candidate| (candidate, levenshtein(field, candidate)))
		.filter(|(_, distance)| *distance <= 2)
		.min_by_key(|(_, distance)| *distance)
		.map(|(candidate, _)| candidate)
	{
		return format!(", did you mean `{suggestion}`?");
	}
	format!(" (valid projection fields: {})", allowed.join(", "))
}

fn field_hint(key: &str, allowed: &'static [&'static str]) -> String {
	if let Some(suggestion) = suggest_field(key, allowed) {
		return format!(", did you mean `{suggestion}`?");
	}
	let mut valid: Vec<&str> = allowed.iter().chain(COMMON_FIELDS).copied().collect();
	valid.sort_unstable();
	format!(" (valid fields: {})", valid.join(", "))
}

fn suggest_field(key: &str, allowed: &'static [&'static str]) -> Option<&'static str> {
	const ALIASES: &[(&str, &str)] = &[("text", "name"), ("query", "name"), ("filename", "path")];
	for (alias, target) in ALIASES {
		if *alias == key && allowed.contains(target) {
			return Some(target);
		}
	}
	allowed
		.iter()
		.chain(COMMON_FIELDS)
		.copied()
		.map(|candidate| (candidate, levenshtein(key, candidate)))
		.filter(|(_, distance)| *distance <= 2)
		.min_by_key(|(_, distance)| *distance)
		.map(|(candidate, _)| candidate)
}

fn levenshtein(a: &str, b: &str) -> usize {
	let b: Vec<char> = b.chars().collect();
	let mut row: Vec<usize> = (0..=b.len()).collect();
	for (i, ca) in a.chars().enumerate() {
		let mut previous = row[0];
		row[0] = i + 1;
		for (j, cb) in b.iter().enumerate() {
			let substitution = previous + usize::from(ca != *cb);
			previous = row[j + 1];
			row[j + 1] = substitution.min(previous + 1).min(row[j] + 1);
		}
	}
	row[b.len()]
}

fn symbol_search_query(fields: &FieldBag) -> Result<SymbolSearchQuery, QueryParseError> {
	Ok(SymbolSearchQuery {
		workspace: fields.one("workspace"),
		text: fields.positional.first().cloned(),
		path: fields.many("path"),
		lang: fields.many("lang"),
		kind: fields.many("kind"),
		shape: fields.many("shape"),
		name: fields.one("name"),
		include_non_navigable: fields.bool("include_non_navigable")?.unwrap_or(false),
		include_code: fields.bool("include_code")?.unwrap_or(false),
		context_lines: fields.usize("context_lines")?.unwrap_or(0),
		projection: fields.projection.clone(),
	})
}

fn symbol_usages_query(fields: &FieldBag) -> Result<SymbolUsagesQuery, QueryParseError> {
	Ok(SymbolUsagesQuery {
		workspace: fields.one("workspace"),
		uri: fields
			.one("uri")
			.or_else(|| fields.positional.first().cloned())
			.ok_or(QueryParseError::MissingRequired("uri"))?,
		direction: fields
			.one("direction")
			.unwrap_or_else(|| "incoming".to_string())
			.parse()?,
		path: fields.many("path"),
		lang: fields.many("lang"),
		include_descendants: fields.bool("include_descendants")?.unwrap_or(false),
		projection: fields.projection.clone(),
	})
}

fn syntax_parse_query(fields: &FieldBag) -> Result<SyntaxParseQuery, QueryParseError> {
	Ok(SyntaxParseQuery {
		language: fields
			.one("language")
			.ok_or(QueryParseError::MissingRequired("language"))?,
		source: fields
			.one("source")
			.ok_or(QueryParseError::MissingRequired("source"))?,
		uri: fields.one("uri"),
		max_depth: fields
			.usize("max_depth")?
			.unwrap_or(SYNTAX_TREE_DEFAULT_MAX_DEPTH),
		max_nodes: fields
			.usize("max_nodes")?
			.unwrap_or(SYNTAX_TREE_DEFAULT_MAX_NODES),
		named_only: fields.bool("named_only")?.unwrap_or(true),
		include_text: fields.bool("include_text")?.unwrap_or(false),
		max_text_chars: fields
			.usize("max_text_chars")?
			.unwrap_or(SYNTAX_TREE_DEFAULT_MAX_TEXT_CHARS),
	})
}

fn symbol_insights_query(fields: &FieldBag) -> Result<SymbolSearchQuery, QueryParseError> {
	let mut query = symbol_search_query(fields)?;
	query.text = None;
	query.include_code = false;
	query.context_lines = 0;
	Ok(query)
}

fn notes_query(fields: &FieldBag) -> Result<NotesQuery, QueryParseError> {
	Ok(NotesQuery {
		action: parse_notes_action(fields.one("action").as_deref().unwrap_or("list"))?,
		id: fields.one("id"),
		moniker: fields.one("moniker"),
		kind: fields.one("kind"),
		status: fields.one("status"),
		title: fields.one("title"),
		body: fields.one("body"),
		created_by: fields.one("created_by"),
		orphan: fields.bool("orphan")?,
		include_done: fields.bool("include_done")?.unwrap_or(false),
	})
}

fn identity_children_query(fields: &FieldBag) -> IdentityChildrenQuery {
	IdentityChildrenQuery {
		workspace: fields.one("workspace"),
		prefix: fields
			.one("prefix")
			.or_else(|| fields.positional.first().cloned())
			.unwrap_or_default(),
	}
}

fn identity_graph_query(fields: &FieldBag) -> Result<IdentityGraphQuery, QueryParseError> {
	Ok(IdentityGraphQuery {
		workspace: fields.one("workspace"),
		prefix: fields
			.one("prefix")
			.or_else(|| fields.positional.first().cloned())
			.unwrap_or_default(),
		path: fields.many("path"),
		min_count: fields.usize("min_count")?.unwrap_or(1).max(1),
	})
}

fn symbol_graph_query(fields: &FieldBag) -> Result<SymbolGraphQuery, QueryParseError> {
	Ok(SymbolGraphQuery {
		workspace: fields.one("workspace"),
		focus: fields
			.one("focus")
			.or_else(|| fields.positional.first().cloned())
			.ok_or(QueryParseError::MissingRequired("focus"))?,
		direction: fields
			.one("direction")
			.unwrap_or_else(|| "both".to_string())
			.parse()?,
		relation: fields.many("relation"),
		min_count: fields.usize("min_count")?.unwrap_or(1).max(1),
		include_internal: fields.bool("include_internal")?.unwrap_or(true),
	})
}

fn graph_path_query(fields: &FieldBag) -> Result<GraphPathQuery, QueryParseError> {
	let max_depth = fields.usize("max_depth")?.unwrap_or(12);
	if max_depth > 64 {
		return Err(QueryParseError::InvalidValue {
			key: "max_depth".to_string(),
			value: max_depth.to_string(),
		});
	}
	let min_coverage = fields.usize("min_coverage")?.unwrap_or(100);
	if min_coverage > 100 {
		return Err(QueryParseError::InvalidValue {
			key: "min_coverage".to_string(),
			value: min_coverage.to_string(),
		});
	}
	let max_symbols = bounded_non_zero_field(fields, "max_symbols", 10_000, 100_000)?;
	let max_edges = bounded_non_zero_field(fields, "max_edges", 50_000, 500_000)?;
	let relation = fields.many("relation");
	Ok(GraphPathQuery {
		workspace: fields.one("workspace"),
		from: fields
			.one("from")
			.ok_or(QueryParseError::MissingRequired("from"))?,
		to: fields
			.one("to")
			.ok_or(QueryParseError::MissingRequired("to"))?,
		expect: fields
			.one("expect")
			.unwrap_or_else(|| "reachable".to_string())
			.parse()?,
		relation: if relation.is_empty() {
			vec!["calls".to_string(), "method_call".to_string()]
		} else {
			relation
		},
		max_depth,
		max_symbols,
		max_edges,
		min_coverage,
	})
}

fn bounded_non_zero_field(
	fields: &FieldBag,
	key: &str,
	default: usize,
	maximum: usize,
) -> Result<usize, QueryParseError> {
	let value = fields.usize(key)?.unwrap_or(default);
	if value == 0 || value > maximum {
		return Err(QueryParseError::InvalidValue {
			key: key.to_string(),
			value: value.to_string(),
		});
	}
	Ok(value)
}

fn parse_notes_action(value: &str) -> Result<NotesAction, QueryParseError> {
	match value {
		"list" => Ok(NotesAction::List),
		"get" => Ok(NotesAction::Get),
		"create" => Ok(NotesAction::Create),
		"update" => Ok(NotesAction::Update),
		"transition" => Ok(NotesAction::Transition),
		"delete" => Ok(NotesAction::Delete),
		_ => Err(QueryParseError::InvalidValue {
			key: "action".to_string(),
			value: value.to_string(),
		}),
	}
}

pub fn format_query_response(response: &QueryResponse) -> String {
	format_query_response_projected(response, &[])
}

pub fn format_query_response_projected(response: &QueryResponse, projection: &[String]) -> String {
	let mut out = String::new();
	if let Some(generation) = response.generation {
		let _ = writeln!(out, "generation: {}", generation.0);
	}
	if let Some(cursor) = &response.next_cursor {
		if let Some(generation) = cursor.generation {
			let _ = writeln!(out, "next_cursor: {}:{}", generation.0, cursor.offset);
		} else {
			let _ = writeln!(out, "next_cursor: {}", cursor.offset);
		}
	}
	match &response.result {
		QueryResult::QueryDescribe(result) => format_query_describe(&mut out, result),
		QueryResult::WorkspaceStatus(status) => format_workspace_status(&mut out, status),
		QueryResult::TreeChildren(result) => format_tree_children(&mut out, result, projection),
		QueryResult::SymbolList(result) => format_symbol_list(&mut out, result, projection),
		QueryResult::SymbolInsights(result) if !projection.is_empty() => {
			format_projected_value(&mut out, result, projection);
		}
		QueryResult::SymbolInsights(result) => format_symbol_insights(&mut out, result),
		QueryResult::SymbolDetail(result) => format_symbol_detail(&mut out, result),
		QueryResult::SyntaxTree(result) => format_syntax_tree(&mut out, result),
		QueryResult::SymbolUsages(result) => format_symbol_usages(&mut out, result, projection),
		QueryResult::ViewRead(result) => format_view_read(&mut out, result),
		QueryResult::RulesList(result) => {
			let _ = writeln!(out, "rules: {}", result.total);
			format_rules_list_rows(&mut out, result);
		}
		QueryResult::RulesCheck(result) => format_rules_check(&mut out, result),
		QueryResult::RulesApplicable(result) => format_rules_applicable(&mut out, result),
		QueryResult::ChangeReview(result) => format_change_review(&mut out, result),
		QueryResult::DiffImpact(result) => format_diff_impact(&mut out, result),
		QueryResult::ChangeContext(result) => format_change_context(&mut out, result),
		QueryResult::SymbolGraph(result) => format_symbol_graph(&mut out, result),
		QueryResult::GraphPath(result) => format_graph_path(&mut out, result),
		QueryResult::IdentityChildren(result) => format_identity_children(&mut out, result),
		QueryResult::IdentityGraph(result) => format_identity_graph(&mut out, result),
		QueryResult::MetricsCoupling(result) => format_metrics_coupling(&mut out, result),
		QueryResult::ResolutionAudit(result) => format_resolution_audit(&mut out, result),
		QueryResult::Notes(result) => format_notes(&mut out, result),
	}
	out
}

fn format_workspace_status(out: &mut String, status: &WorkspaceStatus) {
	let _ = writeln!(out, "workspace: {}", status.root);
	let _ = writeln!(
		out,
		"producer: {} {}",
		status.producer.version, status.producer.fingerprint
	);
	let _ = writeln!(out, "phase: {}", status.phase);
	let _ = writeln!(
		out,
		"files: {} symbols: {} references: {}",
		status.files, status.symbols, status.references
	);
	let _ = writeln!(out, "stale: {} ({})", status.stale, status.stale_summary);
	let timings = &status.timings;
	let _ = writeln!(
		out,
		"timings_ms: total={} catalog={} extract={} semantic={} linkage={} changes={}",
		timings.total_ms,
		timings.source_catalog_ms,
		timings.extract_sources_ms,
		timings.semantic_index_ms,
		timings.linkage_ms,
		timings.change_overlay_ms,
	);
	if status.roots.len() > 1 {
		let _ = writeln!(out, "roots:");
		for root in &status.roots {
			let _ = writeln!(
				out,
				"- {} files:{} symbols:{} references:{} stale:{}",
				root.root, root.files, root.symbols, root.references, root.stale
			);
		}
	}
}

fn format_tree_children(out: &mut String, result: &TreeChildrenResult, projection: &[String]) {
	let _ = writeln!(out, "tree: {}", result.root);
	for row in &result.rows {
		if !projection.is_empty() {
			format_projected_value(out, row, projection);
			continue;
		}
		let kind = match row.kind {
			TreeNodeKind::File => "file",
			TreeNodeKind::Directory => "dir",
		};
		let _ = writeln!(
			out,
			"- {kind} {} defs:{} refs:{}",
			row.path, row.defs, row.refs
		);
	}
}

fn format_symbol_list(out: &mut String, result: &SymbolListResult, projection: &[String]) {
	let _ = writeln!(out, "symbols: {}", result.total);
	for row in &result.rows {
		if projection.is_empty() {
			let _ = writeln!(out, "- {} {} {} {}", row.kind, row.name, row.file, row.uri);
		} else {
			format_projected_value(out, row, projection);
		}
	}
}

fn format_symbol_detail(out: &mut String, result: &SymbolDetailResult) {
	let symbol = &result.symbol;
	let _ = writeln!(out, "symbol: {} {}", symbol.kind, symbol.name);
	let _ = writeln!(out, "uri: {}", symbol.uri);
	let _ = writeln!(out, "file: {}", symbol.file);
	if let Some(source) = &result.source {
		for line in &source.lines {
			let _ = writeln!(out, "{:>6} | {}", line.number, line.text);
		}
	}
}

fn format_syntax_tree(out: &mut String, result: &SyntaxTreeResult) {
	let _ = writeln!(out, "file: {}", result.file);
	let _ = writeln!(out, "language: {}", result.language);
	let _ = writeln!(out, "focus: {}", result.focus);
	let _ = writeln!(
		out,
		"nodes: {}/{} max_depth:{} truncated:{} parse_error:{}",
		result.emitted_nodes,
		result.total_nodes,
		result.max_depth,
		result.truncated,
		result.has_error
	);
	let _ = writeln!(out, "tree:");
	format_syntax_node(out, &result.root, 0);
}

fn format_syntax_node(out: &mut String, node: &SyntaxNodeDto, depth: usize) {
	let language = node
		.language
		.as_deref()
		.map(|language| format!(" lang:{language}"))
		.unwrap_or_default();
	let marker = if node.error {
		" error"
	} else if node.missing {
		" missing"
	} else {
		""
	};
	let text = node
		.text
		.as_deref()
		.map(|text| format!(" text={text:?}"))
		.unwrap_or_default();
	let _ = writeln!(
		out,
		"{}- {}{} {}:{}-{}:{}{}{}",
		"  ".repeat(depth),
		node.kind,
		language,
		node.start.line,
		node.start.column,
		node.end.line,
		node.end.column,
		marker,
		text
	);
	for child in &node.children {
		format_syntax_node(out, child, depth + 1);
	}
}

fn format_symbol_usages(out: &mut String, result: &SymbolUsagesResult, projection: &[String]) {
	let _ = writeln!(out, "uri: {}", result.target.uri);
	let _ = writeln!(out, "direction: {}", result.direction.as_str());
	let scope = if result.include_descendants {
		"descendants"
	} else {
		"exact"
	};
	let _ = writeln!(out, "target_scope: {scope} ({} symbols)", result.targets);
	let _ = writeln!(out, "usages: {}", result.total);
	for row in &result.rows {
		if projection.is_empty() {
			let _ = writeln!(
				out,
				"- {} {} {} {}",
				row.direction.as_str(),
				row.kind,
				row.actor,
				row.file
			);
		} else {
			format_projected_value(out, row, projection);
		}
	}
}

fn format_view_read(out: &mut String, result: &ViewReadResult) {
	match result {
		ViewReadResult::List(list) => {
			let _ = writeln!(out, "views: {}", list.views.len());
			for view in &list.views {
				let _ = writeln!(out, "- {} ({})", view.id, view.scope);
			}
		}
		ViewReadResult::Detail(detail) => {
			let _ = writeln!(out, "view: {}", detail.id);
			let _ = writeln!(out, "fragment: {}", detail.fragment);
			let _ = writeln!(out, "scope: {}", detail.scope);
			let _ = writeln!(
				out,
				"rules: {} boundaries: {} gotchas: {}",
				detail.rules.len(),
				detail.boundaries.len(),
				detail.gotchas.len()
			);
		}
	}
}

fn format_projected_value(out: &mut String, value: &impl Serialize, projection: &[String]) {
	let Ok(Value::Object(fields)) = serde_json::to_value(value) else {
		return;
	};
	let rendered = projection
		.iter()
		.filter_map(|name| fields.get(name).map(|value| (name, value)))
		.map(|(name, value)| format!("{name}={}", compact_json_value(value)))
		.collect::<Vec<_>>()
		.join(" ");
	let _ = writeln!(out, "- {rendered}");
}

fn compact_json_value(value: &Value) -> String {
	match value {
		Value::String(value) => value.clone(),
		Value::Null => "-".to_string(),
		_ => serde_json::to_string(value).unwrap_or_else(|_| "?".to_string()),
	}
}

fn format_query_describe(out: &mut String, result: &QueryDescribeResult) {
	let _ = writeln!(out, "queries: {}", result.capabilities.len());
	for capability in &result.capabilities {
		let _ = writeln!(
			out,
			"- {} [{}] read_only={} mcp={} projection={} paginated={}",
			capability.name,
			capability.category,
			capability.read_only,
			capability.mcp_tool,
			capability.projection,
			capability.paginated
		);
		let fields = capability
			.fields
			.iter()
			.map(|field| {
				let required = if field.required { "!" } else { "" };
				let multiple = if field.multiple { "[]" } else { "" };
				let default = field
					.default
					.as_deref()
					.map_or(String::new(), |value| format!("={value}"));
				format!(
					"{}{}:{}{}{}",
					field.name, required, field.value_type, multiple, default
				)
			})
			.collect::<Vec<_>>()
			.join(", ");
		let _ = writeln!(out, "  fields: {fields}");
		if !capability.projection_fields.is_empty() {
			let _ = writeln!(
				out,
				"  project: {}",
				capability.projection_fields.join(", ")
			);
		}
		let _ = writeln!(out, "  example: {}", capability.example);
	}
}

fn format_rules_applicable(out: &mut String, result: &RulesApplicableResult) {
	let _ = writeln!(out, "focus: {}", result.file);
	let _ = writeln!(out, "language: {}", result.language);
	if let Some(kind) = &result.symbol_kind {
		let _ = writeln!(out, "symbol_kind: {kind}");
	}
	let applicable = result
		.rows
		.iter()
		.filter(|row| row.status == "applicable")
		.count();
	let _ = writeln!(out, "rules: {} applicable: {applicable}", result.total);
	for row in &result.rows {
		let _ = writeln!(
			out,
			"- {} [{}] {} — {}",
			row.rule.id, row.rule.severity, row.status, row.reason
		);
	}
}

fn format_change_context(out: &mut String, result: &ChangeContextResult) {
	let _ = writeln!(out, "facts:");
	match &result.focus {
		SymbolGraphFocus::Symbol { symbol } => {
			let _ = writeln!(out, "focus: {} {}", symbol.kind, symbol.name);
			let _ = writeln!(out, "uri: {}", symbol.uri);
			let _ = writeln!(out, "file: {}", symbol.file);
		}
		SymbolGraphFocus::File { path } => {
			let _ = writeln!(out, "focus: file {path}");
		}
	}
	if let Some(source) = &result.source {
		let _ = writeln!(out, "source:");
		for line in &source.lines {
			let _ = writeln!(out, "{:>6} | {}", line.number, line.text);
		}
	}
	format_context_graph(out, &result.graph);
	let coverage = result.coverage;
	let _ = writeln!(out, "coverage:");
	let _ = writeln!(
		out,
		"- members {}/{} · internal_edges {}/{} · callers {}/{} · callees {}/{}",
		coverage.members_emitted,
		coverage.members_total,
		coverage.internal_edges_emitted,
		coverage.internal_edges_total,
		coverage.callers_emitted,
		coverage.callers_total,
		coverage.callees_emitted,
		coverage.callees_total
	);
	let _ = writeln!(
		out,
		"- notes {}/{} · rules {}/{} · changes {}/{}",
		coverage.notes_emitted,
		coverage.notes_total,
		coverage.rules_emitted,
		coverage.rules_total,
		coverage.changes_emitted,
		coverage.changes_total
	);
	if !result.notes.is_empty() {
		let _ = writeln!(out, "notes:");
		for note in &result.notes {
			let _ = writeln!(out, "- {} [{}] {}", note.id, note.status, note.title);
		}
	}
	if !result.rules.is_empty() {
		let _ = writeln!(out, "applicable_rules:");
		for row in &result.rules {
			let _ = writeln!(out, "- {} [{}]", row.rule.id, row.rule.severity);
		}
	}
	format_context_changes(out, &result.changed_files, &result.changed_symbols);
	if !result.suggested_checks.is_empty() {
		let _ = writeln!(out, "suggested_checks:");
		for check in &result.suggested_checks {
			let _ = writeln!(out, "- {check}");
		}
	}
}

fn format_context_graph(out: &mut String, graph: &SymbolGraphResult) {
	if !graph.members.is_empty() {
		let _ = writeln!(out, "members:");
		for member in &graph.members {
			let _ = writeln!(
				out,
				"- {} {} ({}) {}",
				member.kind, member.name, member.file, member.uri
			);
		}
	}
	if !graph.internal_edges.is_empty() {
		let member_labels: BTreeMap<&str, String> = graph
			.members
			.iter()
			.map(|member| {
				(
					member.id.as_str(),
					format!("{} {}", member.kind, member.name),
				)
			})
			.collect();
		let unlisted_labels: BTreeMap<&str, String> = graph
			.internal_edges
			.iter()
			.flat_map(|edge| [edge.source.as_str(), edge.target.as_str()])
			.filter(|endpoint| {
				endpoint.starts_with("symbol:") && !member_labels.contains_key(endpoint)
			})
			.collect::<std::collections::BTreeSet<_>>()
			.into_iter()
			.enumerate()
			.map(|(index, endpoint)| {
				(
					endpoint,
					format!("unlisted internal member {}", index.saturating_add(1)),
				)
			})
			.collect();
		let endpoint_label = |endpoint: &str| {
			member_labels
				.get(endpoint)
				.or_else(|| unlisted_labels.get(endpoint))
				.cloned()
				.unwrap_or_else(|| endpoint.to_string())
		};
		let _ = writeln!(out, "internal_edges:");
		for edge in &graph.internal_edges {
			let _ = writeln!(
				out,
				"- {} -> {} x{} [{}]",
				endpoint_label(&edge.source),
				endpoint_label(&edge.target),
				edge.count,
				edge.kinds.join(",")
			);
		}
	}
	format_unlinked(out, &graph.unlinked);
	for (marker, neighbors) in [("<", &graph.callers), (">", &graph.callees)] {
		for neighbor in neighbors {
			let _ = writeln!(
				out,
				"{marker} {} {} x{} [{}] {}",
				neighbor.symbol.kind,
				neighbor.symbol.name,
				neighbor.count,
				neighbor.kinds.join(","),
				neighbor.symbol.uri
			);
		}
	}
}

fn format_context_changes(
	out: &mut String,
	files: &[ChangeReviewFile],
	symbols: &[ChangeReviewSymbol],
) {
	if !files.is_empty() {
		let _ = writeln!(out, "changed_files:");
		for file in files {
			let old = file.old_path.as_deref().unwrap_or("-");
			let new = file.new_path.as_deref().unwrap_or("-");
			let _ = writeln!(out, "- {old} -> {new} [{}]", file.disposition);
		}
	}
	if !symbols.is_empty() {
		let _ = writeln!(out, "changed_symbols:");
		for symbol in symbols {
			let old = symbol
				.old
				.as_ref()
				.map(|side| side.identity.as_str())
				.unwrap_or("-");
			let new = symbol
				.new
				.as_ref()
				.map(|side| side.identity.as_str())
				.unwrap_or("-");
			let _ = writeln!(
				out,
				"- {} [{}] {old} -> {new}",
				symbol.kind, symbol.confidence
			);
		}
	}
}

fn format_resolution_audit(out: &mut String, result: &ResolutionAuditResult) {
	let t = &result.totals;
	if !result.prefix.is_empty() {
		let _ = writeln!(out, "prefix: {}", result.prefix);
	}
	let _ = writeln!(
		out,
		"refs: {} unique: {} candidate: {} external: {} sdk: {} dependency: {} injected_external: {} unknown_external: {} dynamic: {} blocked: {} unresolved: {} explained: {} weak_or_unexplained: {} name_match_candidate: {}",
		t.references,
		t.unique,
		t.candidate,
		t.external,
		t.sdk,
		t.dependency,
		t.injected_external,
		t.unknown_external,
		t.dynamic,
		t.blocked,
		t.unresolved,
		t.explained,
		t.weak_or_unexplained,
		t.name_match_candidate
	);
	let _ = writeln!(out, "clusters:");
	for cluster in &result.clusters {
		let _ = writeln!(
			out,
			"- [{:>6}] {} {}",
			cluster.count, cluster.id, cluster.pattern
		);
		let sample_limit = if result.clusters.len() == 1 {
			cluster.samples.len()
		} else {
			1
		};
		for sample in cluster.samples.iter().take(sample_limit) {
			let location = match sample.line_range {
				Some((start, end)) if start == end => format!("{}:{start}", sample.file),
				Some((start, end)) => format!("{}:{start}-{end}", sample.file),
				None => sample.file.clone(),
			};
			let _ = writeln!(
				out,
				"           ex: {} {} {} -> {} evidence:{}",
				location, sample.call_name, sample.receiver, sample.target, sample.evidence
			);
			if !sample.candidates.is_empty() {
				let _ = writeln!(
					out,
					"           candidates: {}",
					sample.candidates.join(", ")
				);
			}
			if !sample.constraints.is_empty() {
				let _ = writeln!(
					out,
					"           constraints: {}",
					sample.constraints.join(", ")
				);
			}
			if !sample.snippet.is_empty() {
				let _ = writeln!(out, "           code: {}", sample.snippet);
			}
		}
	}
	let _ = writeln!(out, "zones:");
	for zone in &result.zones {
		let _ = writeln!(
			out,
			"- [{:>5}] {} — {}",
			zone.unresolved, zone.zone, zone.dominant_pattern
		);
	}
}

fn format_symbol_insights(out: &mut String, result: &SymbolInsightsResult) {
	let _ = writeln!(out, "files: {}", result.files);
	let _ = writeln!(out, "symbols: {}", result.symbols);
	let _ = writeln!(out, "refs: {}", result.references);
	let _ = writeln!(out, "languages:");
	for row in &result.languages {
		let _ = writeln!(out, "- {}: {}", row.name, row.count);
	}
}

fn format_notes(out: &mut String, result: &NotesResult) {
	let _ = writeln!(out, "action: {}", result.action);
	let _ = writeln!(out, "notes: {}", result.total);
	for row in &result.rows {
		let _ = writeln!(out, "- {} [{}] {}", row.id, row.status, row.title);
	}
}

fn format_rules_list_rows(out: &mut String, result: &RulesListResult) {
	for row in &result.rows {
		let _ = writeln!(
			out,
			"- {} [{}] root={} lang={} domain={}",
			row.id, row.severity, row.root, row.lang, row.domain
		);
		if let Some(message) = &row.message {
			let _ = writeln!(out, "  message: {message}");
		}
	}
}

fn format_identity_children(out: &mut String, result: &IdentityChildrenResult) {
	let prefix = if result.prefix.is_empty() {
		"<root>"
	} else {
		&result.prefix
	};
	let _ = writeln!(out, "prefix: {prefix}");
	let _ = writeln!(out, "children: {}", result.children.len());
	for child in &result.children {
		let marker = if child.symbol.is_some() { "def" } else { "…" };
		let _ = writeln!(
			out,
			"- {} [{}] defs={} {}",
			child.segment, marker, child.defs, child.identity
		);
	}
}

fn format_unlinked(out: &mut String, unlinked: &UnlinkedRefsDto) {
	let _ = writeln!(
		out,
		"unlinked refs: external {} (sdk {} · dependency {} · injected {} · unknown {}) · candidate {} · dynamic {} · manifest-blocked {} · unresolved {}",
		unlinked.external,
		unlinked.sdk,
		unlinked.dependency,
		unlinked.injected_external,
		unlinked.unknown_external,
		unlinked.candidate,
		unlinked.dynamic,
		unlinked.manifest_blocked,
		unlinked.unresolved
	);
	if !unlinked.unresolved_reasons.is_empty() {
		let reasons = unlinked
			.unresolved_reasons
			.iter()
			.map(|(reason, count)| format!("{reason} {count}"))
			.collect::<Vec<_>>()
			.join(" · ");
		let _ = writeln!(out, "unresolved by reason: {reasons}");
	}
}

fn format_identity_graph(out: &mut String, result: &IdentityGraphResult) {
	let prefix = if result.prefix.is_empty() {
		"<root>"
	} else {
		&result.prefix
	};
	let _ = writeln!(out, "scope: {prefix}");
	if !result.path.is_empty() {
		let _ = writeln!(out, "path: {}", result.path.join(", "));
	}
	let _ = writeln!(out, "min_count: {}", result.min_count);
	let _ = writeln!(
		out,
		"coverage: rows {}/{} matching ({} total)",
		result.coverage.rows_emitted, result.coverage.rows_matching, result.coverage.rows_total
	);
	let _ = writeln!(
		out,
		"nodes: {}/{} edges: {}/{} matching ({} total) ports_in: {}/{} matching ({} total) ports_out: {}/{} matching ({} total)",
		result.coverage.nodes_emitted,
		result.coverage.nodes_total,
		result.coverage.edges_emitted,
		result.coverage.edges_matching,
		result.coverage.edges_total,
		result.coverage.ports_in_emitted,
		result.coverage.ports_in_matching,
		result.coverage.ports_in_total,
		result.coverage.ports_out_emitted,
		result.coverage.ports_out_matching,
		result.coverage.ports_out_total
	);
	format_unlinked(out, &result.unlinked);
	for node in &result.nodes {
		let _ = writeln!(
			out,
			"- node {} defs:{} children:{}",
			node.identity, node.defs, node.has_children
		);
	}
	for edge in &result.edges {
		let _ = writeln!(
			out,
			"- {} -> {} x{} [{}]",
			edge.source,
			edge.target,
			edge.count,
			edge.kinds.join(",")
		);
	}
	for port in &result.ports_in {
		let _ = writeln!(
			out,
			"< {} x{} [{}]",
			port.identity,
			port.count,
			port.kinds.join(",")
		);
	}
	for port in &result.ports_out {
		let _ = writeln!(
			out,
			"> {} x{} [{}]",
			port.identity,
			port.count,
			port.kinds.join(",")
		);
	}
}

fn format_metrics_coupling(out: &mut String, result: &MetricsCouplingResult) {
	let _ = writeln!(out, "from: {}", result.from);
	let _ = writeln!(out, "to: {}", result.to);
	let _ = writeln!(out, "snapshot: {}", result.snapshot);
	if let Some(git) = &result.git {
		let _ = writeln!(
			out,
			"git: branch {} commit {} dirty {}",
			git.branch, git.commit, git.dirty
		);
	}
	if !result.relation.is_empty() {
		let _ = writeln!(out, "relation: {}", result.relation.join(", "));
	}
	let export = match (result.export_requested, result.export_recorded) {
		(false, _) => "not requested",
		(true, true) => "recorded",
		(true, false) => "telemetry disabled",
	};
	let _ = writeln!(out, "otel export: {export}");
	let _ = writeln!(
		out,
		"coupling: references {} connections {} source_symbols {} target_symbols {}",
		result.references, result.connections, result.source_symbols, result.target_symbols
	);
	let _ = writeln!(
		out,
		"coverage: resolved source references {}/{}",
		result.coverage.resolved_source_references, result.coverage.source_references
	);
	if result.same_symbol_references > 0 {
		let _ = writeln!(
			out,
			"same-symbol references excluded: {}",
			result.same_symbol_references
		);
	}
	format_unlinked(out, &result.unlinked);
	if !result.by_target.is_empty() {
		let _ = writeln!(out, "public boundary usage by target:");
		for target in &result.by_target {
			let _ = writeln!(out, "- {} x{}", target.moniker, target.references);
		}
	}
	if !result.by_kind.is_empty() {
		let _ = writeln!(out, "usage by relation kind:");
	}
	for kind in &result.by_kind {
		let _ = writeln!(out, "- {} x{}", kind.name, kind.count);
	}
}

fn format_symbol_graph(out: &mut String, result: &SymbolGraphResult) {
	match &result.focus {
		SymbolGraphFocus::Symbol { symbol } => {
			let _ = writeln!(
				out,
				"focus: {} {} ({})",
				symbol.kind, symbol.name, symbol.file
			);
		}
		SymbolGraphFocus::File { path } => {
			let _ = writeln!(out, "focus: file {path}");
		}
	}
	let _ = writeln!(
		out,
		"members: {}/{} internal edges: {}/{} matching ({} total)",
		result.coverage.members.returned,
		result.coverage.members.total,
		result.coverage.internal_edges.returned,
		result.coverage.internal_edges.matching,
		result.coverage.internal_edges.total
	);
	format_unlinked(out, &result.unlinked);
	let _ = writeln!(
		out,
		"callers: {}/{} matching ({} total)",
		result.coverage.callers.returned,
		result.coverage.callers.matching,
		result.coverage.callers.total
	);
	for caller in &result.callers {
		let _ = writeln!(
			out,
			"< {} {} ({}) x{} [{}]",
			caller.symbol.kind,
			caller.symbol.name,
			caller.symbol.file,
			caller.count,
			caller.kinds.join(",")
		);
	}
	let _ = writeln!(
		out,
		"callees: {}/{} matching ({} total)",
		result.coverage.callees.returned,
		result.coverage.callees.matching,
		result.coverage.callees.total
	);
	for callee in &result.callees {
		let _ = writeln!(
			out,
			"> {} {} ({}) x{} [{}]",
			callee.symbol.kind,
			callee.symbol.name,
			callee.symbol.file,
			callee.count,
			callee.kinds.join(",")
		);
	}
}

fn format_graph_path(out: &mut String, result: &GraphPathResult) {
	let _ = writeln!(
		out,
		"expectation: {} verdict: {}",
		result.expectation.as_str(),
		result.verdict.as_str()
	);
	let _ = writeln!(
		out,
		"from: {} {} ({})",
		result.from.kind, result.from.name, result.from.file
	);
	let _ = writeln!(
		out,
		"to: {} {} ({})",
		result.to.kind, result.to.name, result.to.file
	);
	let truth = |value: Option<bool>| match value {
		Some(true) => "true",
		Some(false) => "false",
		None => "unknown",
	};
	let _ = writeln!(
		out,
		"reachable: {} no_path: {} path_length: {}",
		truth(result.reachable),
		truth(result.no_path),
		result.path.len()
	);
	let _ = writeln!(
		out,
		"coverage: {}% ({}/{}) resolved={} external={} candidate={} dynamic={} manifest_blocked={} unresolved={}",
		result.coverage.percent,
		result.coverage.decided,
		result.coverage.total,
		result.coverage.resolved,
		result.coverage.external,
		result.coverage.candidate,
		result.coverage.dynamic,
		result.coverage.manifest_blocked,
		result.coverage.unresolved
	);
	let _ = writeln!(
		out,
		"search: depth={}/{} symbols={} edges={} depth_limit_reached={} symbol_limit_reached={} edge_limit_reached={}",
		result.search.depth_reached,
		result.search.max_depth,
		result.search.explored_symbols,
		result.search.explored_edges,
		result.search.depth_limit_reached,
		result.search.symbol_limit_reached,
		result.search.edge_limit_reached
	);
	if !result.reasons.is_empty() {
		let _ = writeln!(out, "reasons: {}", result.reasons.join(", "));
	}
	for (index, step) in result.path.iter().enumerate() {
		let location = step.line_range.map_or_else(
			|| step.file.clone(),
			|(start, end)| {
				if start == end {
					format!("{}:L{start}", step.file)
				} else {
					format!("{}:L{start}-L{end}", step.file)
				}
			},
		);
		let _ = writeln!(
			out,
			"{}. {} -> {} [{}] {}",
			index + 1,
			step.source.uri,
			step.target.uri,
			step.relation,
			location
		);
	}
}

fn format_change_review(out: &mut String, result: &ChangeReviewResult) {
	let _ = writeln!(out, "scope: {}", result.scope);
	let _ = writeln!(
		out,
		"files: {} ({} analyzable) symbols: {} refs: {} ({} retargeted) residual: {}",
		result.summary.files,
		result.summary.analyzable_files,
		result.summary.symbol_changes,
		result.summary.ref_changes,
		result.summary.retargeted_refs,
		result.summary.residual_files
	);
	for file in &result.files {
		let path = match (&file.old_path, &file.new_path) {
			(Some(old), Some(new)) if old != new => format!("{old} -> {new}"),
			(_, Some(new)) => new.clone(),
			(Some(old), None) => old.clone(),
			(None, None) => "<unknown>".to_string(),
		};
		let _ = writeln!(
			out,
			"- {path} {}{}{}",
			file.disposition,
			if file.analyzable {
				""
			} else {
				" (not analyzable)"
			},
			if file.coverage_explained {
				""
			} else {
				" [residual]"
			}
		);
	}
	for change in &result.symbol_changes {
		let side = change.new.as_ref().or(change.old.as_ref());
		let Some(side) = side else { continue };
		let _ = writeln!(
			out,
			"  {} {} {} [{}]",
			change.kind, side.kind, side.name, change.confidence
		);
	}
	for diagnostic in &result.diagnostics {
		let _ = writeln!(out, "diagnostic: {diagnostic}");
	}
}

fn format_diff_impact(out: &mut String, result: &DiffImpactResult) {
	let _ = writeln!(out, "scope: {}", result.scope);
	let _ = writeln!(
		out,
		"files: {} ({} analyzable) symbols: {} refs: {} ({} retargeted) residual: {}",
		result.summary.files,
		result.summary.analyzable_files,
		result.summary.symbol_changes,
		result.summary.ref_changes,
		result.summary.retargeted_refs,
		result.summary.residual_files
	);
	for file in &result.files {
		let path = match (&file.old_path, &file.new_path) {
			(Some(old), Some(new)) if old != new => format!("{old} -> {new}"),
			(_, Some(new)) => new.clone(),
			(Some(old), None) => old.clone(),
			(None, None) => "<unknown>".to_string(),
		};
		let _ = writeln!(
			out,
			"- {path} {}{}{}",
			file.disposition,
			if file.analyzable {
				""
			} else {
				" (not analyzable)"
			},
			if file.coverage_explained {
				""
			} else {
				" [residual]"
			}
		);
	}
	for change in &result.symbol_changes {
		let side = change.new.as_ref().or(change.old.as_ref());
		let Some(side) = side else { continue };
		let _ = writeln!(
			out,
			"  {} {} {} [{}]",
			change.kind, side.kind, side.compact_identity, change.confidence
		);
	}
	for diagnostic in &result.diagnostics {
		let _ = writeln!(out, "diagnostic: {diagnostic}");
	}
}

fn format_rules_check(out: &mut String, result: &RulesCheckResult) {
	let _ = writeln!(out, "verdict: {}", result.verdict.as_str());
	let _ = writeln!(out, "exit: {}", result.exit);
	let _ = writeln!(
		out,
		"violations: {} errors: {} elapsed_ms: {}",
		result.summary.total_violations, result.summary.total_errors, result.summary.elapsed_ms
	);
	for violation in &result.violations {
		let _ = writeln!(
			out,
			"- {} {}:{}-{} [{}] {}",
			violation.root,
			violation.path,
			violation.lines.0,
			violation.lines.1,
			violation.rule_id,
			violation.message
		);
	}
	if !result.rule_reports.is_empty() {
		let _ = writeln!(out, "rule_reports: {}", result.rule_reports.len());
		for report in result.rule_reports.iter().filter(|report| {
			report.verdict.is_some() || report.coverage.is_some() || report.path_analysis.is_some()
		}) {
			let _ = writeln!(
				out,
				"  - {} verdict={} evaluated={} violations={}",
				report.rule_id,
				report.verdict.as_deref().unwrap_or("not_applicable"),
				report.evaluated,
				report.violations
			);
			if let Some(coverage) = &report.coverage {
				let _ = writeln!(
					out,
					"    coverage={}%, minimum={}%",
					coverage.percent, coverage.min_percent
				);
			}
			if let Some(path) = &report.path_analysis {
				let _ = writeln!(
					out,
					"    path expect={} pairs={} explored_symbols={} explored_edges={} witness_steps={}",
					path.expectation,
					path.evaluated_pairs,
					path.explored_symbols,
					path.explored_edges,
					path.witness.len()
				);
			}
		}
	}
}

#[derive(Default)]
struct FieldBag {
	values: Vec<(String, FieldValue)>,
	positional: Vec<String>,
	projection: Vec<String>,
	consistency: Consistency,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FieldValue {
	text: String,
	quoted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct QueryToken {
	text: String,
	quoted: bool,
}

impl FieldBag {
	fn bool(&self, key: &str) -> Result<Option<bool>, QueryParseError> {
		self.one(key)
			.map(|value| match value.as_str() {
				"true" => Ok(true),
				"false" => Ok(false),
				_ => Err(QueryParseError::InvalidValue {
					key: key.to_string(),
					value,
				}),
			})
			.transpose()
	}

	fn page(&self) -> Result<Page, QueryParseError> {
		let limit = self.usize("limit")?.unwrap_or(80);
		let cursor = self
			.one("cursor")
			.map(|value| parse_cursor(&value))
			.transpose()?;
		Ok(Page { cursor, limit })
	}

	fn usize(&self, key: &str) -> Result<Option<usize>, QueryParseError> {
		self.one(key)
			.map(|value| {
				value
					.parse::<usize>()
					.map_err(|_| QueryParseError::InvalidValue {
						key: key.to_string(),
						value,
					})
			})
			.transpose()
	}

	fn one(&self, key: &str) -> Option<String> {
		self.values
			.iter()
			.find(|(candidate, _)| candidate == key)
			.map(|(_, value)| value.text.clone())
	}

	fn many(&self, key: &str) -> Vec<String> {
		self.values
			.iter()
			.filter(|(candidate, _)| candidate == key)
			.flat_map(|(_, value)| {
				if value.quoted || !MULTI_VALUE_FIELDS.contains(&key) {
					vec![value.text.clone()]
				} else {
					split_csv(strip_bracket_list(key, &value.text))
				}
			})
			.collect()
	}
}

fn collect_tokens(
	tokens: &[QueryToken],
	fields: &mut FieldBag,
	positional: &mut Vec<String>,
) -> Result<(), QueryParseError> {
	for token in tokens {
		if let Some((key, value)) = token.text.split_once(':') {
			fields.values.push((
				key.to_string(),
				FieldValue {
					text: value.to_string(),
					quoted: token.quoted,
				},
			));
		} else {
			positional.push(token.text.trim_end_matches(',').to_string());
		}
	}
	Ok(())
}

fn parse_cursor(value: &str) -> Result<QueryCursor, QueryParseError> {
	let Some((generation, offset)) = value.split_once(':') else {
		return Err(QueryParseError::InvalidValue {
			key: "cursor".to_string(),
			value: value.to_string(),
		});
	};
	let generation = generation
		.parse::<u64>()
		.map_err(|_| QueryParseError::InvalidValue {
			key: "cursor".to_string(),
			value: value.to_string(),
		})?;
	let offset = offset
		.parse::<usize>()
		.map_err(|_| QueryParseError::InvalidValue {
			key: "cursor".to_string(),
			value: value.to_string(),
		})?;
	Ok(QueryCursor::new(
		offset,
		Some(WorkspaceGeneration(generation)),
	))
}

fn tokenize(input: &str) -> Result<Vec<QueryToken>, QueryParseError> {
	let mut tokens = Vec::new();
	let mut current = String::new();
	let mut chars = input.chars().peekable();
	let mut quoted = false;
	let mut token_quoted = false;
	while let Some(ch) = chars.next() {
		match ch {
			'"' => {
				quoted = !quoted;
				token_quoted = true;
			}
			'\\' if quoted => {
				if let Some(next) = chars.next() {
					current.push(next);
				}
			}
			ch if ch.is_whitespace() && !quoted => {
				if !current.is_empty() {
					tokens.push(QueryToken {
						text: std::mem::take(&mut current),
						quoted: std::mem::take(&mut token_quoted),
					});
				}
			}
			ch => current.push(ch),
		}
	}
	if quoted {
		return Err(QueryParseError::InvalidToken(input.to_string()));
	}
	if !current.is_empty() {
		tokens.push(QueryToken {
			text: current,
			quoted: token_quoted,
		});
	}
	Ok(tokens)
}

// `shape:[callable,type]` list sugar, restricted to enum-like fields so glob
// character classes in `path:`/`file:` values stay untouched.
fn strip_bracket_list<'a>(key: &str, value: &'a str) -> &'a str {
	if !BRACKET_LIST_FIELDS.contains(&key) {
		return value;
	}
	value
		.strip_prefix('[')
		.and_then(|inner| inner.strip_suffix(']'))
		.unwrap_or(value)
}

pub fn split_csv(value: &str) -> Vec<String> {
	value
		.split(',')
		.map(str::trim)
		.filter(|entry| !entry.is_empty())
		.map(ToOwned::to_owned)
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde::Serialize;

	fn serialized_fields(value: impl Serialize) -> Vec<String> {
		let mut fields = serde_json::to_value(value)
			.expect("serialize query DTO")
			.as_object()
			.expect("query DTO object")
			.keys()
			.cloned()
			.collect::<Vec<_>>();
		fields.sort();
		fields
	}

	fn dto_fields(verb: &str) -> Vec<String> {
		match verb {
			"query.describe" => serialized_fields(QueryDescribeQuery::default()),
			"workspace.status" => Vec::new(),
			"tree.children" => serialized_fields(TreeChildrenQuery::default()),
			"symbol.search" | "symbol.insights" => serialized_fields(SymbolSearchQuery::default()),
			"symbol.detail" => serialized_fields(SymbolDetailQuery {
				workspace: None,
				uri: String::new(),
				context_lines: 0,
			}),
			"syntax.tree" => serialized_fields(SyntaxTreeQuery {
				workspace: None,
				focus: String::new(),
				max_depth: 0,
				max_nodes: 0,
				named_only: true,
				include_text: false,
				max_text_chars: 0,
			}),
			"syntax.parse" => serialized_fields(SyntaxParseQuery {
				language: String::new(),
				source: String::new(),
				uri: None,
				max_depth: 0,
				max_nodes: 0,
				named_only: true,
				include_text: false,
				max_text_chars: 0,
			}),
			"symbol.usages" => serialized_fields(SymbolUsagesQuery {
				workspace: None,
				uri: String::new(),
				direction: UsageDirection::Incoming,
				path: Vec::new(),
				lang: Vec::new(),
				include_descendants: false,
				projection: Vec::new(),
			}),
			"view.read" => serialized_fields(ViewReadQuery {
				uri: String::new(),
				scheme: None,
				context_lines: 0,
				include_code: false,
			}),
			"rules.list" => serialized_fields(RulesListQuery::default()),
			"rules.check" => serialized_fields(RulesCheckQuery::default()),
			"rules.applicable" => serialized_fields(RulesApplicableQuery::default()),
			"change.review" => serialized_fields(ChangeReviewQuery::default()),
			"change.context" => serialized_fields(ChangeContextQuery::default()),
			"symbol.graph" => serialized_fields(SymbolGraphQuery::default()),
			"graph.path" => serialized_fields(GraphPathQuery::default()),
			"identity.children" => serialized_fields(IdentityChildrenQuery::default()),
			"identity.graph" => serialized_fields(IdentityGraphQuery::default()),
			"metrics.coupling" => serialized_fields(MetricsCouplingQuery::default()),
			"resolution.audit" => serialized_fields(ResolutionAuditQuery::default()),
			"notes" => serialized_fields(NotesQuery {
				action: NotesAction::List,
				id: None,
				moniker: None,
				kind: None,
				status: None,
				title: None,
				body: None,
				created_by: None,
				orphan: None,
				include_done: false,
			}),
			other => panic!("missing DTO field fixture for {other}"),
		}
	}

	#[test]
	fn capability_registry_fields_exist_on_query_dtos() {
		for spec in query_capability_specs() {
			let dto = dto_fields(spec.name);
			for field in spec.fields {
				assert!(
					dto.iter().any(|candidate| candidate == field),
					"{} field `{field}` missing from DTO fields {dto:?}",
					spec.name
				);
			}
			assert!(
				spec.fields
					.iter()
					.all(|field| !COMMON_FIELDS.contains(field)),
				"{} repeats a common request field",
				spec.name
			);
		}
	}

	#[test]
	fn describes_live_query_contract() {
		let request = parse_query("query.describe symbol.usages").expect("query describe");
		assert!(matches!(
			request.query,
			Query::QueryDescribe(QueryDescribeQuery { verb: Some(ref verb) })
				if verb == "symbol.usages"
		));
		let result = describe_query_capabilities(Some("symbol.usages")).expect("capability");
		let capability = result.capabilities.first().expect("described query");
		assert!(capability.read_only);
		assert_eq!(capability.mcp_tool, "code_moniker_usages");
		assert!(capability.projection);
		assert!(capability.paginated);
		assert!(
			capability
				.fields
				.iter()
				.any(|field| field.name == "uri" && field.required)
		);
		assert!(capability.projection_fields.contains(&"actor".to_string()));
		assert_eq!(
			CapabilitySet::default().query_mcp_tools["symbol.usages"],
			"code_moniker_usages"
		);
	}

	#[test]
	fn parses_bounded_syntax_tree_contract() {
		let request = parse_query(
			"syntax.tree focus:\"src/service.ts\" max_depth:4 max_nodes:120 named_only:false include_text:true max_text_chars:40",
		)
		.expect("syntax tree query");
		assert_eq!(
			request.query,
			Query::SyntaxTree(SyntaxTreeQuery {
				workspace: None,
				focus: "src/service.ts".to_string(),
				max_depth: 4,
				max_nodes: 120,
				named_only: false,
				include_text: true,
				max_text_chars: 40,
			})
		);
	}

	#[test]
	fn parses_stateless_syntax_contract() {
		let request = parse_query(
			"syntax.parse language:\"rs\" source:\"fn main() {}\" uri:\"snippet.rs\" max_depth:4 max_nodes:120",
		)
		.expect("stateless syntax query");
		assert_eq!(
			request.query,
			Query::SyntaxParse(SyntaxParseQuery {
				language: "rs".to_string(),
				source: "fn main() {}".to_string(),
				uri: Some("snippet.rs".to_string()),
				max_depth: 4,
				max_nodes: 120,
				named_only: true,
				include_text: false,
				max_text_chars: SYNTAX_TREE_DEFAULT_MAX_TEXT_CHARS,
			})
		);
		assert!(!request.query.requires_workspace_snapshot());
	}

	#[test]
	fn parses_symbol_graph_relational_filters() {
		let request = parse_query(
			"symbol.graph focus:\"src/lib.rs\" direction:incoming relation:[calls,uses_type] min_count:2 include_internal:false",
		)
		.expect("symbol graph filters");
		let Query::SymbolGraph(query) = request.query else {
			panic!("expected symbol graph query");
		};
		assert_eq!(query.direction, UsageDirection::Incoming);
		assert_eq!(query.relation, vec!["calls", "uses_type"]);
		assert_eq!(query.min_count, 2);
		assert!(!query.include_internal);
	}

	#[test]
	fn parses_bounded_graph_path_contract() {
		let request = parse_query(
			"graph.path from:\"code+moniker://./fn:callback()\" to:\"code+moniker://./fn:repository()\" expect:no_path relation:[calls,method_call] max_depth:6 max_symbols:200 max_edges:500 min_coverage:95",
		)
		.expect("graph path contract");
		let Query::GraphPath(query) = request.query else {
			panic!("expected graph path query");
		};
		assert_eq!(query.from, "code+moniker://./fn:callback()");
		assert_eq!(query.to, "code+moniker://./fn:repository()");
		assert_eq!(query.expect, GraphPathExpectation::NoPath);
		assert_eq!(query.relation, vec!["calls", "method_call"]);
		assert_eq!(query.max_depth, 6);
		assert_eq!(query.max_symbols, 200);
		assert_eq!(query.max_edges, 500);
		assert_eq!(query.min_coverage, 95);
	}

	#[test]
	fn rejects_unbounded_graph_path_limits() {
		assert!(matches!(
			parse_query(
				"graph.path from:\"symbol:0:0\" to:\"symbol:0:1\" max_depth:65"
			),
			Err(QueryParseError::InvalidValue { ref key, .. }) if key == "max_depth"
		));
		assert!(matches!(
			parse_query(
				"graph.path from:\"symbol:0:0\" to:\"symbol:0:1\" min_coverage:101"
			),
			Err(QueryParseError::InvalidValue { ref key, .. }) if key == "min_coverage"
		));
		assert!(matches!(
			parse_query(
				"graph.path from:\"symbol:0:0\" to:\"symbol:0:1\" max_symbols:0"
			),
			Err(QueryParseError::InvalidValue { ref key, .. }) if key == "max_symbols"
		));
	}

	#[test]
	fn preserves_commas_in_quoted_symbol_uri() {
		let uri = "code+moniker://./lang:ts/dir:src/module:Session/class:Session/method:launchRequest(response:DebugProtocol.LaunchResponse,args:LaunchRequestArguments)";
		let request = parse_query(&format!(
			"symbol.usages uri:\"{uri}\" direction:outgoing limit:5"
		))
		.expect("quoted symbol URI");
		let Query::SymbolUsages(query) = request.query else {
			panic!("expected symbol usages query");
		};
		assert_eq!(query.uri, uri);
	}

	#[test]
	fn quoted_multi_value_field_does_not_split_commas() {
		let request =
			parse_query("symbol.search path:\"src/generated,a.ts\" shape:\"callable,type\"")
				.expect("quoted multi-value fields");
		let Query::SymbolSearch(query) = request.query else {
			panic!("expected symbol search query");
		};
		assert_eq!(query.path, vec!["src/generated,a.ts"]);
		assert_eq!(query.shape, vec!["callable,type"]);
	}

	#[test]
	fn symbol_usages_can_roll_up_descendant_member_activity_explicitly() {
		let request = parse_query(
			"symbol.usages uri:\"symbol:1:2\" direction:incoming include_descendants:true",
		)
		.expect("owner rollup usage query");
		let Query::SymbolUsages(query) = request.query else {
			panic!("expected symbol usages query");
		};
		assert!(query.include_descendants);
		let capability = describe_query_capabilities(Some("symbol.usages"))
			.expect("symbol usages capability")
			.capabilities
			.pop()
			.expect("symbol usages descriptor");
		let field = capability
			.fields
			.iter()
			.find(|field| field.name == "include_descendants")
			.expect("descendant scope field");
		assert_eq!(field.value_type, "boolean");
		assert_eq!(field.default.as_deref(), Some("false"));
	}

	#[test]
	fn unquoted_multi_value_fields_still_split_commas() {
		let request = parse_query("symbol.search path:src,tests shape:callable,type")
			.expect("unquoted multi-value fields");
		let Query::SymbolSearch(query) = request.query else {
			panic!("expected symbol search query");
		};
		assert_eq!(query.path, vec!["src", "tests"]);
		assert_eq!(query.shape, vec!["callable", "type"]);
	}

	#[test]
	fn identity_graph_accepts_path_scope_without_changing_identity_children() {
		let request = parse_query(
			"identity.graph prefix:\"lang:java/package:com/package:acme\" path:src/main/**,src/test/**",
		)
		.expect("path-scoped identity graph");
		let Query::IdentityGraph(query) = request.query else {
			panic!("expected identity graph query");
		};
		assert_eq!(query.path, vec!["src/main/**", "src/test/**"]);

		assert!(matches!(
			parse_query("identity.children path:src/main/**"),
			Err(QueryParseError::UnknownField { .. })
		));
	}

	#[test]
	fn identity_graph_declares_weight_filter_and_real_pagination() {
		let request = parse_query("identity.graph prefix:\"lang:rs/dir:src\" min_count:3 limit:2")
			.expect("filtered identity graph");
		let Query::IdentityGraph(query) = request.query else {
			panic!("expected identity graph query");
		};
		assert_eq!(query.min_count, 3);
		assert_eq!(request.page.limit, 2);
		assert!(
			query_capability_spec("identity.graph")
				.expect("identity graph capability")
				.paginated
		);
	}

	#[test]
	fn coupling_metrics_requires_explicit_scopes_and_accepts_relations() {
		let request = parse_query(
			"metrics.coupling from:\"lang:java/package:com/package:acme\" to:\"lang:java/package:com/package:storage\" relation:calls,imports_symbol snapshot:current export:true",
		)
		.expect("coupling metrics query");
		let Query::MetricsCoupling(query) = request.query else {
			panic!("expected coupling metrics query");
		};
		assert_eq!(query.from, "lang:java/package:com/package:acme");
		assert_eq!(query.to, "lang:java/package:com/package:storage");
		assert_eq!(query.relation, vec!["calls", "imports_symbol"]);
		assert_eq!(query.snapshot.as_deref(), Some("current"));
		assert!(query.export);
		let Query::MetricsCoupling(default_export) =
			parse_query("metrics.coupling from:\"lang:java\" to:\"lang:java/package:com\"")
				.expect("coupling metrics without export")
				.query
		else {
			panic!("expected coupling metrics query");
		};
		assert!(!default_export.export);
		assert!(matches!(
			parse_query("metrics.coupling from:\"lang:java\""),
			Err(QueryParseError::MissingRequired("to"))
		));
		assert!(
			!query_capability_spec("metrics.coupling")
				.expect("metrics capability")
				.paginated
		);
	}

	#[test]
	fn scalar_field_preserves_unquoted_regex_commas() {
		let request = parse_query("symbol.search name:^launch(Request){1,3}$").expect("name regex");
		let Query::SymbolSearch(query) = request.query else {
			panic!("expected symbol search query");
		};
		assert_eq!(query.name.as_deref(), Some("^launch(Request){1,3}$"));
	}

	#[test]
	fn parses_resolution_audit_positional_prefix() {
		let request = parse_query("resolution.audit java limit:7 cluster:resolution-abc")
			.expect("audit query");
		let Query::ResolutionAudit(query) = request.query else {
			panic!("expected resolution audit query");
		};
		assert_eq!(query.prefix, "java");
		assert_eq!(query.limit, 7);
		assert_eq!(query.cluster.as_deref(), Some("resolution-abc"));
	}

	#[test]
	fn resolution_audit_default_limit_matches_its_documented_contract() {
		let request = parse_query("resolution.audit python").expect("audit query");
		let Query::ResolutionAudit(query) = request.query else {
			panic!("expected resolution audit query");
		};
		assert_eq!(query.limit, 20);
	}

	#[test]
	fn formats_resolution_audit_explanation_metrics() {
		let response = QueryResponse {
			generation: None,
			result: QueryResult::ResolutionAudit(Box::new(ResolutionAuditResult {
				prefix: "lang:python".to_string(),
				totals: AuditTotalsDto {
					references: 10,
					resolved: 4,
					unique: 4,
					candidate: 2,
					external: 1,
					sdk: 1,
					dependency: 0,
					injected_external: 0,
					unknown_external: 0,
					dynamic: 1,
					blocked: 1,
					unresolved: 1,
					explained: 9,
					weak_or_unexplained: 3,
					name_match_resolved: 0,
					name_match_candidate: 2,
				},
				clusters: vec![AuditClusterDto {
					id: "resolution-abc".to_string(),
					pattern: "candidate name_match/method_call".to_string(),
					count: 2,
					samples: vec![
						AuditSampleDto {
							file: "src/a.py".to_string(),
							line_range: Some((10, 10)),
							snippet: "value.first()".to_string(),
							source: "module:a".to_string(),
							call_name: "first".to_string(),
							receiver: "value".to_string(),
							target: "method:first".to_string(),
							evidence: "name_match".to_string(),
							constraints: vec!["scope:global".to_string()],
							candidates: vec!["class:A/method:first".to_string()],
						},
						AuditSampleDto {
							file: "src/b.py".to_string(),
							line_range: Some((20, 21)),
							snippet: "other.second()".to_string(),
							source: "module:b".to_string(),
							call_name: "second".to_string(),
							receiver: "other".to_string(),
							target: "method:second".to_string(),
							evidence: "name_match".to_string(),
							constraints: Vec::new(),
							candidates: Vec::new(),
						},
					],
				}],
				zones: Vec::new(),
			})),
			next_cursor: None,
		};

		let formatted = format_query_response(&response);

		assert!(formatted.contains("unique: 4 candidate: 2"));
		assert!(formatted.contains("explained: 9 weak_or_unexplained: 3"));
		assert!(formatted.contains("name_match_candidate: 2"));
		assert!(formatted.contains("resolution-abc"));
		assert!(formatted.contains("first value"));
		assert!(formatted.contains("second other"));
		assert!(formatted.contains("src/a.py:10"));
		assert!(formatted.contains("src/b.py:20-21"));
		assert!(formatted.contains("constraints: scope:global"));
		assert!(formatted.contains("code: value.first()"));
	}

	#[test]
	fn rejects_unknown_projection_field_with_suggestion() {
		let error =
			parse_query("symbol.search name:App\nproject nme uri").expect_err("unknown projection");
		let message = error.to_string();
		assert!(
			message.contains("unknown projection field `nme`"),
			"{message}"
		);
		assert!(message.contains("did you mean `name`?"), "{message}");
	}

	#[test]
	fn projected_formatter_emits_only_requested_symbol_fields() {
		let response = QueryResponse {
			generation: Some(WorkspaceGeneration(3)),
			result: QueryResult::SymbolList(SymbolListResult {
				rows: vec![SymbolDto {
					root: ".".to_string(),
					uri: "code+moniker://./lang:rs/fn:run()".to_string(),
					id: "id".to_string(),
					name: "run".to_string(),
					kind: "fn".to_string(),
					visibility: "public".to_string(),
					signature: "run()".to_string(),
					file: "src/lib.rs".to_string(),
					language: "rs".to_string(),
					line_range: Some((4, 8)),
					navigable: true,
					score: None,
					match_reason: None,
					source: None,
				}],
				total: 1,
			}),
			next_cursor: None,
		};
		let formatted =
			format_query_response_projected(&response, &["name".to_string(), "uri".to_string()]);
		assert!(
			formatted.contains("name=run uri=code+moniker://"),
			"{formatted}"
		);
		assert!(!formatted.contains("src/lib.rs"), "{formatted}");
		assert!(!formatted.contains("signature="), "{formatted}");
	}

	#[test]
	fn parses_human_symbol_search() {
		let query = parse_query(
			r#"symbol.search "SharedWorkspaceIndex"
  filter path:"crates/**" shape:type
  project name, kind, uri
  page limit:20 cursor:7:40"#,
		)
		.expect("query");
		assert_eq!(query.page.limit, 20);
		assert_eq!(
			query.page.cursor,
			Some(QueryCursor::new(40, Some(WorkspaceGeneration(7))))
		);
		match query.query {
			Query::SymbolSearch(search) => {
				assert_eq!(search.text.as_deref(), Some("SharedWorkspaceIndex"));
				assert_eq!(search.path, vec!["crates/**"]);
				assert_eq!(search.shape, vec!["type"]);
				assert_eq!(search.projection, vec!["name", "kind", "uri"]);
			}
			other => panic!("unexpected query {other:?}"),
		}
	}

	#[test]
	fn parses_rules_check_consistency() {
		let query = parse_query(
			r#"rules.check profile:"agent"
  consistency refresh-if-stale
  page limit:50"#,
		)
		.expect("query");
		assert_eq!(query.consistency, Consistency::RefreshIfStale);
		assert_eq!(query.page.limit, 50);
	}

	#[test]
	fn rejects_offset_only_human_cursor() {
		let error =
			parse_query("symbol.search Customer\npage cursor:40").expect_err("offset-only cursor");
		assert!(matches!(
			error,
			QueryParseError::InvalidValue { ref key, .. } if key == "cursor"
		));
	}

	#[test]
	fn parses_bracket_list_shape() {
		let query = parse_query("symbol.search shape:[callable,type] limit:5").expect("query");
		match query.query {
			Query::SymbolSearch(search) => assert_eq!(search.shape, vec!["callable", "type"]),
			other => panic!("unexpected query {other:?}"),
		}
	}

	#[test]
	fn rejects_unterminated_bracket_list() {
		let error = parse_query("symbol.search shape:[callable").expect_err("unterminated list");
		assert!(matches!(
			error,
			QueryParseError::InvalidValue { ref key, .. } if key == "shape"
		));
	}

	#[test]
	fn rejects_unknown_field_with_alias_suggestion() {
		let error = parse_query(r#"symbol.search text:"foo""#).expect_err("unknown field");
		let message = error.to_string();
		assert!(
			message.contains("unknown field `text` for `symbol.search`"),
			"{message}"
		);
		assert!(message.contains("did you mean `name`?"), "{message}");
	}

	#[test]
	fn rejects_typo_field_with_suggestion() {
		let error = parse_query("rules.check profil:agent").expect_err("typo field");
		let message = error.to_string();
		assert!(message.contains("did you mean `profile`?"), "{message}");
	}

	#[test]
	fn lists_valid_fields_without_close_match() {
		let error = parse_query("change.review foobarbaz:1").expect_err("unknown field");
		let message = error.to_string();
		assert!(
			message.contains("valid fields: consistency, cursor, limit, workspace"),
			"{message}"
		);
	}

	#[test]
	fn rejects_unexpected_positional() {
		let error = parse_query("workspace.status extra").expect_err("positional");
		assert!(matches!(
			error,
			QueryParseError::UnexpectedArgument { ref value, .. } if value == "extra"
		));
	}

	#[test]
	fn rejects_projection_on_unsupported_verb() {
		let error = parse_query("rules.list\nproject name").expect_err("projection");
		assert!(matches!(
			error,
			QueryParseError::UnsupportedProjection { .. }
		));
	}

	#[test]
	fn parses_inline_consistency() {
		let query =
			parse_query("rules.check profile:agent consistency:refresh-if-stale").expect("query");
		assert_eq!(query.consistency, Consistency::RefreshIfStale);
	}

	#[test]
	fn formats_generation_aware_cursor() {
		let response = QueryResponse {
			generation: Some(WorkspaceGeneration(7)),
			result: QueryResult::SymbolList(SymbolListResult {
				rows: Vec::new(),
				total: 0,
			}),
			next_cursor: Some(QueryCursor::new(40, Some(WorkspaceGeneration(7)))),
		};
		let formatted = format_query_response(&response);
		assert!(formatted.contains("next_cursor: 7:40"));
	}

	#[test]
	fn context_graph_internal_edges_join_member_names() {
		let member = |id: &str, name: &str| SymbolDto {
			root: String::new(),
			uri: format!("code+moniker://./module:m/fn:{name}"),
			id: id.to_string(),
			name: name.to_string(),
			kind: "fn".to_string(),
			visibility: "pub".to_string(),
			signature: String::new(),
			file: "src/lib.rs".to_string(),
			language: "rs".to_string(),
			line_range: None,
			navigable: true,
			score: None,
			match_reason: None,
			source: None,
		};
		let graph = SymbolGraphResult {
			focus: SymbolGraphFocus::File {
				path: "src/lib.rs".to_string(),
			},
			coverage: SymbolGraphCoverage::default(),
			members: vec![
				member("symbol:40:2", "alpha()"),
				member("symbol:40:7", "beta()"),
			],
			internal_edges: vec![
				SymbolGraphEdge {
					source: "symbol:40:2".to_string(),
					target: "symbol:40:7".to_string(),
					kinds: vec!["calls".to_string()],
					count: 2,
				},
				SymbolGraphEdge {
					source: "symbol:40:2".to_string(),
					target: "symbol:99:9".to_string(),
					kinds: vec!["reads".to_string()],
					count: 1,
				},
				SymbolGraphEdge {
					source: "symbol:98:8".to_string(),
					target: "symbol:99:9".to_string(),
					kinds: vec!["calls".to_string()],
					count: 1,
				},
			],
			callers: Vec::new(),
			callees: Vec::new(),
			unlinked: UnlinkedRefsDto::default(),
		};
		let mut out = String::new();
		format_context_graph(&mut out, &graph);
		assert!(
			out.contains("- fn alpha() -> fn beta() x2 [calls]"),
			"internal edges must join member names, got:\n{out}"
		);
		assert!(
			out.contains("- fn alpha() -> unlisted internal member 2 x1 [reads]"),
			"bounded contexts must explain omitted endpoints, got:\n{out}"
		);
		assert!(
			out.contains("- unlisted internal member 1 -> unlisted internal member 2 x1 [calls]"),
			"distinct omitted members must preserve the graph topology, got:\n{out}"
		);
		assert!(
			!out.contains("symbol:98:8") && !out.contains("symbol:99:9"),
			"internal storage ordinals must not leak into agent output:\n{out}"
		);
	}
}

/// Umbrella over every root RPC type, used only to emit a single JSON Schema
/// document (`export-schema`) whose definitions cover the whole wire contract.
#[cfg(feature = "schema")]
#[derive(schemars::JsonSchema)]
#[allow(dead_code)]
pub struct DaemonProtocol {
	pub handshake: HandshakeResponse,
	pub registry_entry: DaemonRegistryEntry,
	pub workspace_config: DaemonWorkspaceConfig,
	pub query_request: QueryRequest,
	pub query: Query,
	pub query_response: QueryResponse,
	pub query_result: QueryResult,
	pub command_request: CommandRequest,
	pub command_response: CommandResponse,
	pub event: WorkspaceEventDto,
	pub error: QueryError,
}

#[cfg(test)]
mod contract_tests {
	//! Lock the serde wire shapes the JSON Schema (and every generated client)
	//! depends on. These guard the contract, not the Rust layout.
	use super::*;
	use serde_json::json;

	#[test]
	fn query_is_op_tagged() {
		let query = Query::SymbolSearch(SymbolSearchQuery {
			text: Some("widget".to_string()),
			..Default::default()
		});
		let value = serde_json::to_value(&query).unwrap();
		assert_eq!(value["op"], "symbol_search");
		assert_eq!(value["text"], "widget");
	}

	#[test]
	fn query_result_is_kind_and_data_tagged() {
		let result = QueryResult::SymbolList(SymbolListResult {
			rows: Vec::new(),
			total: 0,
		});
		assert_eq!(
			serde_json::to_value(&result).unwrap(),
			json!({ "kind": "symbol_list", "data": { "rows": [], "total": 0 } }),
		);
	}

	#[test]
	fn generation_serializes_as_scalar() {
		assert_eq!(
			serde_json::to_value(WorkspaceGeneration(7)).unwrap(),
			json!(7)
		);
	}

	#[test]
	fn line_range_is_a_two_element_array() {
		let range: Option<(u32, u32)> = Some((3, 9));
		assert_eq!(serde_json::to_value(range).unwrap(), json!([3, 9]));
		assert_eq!(
			serde_json::to_value(Option::<(u32, u32)>::None).unwrap(),
			json!(null)
		);
	}

	#[test]
	fn consistency_is_snake_case() {
		assert_eq!(
			serde_json::to_value(Consistency::RefreshIfStale).unwrap(),
			json!("refresh_if_stale"),
		);
	}

	#[test]
	fn event_kind_is_snake_case() {
		let event = WorkspaceEventDto {
			kind: WorkspaceEventKind::GitBase,
			generation: None,
			stale_summary: None,
		};
		assert_eq!(serde_json::to_value(&event).unwrap()["kind"], "git_base");
	}

	#[test]
	fn workspace_source_set_commands_are_op_tagged() {
		let replace = Command::WorkspaceSourceSetReplace {
			source_set: WorkspaceSourceSetDto {
				srcset: "database".to_string(),
				revision: Some("42".to_string()),
				documents: vec![WorkspaceSourceDocumentDto {
					uri: "schema/accounts.sql".to_string(),
					language: "sql".to_string(),
					content: "CREATE TABLE accounts (id bigint);".to_string(),
				}],
			},
		};
		assert_eq!(
			serde_json::to_value(replace).unwrap(),
			json!({
				"op": "workspace_source_set_replace",
				"source_set": {
					"srcset": "database",
					"revision": "42",
					"documents": [{
						"uri": "schema/accounts.sql",
						"language": "sql",
						"content": "CREATE TABLE accounts (id bigint);"
					}]
				}
			}),
		);

		assert_eq!(
			serde_json::to_value(Command::WorkspaceSourceSetRemove {
				srcset: "database".to_string(),
			})
			.unwrap(),
			json!({
				"op": "workspace_source_set_remove",
				"srcset": "database"
			}),
		);
		assert!(
			CapabilitySet::default()
				.commands
				.iter()
				.any(|command| command == "workspace.source_set.replace")
		);
		assert!(
			CapabilitySet::default()
				.queries
				.iter()
				.any(|query| query == "diff-impact.compare")
		);
		assert_eq!(PROTOCOL_VERSION, 17);
	}

	#[test]
	fn test_artifact_classification_covers_canonical_paths_kinds_and_modules() {
		for path in [
			"tests/service.rs",
			"benches/speed.rs",
			"fixtures/input.py",
			"testdata/schema.sql",
			"src/__tests__/service.ts",
		] {
			assert!(symbol_is_test_artifact("function", path, ""), "{path}");
		}
		assert!(symbol_is_test_artifact("test", "src/lib.rs", ""));
		assert!(symbol_is_test_artifact(
			"function",
			"src/lib.rs",
			"code+moniker://module:tests/function:works()"
		));
		assert!(!symbol_is_test_artifact("function", "src/lib.rs", ""));
	}
}
