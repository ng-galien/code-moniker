import * as vscode from "vscode";

import { launchWorkspaceDaemon } from "../cli/facade";
import { DaemonRpcError, RpcSubscription } from "./client";
import {
	DaemonRegistryEntry,
	PROTOCOL_VERSION,
	Query,
	QueryResponse,
	WorkspaceEventDto,
	WorkspaceStatus,
} from "./model";
import { DaemonRpc, QueryOptions } from "./rpc";
import { findDaemonForRoots, forgetDaemonsForRoots } from "./registry";

// The single live connection to the workspace daemon. Every feature (symbols,
// rules) talks to the daemon through this session, never to the raw client —
// it owns connect-or-start, the loading/ready phase, reconnection and events.

export type DaemonStatus = "disconnected" | "connecting" | "loading" | "ready" | "error";

const ENTRY_POLL_ATTEMPTS = 50;
const ENTRY_POLL_INTERVAL_MS = 100;
const READY_POLL_ATTEMPTS = 300;
const READY_POLL_INTERVAL_MS = 200;
const QUERY_RETRY_ATTEMPTS = 60;
const QUERY_RETRY_INTERVAL_MS = 200;

export class DaemonSession implements vscode.Disposable {
	private rpc?: DaemonRpc;
	private subscription?: RpcSubscription;
	private connecting?: Promise<boolean>;

	status: DaemonStatus = "disconnected";
	ready = false;
	lastError?: string;
	endpoint?: string;
	generation?: number;
	readonly workspaceRoots: string[];

	private readonly statusEmitter = new vscode.EventEmitter<DaemonStatus>();
	readonly onDidChangeStatus = this.statusEmitter.event;
	private readonly eventEmitter = new vscode.EventEmitter<WorkspaceEventDto>();
	readonly onWorkspaceEvent = this.eventEmitter.event;

	constructor(private readonly roots: string[]) {
		this.workspaceRoots = roots;
	}

	connectOrStart(): Promise<boolean> {
		if (this.connecting) {
			return this.connecting;
		}
		if (this.rpc) {
			return Promise.resolve(true);
		}
		this.connecting = this.doConnect().finally(() => {
			this.connecting = undefined;
		});
		return this.connecting;
	}

	async query(query: Query, options?: QueryOptions): Promise<QueryResponse> {
		if (!this.rpc) {
			throw new Error("daemon not connected");
		}
		const queryOptions = {
			...options,
			consistency: options?.consistency ?? "refresh_if_stale" as const,
		};
		for (let attempt = 0; ; attempt++) {
			try {
				const response = await this.rpc.query(query, queryOptions);
				this.noteGeneration(response.generation);
				return response;
			} catch (error) {
				if (shouldRetryLoadingQuery(error, attempt)) {
					await delay(QUERY_RETRY_INTERVAL_MS);
					continue;
				}
				if (shouldRefreshStaleSnapshot(error, attempt)) {
					const response = await this.rpc.query(query, {
						...queryOptions,
						consistency: "refresh_if_stale",
					});
					this.noteGeneration(response.generation);
					return response;
				}
				throw error;
			}
		}
	}

	async workspaceStatus(): Promise<WorkspaceStatus | undefined> {
		const response = await this.query({ op: "workspace_status" });
		return response.result.kind === "workspace_status"
			? (response.result.data as WorkspaceStatus)
			: undefined;
	}

	async refresh(): Promise<void> {
		if (this.rpc) {
			await this.rpc.command({ op: "workspace_refresh" });
		}
	}

	async stop(): Promise<void> {
		if (this.rpc) {
			try {
				await this.rpc.shutdown();
			} catch {
			}
		}
		this.teardown();
		this.setStatus("disconnected");
	}

	dispose(): void {
		this.teardown();
		this.statusEmitter.dispose();
		this.eventEmitter.dispose();
	}

