import type {
	Command,
	CommandResponse,
	Consistency,
	HandshakeResponse,
	Query,
	QueryCursor,
	QueryRequest,
	QueryResponse,
	WorkspaceEventDto,
} from "./generated.js";
import {
	RpcConnection,
	type RpcSubscription,
	type WebSocketFactory,
} from "./transport.js";

export interface QueryOptions {
	consistency?: Consistency;
	limit?: number;
	cursor?: QueryCursor | null;
}

export interface RpcConnectOptions {
	webSocketFactory?: WebSocketFactory;
	timeoutMs?: number;
}

export class DaemonRpc {
	private constructor(private readonly connection: RpcConnection) {}

	static async connect(
		endpoint: string,
		options: RpcConnectOptions = {},
	): Promise<DaemonRpc> {
		const connection = await RpcConnection.connect(
			normalizeEndpoint(endpoint),
			options,
		);
		return new DaemonRpc(connection);
	}

	onDidClose(listener: () => void): () => void {
		return this.connection.onDidClose(listener);
	}

	handshake(client: string): Promise<HandshakeResponse> {
		return this.connection.call<HandshakeResponse>("moniker_handshake", [
			client,
		]);
	}

	query(query: Query, options: QueryOptions = {}): Promise<QueryResponse> {
		const request: QueryRequest = {
			query,
			consistency: options.consistency ?? "current",
			page: {
				cursor: options.cursor ?? null,
				limit: options.limit ?? 200,
			},
		};
		return this.connection.call<QueryResponse>("moniker_query", [request]);
	}

	command(command: Command): Promise<CommandResponse> {
		return this.connection.call<CommandResponse>("moniker_command", [
			{ command },
		]);
	}

	shutdown(): Promise<void> {
		return this.connection.call<void>("moniker_shutdown", []);
	}

	subscribeEvents(
		onEvent: (event: WorkspaceEventDto) => void,
	): Promise<RpcSubscription> {
		return this.connection.subscribe(
			"moniker_subscribeEvents",
			"moniker_unsubscribeEvents",
			this.forwardEvent.bind(this, onEvent),
		);
	}

	close(): void {
		this.connection.close();
	}

	private forwardEvent(
		onEvent: (event: WorkspaceEventDto) => void,
		item: unknown,
	): void {
		onEvent(item as WorkspaceEventDto);
	}
}

function normalizeEndpoint(endpoint: string): string {
	if (endpoint.startsWith("ws://") || endpoint.startsWith("wss://")) {
		return endpoint;
	}
	return `ws://${endpoint}`;
}
