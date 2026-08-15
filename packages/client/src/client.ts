import {
	ProtocolMismatchError,
	UnexpectedQueryResultError,
	WorkspaceMismatchError,
	WorkspaceTargetRequiredError,
} from "./errors.js";
import type {
	Command,
	CommandResponse,
	DiffImpactResult,
	HandshakeResponse,
	IdentityChildrenResult,
	IdentityGraphResult,
	Query,
	QueryCursor,
	QueryResult,
	DiffImpactCompareFile,
	SymbolDetailResult,
	SymbolGraphResult,
	SymbolListResult,
	SymbolUsagesResult,
	SyntaxTreeResult,
	UsageDirection,
	WorkspaceEventDto,
	WorkspaceGeneration,
	WorkspaceSourceSetDto,
	WorkspaceStatus,
} from "./generated.js";
import { PROTOCOL_VERSION } from "./protocol.js";
import {
	DaemonRpc,
	type QueryOptions,
	type RpcConnectOptions,
} from "./rpc.js";
import type { RpcSubscription } from "./transport.js";

interface ClientConnectBaseOptions extends RpcConnectOptions {
	clientName?: string;
}

export type ClientConnectOptions = ClientConnectBaseOptions &
	(
		| {
				expectedWorkspaceRoots: readonly [string, ...string[]];
				acceptAnyWorkspace?: never;
		  }
		| {
				acceptAnyWorkspace: true;
				expectedWorkspaceRoots?: never;
		  }
	);

export interface SymbolSearchOptions {
	workspace?: string | null;
	text?: string | null;
	path?: string[];
	language?: string[];
	kind?: string[];
	shape?: string[];
	name?: string | null;
	includeNonNavigable?: boolean;
	includeCode?: boolean;
	contextLines?: number;
	projection?: string[];
}

export interface SymbolDetailOptions {
	workspace?: string | null;
	contextLines?: number;
}

export interface SymbolUsagesOptions {
	workspace?: string | null;
	direction?: UsageDirection;
	includeDescendants?: boolean;
	path?: string[];
	language?: string[];
	projection?: string[];
}

export interface SymbolGraphOptions {
	workspace?: string | null;
	direction?: UsageDirection;
	relation?: string[];
	minCount?: number;
	includeInternal?: boolean;
}

export interface IdentityChildrenOptions {
	workspace?: string | null;
}

export interface IdentityGraphOptions extends IdentityChildrenOptions {
	path?: string[];
	minCount?: number;
}

export interface DiffImpactCompareOptions {
	scope: string;
	project?: string | null;
	base: WorkspaceSourceSetDto;
	head: WorkspaceSourceSetDto;
	files: DiffImpactCompareFile[];
}

export interface SyntaxRenderOptions {
	maxDepth?: number;
	maxNodes?: number;
	namedOnly?: boolean;
	includeText?: boolean;
	maxTextChars?: number;
}

export interface SyntaxTreeOptions extends SyntaxRenderOptions {
	workspace?: string | null;
}

export interface SyntaxParseOptions extends SyntaxRenderOptions {
	uri?: string | null;
}

type QueryKind = QueryResult["kind"];
type QueryData<Kind extends QueryKind> = Extract<
	QueryResult,
	{ kind: Kind }
>["data"];

export interface QueryPage<Data> {
	data: Data;
	generation: WorkspaceGeneration | null;
	nextCursor: QueryCursor | null;
}

export class CodeMonikerClient {
	readonly workspace: WorkspaceClient;
	readonly sources: SourceSetClient;
	readonly symbols: SymbolsClient;
	readonly graph: GraphClient;
	readonly syntax: SyntaxClient;
	readonly diffImpact: DiffImpactClient;
	readonly events: EventsClient;

	private constructor(
		private readonly rpc: DaemonRpc,
		readonly handshake: HandshakeResponse,
	) {
		this.workspace = new WorkspaceClient(this);
		this.sources = new SourceSetClient(this);
		this.symbols = new SymbolsClient(this);
		this.graph = new GraphClient(this);
		this.syntax = new SyntaxClient(this);
		this.diffImpact = new DiffImpactClient(this);
		this.events = new EventsClient(this);
	}

	static async connect(
		endpoint: string,
		options: ClientConnectOptions,
	): Promise<CodeMonikerClient> {
		validateWorkspaceTarget(options);
		const rpc = await DaemonRpc.connect(endpoint, options);
		try {
			const handshake = await rpc.handshake(
				options.clientName ?? "@code-moniker/client",
			);
			validateProtocol(handshake);
			if (options.expectedWorkspaceRoots !== undefined) {
				validateWorkspaceRoots(
					handshake,
					options.expectedWorkspaceRoots,
				);
			}
			return new CodeMonikerClient(rpc, handshake);
		} catch (error) {
			rpc.close();
			throw error;
		}
	}

