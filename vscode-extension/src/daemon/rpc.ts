import { RpcConnection, RpcSubscription } from "./client";
import {
	CommandResponse,
	Consistency,
	HandshakeResponse,
	Query,
	QueryRequest,
	QueryResponse,
	WorkspaceEventDto,
} from "./model";

// Typed `moniker_*` method wrappers over a raw JSON-RPC connection. The daemon's
// jsonrpsee trait takes a single positional argument per method.

export interface QueryOptions {
	consistency?: Consistency;
	limit?: number;
	cursor?: { offset: number; generation: number | null } | null;
}

export class DaemonRpc {
	constructor(private readonly connection: RpcConnection) {}

	static async connect(endpoint: string): Promise<DaemonRpc> {
		const connection = await RpcConnection.connect(`ws://${endpoint}`);
		return new DaemonRpc(connection);
	}

	onDidClose(listener: () => void): void {
		this.connection.onDidClose(listener);
	}

	handshake(client: string): Promise<HandshakeResponse> {
		return this.connection.call<HandshakeResponse>("moniker_handshake", [client]);
	}

	query(query: Query, options: QueryOptions = {}): Promise<QueryResponse> {
		const request: QueryRequest = {
			query,
			consistency: options.consistency ?? "current",
			page: { cursor: options.cursor ?? null, limit: options.limit ?? 200 },
		};
		return this.connection.call<QueryResponse>("moniker_query", [request]);
	}

	command(command: { op: string }): Promise<CommandResponse> {
		return this.connection.call<CommandResponse>("moniker_command", [{ command }]);
	}

	shutdown(): Promise<void> {
		return this.connection.call<void>("moniker_shutdown", []);
	}

	subscribeEvents(onEvent: (event: WorkspaceEventDto) => void): Promise<RpcSubscription> {
		return this.connection.subscribe(
			"moniker_subscribeEvents",
			"moniker_unsubscribeEvents",
			(item) => onEvent(item as WorkspaceEventDto),
		);
	}

	close(): void {
		this.connection.close();
	}
}
