import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { DaemonRegistryEntry } from "./model";

const HEARTBEAT_TIMEOUT_MS = 15_000;

// The daemon records each running instance as a JSON file under a shared registry
// directory (crates/query/src/discovery.rs). We read it directly rather than
// shelling out, and match the opened workspace by canonical (realpath) root.

export function registryDir(): string {
	return path.join(os.tmpdir(), "code-moniker-daemons");
}

export function listDaemons(): DaemonRegistryEntry[] {
	return readRegistry()
		.map(({ entry }) => entry)
		.sort((a, b) => a.workspace_root.localeCompare(b.workspace_root));
}

export function findDaemonForRoots(roots: string[]): DaemonRegistryEntry | undefined {
	const wanted = canonicalSet(roots);
	return readRegistry().find(({ entry }) => matches(entry, wanted))?.entry;
}

export function forgetDaemonEntry(expected: DaemonRegistryEntry): void {
	for (const { file, entry } of readRegistry()) {
		if (entry.pid === expected.pid && entry.token === expected.token) {
			try {
				const current = JSON.parse(fs.readFileSync(file, "utf8")) as DaemonRegistryEntry;
				if (current.pid === expected.pid && current.token === expected.token) {
					fs.unlinkSync(file);
				}
			} catch {
			}
		}
	}
}

export function daemonProcessAlive(pid: number): boolean {
	try {
		process.kill(pid, 0);
		return true;
	} catch (error) {
		return (error as NodeJS.ErrnoException).code === "EPERM";
	}
}

export function daemonClaimFresh(entry: DaemonRegistryEntry): boolean {
	const heartbeat = entry.heartbeat_unix_ms ?? 0;
	return heartbeat > 0 && Date.now() - heartbeat <= HEARTBEAT_TIMEOUT_MS;
}

export function entryMatchesRoots(entry: DaemonRegistryEntry, roots: string[]): boolean {
	return matches(entry, canonicalSet(roots));
}

export function rootSetsMatch(actual: string[], expected: string[]): boolean {
	return matchesRoots(actual, canonicalSet(expected));
}

interface RegistryFile {
	file: string;
	entry: DaemonRegistryEntry;
}

// Single scan of the registry directory: read, filter, parse. Callers that need
// the filename (to delete) get it; others just take `entry`.
function readRegistry(): RegistryFile[] {
	let names: string[];
	try {
		names = fs.readdirSync(registryDir());
	} catch {
		return [];
	}
	const files: RegistryFile[] = [];
	for (const name of names) {
		if (!name.endsWith(".json")) {
			continue;
		}
		const file = path.join(registryDir(), name);
		try {
			files.push({ file, entry: JSON.parse(fs.readFileSync(file, "utf8")) as DaemonRegistryEntry });
		} catch {
		}
	}
	return files;
}

function canonicalSet(roots: string[]): Set<string> {
	return new Set(roots.map(canonical));
}

function matches(entry: DaemonRegistryEntry, wanted: Set<string>): boolean {
	return (
		entry.project == null &&
		entry.cache_dir == null &&
		matchesRoots(entry.workspace_roots, wanted)
	);
}

function matchesRoots(actual: string[], wanted: Set<string>): boolean {
	const actualSet = canonicalSet(actual);
	return actualSet.size === wanted.size && [...actualSet].every((root) => wanted.has(root));
}

function canonical(p: string): string {
	try {
		return fs.realpathSync(p);
	} catch {
		return path.resolve(p);
	}
}
