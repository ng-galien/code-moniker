import { spawn } from "node:child_process";
import * as vscode from "vscode";

import { binaryCandidates } from "../cli/runner";
import { RpcSubscription } from "./client";
import {
	DaemonRegistryEntry,
	PROTOCOL_VERSION,
	Query,
	QueryResponse,
	WorkspaceEventDto,
	WorkspaceStatus,
} from "./model";
import { DaemonRpc, QueryOptions } from "./rpc";
import { findDaemonForRoots } from "./registry";

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
	private entry?: DaemonRegistryEntry;
	private _status: DaemonStatus = "disconnected";
	private errorMessage?: string;
	private connecting?: Promise<boolean>;

	private readonly statusEmitter = new vscode.EventEmitter<DaemonStatus>();
	readonly onDidChangeStatus = this.statusEmitter.event;
	private readonly eventEmitter = new vscode.EventEmitter<WorkspaceEventDto>();
	readonly onWorkspaceEvent = this.eventEmitter.event;

	constructor(private readonly roots: string[]) {}

	get status(): DaemonStatus {
		return this._status;
	}

	get ready(): boolean {
		return this._status === "ready";
	}

	get lastError(): string | undefined {
		return this.errorMessage;
	}

	get endpoint(): string | undefined {
		return this.entry?.endpoint;
	}

	get workspaceRoots(): string[] {
		return this.roots;
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
		// The daemon answers `workspace.status` even while indexing, but other queries
		// return a transient `workspace_loading` error while the snapshot builds or the
		// daemon lock is held mid-refresh. Honour its "retry once ready" contract here
		// so features never see the transient error.
		for (let attempt = 0; ; attempt++) {
			try {
				return await this.rpc.query(query, options);
			} catch (error) {
				if (attempt < QUERY_RETRY_ATTEMPTS && isLoadingError(error)) {
					await delay(QUERY_RETRY_INTERVAL_MS);
					continue;
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
				// daemon may already be gone
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
				launchDaemon(this.roots[0]);
				entry = await this.waitForEntry();
			}
			if (!entry) {
				throw new Error("daemon did not register for this workspace");
			}
			const rpc = await DaemonRpc.connect(entry.endpoint);
			const handshake = await rpc.handshake("vscode-extension");
			if (handshake.protocol_version !== PROTOCOL_VERSION) {
				rpc.close();
				throw new Error(`unsupported daemon protocol ${handshake.protocol_version}`);
			}
			rpc.onDidClose(() => this.onConnectionClosed());
			this.rpc = rpc;
			this.entry = entry;
			this.subscription = await rpc.subscribeEvents((event) => this.handleEvent(event));
			await this.waitUntilReady();
			return true;
		} catch (error) {
			this.errorMessage = (error as Error).message;
			this.teardown();
			this.setStatus("error");
			return false;
		}
	}

	private async waitForEntry(): Promise<DaemonRegistryEntry | undefined> {
		for (let attempt = 0; attempt < ENTRY_POLL_ATTEMPTS; attempt++) {
			const entry = findDaemonForRoots(this.roots);
			if (entry) {
				return entry;
			}
			await delay(ENTRY_POLL_INTERVAL_MS);
		}
		return undefined;
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
		// Stay in "loading"; events will flip us to ready once the scan completes.
	}

	private handleEvent(event: WorkspaceEventDto): void {
		if (event.kind === "refreshed" && this._status === "loading") {
			this.setStatus("ready");
		}
		this.eventEmitter.fire(event);
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
		this.entry = undefined;
	}

	private setStatus(status: DaemonStatus): void {
		if (this._status === status) {
			return;
		}
		this._status = status;
		if (status !== "error") {
			this.errorMessage = undefined;
		}
		this.statusEmitter.fire(status);
	}
}

function launchDaemon(root: string): void {
	const candidates = binaryCandidates();
	tryLaunch(candidates, 0, root);
}

function tryLaunch(candidates: string[], index: number, root: string): void {
	if (index >= candidates.length) {
		return;
	}
	const child = spawn(candidates[index], ["daemon", "start", root], {
		detached: true,
		stdio: "ignore",
	});
	child.once("error", (err: NodeJS.ErrnoException) => {
		if (err.code === "ENOENT") {
			tryLaunch(candidates, index + 1, root);
		}
	});
	child.unref();
}

function isLoadingError(error: unknown): boolean {
	return error instanceof Error && /workspace_loading|loading/i.test(error.message);
}

function delay(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}
