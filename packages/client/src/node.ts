import { spawn } from "node:child_process";
import {
	readFileSync,
	readdirSync,
	realpathSync,
	unlinkSync,
} from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

import WebSocket from "ws";

import {
	CodeMonikerClient,
	type ClientConnectOptions,
} from "./client.js";
import type { DaemonRegistryEntry } from "./generated.js";
import { DaemonRpc } from "./rpc.js";
import type { WebSocketFactory, WebSocketLike } from "./transport.js";
import { runGitDiffImpact, type DiffImpactOutput, type GitDiffImpactOptions } from "./diff-impact.js";

export * from "./diff-impact.js";

const HEARTBEAT_TIMEOUT_MS = 15_000;
const DEFAULT_REGISTRATION_TIMEOUT_MS = 5_000;
const DEFAULT_EXIT_TIMEOUT_MS = 5_000;
const DEFAULT_POLL_INTERVAL_MS = 100;
const REGISTRY_DIRECTORY_ENV = "CODE_MONIKER_REGISTRY_DIR";
declare const __CODE_MONIKER_MODULE_URL__: string;
const requireFromPackage = createRequire(__CODE_MONIKER_MODULE_URL__);

const BINARY_PACKAGES: Readonly<
	Record<string, { packageName: string; executable: string }>
> = {
	"darwin-arm64": {
		packageName: "@code-moniker/cli-darwin-arm64",
		executable: "code-moniker",
	},
	"darwin-x64": {
		packageName: "@code-moniker/cli-darwin-x64",
		executable: "code-moniker",
	},
	"linux-x64": {
		packageName: "@code-moniker/cli-linux-x64",
		executable: "code-moniker",
	},
	"win32-x64": {
		packageName: "@code-moniker/cli-win32-x64",
		executable: "code-moniker.exe",
	},
};

export interface NodeDaemonRuntimeOptions {
	registryDirectory?: string;
	webSocketFactory?: WebSocketFactory;
	timeoutMs?: number;
	binaryCandidates?: readonly [string, ...string[]];
}

export interface NodeDaemonConnectOptions {
	clientName?: string;
	expectedWorkspaceRoots?: readonly [string, ...string[]];
	timeoutMs?: number;
}

export interface LaunchDaemonOptions {
	workspaceRoots: readonly [string, ...string[]];
	binaryCandidates?: readonly [string, ...string[]];
	supervisorPid?: number;
	environment?: Record<string, string | undefined>;
	registrationTimeoutMs?: number;
	pollIntervalMs?: number;
}

export interface StopDaemonOptions {
	exitTimeoutMs?: number;
	pollIntervalMs?: number;
	timeoutMs?: number;
}

export interface DaemonProcess {
	readonly pid: number;
	isRunning(): boolean;
	terminate(): void;
}

export interface OwnedDaemon {
	readonly entry: DaemonRegistryEntry;
	readonly process: DaemonProcess;
}

export class NodeDaemonRuntime {
	readonly registryDirectory: string;
	private readonly webSocketFactory: WebSocketFactory;
	private readonly timeoutMs?: number;
	private readonly binaryCandidates?: readonly [string, ...string[]];

	constructor(options: NodeDaemonRuntimeOptions = {}) {
		this.registryDirectory = resolve(
			options.registryDirectory ?? defaultRegistryDirectory(),
		);
		this.webSocketFactory =
			options.webSocketFactory ?? nodeWebSocketFactory;
		this.timeoutMs = options.timeoutMs;
		this.binaryCandidates = options.binaryCandidates;
	}

	listDaemons(): DaemonRegistryEntry[] {
		const entries: DaemonRegistryEntry[] = [];
		for (const item of this.readRegistry()) {
			entries.push(item.entry);
		}
		entries.sort(compareRegistryEntries);
		return entries;
	}

	findDaemon(
		workspaceRoots: readonly string[],
	): DaemonRegistryEntry | undefined {
		const wanted = canonicalRoots(workspaceRoots);
		for (const { entry } of this.readRegistry()) {
			if (matchesDefaultWorkspace(entry, wanted)) {
				return entry;
			}
		}
		return undefined;
	}

	entryMatchesRoots(
		entry: DaemonRegistryEntry,
		workspaceRoots: readonly string[],
	): boolean {
		return matchesDefaultWorkspace(entry, canonicalRoots(workspaceRoots));
	}

