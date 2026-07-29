import { DaemonRpcError, WebSocketUnavailableError } from "./errors.js";

export interface WebSocketMessageEvent {
	data: unknown;
}

export interface WebSocketLike {
	addEventListener(
		type: "open" | "message" | "close" | "error",
		listener: (event: unknown) => void,
	): void;
	removeEventListener(
		type: "open" | "message" | "close" | "error",
		listener: (event: unknown) => void,
	): void;
	send(data: string): void;
	close(): void;
}

export type WebSocketFactory = (url: string) => WebSocketLike;

export interface RpcSubscription {
	dispose(): void;
}

interface PendingCall {
	resolve: (value: unknown) => void;
	reject: (error: Error) => void;
	timer: ReturnType<typeof setTimeout>;
}

interface JsonRpcMessage {
	id?: number | string;
	result?: unknown;
	error?: { code?: number; message?: string; data?: unknown };
	params?: unknown;
}

interface SubscriptionParams {
	subscription: string | number;
	result: unknown;
}

const DEFAULT_TIMEOUT_MS = 15_000;

export class RpcConnection {
	private nextId = 1;
	private readonly pending = new Map<number, PendingCall>();
	private readonly subscriptions = new Map<
		string | number,
		(item: unknown) => void
	>();
	private readonly closeListeners = new Set<() => void>();
	private closed = false;

	constructor(
		private readonly socket: WebSocketLike,
		private readonly timeoutMs: number,
	) {
		socket.addEventListener("message", this.handleMessage.bind(this));
		socket.addEventListener("close", this.handleClose.bind(this));
		socket.addEventListener("error", this.handleClose.bind(this));
	}

	static connect(
		url: string,
		options: {
			webSocketFactory?: WebSocketFactory;
			timeoutMs?: number;
		} = {},
	): Promise<RpcConnection> {
		const factory = options.webSocketFactory ?? defaultWebSocketFactory;
		const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
		return new ConnectionAttempt(url, factory, timeoutMs).connect();
	}

	onDidClose(listener: () => void): () => void {
		this.closeListeners.add(listener);
		return this.removeCloseListener.bind(this, listener);
	}

	call<T>(method: string, params: unknown[]): Promise<T> {
		if (this.closed) {
			return Promise.reject(new Error("daemon connection is closed"));
		}
		const id = this.nextId++;
		return new Promise<unknown>(
			this.startCall.bind(this, id, method, params),
		) as Promise<T>;
	}

	async subscribe(
		subscribeMethod: string,
		unsubscribeMethod: string,
		onItem: (item: unknown) => void,
	): Promise<RpcSubscription> {
		const subscription = await this.call<string | number>(subscribeMethod, []);
		this.subscriptions.set(subscription, onItem);
		return {
			dispose: this.disposeSubscription.bind(
				this,
				subscription,
				unsubscribeMethod,
			),
		};
	}

	close(): void {
		if (this.closed) {
			return;
		}
		this.closed = true;
		this.rejectPending("daemon connection closed");
		this.subscriptions.clear();
		this.socket.close();
	}

	private async onMessage(event: WebSocketMessageEvent): Promise<void> {
		const text = await messageText(event.data);
		if (text === undefined) {
			return;
		}
		let message: JsonRpcMessage;
		try {
			message = JSON.parse(text) as JsonRpcMessage;
		} catch {
			return;
		}
		if (typeof message.id === "number" && this.pending.has(message.id)) {
			this.settle(message.id, undefined, message);
			return;
		}
		const params = message.params;
		if (params && typeof params === "object" && "subscription" in params) {
			const subscription = params as SubscriptionParams;
			this.subscriptions
				.get(subscription.subscription)
				?.(subscription.result);
		}
	}

	private handleMessage(event: unknown): void {
		if (isMessageEvent(event)) {
			void this.onMessage(event);
		}
	}

	private handleClose(): void {
		this.onClose();
	}

	private removeCloseListener(listener: () => void): void {
		this.closeListeners.delete(listener);
	}

	private startCall(
		id: number,
		method: string,
		params: unknown[],
		resolve: (value: unknown) => void,
		reject: (reason?: unknown) => void,
	): void {
		const timer = setTimeout(
			this.timeoutCall.bind(this, id, method),
			this.timeoutMs,
		);
		this.pending.set(id, {
			resolve: resolve as (value: unknown) => void,
			reject: reject as (error: Error) => void,
			timer,
		});
		try {
			this.socket.send(
				JSON.stringify({ jsonrpc: "2.0", id, method, params }),
			);
		} catch (error) {
			this.settle(id, error as Error, undefined);
		}
	}