	private async doConnect(): Promise<boolean> {
		if (this.roots.length === 0) {
			return false;
		}
		this.setStatus("connecting");
		try {
			let entry = findDaemonForRoots(this.roots);
			if (!entry) {
				launchWorkspaceDaemon(this.roots[0]);
				entry = await waitForEntry(this.roots);
			}
			if (!entry) {
				throw daemonRegistrationError("starting");
			}
			let rpc: DaemonRpc;
			try {
				rpc = await connectEntry(entry);
			} catch (error) {
				if (isProtocolError(error)) {
					throw error;
				}
				forgetDaemonsForRoots(this.roots);
				launchWorkspaceDaemon(this.roots[0]);
				entry = await waitForEntry(this.roots);
				if (!entry) {
					throw daemonRegistrationError("restarting after a stale registry entry");
				}
				rpc = await connectEntry(entry);
			}
			rpc.onDidClose(() => this.onConnectionClosed());
			this.rpc = rpc;
			this.endpoint = entry.endpoint;
			this.subscription = await rpc.subscribeEvents((event) => this.handleEvent(event));
			await this.waitUntilReady();
			return true;
		} catch (error) {
			this.lastError = (error as Error).message;
			this.teardown();
			this.setStatus("error");
			return false;
		}
	}

	private async waitUntilReady(): Promise<void> {
		this.setStatus("loading");
		for (let attempt = 0; attempt < READY_POLL_ATTEMPTS; attempt++) {
			const status = await this.workspaceStatus();
			if (status?.phase === "ready") {
				this.setStatus("ready");
				return;
			}
			await delay(READY_POLL_INTERVAL_MS);
		}
	}

	private handleEvent(event: WorkspaceEventDto): void {
		this.noteGeneration(event.generation);
		if (event.kind === "refreshed" && this.status === "loading") {
			this.setStatus("ready");
		}
		this.eventEmitter.fire(event);
	}

	private noteGeneration(generation: number | null | undefined): void {
		if (typeof generation === "number") {
			this.generation = generation;
		}
	}

	private onConnectionClosed(): void {
		this.teardown();
		this.setStatus("disconnected");
	}

	private teardown(): void {
		this.subscription?.dispose();
		this.subscription = undefined;
		this.rpc?.close();
		this.rpc = undefined;
		this.endpoint = undefined;
		this.generation = undefined;
	}

	private setStatus(status: DaemonStatus): void {
		if (this.status === status) {
			return;
		}
		this.status = status;
		this.ready = status === "ready";
		if (status !== "error") {
			this.lastError = undefined;
		}
		this.statusEmitter.fire(status);
	}
}

async function connectEntry(entry: DaemonRegistryEntry): Promise<DaemonRpc> {
	const rpc = await DaemonRpc.connect(entry.endpoint);
	const handshake = await rpc.handshake("vscode-extension");
	if (handshake.protocol_version !== PROTOCOL_VERSION) {
		rpc.close();
		throw new Error(`unsupported daemon protocol ${handshake.protocol_version}`);
	}
	return rpc;
}

async function waitForEntry(roots: string[]): Promise<DaemonRegistryEntry | undefined> {
	for (let attempt = 0; attempt < ENTRY_POLL_ATTEMPTS; attempt++) {
		const entry = findDaemonForRoots(roots);
		if (entry) {
			return entry;
		}
		await delay(ENTRY_POLL_INTERVAL_MS);
	}
	return undefined;
}

function isLoadingError(error: unknown): boolean {
	return error instanceof DaemonRpcError && error.code === "workspace_loading";
}

function isStaleError(error: unknown): boolean {
	return error instanceof DaemonRpcError && error.code === "workspace_stale";
}

function shouldRetryLoadingQuery(error: unknown, attempt: number): boolean {
	return attempt < QUERY_RETRY_ATTEMPTS && isLoadingError(error);
}

function shouldRefreshStaleSnapshot(error: unknown, attempt: number): boolean {
	return attempt === 0 && isStaleError(error);
}

function isProtocolError(error: unknown): boolean {
	return error instanceof Error && error.message.startsWith("unsupported daemon protocol ");
}

function daemonRegistrationError(action: string): Error {
	const waitedMs = ENTRY_POLL_ATTEMPTS * ENTRY_POLL_INTERVAL_MS;
	return new Error(
		`daemon did not register for this workspace after ${action} ` +
			`within ${waitedMs}ms; check codeMoniker.binaryPath and daemon startup logs`,
	);
}

function delay(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}
