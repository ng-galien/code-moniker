import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { DaemonRegistryEntry } from "./model";

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

export function forgetDaemonsForRoots(roots: string[]): void {
	const wanted = canonicalSet(roots);
	for (const { file, entry } of readRegistry()) {
		if (matches(entry, wanted)) {
			try {
				fs.unlinkSync(file);
			} catch {
				// Already gone; nothing to forget.
			}
		}
	}
}

export function entryMatchesRoots(entry: DaemonRegistryEntry, roots: string[]): boolean {
	return matches(entry, canonicalSet(roots));
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
			// Skip partially-written or corrupt entries.
		}
	}
	return files;
}

function canonicalSet(roots: string[]): Set<string> {
	return new Set(roots.map(canonical));
}

function matches(entry: DaemonRegistryEntry, wanted: Set<string>): boolean {
	return entry.workspace_roots.some((root) => wanted.has(canonical(root)));
}

function canonical(p: string): string {
	try {
		return fs.realpathSync(p);
	} catch {
		return path.resolve(p);
	}
}