	daemonProcessAlive(pid: number): boolean {
		try {
			process.kill(pid, 0);
			return true;
		} catch (error) {
			return (error as NodeJS.ErrnoException).code === "EPERM";
		}
	}

	daemonClaimFresh(entry: DaemonRegistryEntry): boolean {
		const heartbeat = entry.heartbeat_unix_ms ?? 0;
		return (
			heartbeat > 0 &&
			Date.now() - heartbeat <= HEARTBEAT_TIMEOUT_MS
		);
	}

	forgetDaemon(expected: DaemonRegistryEntry): void {
		for (const { file, entry } of this.readRegistry()) {
			if (
				entry.pid !== expected.pid ||
				entry.token !== expected.token
			) {
				continue;
			}
			try {
				const current = JSON.parse(
					readFileSync(file, "utf8"),
				) as unknown;
				if (
					isDaemonRegistryEntry(current) &&
					current.pid === expected.pid &&
					current.token === expected.token
				) {
					unlinkSync(file);
				}
			} catch {
				// A concurrent daemon update owns the claim now.
			}
		}
	}

	connect(
		entry: DaemonRegistryEntry,
		options: NodeDaemonConnectOptions = {},
	): Promise<CodeMonikerClient> {
		const expectedWorkspaceRoots =
			options.expectedWorkspaceRoots ??
			nonEmptyRoots(entry.workspace_roots);
		const connectOptions: ClientConnectOptions = {
			clientName:
				options.clientName ?? "@code-moniker/client/node",
			expectedWorkspaceRoots,
			webSocketFactory: this.webSocketFactory,
			timeoutMs: options.timeoutMs ?? this.timeoutMs,
		};
		return CodeMonikerClient.connect(entry.endpoint, connectOptions);
	}

	async launch(options: LaunchDaemonOptions): Promise<OwnedDaemon> {
		const binaryCandidates =
			options.binaryCandidates ??
			this.binaryCandidates ??
			defaultBinaryCandidates();
		const processHandle = await launchDetached(
			binaryCandidates,
			daemonArguments(options.workspaceRoots, options.supervisorPid),
			daemonEnvironment(options.environment, this.registryDirectory),
		);
		try {
			const entry = await this.waitForEntry(
				options.workspaceRoots,
				processHandle.pid,
				options.registrationTimeoutMs ??
					DEFAULT_REGISTRATION_TIMEOUT_MS,
				options.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS,
			);
			return { entry, process: processHandle };
		} catch (error) {
			processHandle.terminate();
			throw error;
		}
	}

	async restart(
		entry: DaemonRegistryEntry,
		launchOptions: LaunchDaemonOptions,
		stopOptions: StopDaemonOptions = {},
	): Promise<OwnedDaemon> {
		await this.stop(entry, stopOptions);
		this.forgetDaemon(entry);
		return this.launch(launchOptions);
	}

	async stop(
		entry: DaemonRegistryEntry,
		options: StopDaemonOptions = {},
	): Promise<void> {
		const rpc = await DaemonRpc.connect(entry.endpoint, {
			webSocketFactory: this.webSocketFactory,
			timeoutMs: options.timeoutMs ?? this.timeoutMs,
		});
		try {
			await rpc.shutdown();
		} finally {
			rpc.close();
		}
		await this.waitForExit(
			entry,
			options.exitTimeoutMs ?? DEFAULT_EXIT_TIMEOUT_MS,
			options.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS,
		);
	}

	async stopOwned(
		owned: OwnedDaemon,
		options: StopDaemonOptions = {},
	): Promise<void> {
		const current = this.findDaemon(owned.entry.workspace_roots);
		const ownsCurrentClaim =
			current?.pid === owned.process.pid &&
			current.token === owned.entry.token;
		if (ownsCurrentClaim) {
			try {
				await this.stop(current, options);
				return;
			} catch {
				// Fall back to the process handle that this caller owns.
			}
		}
		if (owned.process.isRunning()) {
			owned.process.terminate();
			await this.waitForExit(
				owned.entry,
				options.exitTimeoutMs ?? DEFAULT_EXIT_TIMEOUT_MS,
				options.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS,
			);
		}
	}

	async waitForExit(
		entry: DaemonRegistryEntry,
		timeoutMs = DEFAULT_EXIT_TIMEOUT_MS,
		pollIntervalMs = DEFAULT_POLL_INTERVAL_MS,
	): Promise<void> {
		const deadline = Date.now() + timeoutMs;
		while (Date.now() <= deadline) {
			if (!this.daemonProcessAlive(entry.pid)) {
				return;
			}
			await delay(pollIntervalMs);
		}
		throw new Error(
			`daemon pid ${entry.pid} did not exit after shutdown`,
		);
	}

