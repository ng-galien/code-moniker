import * as vscode from "vscode";

import { launchWorkspaceDaemon } from "../cli/facade";
import { DetachedProcess } from "../cli/runner";
import { DaemonRpcError, RpcSubscription } from "./client";
import {
	CapabilitySet,
	DaemonRegistryEntry,
	PROTOCOL_VERSION,
	Query,
	QueryResponse,
	WorkspaceEventDto,
	WorkspaceStatus,
} from "./model";
import { DaemonRpc, QueryOptions } from "./rpc";
import {
	daemonProcessAlive,
	daemonClaimFresh,
	findDaemonForRoots,
	forgetDaemonEntry,
	rootSetsMatch,
} from "./registry";

// The single live connection to the workspace daemon. Every feature (symbols,
// rules) talks to the daemon through this session, never to the raw client —
// it owns connect-or-start, the loading/ready phase, reconnection and events.

export type DaemonStatus = "disconnected" | "connecting" | "loading" | "ready" | "error";

// User-facing freshness policy (codeMoniker.daemon.consistency):
// - fresh: every query carries refresh_if_stale, so answers never lag the
//   files but navigation waits for reindexing.
// - hybrid (default): navigation reads the current snapshot instantly
//   (stale_ok); a stale event schedules one background workspace_refresh
//   whose refreshed event rolls the generation and re-fills the trees.
// - current: stale_ok reads only; reindexing happens on explicit command.
export type ConsistencyMode = "fresh" | "hybrid" | "current";

const ENTRY_POLL_ATTEMPTS = 50;
const ENTRY_POLL_INTERVAL_MS = 100;
const READY_POLL_ATTEMPTS = 300;
const READY_POLL_INTERVAL_MS = 200;
const QUERY_RETRY_ATTEMPTS = 60;
const QUERY_RETRY_INTERVAL_MS = 200;
const HYBRID_REFRESH_DEBOUNCE_MS = 300;
const RECONNECT_DELAY_MS = 500;

function consistencyMode(): ConsistencyMode {
	const value = vscode.workspace
		.getConfiguration("codeMoniker")
		.get<string>("daemon.consistency");
	return value === "fresh" || value === "current" ? value : "hybrid";
}

export class DaemonSession implements vscode.Disposable {
	private rpc?: DaemonRpc;
	private subscription?: RpcSubscription;
	private connecting?: Promise<boolean>;
	private hybridRefresh?: NodeJS.Timeout;
	private reconnectTimer?: NodeJS.Timeout;
	private reconnectEnabled = false;
	private disposed = false;
	private shuttingDown = false;
	private ownedDaemon?: DetachedProcess;
	private ownedClaimToken?: string;