	get capabilities(): HandshakeResponse["capabilities"] {
		return this.handshake.capabilities;
	}

	supportsQuery(name: string): boolean {
		return this.capabilities.queries.includes(name);
	}

	supportsCommand(name: string): boolean {
		return this.capabilities.commands.includes(name);
	}

	supportsEvent(name: string): boolean {
		return this.capabilities.events.includes(name);
	}

	query(query: Query, options?: QueryOptions) {
		return this.rpc.query(query, options);
	}

	command(command: Command): Promise<CommandResponse> {
		return this.rpc.command(command);
	}

	close(): void {
		this.rpc.close();
	}

	onDidClose(listener: () => void): () => void {
		return this.rpc.onDidClose(listener);
	}

	async queryData<Kind extends QueryKind>(
		query: Query,
		expected: Kind,
		options?: QueryOptions,
	): Promise<QueryData<Kind>> {
		const page = await this.queryPage(query, expected, options);
		return page.data;
	}

	async queryPage<Kind extends QueryKind>(
		query: Query,
		expected: Kind,
		options?: QueryOptions,
	): Promise<QueryPage<QueryData<Kind>>> {
		const response = await this.query(query, options);
		if (response.result.kind !== expected) {
			throw new UnexpectedQueryResultError(
				expected,
				response.result.kind,
			);
		}
		return {
			data: response.result.data as QueryData<Kind>,
			generation: response.generation ?? null,
			nextCursor: response.next_cursor ?? null,
		};
	}

	subscribeEvents(
		onEvent: (event: WorkspaceEventDto) => void,
	): Promise<RpcSubscription> {
		return this.rpc.subscribeEvents(onEvent);
	}
}

export class WorkspaceClient {
	constructor(private readonly client: CodeMonikerClient) {}

	status(options?: QueryOptions): Promise<WorkspaceStatus> {
		return this.client.queryData(
			{ op: "workspace_status" },
			"workspace_status",
			options,
		);
	}

	refresh(): Promise<CommandResponse> {
		return this.client.command({ op: "workspace_refresh" });
	}
}

export class SourceSetClient {
	constructor(private readonly client: CodeMonikerClient) {}

	replace(sourceSet: WorkspaceSourceSetDto): Promise<CommandResponse> {
		return this.client.command({
			op: "workspace_source_set_replace",
			source_set: sourceSet,
		});
	}

	remove(srcset: string): Promise<CommandResponse> {
		return this.client.command({
			op: "workspace_source_set_remove",
			srcset,
		});
	}
}

export class SymbolsClient {
	constructor(private readonly client: CodeMonikerClient) {}

	search(
		options: SymbolSearchOptions = {},
		queryOptions?: QueryOptions,
	): Promise<QueryPage<SymbolListResult>> {
		return this.client.queryPage(
			{
				op: "symbol_search",
				workspace: options.workspace ?? null,
				text: options.text ?? null,
				path: options.path ?? [],
				lang: options.language ?? [],
				kind: options.kind ?? [],
				shape: options.shape ?? [],
				name: options.name ?? null,
				include_non_navigable:
					options.includeNonNavigable ?? false,
				include_code: options.includeCode ?? false,
				context_lines: options.contextLines ?? 0,
				projection: options.projection ?? [],
			},
			"symbol_list",
			queryOptions,
		);
	}

	detail(
		uri: string,
		options: SymbolDetailOptions = {},
		queryOptions?: QueryOptions,
	): Promise<SymbolDetailResult> {
		return this.client.queryData(
			{
				op: "symbol_detail",
				workspace: options.workspace ?? null,
				uri,
				context_lines: options.contextLines ?? 0,
			},
			"symbol_detail",
			queryOptions,
		);
	}

	usages(
		uri: string,
		options: SymbolUsagesOptions = {},
		queryOptions?: QueryOptions,
	): Promise<QueryPage<SymbolUsagesResult>> {
		return this.client.queryPage(
			{
				op: "symbol_usages",
				workspace: options.workspace ?? null,
				uri,
				direction: options.direction ?? "incoming",
				path: options.path ?? [],
				lang: options.language ?? [],
				include_descendants: options.includeDescendants ?? false,
				projection: options.projection ?? [],
			},
			"symbol_usages",
			queryOptions,
		);
	}
}

export class GraphClient {
	constructor(private readonly client: CodeMonikerClient) {}