	private async waitForEntry(
		workspaceRoots: readonly string[],
		pid: number,
		timeoutMs: number,
		pollIntervalMs: number,
	): Promise<DaemonRegistryEntry> {
		const deadline = Date.now() + timeoutMs;
		while (Date.now() <= deadline) {
			const entry = this.findDaemon(workspaceRoots);
			if (entry?.pid === pid) {
				return entry;
			}
			await delay(pollIntervalMs);
		}
		throw new Error(
			`daemon pid ${pid} did not register for [${workspaceRoots.join(", ")}] within ${timeoutMs}ms; ${this.registryDiagnostic()}`,
		);
	}

	private registryDiagnostic(): string {
		const entries = this.listDaemons();
		if (entries.length === 0) {
			return `registry ${this.registryDirectory} contains no daemon entries`;
		}
		return `registry ${this.registryDirectory} contains ${entries
			.map((entry) => `${entry.pid}=[${entry.workspace_roots.join(", ")}]`)
			.join("; ")}`;
	}

	private readRegistry(): RegistryFile[] {
		let names: string[];
		try {
			names = readdirSync(this.registryDirectory);
		} catch {
			return [];
		}
		const files: RegistryFile[] = [];
		for (const name of names) {
			if (!name.endsWith(".json")) {
				continue;
			}
			const file = join(this.registryDirectory, name);
			try {
				const entry = JSON.parse(
					readFileSync(file, "utf8"),
				) as unknown;
				if (isDaemonRegistryEntry(entry)) {
					files.push({ file, entry });
				}
			} catch {
				// Ignore partial, malformed, or concurrently removed claims.
			}
		}
		return files;
	}
}

export function diffImpactGit(options: GitDiffImpactOptions): Promise<DiffImpactOutput> {
	return runGitDiffImpact(
		options,
		(registryDirectory) => new NodeDaemonRuntime({ registryDirectory }),
	);
}

export function defaultRegistryDirectory(): string {
	return join(tmpdir(), "code-moniker-daemons");
}

export function nodeWebSocketFactory(url: string): WebSocketLike {
	return new WebSocket(url) as unknown as WebSocketLike;
}

function bundledBinaryPath(
	platform: string = process.platform,
	architecture: string = process.arch,
): string | undefined {
	const binaryPackage = BINARY_PACKAGES[`${platform}-${architecture}`];
	if (!binaryPackage) {
		return undefined;
	}
	try {
		return requireFromPackage.resolve(
			`${binaryPackage.packageName}/bin/${binaryPackage.executable}`,
		);
	} catch {
		return undefined;
	}
}

function defaultBinaryCandidates(): readonly [string, ...string[]] {
	const bundled = bundledBinaryPath();
	return bundled === undefined
		? ["code-moniker"]
		: [bundled, "code-moniker"];
}

interface RegistryFile {
	file: string;
	entry: DaemonRegistryEntry;
}

function daemonArguments(
	workspaceRoots: readonly string[],
	supervisorPid = process.pid,
): string[] {
	const args = [
		"daemon",
		"start",
		...workspaceRoots,
		"--supervisor-pid",
		String(supervisorPid),
	];
	if (process.platform !== "win32") {
		args.push("--supervisor-fd", "3");
	}
	return args;
}

function daemonEnvironment(
	environment: NodeJS.ProcessEnv | undefined,
	registryDirectory: string,
): NodeJS.ProcessEnv {
	return {
		...(environment ?? process.env),
		[REGISTRY_DIRECTORY_ENV]: registryDirectory,
	};
}

function launchDetached(
	binaryCandidates: readonly string[],
	args: string[],
	environment: NodeJS.ProcessEnv | undefined,
): Promise<DaemonProcess> {
	return tryLaunchDetached(binaryCandidates, 0, args, environment);
}

