// Wire types mirroring the daemon's JSON-RPC contract (crates/query/src/lib.rs).
// These match the serde shapes exactly: Query is tagged `{ op }`, QueryResult is
// tagged `{ kind, data }`, WorkspaceGeneration is a bare number, line ranges are
// `[start, end]` tuples.

export type LineRange = [number, number];

export interface DaemonRegistryEntry {
	workspace_root: string;
	workspace_roots: string[];
	project?: string | null;
	cache_dir?: string | null;
	live_refresh?: string | null;
	endpoint: string;
	token: string;
	pid: number;
}

export interface HandshakeResponse {
	protocol_version: number;
	daemon_version: string;
	workspace_root: string;
	workspace_roots: string[];
	capabilities: {
		queries: string[];
		commands: string[];
		events: string[];
	};
}

export type Consistency = "current" | "refresh_if_stale" | "stale_ok";

export interface QueryCursor {
	offset: number;
	generation: number | null;
}

export interface Page {
	cursor: QueryCursor | null;
	limit: number;
}

export interface QueryRequest {
	query: Query;
	consistency: Consistency;
	page: Page;
}

export type Query =
	| { op: "workspace_status" }
	| {
			op: "tree_children";
			workspace: string | null;
			path: string[];
			depth: number;
			lang: string[];
			projection: string[];
	  }
	| ({ op: "symbol_search" } & SymbolSearchQuery)
	| ({ op: "symbol_insights" } & SymbolSearchQuery)
	| { op: "symbol_detail"; workspace: string | null; uri: string; context_lines: number }
	| {
			op: "symbol_usages";
			workspace: string | null;
			uri: string;
			direction: UsageDirection;
			path: string[];
			lang: string[];
			projection: string[];
	  }
	| { op: "rules_list"; workspace: string | null; profile: string | null; rules: string | null; lang: string[]; severity: string[] }
	| {
			op: "rules_check";
			workspace: string | null;
			profile: string | null;
			rules: string | null;
			file: string[];
			report: boolean;
	  };

export interface SymbolSearchQuery {
	workspace: string | null;
	text: string | null;
	path: string[];
	lang: string[];
	kind: string[];
	shape: string[];
	name: string | null;
	include_non_navigable: boolean;
	include_code: boolean;
	context_lines: number;
	projection: string[];
}

export type UsageDirection = "incoming" | "outgoing" | "both";

export interface QueryResponse {
	generation: number | null;
	result: QueryResult;
	next_cursor: QueryCursor | null;
}

// Only the result kinds this extension consumes are modelled. Other kinds the
// daemon may return (symbol_insights, view_read, notes) simply never match.
export type QueryResult =
	| { kind: "workspace_status"; data: WorkspaceStatus }
	| { kind: "tree_children"; data: TreeChildrenResult }
	| { kind: "symbol_list"; data: SymbolListResult }
	| { kind: "symbol_detail"; data: SymbolDetailResult }
	| { kind: "symbol_usages"; data: SymbolUsagesResult }
	| { kind: "rules_list"; data: RulesListResult }
	| { kind: "rules_check"; data: RulesCheckResult };

export interface WorkspaceStatus {
	root: string;
	phase: "loading" | "ready" | string;
	roots: WorkspaceRootStatus[];
	generation: number | null;
	files: number;
	symbols: number;
	references: number;
	stale: boolean;
	stale_summary: string;
}

export interface WorkspaceRootStatus {
	root: string;
	generation: number | null;
	files: number;
	symbols: number;
	references: number;
	stale: boolean;
	stale_summary: string;
}

export interface CountDto {
	name: string;
	count: number;
}

export interface TreeNode {
	root: string;
	path: string;
	kind: "file" | "directory";
	language: string | null;
	defs: number;
	refs: number;
	change_count: number;
}

export interface TreeChildrenResult {
	root: string;
	roots: string[];
	rows: TreeNode[];
	total: number;
	total_files: number;
	scoped_files: number;
	languages: CountDto[];
	prefixes: CountDto[];
}

export interface SymbolDto {
	root: string;
	uri: string;
	id: string;
	name: string;
	kind: string;
	visibility: string;
	signature: string;
	file: string;
	language: string;
	line_range: LineRange | null;
	navigable: boolean;
	score: number | null;
	match_reason: string | null;
	source: SourceSnippet | null;
}

export interface SourceLine {
	number: number;
	text: string;
}

export interface SourceSnippet {
	file: string;
	first_line: number;
	last_line: number;
	lines: SourceLine[];
}

export interface SymbolListResult {
	rows: SymbolDto[];
	total: number;
}

export interface SymbolDetailResult {
	symbol: SymbolDto;
	source: SourceSnippet | null;
}

export interface UsageDto {
	root: string;
	direction: UsageDirection;
	reference: string;
	kind: string;
	actor: string;
	context: string;
	endpoint: string;
	file: string;
	prefix: string;
	location: string;
	line_range: LineRange | null;
	via: string | null;
}

export interface UsageSummaryDto {
	refs: number;
	files: number;
	contexts: number;
	prefixes: number;
	dominant_prefix: string;
	kinds: CountDto[];
	top_actors: CountDto[];
	top_prefixes: CountDto[];
	shared_helper_signal: string;
}

export interface SymbolUsagesResult {
	target: SymbolDto;
	direction: UsageDirection;
	rows: UsageDto[];
	total: number;
	incoming_summary: UsageSummaryDto | null;
	outgoing_summary: UsageSummaryDto | null;
}

export interface RuleDto {
	root: string;
	id: string;
	severity: string;
	lang: string;
	domain: string;
	kind: string | null;
	expr: string;
	expanded_expr: string;
	message: string | null;
	rationale: string | null;
	require_doc_comment: string | null;
}

export interface RulesListResult {
	roots: string[];
	rows: RuleDto[];
	total: number;
}

export interface ViolationDto {
	root: string;
	path: string;
	rule_id: string;
	severity: string;
	moniker: string;
	kind: string;
	lines: LineRange;
	message: string;
}

export interface CheckSummaryDto {
	files_scanned: number;
	files_with_violations: number;
	total_violations: number;
	total_rule_errors: number;
	total_warnings: number;
	files_with_errors: number;
	total_errors: number;
	elapsed_ms: number;
}

export interface RulesCheckResult {
	exit: string;
	summary: CheckSummaryDto;
	violations: ViolationDto[];
}

export interface CommandResponse {
	generation: number | null;
	message: string;
	status: WorkspaceStatus | null;
}

export type WorkspaceEventKind = "stale" | "refreshed" | "notes" | "git_base";

export interface WorkspaceEventDto {
	kind: WorkspaceEventKind;
	generation: number | null;
	stale_summary: string | null;
}

export const PROTOCOL_VERSION = 1;