	symbol(
		focus: string,
		options: SymbolGraphOptions = {},
		queryOptions?: QueryOptions,
	): Promise<SymbolGraphResult> {
		return this.client.queryData(
			{
				op: "symbol_graph",
				workspace: options.workspace ?? null,
				focus,
				direction: options.direction ?? "both",
				relation: options.relation ?? [],
				min_count: options.minCount ?? 1,
				include_internal: options.includeInternal ?? true,
			},
			"symbol_graph",
			queryOptions,
		);
	}

	children(
		prefix: string,
		options: IdentityChildrenOptions = {},
		queryOptions?: QueryOptions,
	): Promise<IdentityChildrenResult> {
		return this.client.queryData(
			{
				op: "identity_children",
				workspace: options.workspace ?? null,
				prefix,
			},
			"identity_children",
			queryOptions,
		);
	}

	identity(
		prefix: string,
		options: IdentityGraphOptions = {},
		queryOptions?: QueryOptions,
	): Promise<QueryPage<IdentityGraphResult>> {
		return this.client.queryPage(
			{
				op: "identity_graph",
				workspace: options.workspace ?? null,
				prefix,
				path: options.path ?? [],
				min_count: options.minCount ?? 1,
			},
			"identity_graph",
			queryOptions,
		);
	}
}

export class SyntaxClient {
	constructor(private readonly client: CodeMonikerClient) {}

	tree(
		focus: string,
		options: SyntaxTreeOptions = {},
		queryOptions?: QueryOptions,
	): Promise<SyntaxTreeResult> {
		return this.client.queryData(
			{
				op: "syntax_tree",
				workspace: options.workspace ?? null,
				focus,
				...syntaxRenderQuery(options),
			},
			"syntax_tree",
			queryOptions,
		);
	}

	parse(
		language: string,
		source: string,
		options: SyntaxParseOptions = {},
		queryOptions?: QueryOptions,
	): Promise<SyntaxTreeResult> {
		return this.client.queryData(
			{
				op: "syntax_parse",
				language,
				source,
				uri: options.uri ?? null,
				...syntaxRenderQuery(options),
			},
			"syntax_tree",
			queryOptions,
		);
	}
}

export class DiffImpactClient {
	constructor(private readonly client: CodeMonikerClient) {}

	compare(
		options: DiffImpactCompareOptions,
		queryOptions?: QueryOptions,
	): Promise<DiffImpactResult> {
		return this.client.queryData(
			{
				op: "diff_impact_compare",
				scope: options.scope,
				project: options.project ?? null,
				base: options.base,
				head: options.head,
				files: options.files,
			},
			"diff_impact",
			queryOptions,
		);
	}
}

export class EventsClient {
	constructor(private readonly client: CodeMonikerClient) {}

	subscribe(
		onEvent: (event: WorkspaceEventDto) => void,
	): Promise<RpcSubscription> {
		return this.client.subscribeEvents(onEvent);
	}
}

function syntaxRenderQuery(options: SyntaxRenderOptions) {
	return {
		max_depth: options.maxDepth ?? 6,
		max_nodes: options.maxNodes ?? 100,
		named_only: options.namedOnly ?? true,
		include_text: options.includeText ?? false,
		max_text_chars: options.maxTextChars ?? 80,
	};
}

function validateProtocol(handshake: HandshakeResponse): void {
	if (handshake.protocol_version !== PROTOCOL_VERSION) {
		throw new ProtocolMismatchError(
			PROTOCOL_VERSION,
			handshake.protocol_version,
		);
	}
}

function validateWorkspaceTarget(
	options: ClientConnectOptions | undefined,
): asserts options is ClientConnectOptions {
	const expected = options?.expectedWorkspaceRoots;
	const hasExpectedRoots =
		Array.isArray(expected) &&
		expected.length > 0 &&
		expected.every(isNonEmptyString);
	const acceptsAnyWorkspace = options?.acceptAnyWorkspace === true;
	if (hasExpectedRoots === acceptsAnyWorkspace) {
		throw new WorkspaceTargetRequiredError();
	}
}

function validateWorkspaceRoots(
	handshake: HandshakeResponse,
	expectedWorkspaceRoots: readonly string[],
): void {
	if (!sameRoots(expectedWorkspaceRoots, handshake.workspace_roots)) {
		throw new WorkspaceMismatchError(
			expectedWorkspaceRoots,
			handshake.workspace_roots,
		);
	}
}

function isNonEmptyString(value: unknown): value is string {
	return typeof value === "string" && value.length > 0;
}

function sameRoots(
	expected: readonly string[],
	actual: readonly string[],
): boolean {
	const wanted = new Set(expected);
	const served = new Set(actual);
	if (
		wanted.size !== expected.length ||
		served.size !== actual.length ||
		wanted.size !== served.size
	) {
		return false;
	}
	for (const root of served) {
		if (!wanted.has(root)) {
			return false;
		}
	}
	return true;
}