function tryLaunchDetached(
	binaryCandidates: readonly string[],
	index: number,
	args: string[],
	environment: NodeJS.ProcessEnv | undefined,
): Promise<DaemonProcess> {
	if (index >= binaryCandidates.length) {
		return Promise.reject(
			new Error(
				`could not launch code-moniker (tried: ${binaryCandidates.join(", ")})`,
			),
		);
	}
	return new Promise(startLaunch);

	function startLaunch(
		resolveLaunch: (process: DaemonProcess) => void,
		rejectLaunch: (reason?: unknown) => void,
	): void {
		const child = spawn(binaryCandidates[index], args, {
			detached: true,
			env: environment,
			stdio:
				process.platform === "win32"
					? ["ignore", "ignore", "inherit"]
					: ["ignore", "ignore", "inherit", "pipe"],
			windowsHide: process.platform === "win32",
		});
		let settled = false;
		child.once("spawn", onSpawn);
		child.once("error", onError);

		function onSpawn(): void {
			if (settled) {
				return;
			}
			settled = true;
			const pid = child.pid;
			const supervisorPipe =
				process.platform === "win32"
					? null
					: (child.stdio[3] as
							| (NodeJS.ReadableStream & { unref?: () => void })
							| null);
			supervisorPipe?.unref?.();
			child.unref();
			if (pid === undefined) {
				rejectLaunch(
					new Error(
						`code-moniker launched without a process id: ${binaryCandidates[index]}`,
					),
				);
				return;
			}
			resolveLaunch({ pid, isRunning, terminate });
		}

		function onError(error: NodeJS.ErrnoException): void {
			if (settled) {
				return;
			}
			settled = true;
			if (error.code === "ENOENT") {
				void tryLaunchDetached(
					binaryCandidates,
					index + 1,
					args,
					environment,
				).then(resolveLaunch, rejectLaunch);
				return;
			}
			rejectLaunch(error);
		}

		function isRunning(): boolean {
			return child.exitCode === null && child.signalCode === null;
		}

		function terminate(): void {
			if (isRunning()) {
				child.kill();
			}
		}
	}
}

function canonicalRoots(roots: readonly string[]): readonly string[] {
	return roots.map(canonicalPath);
}

function matchesDefaultWorkspace(
	entry: DaemonRegistryEntry,
	wanted: readonly string[],
): boolean {
	return (
		entry.project == null &&
		entry.cache_dir == null &&
		rootSetsMatch(entry.workspace_roots, wanted)
	);
}

function rootSetsMatch(
	actual: readonly string[],
	expected: readonly string[],
): boolean {
	const actualCanonical = actual.map(canonicalPath);
	const expectedCanonical = expected.map(canonicalPath);
	const actualSet = new Set(actualCanonical);
	const expectedSet = new Set(expectedCanonical);
	if (
		actualSet.size !== actualCanonical.length ||
		expectedSet.size !== expectedCanonical.length ||
		actualSet.size !== expectedSet.size
	) {
		return false;
	}
	for (const root of actualSet) {
		if (!expectedSet.has(root)) {
			return false;
		}
	}
	return true;
}

function canonicalPath(candidate: string): string {
	let canonical: string;
	try {
		canonical =
			process.platform === "win32"
				? realpathSync.native(candidate)
				: realpathSync(candidate);
	} catch {
		canonical = resolve(candidate);
	}
	if (process.platform !== "win32") {
		return canonical;
	}
	if (canonical.startsWith("\\\\?\\UNC\\")) {
		canonical = `\\\\${canonical.slice(8)}`;
	} else if (canonical.startsWith("\\\\?\\")) {
		canonical = canonical.slice(4);
	}
	return canonical;
}

function nonEmptyRoots(
	roots: string[],
): readonly [string, ...string[]] {
	if (roots.length === 0) {
		throw new Error("daemon registry entry has no workspace roots");
	}
	return roots as [string, ...string[]];
}

function isDaemonRegistryEntry(
	value: unknown,
): value is DaemonRegistryEntry {
	if (value === null || typeof value !== "object") {
		return false;
	}
	const entry = value as Partial<DaemonRegistryEntry>;
	if (
		!Array.isArray(entry.workspace_roots) ||
		entry.workspace_roots.length === 0
	) {
		return false;
	}
	for (const root of entry.workspace_roots) {
		if (typeof root !== "string" || root.length === 0) {
			return false;
		}
	}
	return (
		typeof entry.workspace_root === "string" &&
		typeof entry.endpoint === "string" &&
		entry.endpoint.length > 0 &&
		typeof entry.token === "string" &&
		entry.token.length > 0 &&
		typeof entry.pid === "number"
	);
}

function compareRegistryEntries(
	left: DaemonRegistryEntry,
	right: DaemonRegistryEntry,
): number {
	return left.workspace_root.localeCompare(right.workspace_root);
}
