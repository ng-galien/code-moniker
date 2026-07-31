import * as vscode from "vscode";

import {
	CodeMonikerClient,
	DaemonRpcError,
	ProtocolMismatchError,
	type CapabilitySet,
	type DaemonRegistryEntry,
	type Query,
	type QueryOptions,
	type QueryResponse,
	type RpcSubscription,
	type WorkspaceEventDto,
	type WorkspacePhase,
	type WorkspaceStatus,
} from "@code-moniker/client";
import type { OwnedDaemon } from "@code-moniker/client/node";

import { binaryCandidates } from "../cli/runner";
import { daemonRuntime } from "./runtime";
import { withShutdownCleanup } from "./shutdown";
import {
	nonEmptyBinaryCandidates,
	nonEmptyRoots,
	protocolFaultMessage,
} from "./sessionSupport";

// The single live connection to the workspace daemon. Every feature (symbols,
// rules) talks to the daemon through this session, never to the raw client. It
// owns connection/reconnection and projects the protocol-owned WorkspacePhase
// into UI state without recreating workspace lifecycle policy.

export type DaemonStatus = "disconnected" | "connecting" | "loading" | "ready" | "error";
export type DaemonConnectionStatus = "disconnected" | "connecting" | "connected" | "error";

// User-facing freshness policy (codeMoniker.daemon.consistency):
// - fresh: every query carries refresh_if_stale, so answers never lag the
//   files but navigation waits for reindexing.
// - hybrid (default): navigation reads the current snapshot instantly
//   (stale_ok); a stale event schedules one background workspace_refresh
//   whose refreshed event rolls the generation and re-fills the trees.
// - current: stale_ok reads only; reindexing happens on explicit command.
export type ConsistencyMode = "fresh" | "hybrid" | "current";

const HYBRID_REFRESH_DEBOUNCE_MS = 300;
const RECONNECT_DELAY_MS = 500;

function consistencyMode(): ConsistencyMode {
	const value = vscode.workspace
		.getConfiguration("codeMoniker")
		.get<string>("daemon.consistency");
	return value === "fresh" || value === "current" ? value : "hybrid";
}

export class DaemonSession implements vscode.Disposable {
	private rpc?: CodeMonikerClient;
	private subscription?: RpcSubscription;
	private connecting?: Promise<boolean>;
	private hybridRefresh?: NodeJS.Timeout;
	private reconnectTimer?: NodeJS.Timeout;
	private reconnectEnabled = false;
	private disposed = false;
	private shuttingDown = false;
	private ownedDaemon?: OwnedDaemon;

	connectionStatus: DaemonConnectionStatus = "disconnected";
	workspacePhase?: WorkspacePhase;
	lastError?: string;
	// Sticky installation fault: the extension and the installed CLI do not
	// speak the same protocol. While set, connectOrStart fails fast instead of
	// killing and relaunching daemons on every retry — only a deliberate user
	// reconnect (after reinstalling one side) clears it.
	protocolFault?: string;
	endpoint?: string;
	generation?: number;
	capabilities?: CapabilitySet;
	readonly workspaceRoots: string[];

	get status(): DaemonStatus {
		switch (this.connectionStatus) {
			case "connected":
				if (this.workspacePhase === "failed") {
					return "error";
				}
				return this.workspacePhase === "ready" || this.workspacePhase === "refreshing"
					? "ready"
					: "loading";
			case "connecting":
			case "error":
			case "disconnected":
				return this.connectionStatus;
		}
	}

	get ready(): boolean {
		return (
			this.connectionStatus === "connected" &&
			(this.workspacePhase === "ready" || this.workspacePhase === "refreshing")
		);
	}

	// The protocol version guards the wire shape. Query capabilities still guard
	// individual verbs because the package version string is only informational.
	supportsQuery(name: string): boolean {
		return this.capabilities?.queries.includes(name) ?? false;
	}

	private readonly statusEmitter = new vscode.EventEmitter<DaemonStatus>();
	readonly onDidChangeStatus = this.statusEmitter.event;
	private readonly eventEmitter = new vscode.EventEmitter<WorkspaceEventDto>();
	readonly onWorkspaceEvent = this.eventEmitter.event;

	constructor(private readonly roots: string[]) {
		this.workspaceRoots = roots;
	}