	status: DaemonStatus = "disconnected";
	ready = false;
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
			this.setStatus("error");
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
		this.shuttingDown = true;
		this.reconnectEnabled = false;
		this.clearReconnectTimer();
		await this.connecting;
		let rpc = this.rpc;
		let temporaryRpc: DaemonRpc | undefined;
		if (!rpc) {
			const entry = findDaemonForRoots(this.roots);
			if (entry) {
				try {
					temporaryRpc = await DaemonRpc.connect(entry.endpoint);
					rpc = temporaryRpc;
				} catch {
				}
			}
		}
		let shutdownSucceeded = false;
		if (rpc) {
			try {
				await rpc.shutdown();
				shutdownSucceeded = true;
			} catch {
			}
		}
		if (!shutdownSucceeded) {
			this.ownedDaemon?.terminate();
		}
		temporaryRpc?.close();
		this.clearOwnership();
		this.teardown();
		this.setStatus("disconnected");
		this.shuttingDown = false;
	}

	async shutdownOwned(): Promise<void> {
		this.disposed = true;
		this.shuttingDown = true;
		this.reconnectEnabled = false;
		this.clearReconnectTimer();
		await this.connecting;
		let rpc: DaemonRpc | undefined;
		let temporaryRpc: DaemonRpc | undefined;
		const entry = findDaemonForRoots(this.roots);
		const ownsCurrentClaim =
			this.ownedDaemon !== undefined &&
			this.ownedClaimToken !== undefined &&
			entry?.pid === this.ownedDaemon.pid &&
			entry.token === this.ownedClaimToken;
		if (ownsCurrentClaim) {
			rpc = this.rpc;
			if (!rpc) {
				try {
					temporaryRpc = await DaemonRpc.connect(entry!.endpoint);
					rpc = temporaryRpc;
				} catch {
				}
			}
		}
		let shutdownSucceeded = false;
		if (this.ownedDaemon !== undefined && rpc) {
			try {
				await rpc.shutdown();
				shutdownSucceeded = true;
			} catch {
			}
		}
		if (!shutdownSucceeded && this.ownedDaemon?.isRunning()) {
			this.ownedDaemon?.terminate();
		}
		temporaryRpc?.close();
		this.clearOwnership();
		this.teardown();
		this.setStatus("disconnected");
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
		this.setStatus("connecting");
		try {
			let entry = findDaemonForRoots(this.roots);
			let launched: DetachedProcess | undefined;
			if (!entry) {
				launched = await launchWorkspaceDaemon(this.roots);
				this.ownedDaemon = launched;
				entry = await waitForEntry(this.roots);
			}
			if (!entry) {
				throw daemonRegistrationError("starting");
			}
			this.noteOwnership(entry, launched);
			this.ensureActive();
			let link: DaemonLink;
			try {
				link = await connectEntry(entry, this.roots);
			} catch (error) {
				if (error instanceof ProtocolMismatchError) {
					// Only an outdated daemon is worth recycling — it likely
					// predates a binary upgrade. A newer one serves other
					// up-to-date clients, so relaunching cannot help.
					if (!error.stale) {
						throw error;
					}
					await recycleDaemon(entry);
				}
				({ link, entry } = await this.relaunchAndReconnect(entry, error));
			}
			this.ensureActive();
			const rpc = link.rpc;
			rpc.onDidClose(() => this.onConnectionClosed());
			this.rpc = rpc;
			this.capabilities = link.capabilities;
			this.endpoint = entry.endpoint;
			this.subscription = await rpc.subscribeEvents((event) => this.handleEvent(event));
			this.ensureActive();
			await this.waitUntilReady();
			this.ensureActive();
			return true;
		} catch (error) {
			if (this.disposed || this.shuttingDown) {
				this.teardown();
				this.setStatus("disconnected");
				return false;
			}
			if (error instanceof ProtocolMismatchError) {
				this.protocolFault = error.message;
			}
			this.lastError = (error as Error).message;
			this.teardown();
			this.setStatus("error");
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
		if (!(error instanceof ProtocolMismatchError)) {
			const current = findDaemonForRoots(this.roots);
			const sameFreshClaim =
				current?.pid === entry.pid &&
				current.token === entry.token &&
					daemonProcessAlive(current.pid) &&
					daemonClaimFresh(current);
			if (sameFreshClaim) {
				throw new Error(
					`registered daemon pid ${entry.pid} for ${entry.workspace_root} is alive but unavailable; stop that process before retrying: ${(error as Error).message}`,
				);
			}
		}
		forgetDaemonEntry(entry);
		const launched = await launchWorkspaceDaemon(this.roots);
		this.ownedDaemon = launched;
		const fresh = await waitForEntry(this.roots);
		if (!fresh) {
			throw daemonRegistrationError("restarting after a stale registry entry");
		}
		this.noteOwnership(fresh, launched);
		this.ensureActive();
		// A mismatch here means the relaunched daemon speaks the same protocol
		// as before: the binaries genuinely disagree. It propagates as a fault,
		// leaving the daemon up for CLI/MCP clients.
		const link = await connectEntry(fresh, this.roots);
		return { link, entry: fresh };
	}

	private noteOwnership(entry: DaemonRegistryEntry, launched: DetachedProcess | undefined): void {
		if (launched !== undefined) {
			if (launched.pid === entry.pid) {
				this.ownedDaemon = launched;
				this.ownedClaimToken = entry.token;
			} else {
				this.clearOwnership();
			}
			return;
		}
		if (this.ownedDaemon?.pid !== entry.pid || this.ownedClaimToken !== entry.token) {
			this.clearOwnership();
		}
	}

	private clearOwnership(): void {
		this.ownedDaemon = undefined;
		this.ownedClaimToken = undefined;
	}

	private async waitUntilReady(): Promise<void> {
		this.setStatus("loading");
		for (let attempt = 0; attempt < READY_POLL_ATTEMPTS; attempt++) {
			this.ensureActive();
			const status = await this.workspaceStatus();
			this.ensureActive();
			if (status?.phase === "ready") {
				this.setStatus("ready");
				return;
			}
			await delay(READY_POLL_INTERVAL_MS);
		}
	}

	private handleEvent(event: WorkspaceEventDto): void {
		this.noteGeneration(event.generation);
		if (event.kind === "refreshed") {
			this.generation = undefined;
		}
		if (event.kind === "refreshed" && this.status === "loading") {
			this.setStatus("ready");
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
		this.setStatus("disconnected");
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

interface DaemonLink {
	rpc: DaemonRpc;
	capabilities: CapabilitySet;
}

// The wire shapes disagree. `stale` says which side is behind, which is the
// only input to the retry policy — the policy itself lives at the call site.
class ProtocolMismatchError extends Error {
	readonly stale: boolean;

	constructor(daemonProtocol: number) {
		const stale = daemonProtocol < PROTOCOL_VERSION;
		super(
			stale
				? `the workspace daemon speaks protocol ${daemonProtocol} but the extension expects ${PROTOCOL_VERSION}; the installed code-moniker CLI is outdated — update it (or point codeMoniker.binaryPath at a matching build), then reconnect`
				: `the workspace daemon speaks protocol ${daemonProtocol}, newer than this extension's ${PROTOCOL_VERSION}; update the Code Moniker extension, then reconnect (the daemon was left running)`,
		);
		this.name = "ProtocolMismatchError";
		this.stale = stale;
	}
}

// Stops an outdated daemon so a relaunch from the current binaries can take
// its registry claim.
async function recycleDaemon(entry: DaemonRegistryEntry): Promise<void> {
	try {
		const rpc = await DaemonRpc.connect(entry.endpoint);
		await rpc.shutdown();
		rpc.close();
	} catch {
	}
	await waitForDaemonExit(entry);
}

async function connectEntry(
	entry: DaemonRegistryEntry,
	expectedRoots: string[],
): Promise<DaemonLink> {
	const rpc = await DaemonRpc.connect(entry.endpoint);
	const handshake = await rpc.handshake("vscode-extension");
	if (handshake.protocol_version !== PROTOCOL_VERSION) {
		rpc.close();
		throw new ProtocolMismatchError(handshake.protocol_version);
	}
	if (!rootSetsMatch(handshake.workspace_roots, expectedRoots)) {
		rpc.close();
		throw new Error(
			`daemon workspace mismatch: expected [${expectedRoots.join(", ")}], daemon serves [${handshake.workspace_roots.join(", ")}]`,
		);
	}
	return { rpc, capabilities: handshake.capabilities };
}

async function waitForDaemonExit(entry: DaemonRegistryEntry): Promise<void> {
	for (let attempt = 0; attempt < ENTRY_POLL_ATTEMPTS; attempt++) {
		if (!daemonProcessAlive(entry.pid)) {
			return;
		}
		await delay(ENTRY_POLL_INTERVAL_MS);
	}
	throw new Error(
		`daemon pid ${entry.pid} did not exit after a requested protocol recycle; its registry claim was preserved`,
	);
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