	private timeoutCall(id: number, method: string): void {
		const pending = this.pending.get(id);
		if (!pending) {
			return;
		}
		this.pending.delete(id);
		pending.reject(new Error(`daemon call ${method} timed out`));
	}

	private disposeSubscription(
		subscription: string | number,
		unsubscribeMethod: string,
	): void {
		if (!this.subscriptions.delete(subscription) || this.closed) {
			return;
		}
		void this.call(unsubscribeMethod, [subscription]).catch(ignoreError);
	}

	private settle(
		id: number,
		error: Error | undefined,
		message: JsonRpcMessage | undefined,
	): void {
		const pending = this.pending.get(id);
		if (!pending) {
			return;
		}
		this.pending.delete(id);
		clearTimeout(pending.timer);
		if (error) {
			pending.reject(error);
		} else if (message?.error) {
			pending.reject(rpcError(message.error));
		} else {
			pending.resolve(message?.result);
		}
	}

	private onClose(): void {
		if (this.closed) {
			return;
		}
		this.closed = true;
		this.rejectPending("daemon connection closed");
		this.subscriptions.clear();
		for (const listener of this.closeListeners) {
			listener();
		}
	}

	private rejectPending(message: string): void {
		for (const [id, pending] of this.pending) {
			clearTimeout(pending.timer);
			pending.reject(new Error(message));
			this.pending.delete(id);
		}
	}
}

class ConnectionAttempt {
	private socket?: WebSocketLike;
	private timer?: ReturnType<typeof setTimeout>;
	private resolve?: (connection: RpcConnection) => void;
	private reject?: (error: Error) => void;
	private readonly openHandler = this.onOpen.bind(this);
	private readonly errorHandler = this.onError.bind(this);

	constructor(
		private readonly url: string,
		private readonly factory: WebSocketFactory,
		private readonly timeoutMs: number,
	) {}

	connect(): Promise<RpcConnection> {
		return new Promise(this.start.bind(this));
	}

	private start(
		resolve: (connection: RpcConnection) => void,
		reject: (error: Error) => void,
	): void {
		this.resolve = resolve;
		this.reject = reject;
		try {
			this.socket = this.factory(this.url);
		} catch (error) {
			reject(error as Error);
			return;
		}
		this.timer = setTimeout(this.onTimeout.bind(this), this.timeoutMs);
		this.socket.addEventListener("error", this.errorHandler);
		this.socket.addEventListener("open", this.openHandler);
	}

	private onOpen(): void {
		const socket = this.socket;
		const resolve = this.resolve;
		if (!socket || !resolve) {
			return;
		}
		this.cleanup();
		resolve(new RpcConnection(socket, this.timeoutMs));
	}

	private onError(): void {
		const reject = this.reject;
		if (!reject) {
			return;
		}
		this.cleanup();
		this.socket?.close();
		reject(new Error(`daemon connection to ${this.url} failed`));
	}

	private onTimeout(): void {
		const reject = this.reject;
		if (!reject) {
			return;
		}
		this.cleanup();
		this.socket?.close();
		reject(new Error(`daemon connection to ${this.url} timed out`));
	}

	private cleanup(): void {
		if (this.timer !== undefined) {
			clearTimeout(this.timer);
			this.timer = undefined;
		}
		this.socket?.removeEventListener("open", this.openHandler);
		this.socket?.removeEventListener("error", this.errorHandler);
		this.resolve = undefined;
		this.reject = undefined;
	}
}

function defaultWebSocketFactory(url: string): WebSocketLike {
	const Constructor = globalThis.WebSocket;
	if (Constructor === undefined) {
		throw new WebSocketUnavailableError();
	}
	return new Constructor(url);
}

function isMessageEvent(event: unknown): event is WebSocketMessageEvent {
	return event !== null && typeof event === "object" && "data" in event;
}

async function messageText(data: unknown): Promise<string | undefined> {
	if (typeof data === "string") {
		return data;
	}
	if (data instanceof ArrayBuffer) {
		return new TextDecoder().decode(data);
	}
	if (ArrayBuffer.isView(data)) {
		return new TextDecoder().decode(data);
	}
	if (data instanceof Blob) {
		return data.text();
	}
	return undefined;
}

function rpcError(error: {
	message?: string;
	data?: unknown;
}): DaemonRpcError {
	const data = error.data;
	if (data && typeof data === "object") {
		const detail = data as { code?: unknown; message?: unknown };
		return new DaemonRpcError(
			typeof detail.message === "string"
				? detail.message
				: (error.message ?? "daemon error"),
			typeof detail.code === "string" ? detail.code : undefined,
		);
	}
	return new DaemonRpcError(error.message ?? "daemon error");
}

function ignoreError(): undefined {
	return undefined;
}