	// retryFault is the deliberate-reconnect escape hatch: the fault is an
	// installation state, so only a user gesture may retry past it.
	connectOrStart(options?: { retryFault?: boolean }): Promise<boolean> {
		if (this.disposed || this.shuttingDown) {
			return Promise.resolve(false);
		}
		if (options?.retryFault) {
			this.protocolFault = undefined;
		}
		if (this.protocolFault) {
			this.lastError = this.protocolFault;
			this.setConnectionStatus("error");
			return Promise.resolve(false);
		}
		this.reconnectEnabled = true;
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
		const mode = consistencyMode();
		const queryOptions = {
			...options,
			consistency:
				options?.consistency ?? (mode === "fresh" ? ("refresh_if_stale" as const) : ("stale_ok" as const)),
		};
		try {
			const response = await this.rpc.query(query, queryOptions);
			this.noteGeneration(response.generation);
			return response;
		} catch (error) {
			if (!isStaleError(error)) {
				throw error;
			}
			const response = await this.rpc.query(query, {
				...queryOptions,
				consistency: "refresh_if_stale",
			});
			this.noteGeneration(response.generation);
			return response;
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
		this.shuttingDown = true;
		this.reconnectEnabled = false;
		this.clearReconnectTimer();
		await withShutdownCleanup(
			async () => {
				await this.connecting;
				const entry = daemonRuntime.findDaemon(this.roots);
				if (!entry) {
					if (this.ownedDaemon) {
						await daemonRuntime.stopOwned(this.ownedDaemon);
					}
					return;
				}
				try {
					await daemonRuntime.stop(entry);
				} catch (error) {
					if (!this.ownedDaemon) {
						throw error;
					}
					await daemonRuntime.stopOwned(this.ownedDaemon);
				}
			},
			() => {
				this.clearOwnership();
				this.teardown();
				this.setConnectionStatus("disconnected");
				this.shuttingDown = false;
			},
		);
	}

	async shutdownOwned(): Promise<void> {
		this.disposed = true;
		this.shuttingDown = true;
		this.reconnectEnabled = false;
		this.clearReconnectTimer();
		await withShutdownCleanup(
			async () => {
				await this.connecting;
				if (this.ownedDaemon) {
					await daemonRuntime.stopOwned(this.ownedDaemon);
				}
			},
			() => {
				this.clearOwnership();
				this.teardown();
				this.setConnectionStatus("disconnected");
				this.shuttingDown = false;
			},
		);
	}

	dispose(): void {
		this.disposed = true;
		this.reconnectEnabled = false;
		this.clearReconnectTimer();
		this.teardown();
		this.statusEmitter.dispose();
		this.eventEmitter.dispose();
	}

	private async doConnect(): Promise<boolean> {
		if (this.roots.length === 0) {
			return false;
		}
		this.setConnectionStatus("connecting");
		try {
			let entry = daemonRuntime.findDaemon(this.roots);
			let launched: OwnedDaemon | undefined;
			if (!entry) {
				launched = await daemonRuntime.launch({
					workspaceRoots: nonEmptyRoots(this.roots),
					binaryCandidates: nonEmptyBinaryCandidates(
						binaryCandidates(),
					),
				});
				this.ownedDaemon = launched;
				entry = launched.entry;
			}
			this.noteOwnership(entry, launched);
			this.ensureActive();
			let link: DaemonLink;
			try {
				link = await connectEntry(entry);
			} catch (error) {
				if (error instanceof ProtocolMismatchError) {
					// A newer daemon serves other up-to-date clients, so
					// relaunching from this extension cannot help.
					if (error.direction !== "daemon_older") {
						throw error;
					}
				}
				({ link, entry } = await this.relaunchAndReconnect(entry, error));
			}
			this.ensureActive();
			const rpc = link.client;
			rpc.onDidClose(() => this.onConnectionClosed());
			this.rpc = rpc;
			this.capabilities = rpc.capabilities;
			this.endpoint = entry.endpoint;
			this.subscription = await rpc.events.subscribe((event) =>
				this.handleEvent(event),
			);
			this.ensureActive();
			await this.syncWorkspaceStatus();
			this.ensureActive();
			return true;
		} catch (error) {
			if (this.disposed || this.shuttingDown) {
				this.teardown();
				this.setConnectionStatus("disconnected");
				return false;
			}
			if (error instanceof ProtocolMismatchError) {
				this.protocolFault = protocolFaultMessage(error);
			}
			this.lastError = (error as Error).message;
			this.teardown();
			this.setConnectionStatus("error");
			return false;
		}
	}

	private ensureActive(): void {
		if (this.disposed || this.shuttingDown) {
			throw new Error("daemon session is shutting down");
		}
	}

	// One recycle only: replace the failed daemon and reconnect once.
	private async relaunchAndReconnect(
		entry: DaemonRegistryEntry,
		error: unknown,
	): Promise<{ link: DaemonLink; entry: DaemonRegistryEntry }> {
		this.ensureActive();
		let launched: OwnedDaemon;
		if (error instanceof ProtocolMismatchError) {
			launched = await daemonRuntime.restart(
				entry,
				this.daemonLaunchOptions(),
			);
		} else {
			const current = daemonRuntime.findDaemon(this.roots);
			const sameFreshClaim =
				current?.pid === entry.pid &&
				current.token === entry.token &&
				daemonRuntime.daemonProcessAlive(current.pid) &&
				daemonRuntime.daemonClaimFresh(current);
			if (sameFreshClaim) {
				throw new Error(
					`registered daemon pid ${entry.pid} for ${entry.workspace_root} is alive but unavailable; stop that process before retrying: ${(error as Error).message}`,
				);
			}
			daemonRuntime.forgetDaemon(entry);
			launched = await daemonRuntime.launch(
				this.daemonLaunchOptions(),
			);
		}
		this.ownedDaemon = launched;
		const fresh = launched.entry;
		this.noteOwnership(fresh, launched);
		this.ensureActive();
		// A mismatch here means the relaunched daemon speaks the same protocol
		// as before: the binaries genuinely disagree. It propagates as a fault,
		// leaving the daemon up for CLI/MCP clients.
		const link = await connectEntry(fresh);
		return { link, entry: fresh };
	}

	private daemonLaunchOptions(): {
		workspaceRoots: [string, ...string[]];
		binaryCandidates: [string, ...string[]];
	} {
		return {
			workspaceRoots: nonEmptyRoots(this.roots),
			binaryCandidates: nonEmptyBinaryCandidates(binaryCandidates()),
		};
	}

	private noteOwnership(
		entry: DaemonRegistryEntry,
		launched: OwnedDaemon | undefined,
	): void {
		if (launched !== undefined) {
			if (
				launched.process.pid === entry.pid &&
				launched.entry.token === entry.token
			) {
				this.ownedDaemon = launched;
			} else {
				this.clearOwnership();
			}
			return;
		}
		if (
			this.ownedDaemon?.process.pid !== entry.pid ||
			this.ownedDaemon.entry.token !== entry.token
		) {
			this.clearOwnership();
		}
	}

	private clearOwnership(): void {
		this.ownedDaemon = undefined;
	}

	private async syncWorkspaceStatus(): Promise<void> {
		this.setWorkspacePhase("loading");
		this.setConnectionStatus("connected");
		this.ensureActive();
		const status = await this.workspaceStatus();
		this.ensureActive();
		if (status?.phase === "failed") {
			this.lastError = status.failure?.message ?? "Workspace index failed";
		}
		if (status) {
			this.setWorkspacePhase(status.phase);
		}
	}

	private handleEvent(event: WorkspaceEventDto): void {
		this.noteGeneration(event.generation);
		if (event.kind === "refreshed") {
			this.generation = undefined;
		}
		if (event.kind === "refreshed") {
			this.setWorkspacePhase("ready");
		}
		if (event.kind === "failed") {
			this.lastError = event.stale_summary ?? "Workspace index failed";
			this.setWorkspacePhase("failed");
		}
		if (event.kind === "stale" && this.ready && consistencyMode() === "hybrid") {
			this.scheduleHybridRefresh();
		}
		this.eventEmitter.fire(event);
	}

	private scheduleHybridRefresh(): void {
		if (this.hybridRefresh) {
			return;
		}
		this.hybridRefresh = setTimeout(() => {
			this.hybridRefresh = undefined;
			void this.refresh().catch(() => {});
		}, HYBRID_REFRESH_DEBOUNCE_MS);
	}

	private noteGeneration(generation: number | null | undefined): void {
		if (typeof generation === "number") {
			this.generation = generation;
		}
	}

	private onConnectionClosed(): void {
		this.teardown();
		this.setConnectionStatus("disconnected");
		if (!this.disposed && this.reconnectEnabled && !this.reconnectTimer) {
			this.reconnectTimer = setTimeout(() => {
				this.reconnectTimer = undefined;
				void this.connectOrStart();
			}, RECONNECT_DELAY_MS);
		}
	}

	private clearReconnectTimer(): void {
		if (this.reconnectTimer) {
			clearTimeout(this.reconnectTimer);
			this.reconnectTimer = undefined;
		}
	}

	private teardown(): void {
		if (this.hybridRefresh) {
			clearTimeout(this.hybridRefresh);
			this.hybridRefresh = undefined;
		}
		this.subscription?.dispose();
		this.subscription = undefined;
		this.rpc?.close();
		this.rpc = undefined;
		this.endpoint = undefined;
		this.generation = undefined;
		this.capabilities = undefined;
		this.workspacePhase = undefined;
	}

	private setConnectionStatus(status: DaemonConnectionStatus): void {
		const previous = this.status;
		this.connectionStatus = status;
		if (status !== "connected") {
			this.workspacePhase = undefined;
		}
		if (status !== "error") {
			this.lastError = undefined;
		}
		this.emitStatusChange(previous);
	}

	private setWorkspacePhase(phase: WorkspacePhase): void {
		const previous = this.status;
		this.workspacePhase = phase;
		if (phase !== "failed") {
			this.lastError = undefined;
		}
		this.emitStatusChange(previous);
	}

	private emitStatusChange(previous: DaemonStatus): void {
		const status = this.status;
		if (previous === status) {
			return;
		}
		this.statusEmitter.fire(status);
	}
}

interface DaemonLink {
	client: CodeMonikerClient;
}

async function connectEntry(
	entry: DaemonRegistryEntry,
): Promise<DaemonLink> {
	const client = await daemonRuntime.connect(entry, {
		clientName: "vscode-extension",
	});
	return { client };
}

function isStaleError(error: unknown): boolean {
	return error instanceof DaemonRpcError && error.code === "workspace_stale";
}
