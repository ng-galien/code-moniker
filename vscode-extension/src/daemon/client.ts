import WebSocket from "ws";

// Minimal JSON-RPC 2.0 client over a WebSocket, matching the jsonrpsee server the
// daemon exposes (positional params; subscriptions deliver notifications carrying a
// `{ subscription, result }` payload). One socket per daemon connection.

interface Pending {
	resolve: (value: unknown) => void;
	reject: (error: Error) => void;
	timer: ReturnType<typeof setTimeout>;
}

export interface RpcSubscription {
	dispose(): void;
}

const CALL_TIMEOUT_MS = 15000;

export class RpcConnection {
	private readonly socket: WebSocket;
	private nextId = 1;
	private readonly pending = new Map<number, Pending>();
	private readonly subscriptions = new Map<string | number, (item: unknown) => void>();
	private closed = false;
	private readonly closeListeners: Array<() => void> = [];

	private constructor(socket: WebSocket) {
		this.socket = socket;
		this.socket.on("message", (raw) => this.onMessage(raw.toString()));
		this.socket.on("close", () => this.onClose());
		this.socket.on("error", () => this.onClose());
	}

	static connect(url: string): Promise<RpcConnection> {
		return new Promise((resolve, reject) => {
			const socket = new WebSocket(url);
			const timer = setTimeout(() => {
				socket.removeAllListeners();
				socket.terminate();
				reject(new Error(`daemon connection to ${url} timed out`));
			}, CALL_TIMEOUT_MS);
			const onError = (err: Error) => {
				clearTimeout(timer);
				socket.removeAllListeners();
				reject(err);
			};
			socket.once("error", onError);
			socket.once("open", () => {
				clearTimeout(timer);
				socket.removeListener("error", onError);
				resolve(new RpcConnection(socket));
			});
		});
	}

	onDidClose(listener: () => void): void {
		this.closeListeners.push(listener);
	}

	call<T>(method: string, params: unknown[]): Promise<T> {
		if (this.closed) {
			return Promise.reject(new Error("daemon connection is closed"));
		}
		const id = this.nextId++;
		const payload = JSON.stringify({ jsonrpc: "2.0", id, method, params });
		return new Promise<T>((resolve, reject) => {
			const timer = setTimeout(() => {
				this.pending.delete(id);
				reject(new Error(`daemon call ${method} timed out`));
			}, CALL_TIMEOUT_MS);
			this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject, timer });
			this.socket.send(payload, (err) => {
				if (err) {
					this.settle(id, err, undefined);
				}
			});
		});
	}

	async subscribe(
		subscribeMethod: string,
		unsubscribeMethod: string,
		onItem: (item: unknown) => void,
	): Promise<RpcSubscription> {
		const subId = await this.call<string | number>(subscribeMethod, []);
		this.subscriptions.set(subId, onItem);
		return {
			dispose: () => {
				if (!this.subscriptions.delete(subId) || this.closed) {
					return;
				}
				void this.call(unsubscribeMethod, [subId]).catch(() => undefined);
			},
		};
	}

	close(): void {
		if (this.closed) {
			return;
		}
		this.closed = true;
		try {
			this.socket.close();
		} catch {
			// ignore
		}
	}

	private onMessage(text: string): void {
		let message: JsonRpcMessage;
		try {
			message = JSON.parse(text);
		} catch {
			return;
		}
		if (typeof message.id === "number" && this.pending.has(message.id)) {
			this.settle(message.id, undefined, message);
			return;
		}
		const params = message.params;
		if (params && typeof params === "object" && "subscription" in params) {
			const handler = this.subscriptions.get((params as SubscriptionParams).subscription);
			handler?.((params as SubscriptionParams).result);
		}
	}

	private settle(id: number, error: Error | undefined, message: JsonRpcMessage | undefined): void {
		const entry = this.pending.get(id);
		if (!entry) {
			return;
		}
		this.pending.delete(id);
		clearTimeout(entry.timer);
		if (error) {
			entry.reject(error);
			return;
		}
		if (message?.error) {
			entry.reject(new Error(rpcErrorMessage(message.error)));
			return;
		}
		entry.resolve(message?.result);
	}

	private onClose(): void {
		if (this.closed) {
			return;
		}
		this.closed = true;
		for (const [id, entry] of this.pending) {
			clearTimeout(entry.timer);
			entry.reject(new Error("daemon connection closed"));
			this.pending.delete(id);
		}
		this.subscriptions.clear();
		for (const listener of this.closeListeners) {
			listener();
		}
	}
}

interface JsonRpcMessage {
	id?: number | string;
	result?: unknown;
	error?: { code?: number; message?: string; data?: unknown };
	method?: string;
	params?: unknown;
}

interface SubscriptionParams {
	subscription: string | number;
	result: unknown;
}

function rpcErrorMessage(error: { message?: string; data?: unknown }): string {
	if (error.data && typeof error.data === "object" && "message" in error.data) {
		const inner = (error.data as { message?: unknown }).message;
		if (typeof inner === "string") {
			return inner;
		}
	}
	return error.message ?? "daemon error";
}
